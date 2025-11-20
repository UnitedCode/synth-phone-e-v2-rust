pub struct SamplePlayer {
    samples: &'static [f32],
    position: f32,
    playback_rate: f32,  // 1.0 = normal speed
    sample_rate: f32,
    looping: bool,
}

impl SamplePlayer {
    pub fn new(samples: &'static [f32], sample_rate: f32) -> Self {
        Self {
            samples,
            position: 0.0,
            playback_rate: 1.0,
            sample_rate,
            looping: false,
        }
    }
    
    pub fn trigger(&mut self) {
        self.position = 0.0;
    }
    
    pub fn set_pitch(&mut self, freq_ratio: f32) {
        self.playback_rate = freq_ratio;
    }
    
    pub fn next_value(&mut self) -> f32 {
        if self.position >= self.samples.len() as f32 {
            if self.looping {
                self.position = 0.0;
            } else {
                return 0.0;
            }
        }
        
        // Linear interpolation between samples
        let index = self.position as usize;
        let frac = self.position - index as f32;
        
        let sample1 = self.samples.get(index).copied().unwrap_or(0.0);
        let sample2 = self.samples.get(index + 1).copied().unwrap_or(sample1);
        
        let output = sample1 + (sample2 - sample1) * frac;
        
        self.position += self.playback_rate;
        
        output
    }
}

// 8-bit drum samples (normalized to -1.0 to 1.0)
pub const KICK_DRUM: &[f32] = &[
    // 8-bit samples: (value as f32 / 128.0) - 1.0
];

pub const SNARE_DRUM: &[f32] = &[
    // ...
];