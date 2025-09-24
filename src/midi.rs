use log::info;
use stm32h7xx_hal::{nb, prelude::*, serial::Rx, stm32};

pub struct MidiReceiver {
    rx: Rx<stm32::USART1>,
    state: MidiParserState,
    status_byte: u8,
    data_bytes: [u8; 2],
    data_count: usize,
    expected_data_bytes: usize,
}

#[derive(Debug, Clone, Copy)]
enum MidiParserState {
    WaitingForStatus,
    CollectingData,
}

impl MidiReceiver {
    pub fn new(rx: Rx<stm32::USART1>) -> Self {
        Self {
            rx,
            state: MidiParserState::WaitingForStatus,
            status_byte: 0,
            data_bytes: [0, 0],
            data_count: 0,
            expected_data_bytes: 0,
        }
    }

    pub fn try_read(
        &mut self,
    ) -> Result<Option<MidiEvent>, nb::Error<stm32h7xx_hal::serial::Error>> {
        match self.rx.read() {
            Ok(byte) => {
                info!("MIDI Byte: 0x{:02X} ({})", byte, byte);

                match self.state {
                    MidiParserState::WaitingForStatus => {
                        if byte & 0x80 != 0 {
                            // This is a status byte
                            self.status_byte = byte;
                            self.data_count = 0;

                            // Determine how many data bytes we expect
                            self.expected_data_bytes = match byte & 0xF0 {
                                0x80 | 0x90 | 0xA0 | 0xB0 | 0xE0 => 2, // Note Off, Note On, Aftertouch, CC, Pitch Bend
                                0xC0 | 0xD0 => 1, // Program Change, Channel Pressure
                                _ => {
                                    info!("Unknown or unsupported MIDI status: 0x{:02X}", byte);
                                    return Ok(Some(MidiEvent::Other));
                                }
                            };

                            if self.expected_data_bytes > 0 {
                                self.state = MidiParserState::CollectingData;
                            } else {
                                // No data bytes expected, process immediately
                                return Ok(Some(self.create_midi_event()));
                            }
                        }
                        // If not a status byte, ignore (could be running status, but we'll keep it simple)
                    }

                    MidiParserState::CollectingData => {
                        if byte & 0x80 != 0 {
                            // New status byte received while collecting data - reset
                            info!("New status byte received while collecting data, resetting");
                            self.status_byte = byte;
                            self.data_count = 0;
                            self.expected_data_bytes = match byte & 0xF0 {
                                0x80 | 0x90 | 0xA0 | 0xB0 | 0xE0 => 2,
                                0xC0 | 0xD0 => 1,
                                _ => return Ok(Some(MidiEvent::Other)),
                            };
                        } else {
                            // This is a data byte
                            if self.data_count < self.expected_data_bytes && self.data_count < 2 {
                                self.data_bytes[self.data_count] = byte;
                                self.data_count += 1;

                                // Check if we have all the data we need
                                if self.data_count >= self.expected_data_bytes {
                                    let event = self.create_midi_event();
                                    self.state = MidiParserState::WaitingForStatus;
                                    return Ok(Some(event));
                                }
                            } else {
                                info!("Too many data bytes, resetting parser");
                                self.state = MidiParserState::WaitingForStatus;
                            }
                        }
                    }
                }

                Ok(None)
            }
            Err(nb::Error::WouldBlock) => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn create_midi_event(&self) -> MidiEvent {
        let channel = self.status_byte & 0x0F;
        let message_type = self.status_byte & 0xF0;

        match message_type {
            0x90 => {
                // Note On
                let key = self.data_bytes[0];
                let velocity = self.data_bytes[1];
                if velocity == 0 {
                    // Velocity 0 is actually a Note Off
                    MidiEvent::NoteOff {
                        channel,
                        key,
                        velocity,
                    }
                } else {
                    MidiEvent::NoteOn {
                        channel,
                        key,
                        velocity,
                    }
                }
            }
            0x80 => {
                // Note Off
                MidiEvent::NoteOff {
                    channel,
                    key: self.data_bytes[0],
                    velocity: self.data_bytes[1],
                }
            }
            0xB0 => {
                // Control Change
                MidiEvent::ControlChange {
                    channel,
                    controller: self.data_bytes[0],
                    value: self.data_bytes[1],
                }
            }
            0xE0 => {
                // Pitch Bend
                let lsb = self.data_bytes[0] as u16;
                let msb = self.data_bytes[1] as u16;
                let value = (msb << 7) | lsb;
                MidiEvent::PitchBend { channel, value }
            }
            _ => MidiEvent::Other,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum MidiEvent {
    NoteOn {
        channel: u8,
        key: u8,
        velocity: u8,
    },
    NoteOff {
        channel: u8,
        key: u8,
        velocity: u8,
    },
    ControlChange {
        channel: u8,
        controller: u8,
        value: u8,
    },
    PitchBend {
        channel: u8,
        value: u16,
    },
    Other,
}

// MIDI note utilities
impl MidiEvent {
    pub fn note_to_frequency(note: u8) -> f32 {
        // MIDI note to frequency conversion
        // A4 (note 69) = 440 Hz
        440.0 * libm::powf(2.0, (note as f32 - 69.0) / 12.0)
    }

    pub fn is_note_event(&self) -> bool {
        matches!(self, MidiEvent::NoteOn { .. } | MidiEvent::NoteOff { .. })
    }

    pub fn get_channel(&self) -> Option<u8> {
        match self {
            MidiEvent::NoteOn { channel, .. }
            | MidiEvent::NoteOff { channel, .. }
            | MidiEvent::ControlChange { channel, .. }
            | MidiEvent::PitchBend { channel, .. } => Some(*channel),
            MidiEvent::Other => None,
        }
    }
}
