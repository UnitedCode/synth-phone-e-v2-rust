//! Basic MIDI module for note parsing and logging

use heapless::Vec;
use log::info;

/// MIDI message types
#[derive(Debug, Clone, Copy)]
pub enum MidiMessage {
    NoteOn { channel: u8, note: u8, velocity: u8 },
    NoteOff { channel: u8, note: u8, velocity: u8 },
    Unknown,
}

/// MIDI message parser
pub struct MidiParser {
    buffer: Vec<u8, 3>,
    expected_bytes: usize,
}

impl MidiParser {
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            expected_bytes: 0,
        }
    }

    /// Process a received MIDI byte and return a complete message if available
    pub fn process_byte(&mut self, byte: u8) -> Option<MidiMessage> {
        // Check if this is a status byte (MSB set)
        if byte & 0x80 != 0 {
            // Clear buffer for new message
            self.buffer.clear();
            self.buffer.push(byte).ok()?;

            // Determine expected message length
            let status = byte & 0xF0;
            self.expected_bytes = match status {
                0x80 | 0x90 => 3, // Note Off / Note On
                _ => 0,           // Unknown or unsupported
            };

            return None;
        }

        // This is a data byte
        if self.buffer.len() > 0 && self.buffer.len() < self.expected_bytes {
            self.buffer.push(byte).ok()?;

            // Check if we have a complete message
            if self.buffer.len() == self.expected_bytes {
                return self.parse_complete_message();
            }
        }

        None
    }

    fn parse_complete_message(&mut self) -> Option<MidiMessage> {
        if self.buffer.len() < 3 {
            return None;
        }

        let status = self.buffer[0];
        let channel = status & 0x0F;
        let message_type = status & 0xF0;
        let note = self.buffer[1];
        let velocity = self.buffer[2];

        let message = match message_type {
            0x90 => {
                if velocity > 0 {
                    MidiMessage::NoteOn {
                        channel,
                        note,
                        velocity,
                    }
                } else {
                    // Note on with velocity 0 is equivalent to note off
                    MidiMessage::NoteOff {
                        channel,
                        note,
                        velocity,
                    }
                }
            }
            0x80 => MidiMessage::NoteOff {
                channel,
                note,
                velocity,
            },
            _ => MidiMessage::Unknown,
        };

        // Clear buffer for next message
        self.buffer.clear();
        self.expected_bytes = 0;

        Some(message)
    }
}

/// Convert MIDI note number to note name
pub fn note_to_name(note: u8) -> &'static str {
    let note_names = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];

    let note_index = (note % 12) as usize;
    if note_index < note_names.len() {
        note_names[note_index]
    } else {
        "Unknown"
    }
}

/// Convert MIDI note number to octave
pub fn note_to_octave(note: u8) -> i8 {
    (note / 12) as i8 - 1
}

/// Log a MIDI message
pub fn log_midi_message(message: MidiMessage) {
    match message {
        MidiMessage::NoteOn {
            channel,
            note,
            velocity,
        } => {
            let note_name = note_to_name(note);
            let octave = note_to_octave(note);
            info!(
                "MIDI Note ON: {}{}  (Ch: {}, Note: {}, Vel: {})",
                note_name,
                octave,
                channel + 1,
                note,
                velocity
            );
        }
        MidiMessage::NoteOff {
            channel,
            note,
            velocity,
        } => {
            let note_name = note_to_name(note);
            let octave = note_to_octave(note);
            info!(
                "MIDI Note OFF: {}{}  (Ch: {}, Note: {}, Vel: {})",
                note_name,
                octave,
                channel + 1,
                note,
                velocity
            );
        }
        MidiMessage::Unknown => {
            info!("MIDI: Unknown message received");
        }
    }
}
