use midly::{live::LiveEvent, stream::MidiStream, MidiMessage};
use stm32h7xx_hal::{nb, prelude::*, serial::Rx, stm32};

pub struct MidiReceiver {
    midi_stream: MidiStream,
    rx: Rx<stm32::USART1>,
}

impl MidiReceiver {
    pub fn new(rx: Rx<stm32::USART1>) -> Self {
        Self {
            midi_stream: MidiStream::new(),
            rx,
        }
    }

    pub fn try_read(
        &mut self,
    ) -> Result<Option<MidiEvent>, nb::Error<stm32h7xx_hal::serial::Error>> {
        match self.rx.read() {
            Ok(byte) => {
                let mut midi_event = None;

                // Feed the byte to the MIDI stream
                self.midi_stream.feed(&[byte], |event| {
                    midi_event = Some(MidiEvent::from_live_event(event));
                });

                Ok(midi_event)
            }
            Err(nb::Error::WouldBlock) => Ok(None),
            Err(e) => Err(e),
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

impl MidiEvent {
    fn from_live_event(event: LiveEvent) -> Self {
        match event {
            LiveEvent::Midi { channel, message } => match message {
                MidiMessage::NoteOn { key, vel } => MidiEvent::NoteOn {
                    channel: channel.as_int(),
                    key: key.as_int(),
                    velocity: vel.as_int(),
                },
                MidiMessage::NoteOff { key, vel } => MidiEvent::NoteOff {
                    channel: channel.as_int(),
                    key: key.as_int(),
                    velocity: vel.as_int(),
                },
                MidiMessage::Controller { controller, value } => MidiEvent::ControlChange {
                    channel: channel.as_int(),
                    controller: controller.as_int(),
                    value: value.as_int(),
                },
                MidiMessage::PitchBend { bend } => MidiEvent::PitchBend {
                    channel: channel.as_int(),
                    value: bend.as_int() as u16,
                },
                _ => MidiEvent::Other,
            },
            _ => MidiEvent::Other,
        }
    }
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
