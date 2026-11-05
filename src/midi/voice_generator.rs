use crate::audio::drum_synth::{midi_note_to_drum_type, DrumSampler};
use crate::percussion::RINGER_FIRST_NOTE;
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
    key_down: bool,
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
            key_down: false,
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
        self.key_down = true;

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
        self.key_down = false;
        // Synth will just stop producing sound when we stop calling it
        // Drum will finish its envelope naturally
    }

    /// Fades out a held drum-channel sound (dial tone, ringer). The voice frees
    /// itself in `get_sample` once the fade finishes.
    #[inline]
    pub fn release(&mut self) {
        if self.active_type == VoiceTypeId::Drum {
            self.drum.release();
        }
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
                // Compress velocity into 0.5..1.0 so soft hits stay audible while
                // hard hits still accent — a linear 0..1 curve made low-velocity
                // drums vanish under the sustained synth.
                let drum_vel = 0.5 + 0.5 * (self.velocity as f32 * (1.0 / 127.0));
                // Drum makeup gain > 1.0: percussion is a short transient, so it
                // needs a higher peak than a sustained synth note to feel equally
                // loud. (Was 0.75, then 1.4.)
                sample * drum_vel * 2.5
            }
        }
    }

    #[inline]
    pub fn apply_pitch_bend(&mut self, bend_ratio: f32) {
        if let Some(note) = self.note {
            if self.active_type == VoiceTypeId::Synth {
                let base_freq = crate::midi::notes::frequency(note);
                let bent_freq = base_freq * bend_factor(bend_ratio);
                self.synth.set_freq(bent_freq);
            }
            // Drums don't pitch bend
        }
    }
}

/// MIDI channel routing (0-indexed):
///   0 (MIDI ch 1)  — voice effects only, no audio
///   1 (MIDI ch 2)  — audio only, no voice effects
///   2 (MIDI ch 3)  — auto-switch: plays audio normally, but in Vocode/Harmony it
///                    goes silent-as-synth and drives the vocal effect instead
///   3 (MIDI ch 4)  — designated vocal lead only; never a raw synth
///   9 (MIDI ch 10) — drums, no voice effects
const VOICE_CTRL_CHANNEL: u8 = 0;
/// Explicit MIDI lead (channel 4), silent as a raw synth.
pub const LEAD_CTRL_CHANNEL: u8 = 3;
fn is_voice_ctrl(channel: u8) -> bool {
    channel == VOICE_CTRL_CHANNEL || channel == LEAD_CTRL_CHANNEL
}

/// The auto-switch channel (MIDI ch 3). Feeds the vocal-effect frequency cache and
/// is silenced as raw synth in Vocode/Harmony (see `get_mixed_sample`).
const AUTO_SWITCH_CHANNEL: u8 = 2;

#[inline(always)]
fn is_sound_only_channel(channel: u8) -> bool {
    channel == 1 || channel == 9
}

/// Fixed default wheel range: two semitones in either direction. RPN bend
/// sensitivity is not implemented; keep this explicit rather than a linear Hz shift.
fn bend_factor(bend: f32) -> f32 {
    libm::exp2f(bend.clamp(-1.0, 1.0) / 6.0)
}

pub struct VoiceManager<const MAX_VOICES: usize> {
    voices: [HybridVoice; MAX_VOICES],
    pitch_bend: [f32; 16],
    sustain: [bool; 16],
    /// Frequencies from voices whose channel isn't voice-ctrl-only or sound-only — fed to vocal effects DSP.
    cached_frequencies: [f32; MAX_VOICES],
    /// Frequencies from the backing/lead control channels, without raw synths.
    voice_ctrl_freqs: [f32; MAX_VOICES],
    voice_ctrl_notes: [Option<u8>; MAX_VOICES],
    voice_ctrl_down: [bool; MAX_VOICES],
    voice_ctrl_channels: [u8; MAX_VOICES],
    lead_routing: bool,
}

impl<const MAX_VOICES: usize> VoiceManager<MAX_VOICES> {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            voices: core::array::from_fn(|_| HybridVoice::new(sample_rate)),
            pitch_bend: [0.0; 16],
            sustain: [false; 16],
            cached_frequencies: [0.0; MAX_VOICES],
            voice_ctrl_freqs: [0.0; MAX_VOICES],
            voice_ctrl_notes: [None; MAX_VOICES],
            voice_ctrl_down: [false; MAX_VOICES],
            voice_ctrl_channels: [0; MAX_VOICES],
            lead_routing: false,
        }
    }

    pub fn set_waveform(&mut self, waveform: Waveform) {
        for voice in &mut self.voices {
            voice.set_waveform(waveform);
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: u8, channel: u8) {
        if channel >= 16 || MAX_VOICES == 0 {
            return;
        }
        if velocity == 0 {
            self.note_off(note, channel);
            return;
        }
        let frequency =
            crate::midi::notes::frequency(note) * bend_factor(self.pitch_bend[channel as usize]);

        // Ch 0: voice effects only — track frequency, no audio voice.
        if is_voice_ctrl(channel) {
            if channel == LEAD_CTRL_CHANNEL {
                self.lead_routing = true;
            }
            let i = self
                .voice_ctrl_notes
                .iter()
                .enumerate()
                .position(|(i, n)| *n == Some(note) && self.voice_ctrl_channels[i] == channel)
                .or_else(|| self.voice_ctrl_notes.iter().position(Option::is_none))
                .or_else(|| self.voice_ctrl_down.iter().position(|down| !down))
                .or_else(|| {
                    self.voice_ctrl_channels
                        .iter()
                        .position(|&ch| ch != LEAD_CTRL_CHANNEL)
                })
                .or_else(|| (channel == LEAD_CTRL_CHANNEL).then_some(0));
            let Some(i) = i else {
                return;
            };
            self.voice_ctrl_notes[i] = Some(note);
            self.voice_ctrl_channels[i] = channel;
            self.voice_ctrl_freqs[i] = frequency;
            self.voice_ctrl_down[i] = true;
            return;
        }

        let needed_type = if channel == 9 {
            VoiceTypeId::Drum
        } else {
            VoiceTypeId::Synth
        };

        // Whether this channel feeds the vocal effects frequency cache.
        // Sound-only channels and drum-map note numbers must not retune the
        // frequency cache used by Pitch Control / Harmony / Vocode.
        let update_cache = !is_sound_only_channel(channel);

        // 1. Check for retrigger
        for i in 0..MAX_VOICES {
            if self.voices[i].is_playing(note, channel) {
                self.voices[i].note_on(note, velocity, channel, frequency);
                self.cached_frequencies[i] = 0.0;
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
                self.cached_frequencies[i] = 0.0;
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
                self.cached_frequencies[i] = 0.0;
                if update_cache {
                    self.update_frequency_cache(i, frequency);
                }
                return;
            }
        }

        // 4. Voice stealing - steal voice 0
        self.voices[0].note_on(note, velocity, channel, frequency);
        self.cached_frequencies[0] = 0.0;
        if update_cache {
            self.update_frequency_cache(0, frequency);
        }
    }

    #[inline(always)]
    fn update_frequency_cache(&mut self, idx: usize, base_freq: f32) {
        self.cached_frequencies[idx] = base_freq;
    }

    pub fn note_off(&mut self, note: u8, channel: u8) {
        if channel >= 16 {
            return;
        }
        if is_voice_ctrl(channel) {
            for i in 0..MAX_VOICES {
                if self.voice_ctrl_notes[i] == Some(note) && self.voice_ctrl_channels[i] == channel
                {
                    self.voice_ctrl_down[i] = false;
                    if self.sustain[channel as usize] {
                        return;
                    }
                    self.voice_ctrl_notes[i] = None;
                    self.voice_ctrl_freqs[i] = 0.0;
                    return;
                }
            }
            return;
        }

        // GM spec: drum channel ignores NoteOff — voices self-release via envelope.
        // Ringers and dial tones (82+) are held sounds and fade out on release.
        if channel == 9 {
            if note >= RINGER_FIRST_NOTE {
                for voice in &mut self.voices {
                    if voice.is_playing(note, channel) {
                        voice.release();
                        return;
                    }
                }
            }
            return;
        }

        for i in 0..MAX_VOICES {
            if self.voices[i].is_playing(note, channel) {
                self.voices[i].key_down = false;
                if self.sustain[channel as usize] {
                    return;
                }
                self.voices[i].note_off();
                self.cached_frequencies[i] = 0.0;
                return;
            }
        }
    }

    /// Mix all active voices. When `mute_melody_synth` is set (Vocode / Harmony
    /// profiles), pitched synth voices on the auto-switch channel (MIDI ch 3) stay
    /// silent — they still track pitch for the vocal-effect frequency cache, so
    /// you hear your processed voice instead of a raw synth on top. Drums and any
    /// other channel still sound.
    #[inline(always)]
    pub fn get_mixed_sample(
        &mut self,
        mute_melody_synth: bool,
        melody_gain: f32,
        drum_gain: f32,
    ) -> f32 {
        let (melody, drums) =
            self.get_mixed_sources_with_gains(mute_melody_synth, melody_gain, drum_gain);
        melody + drums
    }

    pub fn get_mixed_sources(&mut self) -> (f32, f32) {
        self.get_mixed_sources_with_gains(false, 1.0, 1.0)
    }

    pub fn get_mixed_sources_with_gains(
        &mut self,
        mute_melody_synth: bool,
        melody_gain: f32,
        drum_gain: f32,
    ) -> (f32, f32) {
        let mut melody = 0.0f32;
        let mut drums = 0.0f32;
        let mut count = 0usize;

        for voice in &mut self.voices {
            let s = voice.get_sample();
            if s != 0.0 {
                if mute_melody_synth
                    && voice.channel == AUTO_SWITCH_CHANNEL
                    && voice.type_id() == VoiceTypeId::Synth
                {
                    continue;
                }
                let scaled = match voice.type_id() {
                    VoiceTypeId::Synth => s * melody_gain,
                    VoiceTypeId::Drum => s * drum_gain,
                };
                if scaled != 0.0 {
                    match voice.type_id() {
                        VoiceTypeId::Synth => melody += scaled,
                        VoiceTypeId::Drum => drums += scaled,
                    }
                    count += 1;
                }
            }
        }

        if count > 0 {
            (melody * INV_SQRT[count], drums * INV_SQRT[count])
        } else {
            (0.0, 0.0)
        }
    }

    pub fn apply_pitch_bend(&mut self, bend_ratio: f32) {
        for channel in 0..16 {
            self.apply_channel_pitch_bend(channel, bend_ratio);
        }
    }

    pub fn apply_channel_pitch_bend(&mut self, channel: u8, bend_ratio: f32) {
        if channel >= 16 || !bend_ratio.is_finite() {
            return;
        }
        let bend_ratio = bend_ratio.clamp(-1.0, 1.0);
        self.pitch_bend[channel as usize] = bend_ratio;
        if is_voice_ctrl(channel) {
            for i in 0..MAX_VOICES {
                if let Some(note) =
                    self.voice_ctrl_notes[i].filter(|_| self.voice_ctrl_channels[i] == channel)
                {
                    self.voice_ctrl_freqs[i] =
                        crate::midi::notes::frequency(note) * bend_factor(bend_ratio);
                }
            }
        }
        for i in 0..MAX_VOICES {
            if self.voices[i].channel != channel {
                continue;
            }
            self.voices[i].apply_pitch_bend(bend_ratio);
            self.cached_frequencies[i] = 0.0;
            if let Some(note) = self.voices[i]
                .note
                .filter(|_| !is_sound_only_channel(channel))
            {
                let base_freq = crate::midi::notes::frequency(note);
                self.cached_frequencies[i] = base_freq * bend_factor(bend_ratio);
            }
        }
    }

    pub fn set_sustain(&mut self, channel: u8, down: bool) {
        if channel >= 16 || channel == 9 {
            return;
        }
        self.sustain[channel as usize] = down;
        if down {
            return;
        }
        if is_voice_ctrl(channel) {
            for i in 0..MAX_VOICES {
                if !self.voice_ctrl_down[i] && self.voice_ctrl_channels[i] == channel {
                    self.voice_ctrl_notes[i] = None;
                    self.voice_ctrl_freqs[i] = 0.0;
                }
            }
        }
        for i in 0..MAX_VOICES {
            if self.voices[i].channel == channel && !self.voices[i].key_down {
                self.voices[i].note_off();
                self.cached_frequencies[i] = 0.0;
            }
        }
    }

    /// MIDI CC123 releases keys while respecting Hold 1; app panic uses the
    /// immediate all_notes_off API below (also used by MIDI CC120).
    pub fn release_channel_notes(&mut self, channel: u8) {
        if channel >= 16 {
            return;
        }
        if is_voice_ctrl(channel) {
            for i in 0..MAX_VOICES {
                if let Some(note) =
                    self.voice_ctrl_notes[i].filter(|_| self.voice_ctrl_channels[i] == channel)
                {
                    self.note_off(note, channel);
                }
            }
        } else {
            for i in 0..MAX_VOICES {
                if self.voices[i].channel == channel {
                    if let Some(note) = self.voices[i].note {
                        self.note_off(note, channel);
                    }
                }
            }
        }
    }

    pub fn all_notes_off(&mut self, channel: Option<u8>) {
        if channel.is_some_and(is_voice_ctrl) {
            for i in 0..MAX_VOICES {
                if Some(self.voice_ctrl_channels[i]) == channel {
                    self.voice_ctrl_notes[i] = None;
                    self.voice_ctrl_freqs[i] = 0.0;
                    self.voice_ctrl_down[i] = false;
                }
            }
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
            self.voice_ctrl_down = [false; MAX_VOICES];
            self.lead_routing = false;
        }
    }

    /// Returns active vocal frequencies, prioritizing an explicitly designated
    /// MIDI lead. Without that role, preserve the legacy cache ordering.
    pub fn get_cached_frequencies(&self) -> [f32; MAX_VOICES] {
        self.collect_vocal_notes().0
    }
    pub fn enable_midi_lead_routing(&mut self) {
        self.lead_routing = true;
    }

    fn collect_vocal_notes(&self) -> ([f32; MAX_VOICES], [bool; MAX_VOICES]) {
        let mut result = [0.0f32; MAX_VOICES];
        let mut lead = [false; MAX_VOICES];
        let mut idx = 0;
        // Designated lead gets capacity before backing; shared unisons appear once.
        for i in 0..MAX_VOICES {
            let f = self.voice_ctrl_freqs[i];
            if self.voice_ctrl_channels[i] == LEAD_CTRL_CHANNEL
                && f > 0.0
                && idx < MAX_VOICES
                && !result[..idx].contains(&f)
            {
                result[idx] = f;
                lead[idx] = true;
                idx += 1;
            }
        }
        for &f in &self.cached_frequencies {
            if f != 0.0 && idx < MAX_VOICES && !result[..idx].contains(&f) {
                result[idx] = f;
                idx += 1;
            }
        }
        for &f in &self.voice_ctrl_freqs {
            if f != 0.0 && idx < MAX_VOICES && !result[..idx].contains(&f) {
                result[idx] = f;
                idx += 1;
            }
        }
        (result, lead)
    }

    /// DSP-relative gains on its equal-power chord shares. Group headroom is
    /// reserved while the MIDI lead rests, avoiding backing jumps.
    pub fn get_harmony_voices(&self, backing_level: u8) -> ([f32; MAX_VOICES], [f32; MAX_VOICES]) {
        let (frequencies, lead) = self.collect_vocal_notes();
        if !self.lead_routing {
            return (frequencies, [1.0; MAX_VOICES]);
        }
        let count = frequencies.iter().filter(|&&f| f > 0.0).count();
        let leads = lead.iter().filter(|&&value| value).count();
        let backs = count - leads;
        let back = backing_level.min(127) as f32 / 127.0;
        let lead_root = libm::sqrtf(leads.max(1) as f32);
        let back_root = libm::sqrtf(backs.max(1) as f32);
        // The DSP gives each unity-gain voice 1/sqrt(count) of the chord's
        // power, so sqrt(count) cancels that and these gains become absolute.
        // The lead group gets power 1 and backing back^2, scaled so a full
        // lead+backing chord exactly fills the chord's power. Shares do not
        // grow when the other group rests, so neither jumps.
        let scale = libm::sqrtf(count as f32) / libm::sqrtf(1.0 + back * back);
        let gains = core::array::from_fn(|i| {
            if frequencies[i] <= 0.0 {
                0.0
            } else if lead[i] {
                scale / lead_root
            } else {
                scale * back / back_root
            }
        });
        (frequencies, gains)
    }
}
