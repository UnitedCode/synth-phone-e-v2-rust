use log::info;
use synthphone_e_vocal_dsp::audio::{Oscillator, Waveform};

// Helper functions for atomic f32 storage
fn f32_to_u32(f: f32) -> u32 {
    f.to_bits()
}

fn u32_to_f32(u: u32) -> f32 {
    f32::from_bits(u)
}

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
    // Lock-free frequency cache for vocal effects
    pub cached_frequencies: [core::sync::atomic::AtomicU32; MAX_VOICES],
}

impl<const MAX_VOICES: usize> VoiceManager<MAX_VOICES> {
    pub fn new(sample_rate: f32) -> Self {
        // Create array of voices using from_fn
        let voices = core::array::from_fn(|_| Voice::new(sample_rate));
        // Initialize atomic frequency cache with zeros (0.0 Hz)
        let cached_frequencies = core::array::from_fn(|_| core::sync::atomic::AtomicU32::new(0));
        Self {
            voices,
            pitch_bend_ratio: 0.0,
            cached_frequencies,
        }
    }

    pub fn set_waveform(&mut self, waveform: Waveform) {
        for voice in self.voices.iter_mut() {
            voice.set_waveform(waveform);
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: u8, channel: u8) {
        let frequency = crate::midi::MidiEvent::note_to_frequency(note);

        // First, check if this note is already playing - if so, retrigger it
        for (i, voice) in self.voices.iter_mut().enumerate() {
            if voice.note == Some(note) && voice.channel == channel {
                voice.note_on(note, velocity, channel);
                // Apply current pitch bend
                voice.apply_pitch_bend(note, self.pitch_bend_ratio);
                // Update atomic frequency cache
                self.cached_frequencies[i]
                    .store(f32_to_u32(frequency), core::sync::atomic::Ordering::Relaxed);
                return;
            }
        }

        // Find a free voice
        for (i, voice) in self.voices.iter_mut().enumerate() {
            if voice.is_free() {
                voice.note_on(note, velocity, channel);
                // Apply current pitch bend
                voice.apply_pitch_bend(note, self.pitch_bend_ratio);
                // Update atomic frequency cache
                self.cached_frequencies[i]
                    .store(f32_to_u32(frequency), core::sync::atomic::Ordering::Relaxed);
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
        // Update atomic frequency cache for stolen voice
        self.cached_frequencies[0]
            .store(f32_to_u32(frequency), core::sync::atomic::Ordering::Relaxed);
    }

    pub fn note_off(&mut self, note: u8, channel: u8) {
        for (i, voice) in self.voices.iter_mut().enumerate() {
            if voice.note == Some(note) && voice.channel == channel {
                voice.note_off();
                // Clear atomic frequency cache
                self.cached_frequencies[i].store(0, core::sync::atomic::Ordering::Relaxed);
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
        for (i, voice) in self.voices.iter_mut().enumerate() {
            if let Some(note) = voice.note {
                voice.apply_pitch_bend(note, bend_ratio);
                // Update cached frequency with pitch bend applied
                let base_freq = crate::midi::MidiEvent::note_to_frequency(note);
                let bent_freq = base_freq * (1.0 + bend_ratio * 0.1);
                self.cached_frequencies[i]
                    .store(f32_to_u32(bent_freq), core::sync::atomic::Ordering::Relaxed);
            }
        }
    }

    pub fn all_notes_off(&mut self, channel: Option<u8>) {
        for (i, voice) in self.voices.iter_mut().enumerate() {
            if channel.is_none() || voice.channel == channel.unwrap() {
                voice.note_off();
                // Clear atomic frequency cache
                self.cached_frequencies[i].store(0, core::sync::atomic::Ordering::Relaxed);
            }
        }
    }

    pub fn fill_audio_buffer(&mut self, buffer: &mut [f32]) {
        for sample in buffer.iter_mut() {
            *sample = self.get_mixed_sample();
        }
    }

    /// Lock-free method to get current frequencies for vocal effects
    /// Returns frequencies in Hz, 0.0 means voice is inactive
    pub fn get_cached_frequencies(&self) -> [f32; MAX_VOICES] {
        let mut frequencies = [0.0; MAX_VOICES];
        for (i, cached_freq) in self.cached_frequencies.iter().enumerate() {
            let freq_bits = cached_freq.load(core::sync::atomic::Ordering::Relaxed);
            frequencies[i] = u32_to_f32(freq_bits);
        }
        frequencies
    }
}
