use crate::audio::drum_synth::{DrumSynth, DrumType};
use synthphone_e_vocal_dsp::audio::{Oscillator, Waveform};
use crate::audio::sample_player::SamplePlayer;

// Common interface for all voice types
pub trait VoiceGenerator {
    fn trigger(&mut self, note: u8, velocity: u8);
    fn release(&mut self);
    fn get_sample(&mut self) -> f32;
    fn is_finished(&self) -> bool;
    fn apply_pitch_bend(&mut self, note: u8, bend_ratio: f32);
}

pub enum VoiceType {
    Synth(Oscillator),  // This is the library's Oscillator
    Drum(DrumSynth),
    //Sample(SamplePlayer),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceTypeId {
    Synth,
    Drum,
    //Sample, 
}

impl VoiceType {
    #[inline]
    pub fn as_trait_mut(&mut self) -> &mut dyn VoiceGenerator {
        match self {
            Self::Synth(s) => s,
            Self::Drum(d) => d,
            //Self::Sample(p) => p,
        }
    }
    
    #[inline]
    pub fn as_trait(&self) -> &dyn VoiceGenerator {
        match self {
            Self::Synth(s) => s,
            Self::Drum(d) => d,
            //Self::Sample(p) => p,
        }
    }

    pub fn type_id(&self) -> VoiceTypeId {
        match self {
            VoiceType::Synth(_) => VoiceTypeId::Synth,
            VoiceType::Drum(_) => VoiceTypeId::Drum,
            //VoiceType::Sample(_) => VoiceTypeId::Sample,
        }
    }
    
    pub fn new_synth(sample_rate: f32) -> Self {
        Self::Synth(Oscillator::new(440.0, sample_rate, Waveform::Triangle))
    }
    
    pub fn new_drum(sample_rate: f32) -> Self {
        Self::Drum(DrumSynth::new(sample_rate, DrumType::Kick))
    }

    // pub fn new_sample(sample_rate: f32) -> Self {
    //    Self::Sample(SamplePlayer::new([0.0, 0.0], sample_rate));
    // }
}

// Implement the trait for the library's Oscillator
impl VoiceGenerator for Oscillator {
    fn trigger(&mut self, note: u8, _velocity: u8) {
        let freq = crate::midi::MidiEvent::note_to_frequency(note);
        self.set_freq(freq);
    }
    
    fn release(&mut self) {
        // Sustained oscillators don't need release
    }
    
    fn get_sample(&mut self) -> f32 {
        self.next_value()
    }
    
    fn is_finished(&self) -> bool {
        false  // Oscillators sustain indefinitely
    }
    
    fn apply_pitch_bend(&mut self, note: u8, bend_ratio: f32) {
        let base_freq = crate::midi::MidiEvent::note_to_frequency(note);
        let bent_freq = base_freq * (1.0 + bend_ratio * 0.1);
        self.set_freq(bent_freq);
    }
}

// Implement for DrumSynth
impl VoiceGenerator for DrumSynth {
    fn trigger(&mut self, note: u8, _velocity: u8) {
        use crate::audio::drum_synth::midi_note_to_drum_type;
        
        if let Some(drum_type) = midi_note_to_drum_type(note) {
            self.set_drum_type(drum_type);
            DrumSynth::trigger(self);  // Call the trigger method on DrumSynth
        }
    }
    
    fn release(&mut self) {}
    
    fn get_sample(&mut self) -> f32 {
        self.next_value()
    }
    
    fn is_finished(&self) -> bool {
        DrumSynth::is_finished(self)
    }
    
    fn apply_pitch_bend(&mut self, _note: u8, _bend_ratio: f32) {}
}

