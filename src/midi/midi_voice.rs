use log::info;
use synthphone_e_vocal_dsp::audio::{Oscillator, Waveform};
use crate::audio::drum_synth::{DrumSynth, DrumType};
use crate::midi::voice_generator::{VoiceType, VoiceTypeId, VoiceGenerator};
// Voice management for polyphony

pub struct Voice {
    pub voice_type: VoiceType,
    // None means voice is free
    pub note: Option<u8>,
    pub velocity: u8,
    pub channel: u8,
}

impl Voice {
    // Default to synth, but can be repurposed
      pub fn new(voice_type: VoiceType) -> Self {
        Self {
            voice_type,
            note: None,
            velocity: 0,
            channel: 0,
        }
    }
    
    // Convert voice type on-the-fly when triggered
    pub fn set_voice_type(&mut self, voice_type: VoiceType) {
        self.voice_type = voice_type;
    }

    pub fn set_waveform(&mut self, waveform: Waveform) {
        if let VoiceType::Synth(osc) = &mut self.voice_type {
            osc.set_waveform(waveform);
        }
    }

    pub fn is_free(&self) -> bool {
        self.note.is_none()
    }

    pub fn is_playing(&self, note: u8, channel: u8) -> bool {
        self.note == Some(note) && self.channel == channel
    }

    pub fn type_id(&self) -> VoiceTypeId {
        self.voice_type.type_id()
    }

    pub fn note_on(&mut self, note: u8, velocity: u8, channel: u8) {
        self.note = Some(note);
        self.velocity = velocity;
        self.channel = channel;
        //let frequency = crate::midi::MidiEvent::note_to_frequency(note);
        //self.oscillator.set_freq(frequency);
        self.voice_type.as_trait_mut().trigger(note, velocity);
    }

    pub fn note_off(&mut self) {
        self.voice_type.as_trait_mut().release();
        self.note = None;
        self.velocity = 0;
    }

    pub fn get_sample(&mut self) -> f32 {
        if self.note.is_some() && self.velocity > 0 {
            // Scale by velocity (0-127 -> 0.0-1.0)
            // self.oscillator.next_value() * (self.velocity as f32 / 127.0)
            self.voice_type.as_trait_mut().get_sample() * (self.velocity as f32 / 127.0)
        } else {
            0.0
        }
    }

    pub fn apply_pitch_bend(&mut self, note: u8, bend_ratio: f32) {
        // let base_freq = crate::midi::MidiEvent::note_to_frequency(note);
        // let bent_freq = base_freq * (1.0 + bend_ratio * 0.1); // +/- 10% bend range
        // self.oscillator.set_freq(bent_freq);

        self.voice_type.as_trait_mut().apply_pitch_bend(note, bend_ratio);
    }
}

pub struct VoiceManager<const MAX_VOICES: usize> {
    pub voices: [Voice; MAX_VOICES],
    pub pitch_bend_ratio: f32,
    pub cached_frequencies: [f32; MAX_VOICES],
    sample_rate: f32,  // Store this so we can create voice types on demand
}

impl<const MAX_VOICES: usize> VoiceManager<MAX_VOICES> {
    pub fn new(sample_rate: f32) -> Self {
        // Create array of voices using from_fn
        let voices = core::array::from_fn(|_| {
            Voice::new(VoiceType::new_synth(sample_rate))
        });
        // Initialize frequency cache with zeros (0.0 Hz)
        let cached_frequencies = [0.0; MAX_VOICES];

        Self {
            voices,
            pitch_bend_ratio: 0.0,
            cached_frequencies,
            sample_rate,
        }
    }

    //TODO: skip setting waveform for voices that are not Osc?
    pub fn set_waveform(&mut self, waveform: Waveform) {
        for voice in self.voices.iter_mut() {
            voice.set_waveform(waveform);
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: u8, channel: u8) {
        let frequency = crate::midi::MidiEvent::note_to_frequency(note);
        let needed_type = if channel == 9 {  // MIDI channel 10 (0-indexed = 9)
            VoiceTypeId::Drum
        } else {
            VoiceTypeId::Synth
        };
        
          // Step 1: Check if this note is already playing (retrigger)
        for i in 0..MAX_VOICES {
            if self.voices[i].is_playing(note, channel) {
                self.voices[i].note_on(note, velocity, channel);
                self.voices[i].apply_pitch_bend(note, self.pitch_bend_ratio);
                self.cached_frequencies[i] = frequency * (1.0 + self.pitch_bend_ratio * 0.1);
                return;
            }
        }
        
        // Step 2: Find a free voice of the correct type (prefer matching type)
        for i in 0..MAX_VOICES {
            if self.voices[i].is_free() && self.voices[i].type_id() == needed_type {
                self.voices[i].note_on(note, velocity, channel);
                self.voices[i].apply_pitch_bend(note, self.pitch_bend_ratio);
                self.cached_frequencies[i] = frequency * (1.0 + self.pitch_bend_ratio * 0.1);
                return;
            }
        }
        
        // Step 3: Find ANY free voice and convert it to the type we need
        for i in 0..MAX_VOICES {
            if self.voices[i].is_free() {
                // Convert voice to correct type
                let new_generator = match needed_type {
                    VoiceTypeId::Synth => VoiceType::new_synth(self.sample_rate),
                    VoiceTypeId::Drum => VoiceType::new_drum(self.sample_rate),
                    //VoiceTypeId::Sample => VoiceType::new_sample(self.sample_rate),
                };
                self.voices[i].set_voice_type(new_generator);
                
                self.voices[i].note_on(note, velocity, channel);
                self.voices[i].apply_pitch_bend(note, self.pitch_bend_ratio);
                self.cached_frequencies[i] = frequency * (1.0 + self.pitch_bend_ratio * 0.1);
                return;
            }
        }
        
        // Step 4: No free voices - voice stealing
        info!("Voice stealing - note {} on channel {}", note, channel);
        
        // Convert stolen voice to correct type
        let new_generator = match needed_type {
            VoiceTypeId::Synth => VoiceType::new_synth(self.sample_rate),
            VoiceTypeId::Drum => VoiceType::new_drum(self.sample_rate),
            // VoiceTypeId::Sample => VoiceType::new_sample(self.sample_rate),
        };
        self.voices[0].set_voice_type(new_generator);
        
        self.voices[0].note_on(note, velocity, channel);
        self.voices[0].apply_pitch_bend(note, self.pitch_bend_ratio);
        self.cached_frequencies[0] = frequency * (1.0 + self.pitch_bend_ratio * 0.1);
    }

    pub fn note_off(&mut self, note: u8, channel: u8) {
        for (i, voice) in self.voices.iter_mut().enumerate() {
            if voice.note == Some(note) && voice.channel == channel {
                voice.note_off();
                // Clear frequency cache
                self.cached_frequencies[i] = 0.0;
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
                self.cached_frequencies[i] = bent_freq;
            }
        }
    }

    pub fn all_notes_off(&mut self, channel: Option<u8>) {
        for (i, voice) in self.voices.iter_mut().enumerate() {
            if channel.is_none() || voice.channel == channel.unwrap() {
                voice.note_off();
                // Clear frequency cache
                self.cached_frequencies[i] = 0.0;
            }
        }
    }

    pub fn fill_audio_buffer(&mut self, buffer: &mut [f32]) {
        for sample in buffer.iter_mut() {
            *sample = self.get_mixed_sample();
        }
    }

    /// method to get current frequencies for vocal effects
    /// Returns frequencies in Hz, 0.0 means voice is inactive
    pub fn get_cached_frequencies(&self) -> [f32; MAX_VOICES] {
        let mut frequencies = [0.0; MAX_VOICES];
        for (i, cached_freq) in self.cached_frequencies.iter().enumerate() {
            let freq_bits = cached_freq;
            frequencies[i] = *freq_bits;
        }
        frequencies
    }
}
