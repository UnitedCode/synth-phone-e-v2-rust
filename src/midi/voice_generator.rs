// voice_generator.rs - Optimized for real-time audio

use crate::audio::drum_synth::{DrumSampler, DrumType};
use synthphone_e_vocal_dsp::audio::{Oscillator, Waveform};

/// Map MIDI note to drum type (General MIDI standard)
fn note_to_drum_type(note: u8) -> Option<DrumType> {
    match note {
        35 | 36 => Some(DrumType::Kick),
        38 | 40 => Some(DrumType::Snare),
        42 | 44 | 46 => Some(DrumType::HiHat),
        41 | 43 | 45 | 47 | 48 | 50 => Some(DrumType::Tom),
        39 => Some(DrumType::Clap),
        49 | 55 | 57 => Some(DrumType::Cymbal),
        _ => None,
    }
}

/// Voice that holds BOTH generators, only one active at a time
/// This avoids dynamic dispatch and runtime allocation
pub struct HybridVoice {
    synth: Oscillator,
    drum: DrumSampler,
    active_type: VoiceTypeId,
    note: Option<u8>,
    velocity: u8,
    channel: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceTypeId {
    Synth,
    Drum,
}

impl HybridVoice {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            synth: Oscillator::new(440.0, sample_rate, Waveform::Triangle),
            drum: DrumSampler::new(),
            active_type: VoiceTypeId::Synth,
            note: None,
            velocity: 0,
            channel: 0,
        }
    }

    #[inline(always)]
    pub fn is_free(&self) -> bool {
        self.note.is_none()
    }

    #[inline(always)]
    pub fn is_playing(&self, note: u8, channel: u8) -> bool {
        self.note == Some(note) && self.channel == channel
    }

    #[inline(always)]
    pub fn type_id(&self) -> VoiceTypeId {
        self.active_type
    }

    pub fn set_waveform(&mut self, waveform: Waveform) {
        self.synth.set_waveform(waveform);
    }

    /// Note on with automatic type selection based on channel
    #[inline]
    pub fn note_on(&mut self, note: u8, velocity: u8, channel: u8) {
        self.note = Some(note);
        self.velocity = velocity;
        self.channel = channel;

        // Channel 9 (0-indexed) = MIDI channel 10 = drums
        if channel == 9 {
            self.active_type = VoiceTypeId::Drum;
            if let Some(drum_type) = note_to_drum_type(note) {
                self.drum.set_drum_type(drum_type);
                self.drum.trigger();
            }
        } else {
            self.active_type = VoiceTypeId::Synth;
            let freq = crate::midi::MidiEvent::note_to_frequency(note);
            self.synth.set_freq(freq);
        }
    }

    #[inline]
    pub fn note_off(&mut self) {
        self.note = None;
        self.velocity = 0;
        // Synth will just stop producing sound when we stop calling it
        // Drum will finish its envelope naturally
    }

    /// Get next sample - NO dynamic dispatch, just a match
    #[inline(always)]
    pub fn get_sample(&mut self) -> f32 {
        if self.velocity == 0 {
            return 0.0;
        }

        let vel_scale = self.velocity as f32 * (1.0 / 127.0);

        match self.active_type {
            VoiceTypeId::Synth => {
                if self.note.is_some() {
                    self.synth.next_value() * vel_scale
                } else {
                    0.0
                }
            }
            VoiceTypeId::Drum => {
                let sample = self.drum.next_value();
                // Drums auto-release when sample returns 0 consistently
                if sample == 0.0 && self.note.is_some() {
                    self.note = None;
                    self.velocity = 0;
                }
                sample * vel_scale
            }
        }
    }

    #[inline]
    pub fn apply_pitch_bend(&mut self, bend_ratio: f32) {
        if let Some(note) = self.note {
            if self.active_type == VoiceTypeId::Synth {
                let base_freq = crate::midi::MidiEvent::note_to_frequency(note);
                let bent_freq = base_freq * (1.0 + bend_ratio * 0.1);
                self.synth.set_freq(bent_freq);
            }
            // Drums don't pitch bend
        }
    }
}

pub struct VoiceManager<const MAX_VOICES: usize> {
    voices: [HybridVoice; MAX_VOICES],
    pitch_bend_ratio: f32,
    cached_frequencies: [f32; MAX_VOICES],
}

impl<const MAX_VOICES: usize> VoiceManager<MAX_VOICES> {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            voices: core::array::from_fn(|_| HybridVoice::new(sample_rate)),
            pitch_bend_ratio: 0.0,
            cached_frequencies: [0.0; MAX_VOICES],
        }
    }

    pub fn set_waveform(&mut self, waveform: Waveform) {
        for voice in &mut self.voices {
            voice.set_waveform(waveform);
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: u8, channel: u8) {
        let frequency = crate::midi::MidiEvent::note_to_frequency(note);
        let needed_type = if channel == 9 {
            VoiceTypeId::Drum
        } else {
            VoiceTypeId::Synth
        };

        // 1. Check for retrigger
        for i in 0..MAX_VOICES {
            if self.voices[i].is_playing(note, channel) {
                self.voices[i].note_on(note, velocity, channel);
                self.update_frequency_cache(i, frequency);
                return;
            }
        }

        // 2. Find free voice (prefer matching type)
        for i in 0..MAX_VOICES {
            if self.voices[i].is_free() && self.voices[i].type_id() == needed_type {
                self.voices[i].note_on(note, velocity, channel);
                self.update_frequency_cache(i, frequency);
                return;
            }
        }

        // 3. Find ANY free voice
        for i in 0..MAX_VOICES {
            if self.voices[i].is_free() {
                self.voices[i].note_on(note, velocity, channel);
                self.update_frequency_cache(i, frequency);
                return;
            }
        }

        // 4. Voice stealing - steal voice 0
        self.voices[0].note_on(note, velocity, channel);
        self.update_frequency_cache(0, frequency);
    }

    #[inline(always)]
    fn update_frequency_cache(&mut self, idx: usize, base_freq: f32) {
        self.cached_frequencies[idx] = base_freq * (1.0 + self.pitch_bend_ratio * 0.1);
    }

    pub fn note_off(&mut self, note: u8, channel: u8) {
        for i in 0..MAX_VOICES {
            if self.voices[i].is_playing(note, channel) {
                self.voices[i].note_off();
                self.cached_frequencies[i] = 0.0;
                return;
            }
        }
    }

    #[inline(always)]
    pub fn get_mixed_sample(&mut self) -> f32 {
        let mut sum = 0.0f32;
        let mut count = 0u32;

        for voice in &mut self.voices {
            let s = voice.get_sample();
            if s != 0.0 {
                sum += s;
                count += 1;
            }
        }

        if count > 0 {
            // sqrt(N) normalization: each added voice increases loudness by ~3dB,
            // matching how real polyphonic instruments blend acoustically.
            // No clamp needed — handler mixes this at 0.1 gain, so max output is well within range.
            sum / libm::sqrtf(count as f32)
        } else {
            0.0
        }
    }

    pub fn apply_pitch_bend(&mut self, bend_ratio: f32) {
        self.pitch_bend_ratio = bend_ratio;
        for i in 0..MAX_VOICES {
            self.voices[i].apply_pitch_bend(bend_ratio);
            if let Some(note) = self.voices[i].note {
                let base_freq = crate::midi::MidiEvent::note_to_frequency(note);
                self.cached_frequencies[i] = base_freq * (1.0 + bend_ratio * 0.1);
            }
        }
    }

    pub fn all_notes_off(&mut self, channel: Option<u8>) {
        for i in 0..MAX_VOICES {
            if channel.is_none() || self.voices[i].channel == channel.unwrap() {
                self.voices[i].note_off();
                self.cached_frequencies[i] = 0.0;
            }
        }
    }

    pub fn get_cached_frequencies(&self) -> [f32; MAX_VOICES] {
        self.cached_frequencies
    }
}