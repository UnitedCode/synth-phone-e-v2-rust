pub mod midi_voice;
pub mod voice_generator;

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
            Err(nb::Error::Other(_)) => {
                // UART hardware error (framing, noise, overrun) — typically caused by
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
    ProgramChange {
        channel: u8,
        program: u8,
    },
    Other,
}

// Precomputed MIDI note → frequency table (440 * 2^((n-69)/12)), notes 0-127
const MIDI_NOTE_FREQUENCIES: [f32; 128] = [
    8.175799,
    8.661957,
    9.177024,
    9.722718,
    10.300861,
    10.913382,
    11.562326,
    12.249857,
    12.978272,
    13.750000,
    14.567618,
    15.433853,
    16.351598,
    17.323914,
    18.354048,
    19.445436,
    20.601722,
    21.826764,
    23.124651,
    24.499715,
    25.956543,
    27.500000,
    29.135235,
    30.867706,
    32.703197,
    34.647827,
    36.708096,
    38.890873,
    41.203445,
    43.653529,
    46.249303,
    48.999430,
    51.913087,
    55.000000,
    58.270470,
    61.735413,
    65.406395,
    69.295654,
    73.416192,
    77.781746,
    82.406889,
    87.307058,
    92.498606,
    97.998859,
    103.826174,
    110.000000,
    116.540940,
    123.470825,
    130.812791,
    138.591309,
    146.832384,
    155.563492,
    164.813778,
    174.614116,
    184.997211,
    195.997718,
    207.652349,
    220.000000,
    233.081881,
    246.941651,
    261.625565,
    277.182631,
    293.664768,
    311.126984,
    329.627557,
    349.228231,
    369.994423,
    391.995436,
    415.304698,
    440.000000,
    466.163762,
    493.883301,
    523.251131,
    554.365262,
    587.329536,
    622.253967,
    659.255114,
    698.456463,
    739.988845,
    783.990872,
    830.609395,
    880.000000,
    932.327523,
    987.766603,
    1046.502261,
    1108.730524,
    1174.659072,
    1244.507935,
    1318.510228,
    1396.912926,
    1479.977691,
    1567.981744,
    1661.218790,
    1760.000000,
    1864.655046,
    1975.533205,
    2093.004522,
    2217.461048,
    2349.318143,
    2489.015870,
    2637.020455,
    2793.825851,
    2959.955382,
    3135.963488,
    3322.437581,
    3520.000000,
    3729.310092,
    3951.066410,
    4186.009045,
    4434.922096,
    4698.636287,
    4978.031740,
    5274.040909,
    5587.651703,
    5919.910763,
    6271.926976,
    6644.875161,
    7040.000000,
    7458.620184,
    7902.132820,
    8372.018090,
    8869.844191,
    9397.272574,
    9956.063479,
    10548.081821,
    11175.303406,
    11839.821527,
    12543.853951,
];

// MIDI note utilities
impl MidiEvent {
    pub fn note_to_frequency(note: u8) -> f32 {
        MIDI_NOTE_FREQUENCIES[note.min(127) as usize]
    }

    pub fn is_note_event(&self) -> bool {
        matches!(self, MidiEvent::NoteOn { .. } | MidiEvent::NoteOff { .. })
    }

    pub fn get_channel(&self) -> Option<u8> {
        match self {
            MidiEvent::NoteOn { channel, .. }
            | MidiEvent::NoteOff { channel, .. }
            | MidiEvent::ControlChange { channel, .. }
            | MidiEvent::PitchBend { channel, .. }
            | MidiEvent::ProgramChange { channel, .. } => Some(*channel),
            MidiEvent::Other => None,
        }
    }
}

// Helper function to check MIDI queue status
pub fn get_midi_queue_status(
    midi_events: &heapless::spsc::Queue<MidiEvent, 128>,
) -> (usize, usize) {
    let capacity = midi_events.capacity();
    let len = midi_events.len();
    (len, capacity)
}

// Function to handle queue overflow protection
pub fn try_enqueue_midi_event(
    midi_events: &mut heapless::spsc::Queue<MidiEvent, 128>,
    event: MidiEvent,
) -> Result<(), MidiEvent> {
    match midi_events.enqueue(event) {
        Ok(()) => Ok(()),
        Err(event) => {
            // Queue is full - try to make space by removing oldest non-critical events
            // Keep note events but drop less critical ones like CC messages
            let mut dropped_non_critical = false;

            // Try to find and remove a non-critical event to make space
            let mut temp_events = heapless::Vec::<MidiEvent, 16>::new();

            // Dequeue up to 16 events looking for non-critical ones
            for _ in 0..16 {
                if let Some(old_event) = midi_events.dequeue() {
                    match old_event {
                        MidiEvent::ControlChange { .. } | MidiEvent::PitchBend { .. } => {
                            // Drop this non-critical event and try to enqueue the new one
                            dropped_non_critical = true;
                            break;
                        }
                        _ => {
                            // Keep critical events
                            let _ = temp_events.push(old_event);
                        }
                    }
                } else {
                    break;
                }
            }

            // Re-enqueue the kept events
            for temp_event in temp_events.iter() {
                let _ = midi_events.enqueue(*temp_event);
            }

            if dropped_non_critical {
                // Try to enqueue the new event now that we made space
                midi_events.enqueue(event).map_err(|e| e)
            } else {
                // Couldn't make space, return error
                warn!("MIDI queue full and couldn't drop non-critical events");
                Err(event)
            }
        }
    }
}
