// In your main project, create: src/audio/drum_synth.rs

use libm::{expf, floorf, sinf};

/// Drum synthesizer for percussive sounds
pub struct DrumSynth {
    sample_rate: f32,
    phase: f32,
    drum_type: DrumType,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum DrumType {
    Kick,
    Snare,
    HiHat,
    Tom,
    Clap,
    Cymbal,
}

impl DrumSynth {
    pub fn new(sample_rate: f32, drum_type: DrumType) -> Self {
        Self {
            sample_rate,
            phase: 0.0,
            drum_type,
        }
    }
    
    /// Trigger the drum sound (reset to beginning)
    pub fn trigger(&mut self) {
        self.phase = 0.0;
    }
    
    /// Change drum type
    pub fn set_drum_type(&mut self, drum_type: DrumType) {
        self.drum_type = drum_type;
    }
    
    /// Check if the drum sound has finished playing
    pub fn is_finished(&self) -> bool {
        match self.drum_type {
            DrumType::Kick | DrumType::Snare | DrumType::HiHat | DrumType::Clap => {
                self.phase > 0.5
            }
            DrumType::Tom => self.phase > 1.0,
            DrumType::Cymbal => self.phase > 3.0,
        }
    }
    
    /// Get the next audio sample
    pub fn next_value(&mut self) -> f32 {
        if self.is_finished() {
            return 0.0;
        }
        
        let output = match self.drum_type {
            DrumType::Kick => self.synthesize_kick(),
            DrumType::Snare => self.synthesize_snare(),
            DrumType::HiHat => self.synthesize_hihat(),
            DrumType::Tom => self.synthesize_tom(),
            DrumType::Clap => self.synthesize_clap(),
            DrumType::Cymbal => self.synthesize_cymbal(),
        };
        
        // Advance phase (time)
        self.phase += 1.0 / self.sample_rate;
        
        output
    }
    
    // === KICK DRUM ===
    // Classic 808-style kick: pitch-swept sine with tight envelope
    fn synthesize_kick(&self) -> f32 {
        let t = self.phase;
        
        // Amplitude envelope (tight decay)
        let env = expf(-8.0 * t);
        
        // Pitch sweep: starts high, drops quickly
        let pitch_hz = 60.0 + 120.0 * expf(-40.0 * t);
        
        // Generate sine wave at swept pitch
        let osc_phase = 2.0 * core::f32::consts::PI * pitch_hz * t;
        let sine = sinf(osc_phase);
        
        // Add a bit of click at the start
        let click = expf(-100.0 * t) * 0.3;
        
        (sine * env * 0.9 + click) * 0.8
    }
    
    // === SNARE DRUM ===
    // Mix of tonal component (body) and noise (snares)
    fn synthesize_snare(&self) -> f32 {
        let t = self.phase;
        
        // Two-stage envelope
        let env1 = expf(-15.0 * t); // Body decay
        let env2 = expf(-8.0 * t);  // Snare buzz decay
        
        // Tonal body (around 200Hz with harmonics)
        let body = sinf(2.0 * core::f32::consts::PI * 200.0 * t);
        let harmonic = sinf(2.0 * core::f32::consts::PI * 350.0 * t) * 0.5;
        
        // Noise (simulates snare wires)
        let noise = Self::fast_noise(t * 12345.6789);
        
        // Mix: body + harmonics + filtered noise
        let tonal = (body + harmonic) * env1 * 0.4;
        let rattle = noise * env2 * 0.6;
        
        (tonal + rattle) * 0.7
    }
    
    // === HI-HAT ===
    // High-frequency noise with very fast decay
    fn synthesize_hihat(&self) -> f32 {
        let t = self.phase;
        
        // Very fast decay
        let env = expf(-35.0 * t);
        
        // Multi-frequency noise for metallic sound
        let noise1 = Self::fast_noise(t * 98765.4321);
        let noise2 = Self::fast_noise(t * 45678.9012);
        let noise3 = Self::fast_noise(t * 23456.7890);
        
        // Mix and filter
        let mixed = (noise1 + noise2 * 0.7 + noise3 * 0.5) / 2.2;
        
        mixed * env * 0.5
    }
    
    // === TOM ===
    // Similar to kick but higher pitch, longer decay
    fn synthesize_tom(&self) -> f32 {
        let t = self.phase;
        
        // Medium decay
        let env = expf(-6.0 * t);
        
        // Pitch sweep (configurable base pitch)
        let base_pitch = 120.0; // Mid tom frequency
        let pitch_hz = base_pitch + 150.0 * expf(-20.0 * t);
        
        let sine = sinf(2.0 * core::f32::consts::PI * pitch_hz * t);
        
        // Add overtone for depth
        let overtone = sinf(2.0 * core::f32::consts::PI * pitch_hz * 2.1 * t) * 0.3;
        
        (sine + overtone) * env * 0.75
    }
    
    // === CLAP ===
    // Multiple short bursts of noise
    fn synthesize_clap(&self) -> f32 {
        let t = self.phase;
        
        // Create multiple "slaps" with slight delays
        let slap1 = Self::noise_burst(t, 0.0, 15.0);
        let slap2 = Self::noise_burst(t, 0.01, 20.0);
        let slap3 = Self::noise_burst(t, 0.02, 25.0);
        let slap4 = Self::noise_burst(t, 0.03, 30.0);
        
        (slap1 + slap2 + slap3 + slap4) * 0.6
    }
    
    // === CRASH CYMBAL ===
    // Complex metallic noise with slower decay
    fn synthesize_cymbal(&self) -> f32 {
        let t = self.phase;
        
        // Slower decay than hi-hat
        let env = expf(-3.0 * t);
        
        // Multiple noise sources for complex timbre
        let noise1 = Self::fast_noise(t * 87654.3210);
        let noise2 = Self::fast_noise(t * 65432.1098);
        let noise3 = Self::fast_noise(t * 43210.9876);
        let noise4 = Self::fast_noise(t * 21098.7654);
        
        // Add some metallic ringing (high sine waves)
        let ring = sinf(2.0 * core::f32::consts::PI * 4200.0 * t) * 0.15;
        
        let mixed = (noise1 + noise2 * 0.8 + noise3 * 0.6 + noise4 * 0.4) / 2.8 + ring;
        
        mixed * env * 0.5
    }
    
    // === HELPER FUNCTIONS ===
    
    /// Fast pseudo-random noise generator (-1 to 1)
    #[inline(always)]
    fn fast_noise(seed: f32) -> f32 {
        let x = seed * 12.9898;
        let y = seed * 78.233;
        let value = sinf(x + y) * 43758.5453;
        2.0 * (value - floorf(value)) - 1.0
    }
    
    /// Helper for clap: single noise burst
    #[inline(always)]
    fn noise_burst(t: f32, delay: f32, decay_rate: f32) -> f32 {
        if t < delay {
            return 0.0;
        }
        let local_t = t - delay;
        let env = expf(-decay_rate * local_t);
        Self::fast_noise(local_t * 54321.0987) * env
    }
}

/// Helper function to map MIDI notes to drum types (General MIDI standard)
pub fn midi_note_to_drum_type(note: u8) -> Option<DrumType> {
    match note {
        35 | 36 => Some(DrumType::Kick),      // Bass Drum
        38 | 40 => Some(DrumType::Snare),     // Snare
        42 | 44 | 46 => Some(DrumType::HiHat), // Hi-Hats
        41 | 43 | 45 | 47 | 48 | 50 => Some(DrumType::Tom), // Toms
        49 | 55 | 57 => Some(DrumType::Cymbal), // Cymbals
        39 => Some(DrumType::Clap),           // Hand Clap
        _ => None,
    }
}