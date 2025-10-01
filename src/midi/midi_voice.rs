use log::info;
use synthphone_e_vocal_dsp::audio::{Oscillator, Waveform};

// Voice management for polyphony
pub struct Voice {
    pub oscillator: Oscillator,
    // None means voice is free
    pub note: Option<u8>,
    pub velocity: u8,
    pub channel: u8,
}

impl Voice {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            oscillator: Oscillator::new(440.0, sample_rate, Waveform::Triangle),
            note: None,
            velocity: 0,
            channel: 0,
        }
    }

    pub fn set_waveform(&mut self, waveform: Waveform) {
        self.oscillator.set_waveform(waveform);
    }

    pub fn is_free(&self) -> bool {
        self.note.is_none()
    }

    pub fn note_on(&mut self, note: u8, velocity: u8, channel: u8) {
        self.note = Some(note);
        self.velocity = velocity;
        self.channel = channel;
        let frequency = crate::midi::MidiEvent::note_to_frequency(note);
        self.oscillator.set_freq(frequency);
    }

    pub fn note_off(&mut self) {
        self.note = None;
        self.velocity = 0;
    }

    pub fn get_sample(&mut self) -> f32 {
        if self.note.is_some() && self.velocity > 0 {
            // Scale by velocity (0-127 -> 0.0-1.0)
            self.oscillator.next_value() * (self.velocity as f32 / 127.0)
        } else {
            0.0
        }
    }

    pub fn apply_pitch_bend(&mut self, note: u8, bend_ratio: f32) {
        let base_freq = crate::midi::MidiEvent::note_to_frequency(note);
        let bent_freq = base_freq * (1.0 + bend_ratio * 0.1); // +/- 10% bend range
        self.oscillator.set_freq(bent_freq);
    }
}

pub struct VoiceManager<const MAX_VOICES: usize> {
    pub voices: [Voice; MAX_VOICES],
    pub pitch_bend_ratio: f32,
}

impl<const MAX_VOICES: usize> VoiceManager<MAX_VOICES> {
    pub fn new(sample_rate: f32) -> Self {
        // Create array of voices using from_fn
        let voices = core::array::from_fn(|_| Voice::new(sample_rate));
        Self {
            voices,
            pitch_bend_ratio: 0.0,
        }
    }

    pub fn set_waveform(&mut self, waveform: Waveform) {
        for voice in self.voices.iter_mut() {
            voice.set_waveform(waveform);
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: u8, channel: u8) {
        // First, check if this note is already playing - if so, retrigger it
        for voice in self.voices.iter_mut() {
            if voice.note == Some(note) && voice.channel == channel {
                voice.note_on(note, velocity, channel);
                // Apply current pitch bend
                voice.apply_pitch_bend(note, self.pitch_bend_ratio);
                return;
            }
        }

        // Find a free voice
        for voice in self.voices.iter_mut() {
            if voice.is_free() {
                voice.note_on(note, velocity, channel);
                // Apply current pitch bend
                voice.apply_pitch_bend(note, self.pitch_bend_ratio);
                return;
            }
        }

        // No free voices - steal the oldest one (voice stealing)
        info!(
            "Voice stealing - taking voice 0 for note {} on channel {}",
            note, channel
        );
        self.voices[0].note_on(note, velocity, channel);
        self.voices[0].apply_pitch_bend(note, self.pitch_bend_ratio);
    }

    pub fn note_off(&mut self, note: u8, channel: u8) {
        for voice in self.voices.iter_mut() {
            if voice.note == Some(note) && voice.channel == channel {
                voice.note_off();
                return;
            }
        }
    }

    pub fn get_mixed_sample(&mut self) -> f32 {
        let mut mixed_sample = 0.0;
        let mut active_voices = 0;

        for voice in self.voices.iter_mut() {
            let sample = voice.get_sample();
            if sample != 0.0 {
                mixed_sample += sample;
                active_voices += 1;
            }
        }

        // Normalize by number of active voices to prevent clipping
        // Use a simple division instead of sqrt to avoid trait issues
        if active_voices > 0 {
            mixed_sample / active_voices as f32
        } else {
            0.0
        }
    }

    pub fn apply_pitch_bend(&mut self, bend_ratio: f32) {
        self.pitch_bend_ratio = bend_ratio;
        for voice in self.voices.iter_mut() {
            if let Some(note) = voice.note {
                voice.apply_pitch_bend(note, bend_ratio);
            }
        }
    }

    pub fn all_notes_off(&mut self, channel: Option<u8>) {
        for voice in self.voices.iter_mut() {
            if channel.is_none() || voice.channel == channel.unwrap() {
                voice.note_off();
            }
        }
    }

    pub fn fill_audio_buffer(&mut self, buffer: &mut [f32]) {
        for sample in buffer.iter_mut() {
            *sample = self.get_mixed_sample();
        }
    }
}
