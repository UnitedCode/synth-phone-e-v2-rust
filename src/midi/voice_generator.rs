use crate::audio::drum_synth::{midi_note_to_drum_type, DrumSampler};
use synthphone_e_vocal_dsp::audio::{Oscillator, Waveform};

// Precomputed 1/sqrt(n) for n = 1..=8, indexed by active voice count
const INV_SQRT: [f32; 9] = [
    0.0,        // unused (count = 0 handled separately)
    1.0,        // 1/sqrt(1)
    0.70710678, // 1/sqrt(2)
    0.57735027, // 1/sqrt(3)
    0.5,        // 1/sqrt(4)
    0.44721360, // 1/sqrt(5)
    0.40824829, // 1/sqrt(6)
    0.37796447, // 1/sqrt(7)
    0.35355339, // 1/sqrt(8)
];

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

    /// Note on with automatic type selection based on channel.
    /// `freq` must be pre-computed by the caller (avoids double table lookup).
    #[inline]
    pub fn note_on(&mut self, note: u8, velocity: u8, channel: u8, freq: f32) {
        self.note = Some(note);
        self.velocity = velocity;
        self.channel = channel;

        // Channel 9 (0-indexed) = MIDI channel 10 = drums
        if channel == 9 {
            self.active_type = VoiceTypeId::Drum;
            if let Some(drum_type) = midi_note_to_drum_type(note) {
                self.drum.set_drum_type(drum_type);
                self.drum.trigger();
            }
        } else {
            self.active_type = VoiceTypeId::Synth;
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
                if self.drum.is_finished() {
                    self.note = None;
                    self.velocity = 0;
                }
                sample * vel_scale * 0.9
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

/// MIDI channel routing (0-indexed):
///   0 (MIDI ch 1)  — voice effects only, no audio
///   1 (MIDI ch 2)  — audio only, no voice effects
///   2 (MIDI ch 3)  — audio only, no voice effects
///   9 (MIDI ch 10) — drums, no voice effects
const VOICE_CTRL_CHANNEL: u8 = 0;

#[inline(always)]
fn is_sound_only_channel(channel: u8) -> bool {
    channel == 1 || channel == 2 || channel == 9
}

pub struct VoiceManager<const MAX_VOICES: usize> {
    voices: [HybridVoice; MAX_VOICES],
    pitch_bend_ratio: f32,
    /// Frequencies from voices whose channel isn't voice-ctrl-only or sound-only — fed to vocal effects DSP.
    cached_frequencies: [f32; MAX_VOICES],
    /// Frequencies from ch 0 (voice-ctrl-only) — also fed to vocal effects DSP.
    voice_ctrl_freqs: [f32; MAX_VOICES],
    voice_ctrl_notes: [Option<u8>; MAX_VOICES],
}

impl<const MAX_VOICES: usize> VoiceManager<MAX_VOICES> {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            voices: core::array::from_fn(|_| HybridVoice::new(sample_rate)),
            pitch_bend_ratio: 0.0,
            cached_frequencies: [0.0; MAX_VOICES],
            voice_ctrl_freqs: [0.0; MAX_VOICES],
            voice_ctrl_notes: [None; MAX_VOICES],
        }
    }

    pub fn set_waveform(&mut self, waveform: Waveform) {
        for voice in &mut self.voices {
            voice.set_waveform(waveform);
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: u8, channel: u8) {
        let frequency = crate::midi::MidiEvent::note_to_frequency(note);

        // Ch 0: voice effects only — track frequency, no audio voice.
        if channel == VOICE_CTRL_CHANNEL {
            for i in 0..MAX_VOICES {
                if self.voice_ctrl_notes[i] == Some(note) || self.voice_ctrl_notes[i].is_none() {
                    self.voice_ctrl_notes[i] = Some(note);
                    self.voice_ctrl_freqs[i] = frequency;
                    return;
                }
            }
            return;
        }

        let needed_type = if channel == 9 {
            VoiceTypeId::Drum
        } else {
            VoiceTypeId::Synth
        };

        // Whether this channel feeds the vocal effects frequency cache.
        let update_cache = !is_sound_only_channel(channel);

        // 1. Check for retrigger
        for i in 0..MAX_VOICES {
            if self.voices[i].is_playing(note, channel) {
                self.voices[i].note_on(note, velocity, channel, frequency);
                if update_cache {
                    self.update_frequency_cache(i, frequency);
                }
                return;
            }
        }

        // 2. Find free voice (prefer matching type)
        for i in 0..MAX_VOICES {
            if self.voices[i].is_free() && self.voices[i].type_id() == needed_type {
                self.voices[i].note_on(note, velocity, channel, frequency);
                if update_cache {
                    self.update_frequency_cache(i, frequency);
                }
                return;
            }
        }

        // 3. Find ANY free voice
        for i in 0..MAX_VOICES {
            if self.voices[i].is_free() {
                self.voices[i].note_on(note, velocity, channel, frequency);
                if update_cache {
                    self.update_frequency_cache(i, frequency);
                }
                return;
            }
        }

        // 4. Voice stealing - steal voice 0
        self.voices[0].note_on(note, velocity, channel, frequency);
        if update_cache {
            self.update_frequency_cache(0, frequency);
        }
    }

    #[inline(always)]
    fn update_frequency_cache(&mut self, idx: usize, base_freq: f32) {
        self.cached_frequencies[idx] = base_freq * (1.0 + self.pitch_bend_ratio * 0.1);
    }

    pub fn note_off(&mut self, note: u8, channel: u8) {
        if channel == VOICE_CTRL_CHANNEL {
            for i in 0..MAX_VOICES {
                if self.voice_ctrl_notes[i] == Some(note) {
                    self.voice_ctrl_notes[i] = None;
                    self.voice_ctrl_freqs[i] = 0.0;
                    return;
                }
            }
            return;
        }

        // GM spec: drum channel ignores NoteOff — voices self-release via envelope
        if channel == 9 {
            return;
        }

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
        let mut count = 0usize;

        for voice in &mut self.voices {
            let s = voice.get_sample();
            if s != 0.0 {
                sum += s;
                count += 1;
            }
        }

        if count > 0 {
            sum * INV_SQRT[count]
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
        if channel == Some(VOICE_CTRL_CHANNEL) {
            self.voice_ctrl_notes = [None; MAX_VOICES];
            self.voice_ctrl_freqs = [0.0; MAX_VOICES];
            return;
        }

        for i in 0..MAX_VOICES {
            if channel.is_none() || self.voices[i].channel == channel.unwrap() {
                self.voices[i].note_off();
                self.cached_frequencies[i] = 0.0;
            }
        }

        if channel.is_none() {
            self.voice_ctrl_notes = [None; MAX_VOICES];
            self.voice_ctrl_freqs = [0.0; MAX_VOICES];
        }
    }

    /// Returns all active frequencies for the vocal effects DSP —
    /// non-sound-only voice frequencies followed by ch 0 voice-ctrl frequencies.
    pub fn get_cached_frequencies(&self) -> [f32; MAX_VOICES] {
        let mut result = [0.0f32; MAX_VOICES];
        let mut idx = 0;
        for &f in &self.cached_frequencies {
            if f != 0.0 && idx < MAX_VOICES {
                result[idx] = f;
                idx += 1;
            }
        }
        for &f in &self.voice_ctrl_freqs {
            if f != 0.0 && idx < MAX_VOICES {
                result[idx] = f;
                idx += 1;
            }
        }
        result
    }
}
