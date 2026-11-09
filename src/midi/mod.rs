mod events;
pub mod midi_voice;
pub mod notes;
pub mod voice_generator;

pub use events::{get_midi_queue_status, try_enqueue_midi_event, MidiEvent};

use log::warn;
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
                match self.state {
                    MidiParserState::WaitingForStatus => {
                        if byte >= 0xF8 {
                            // Real-Time message (clock, active sense, reset): single byte,
                            // must not alter running-status context — process and discard.
                            return Ok(Some(MidiEvent::Other));
                        }
                        if byte & 0x80 != 0 {
                            // System real-time messages (0xF8–0xFF) are single-byte and
                            // transparent — they must NOT update status_byte so they don't
                            // corrupt the running-status register (e.g. MIDI Clock 0xF8
                            // arriving between two drum notes).
                            if byte >= 0xF8 {
                                return Ok(Some(MidiEvent::Other));
                            }

                            // This is a channel/system-common status byte
                            self.status_byte = byte;
                            self.data_count = 0;

                            // Determine how many data bytes we expect
                            self.expected_data_bytes = match byte & 0xF0 {
                                0x80 | 0x90 | 0xA0 | 0xB0 | 0xE0 => 2, // Note Off, Note On, Aftertouch, CC, Pitch Bend
                                0xC0 | 0xD0 => 1, // Program Change, Channel Pressure
                                _ => {
                                    return Ok(Some(MidiEvent::Other));
                                }
                            };

                            if self.expected_data_bytes > 0 {
                                self.state = MidiParserState::CollectingData;
                            } else {
                                // No data bytes expected, process immediately
                                return Ok(Some(self.create_midi_event()));
                            }
                        } else if self.status_byte != 0
                            && self.status_byte < 0xF0
                            && self.expected_data_bytes > 0
                        {
                            // Running status: reuse the previous channel status byte.
                            // Most MIDI controllers omit the repeated status byte for
                            // consecutive messages on the same channel (e.g. drum chords).
                            // Only valid for channel messages (status < 0xF0).
                            self.data_bytes[0] = byte;
                            self.data_count = 1;
                            if self.expected_data_bytes == 1 {
                                return Ok(Some(self.create_midi_event()));
                            } else {
                                self.state = MidiParserState::CollectingData;
                            }
                        }
                    }

                    MidiParserState::CollectingData => {
                        if byte >= 0xF8 {
                            // Real-Time message mid-stream: discard without touching state.
                            return Ok(Some(MidiEvent::Other));
                        }
                        if byte & 0x80 != 0 {
                            // System real-time bytes are transparent — ignore them without
                            // aborting the message currently being collected.
                            if byte >= 0xF8 {
                                return Ok(Some(MidiEvent::Other));
                            }
                            // New channel/system-common status byte while collecting data
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
                                self.state = MidiParserState::WaitingForStatus;
                            }
                        }
                    }
                }

                Ok(None)
            }
            Err(nb::Error::WouldBlock) => Ok(None),
            Err(nb::Error::Other(stm32h7xx_hal::serial::Error::Noise)) => {
                // Noise only flags a marginal edge sample: the HAL clears the flag
                // and leaves the byte in the FIFO, so the next read still returns
                // it. Resetting here would wipe running status, and dense streams
                // send their NoteOffs as running-status NoteOn vel 0. Every one of
                // those would then be ignored until the next status byte,
                // leaving notes stuck on.
                warn!("MIDI UART noise flag (byte kept)");
                Ok(None)
            }
            Err(nb::Error::Other(e)) => {
                warn!("MIDI UART error {:?}: parser reset", e);
                // UART hardware error (framing, parity, overrun) — typically caused by
                // a MIDI device being disconnected.  Reset parser state so the next
                // device can start fresh without misinterpreting stale context.
                self.state = MidiParserState::WaitingForStatus;
                self.status_byte = 0;
                self.data_count = 0;
                self.expected_data_bytes = 0;
                Ok(None)
            }
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
            0xC0 => {
                // Program Change
                MidiEvent::ProgramChange {
                    channel,
                    program: self.data_bytes[0],
                }
            }
            _ => MidiEvent::Other,
        }
    }
}

// MIDI note utilities
impl MidiEvent {
    pub fn note_to_frequency(note: u8) -> f32 {
        notes::frequency(note)
    }
}
