// drum_synth.rs - Optimized for embedded real-time audio

use core::f32::consts::PI;
use libm::{expf, sinf};

const TWO_PI: f32 = 2.0 * PI;

/// Drum synthesizer optimized for embedded systems
pub struct DrumSynth {
    sample_rate: f32,
    inv_sample_rate: f32,  // Pre-computed 1/sample_rate
    phase: f32,            // Time in seconds
    sample_count: u32,     // Integer sample counter (more precise)
    drum_type: DrumType,
    // Pre-computed envelope values to avoid expf() calls
    env_state: f32,
    env_decay: f32,
    // Oscillator state for kicks/toms
    osc_phase: f32,
    osc_freq: f32,
    // Noise state (LFSR is faster than sinf-based noise)
    noise_state: u32,
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
            inv_sample_rate: 1.0 / sample_rate,
            phase: 0.0,
            sample_count: 0,
            drum_type,
            env_state: 0.0,
            env_decay: 0.0,
            osc_phase: 0.0,
            osc_freq: 60.0,
            noise_state: 0x12345678,  // Non-zero seed for LFSR
        }
    }

    pub fn trigger(&mut self) {
        self.phase = 0.0;
        self.sample_count = 0;
        self.env_state = 1.0;
        self.osc_phase = 0.0;
        self.noise_state = 0x12345678;
        
        // Pre-compute envelope decay rate per sample
        self.env_decay = match self.drum_type {
            DrumType::Kick => expf(-8.0 * self.inv_sample_rate),
            DrumType::Snare => expf(-12.0 * self.inv_sample_rate),
            DrumType::HiHat => expf(-35.0 * self.inv_sample_rate),
            DrumType::Tom => expf(-6.0 * self.inv_sample_rate),
            DrumType::Clap => expf(-20.0 * self.inv_sample_rate),
            DrumType::Cymbal => expf(-3.0 * self.inv_sample_rate),
        };
        
        // Initial oscillator frequency
        self.osc_freq = match self.drum_type {
            DrumType::Kick => 180.0,  // Start high for pitch sweep
            DrumType::Tom => 270.0,
            _ => 200.0,
        };
    }

    pub fn set_drum_type(&mut self, drum_type: DrumType) {
        self.drum_type = drum_type;
    }

    #[inline(always)]
    pub fn is_finished(&self) -> bool {
        // Use envelope state instead of time comparison
        self.env_state < 0.001
    }

    #[inline(always)]
    pub fn next_value(&mut self) -> f32 {
        if self.env_state < 0.001 {
            return 0.0;
        }

        let output = match self.drum_type {
            DrumType::Kick => self.synth_kick_fast(),
            DrumType::Snare => self.synth_snare_fast(),
            DrumType::HiHat => self.synth_hihat_fast(),
            DrumType::Tom => self.synth_tom_fast(),
            DrumType::Clap => self.synth_clap_fast(),
            DrumType::Cymbal => self.synth_cymbal_fast(),
        };

        // Update envelope (multiply is faster than expf)
        self.env_state *= self.env_decay;
        self.sample_count += 1;
        self.phase += self.inv_sample_rate;

        output
    }

    // === FAST LFSR NOISE (much faster than sinf-based) ===
    #[inline(always)]
    fn fast_noise(&mut self) -> f32 {
        // Galois LFSR - very fast pseudo-random
        let lsb = self.noise_state & 1;
        self.noise_state >>= 1;
        if lsb == 1 {
            self.noise_state ^= 0xB400; // Taps for maximal length
        }
        // Convert to -1.0 to 1.0
        (self.noise_state as f32 / 32768.0) - 1.0
    }

    // === OPTIMIZED KICK ===
    #[inline(always)]
    fn synth_kick_fast(&mut self) -> f32 {
        // Pitch sweep using incremental update (no expf per sample)
        // Approximate exponential decay of pitch
        let pitch_decay = 0.9997_f32;  // Pre-tuned for ~40x decay rate
        self.osc_freq = 60.0 + (self.osc_freq - 60.0) * pitch_decay;
        
        // Update oscillator phase
        self.osc_phase += TWO_PI * self.osc_freq * self.inv_sample_rate;
        if self.osc_phase > TWO_PI {
            self.osc_phase -= TWO_PI;
        }
        
        // Fast sine approximation for kick body
        let sine = fast_sin(self.osc_phase);
        
        // Click component (first ~50 samples)
        let click = if self.sample_count < 50 {
            let click_env = 1.0 - (self.sample_count as f32 / 50.0);
            click_env * click_env * 0.3
        } else {
            0.0
        };
        
        (sine * self.env_state * 0.9 + click) * 0.8
    }

    // === OPTIMIZED SNARE ===
    #[inline(always)]
    fn synth_snare_fast(&mut self) -> f32 {
        // Two separate envelope states
        let body_env = self.env_state;
        // Noise envelope decays slower - use fast inverse sqrt approximation
        let noise_env = self.env_state * fast_sqrt(self.env_state);
        
        // Body oscillator
        self.osc_phase += TWO_PI * 200.0 * self.inv_sample_rate;
        if self.osc_phase > TWO_PI {
            self.osc_phase -= TWO_PI;
        }
        
        let body = fast_sin(self.osc_phase);
        let noise = self.fast_noise();
        
        (body * body_env * 0.4 + noise * noise_env * 0.6) * 0.7
    }

    // === OPTIMIZED HI-HAT ===
    #[inline(always)]
    fn synth_hihat_fast(&mut self) -> f32 {
        // Just filtered noise with fast envelope
        let n1 = self.fast_noise();
        let n2 = self.fast_noise();
        
        // Simple high-pass effect by mixing two noise sources
        let mixed = (n1 + n2 * 0.7) * 0.6;
        
        mixed * self.env_state * 0.5
    }

    // === OPTIMIZED TOM ===
    #[inline(always)]
    fn synth_tom_fast(&mut self) -> f32 {
        // Similar to kick but higher, longer
        let pitch_decay = 0.9998_f32;
        self.osc_freq = 120.0 + (self.osc_freq - 120.0) * pitch_decay;
        
        self.osc_phase += TWO_PI * self.osc_freq * self.inv_sample_rate;
        if self.osc_phase > TWO_PI {
            self.osc_phase -= TWO_PI;
        }
        
        let sine = fast_sin(self.osc_phase);
        sine * self.env_state * 0.75
    }

    // === OPTIMIZED CLAP ===
    #[inline(always)]
    fn synth_clap_fast(&mut self) -> f32 {
        // Multiple bursts - use sample count for timing
        let burst_len = (self.sample_rate * 0.01) as u32;  // 10ms per burst
        let burst_idx = self.sample_count / burst_len;
        
        if burst_idx > 3 {
            return 0.0;
        }
        
        let local_sample = self.sample_count % burst_len;
        let burst_env = 1.0 - (local_sample as f32 / burst_len as f32);
        
        self.fast_noise() * burst_env * self.env_state * 0.6
    }

    // === OPTIMIZED CYMBAL ===
    #[inline(always)]
    fn synth_cymbal_fast(&mut self) -> f32 {
        // Multiple noise sources + metallic ring
        let n1 = self.fast_noise();
        let n2 = self.fast_noise();
        
        // Simple ring oscillator
        self.osc_phase += TWO_PI * 4200.0 * self.inv_sample_rate;
        if self.osc_phase > TWO_PI {
            self.osc_phase -= TWO_PI;
        }
        let ring = fast_sin(self.osc_phase) * 0.15;
        
        let mixed = (n1 + n2 * 0.6) * 0.5 + ring;
        mixed * self.env_state * 0.5
    }
}

/// Fast sine approximation using parabolic approximation
/// Accurate to ~0.1% which is fine for audio
#[inline(always)]
fn fast_sin(x: f32) -> f32 {
    // Normalize to -PI to PI
    let mut x = x;
    while x > PI {
        x -= TWO_PI;
    }
    while x < -PI {
        x += TWO_PI;
    }
    
    // Parabolic approximation
    const B: f32 = 4.0 / PI;
    const C: f32 = -4.0 / (PI * PI);
    
    let y = B * x + C * x * x.abs();
    
    // Extra precision (optional, comment out if too slow)
    const P: f32 = 0.225;
    P * (y * y.abs() - y) + y
}

/// Fast square root approximation (Quake III style)
/// Good enough for audio envelope shaping
#[inline(always)]
fn fast_sqrt(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    // Fast inverse sqrt then invert
    let half = 0.5 * x;
    let mut i = x.to_bits();
    i = 0x5f3759df - (i >> 1);  // Initial guess
    let mut y = f32::from_bits(i);
    y = y * (1.5 - half * y * y);  // One Newton iteration
    x * y  // x * (1/sqrt(x)) = sqrt(x)
}

/// Map MIDI note to drum type (General MIDI standard)
pub fn midi_note_to_drum_type(note: u8) -> Option<DrumType> {
    match note {
        35 | 36 => Some(DrumType::Kick),
        38 | 40 => Some(DrumType::Snare),
        42 | 44 | 46 => Some(DrumType::HiHat),
        41 | 43 | 45 | 47 | 48 | 50 => Some(DrumType::Tom),
        49 | 55 | 57 => Some(DrumType::Cymbal),
        39 => Some(DrumType::Clap),
        _ => None,
    }
}
