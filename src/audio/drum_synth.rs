// src/audio/sample_player.rs
// Pure synthesis - no flash reads

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum DrumType {
    Kick,
    Snare,
    HiHat,
    Tom,
    Clap,
    Cymbal,
}

pub struct DrumSampler {
    drum_type: DrumType,
    sample_count: u32,
    max_samples: u32,

    // State
    phase: u32,          // Fixed-point phase (16.16)
    phase_inc: u32,      // Fixed-point increment
    base_phase_inc: u32, // Starting pitch (for decay)
    env: u16,            // 16-bit envelope
    noise: u16,          // LFSR state
}

impl DrumSampler {
    pub fn new() -> Self {
        Self {
            drum_type: DrumType::Kick,
            sample_count: 0,
            max_samples: 0,
            phase: 0,
            phase_inc: 0,
            base_phase_inc: 10737000, // Default tom ~120Hz
            env: 0,
            noise: 0xACE1,
        }
    }

    pub fn set_drum_type(&mut self, drum_type: DrumType) {
        self.drum_type = drum_type;
    }

    /// Set pitch for toms (MIDI note number)  
    pub fn set_pitch(&mut self, note: u8) {
        // phase_inc = freq * 89478 (for 32-bit phase at 48kHz)
        self.base_phase_inc = match note {
            41 => 7158000,  // Low Floor Tom ~80Hz
            43 => 8948000,  // High Floor Tom ~100Hz
            45 => 10737000, // Low Tom ~120Hz
            47 => 13421000, // Low-Mid Tom ~150Hz
            48 => 16106000, // Hi-Mid Tom ~180Hz
            50 => 17895000, // High Tom ~200Hz
            _ => 10737000,  // Default ~120Hz
        };
    }

    pub fn trigger(&mut self) {
        self.sample_count = 0;
        self.phase = 0;
        self.noise = 0xACE1;
        self.env = 0xFFFF;

        match self.drum_type {
            DrumType::Kick => {
                self.max_samples = 4800;
                // No pitch sweep - just steady low frequency
                self.phase_inc = 900000; // ~40Hz steady
                self.base_phase_inc = 900000;
            }
            DrumType::Snare => {
                self.max_samples = 3800;
                self.phase_inc = 273000; // ~200Hz
                self.base_phase_inc = 273000;
            }
            DrumType::HiHat => {
                self.max_samples = 2400;
                self.phase_inc = 0;
                self.base_phase_inc = 0;
            }
            DrumType::Tom => {
                self.max_samples = 4000;
                // NO pitch bend for toms - just start at the target pitch
                self.phase_inc = self.base_phase_inc;
            }
            DrumType::Clap => {
                self.max_samples = 2900;
                self.phase_inc = 0;
                self.base_phase_inc = 0;
            }
            DrumType::Cymbal => {
                self.max_samples = 7200;
                self.phase_inc = 0;
                self.base_phase_inc = 0;
            }
        }
    }

    #[inline(always)]
    pub fn next_value(&mut self) -> f32 {
        if self.sample_count >= self.max_samples || self.env < 100 {
            return 0.0;
        }
        self.sample_count += 1;

        // LFSR noise - always update
        let lsb = self.noise & 1;
        self.noise >>= 1;
        if lsb == 1 {
            self.noise ^= 0xB400;
        }
        // Noise: -1.0 to 1.0 range (as i16: -32768 to 32767)
        let noise = ((self.noise & 0x7FFF) as i16).wrapping_sub(16384) << 1;

        // Square wave from phase
        let square: i16 = if self.phase < 0x80000000 {
            32000
        } else {
            -32000
        };

        // Update phase
        self.phase = self.phase.wrapping_add(self.phase_inc);

        // Mix based on drum type
        let raw: i32 = match self.drum_type {
            DrumType::Kick => {
                // Pitch decay - slower
                if self.phase_inc > 81900 {
                    self.phase_inc = self.phase_inc.saturating_sub(100);
                }
                // Env decay - slower (65530/65536 ≈ 0.9999)
                self.env = ((self.env as u32 * 65530) >> 16) as u16;
                square as i32
            }
            DrumType::Snare => {
                self.env = ((self.env as u32 * 65500) >> 16) as u16;
                (square as i32 / 3) + (noise as i32 * 2 / 3)
            }
            DrumType::HiHat => {
                self.env = ((self.env as u32 * 65400) >> 16) as u16;
                noise as i32
            }
            DrumType::Tom => {
                // NO pitch decay - toms hold their pitch
                // Just envelope decay
                self.env = ((self.env as u32 * 65510) >> 16) as u16;

                // Use triangle wave instead of square for cleaner tone
                // Triangle has fewer harmonics, sounds more like a drum head
                let tri: i32 = if self.phase < 0x80000000 {
                    // Rising: 0 to max
                    ((self.phase >> 16) as i32) - 16384
                } else {
                    // Falling: max to 0
                    16384 - (((self.phase - 0x80000000) >> 16) as i32)
                };
                tri * 2 // Scale up
            }
            DrumType::Clap => {
                self.env = ((self.env as u32 * 65450) >> 16) as u16;
                noise as i32
            }
            DrumType::Cymbal => {
                self.env = ((self.env as u32 * 65510) >> 16) as u16;
                noise as i32
            }
        };

        // Apply envelope (env is 0-65535, raw is ~-32000 to 32000)
        let out = (raw * self.env as i32) >> 16;

        // Convert to float (-1.0 to 1.0)
        (out as f32) / 32768.0
    }
}

impl Default for DrumSampler {
    fn default() -> Self {
        Self::new()
    }
}

/// Map MIDI note to drum type
pub fn midi_note_to_drum_type(note: u8) -> Option<DrumType> {
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
