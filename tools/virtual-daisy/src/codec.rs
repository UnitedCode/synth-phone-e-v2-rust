//! Documented WM8731 ideal line gain and DC-removal stage, not a complete analog ADC.
#[derive(Default)]
pub struct LineStage {
    offset: [f64; 2],
}
impl LineStage {
    pub fn process(&mut self, sample: [f32; 2], volume: [u32; 2], digital: u32) -> [f32; 2] {
        std::array::from_fn(|channel| {
            let gain = 10_f64.powf((f64::from(volume[channel] & 31) - 23.0) * 1.5 / 20.0);
            let input = if volume[channel] & 0x80 != 0 {
                0.0
            } else {
                (f64::from(sample[channel]) * gain).clamp(-1.0, 1.0)
            };
            if digital & 1 == 0 {
                let output = input - self.offset[channel];
                self.offset[channel] += 0.0005 * output;
                output as f32
            } else {
                if digital & 0x10 == 0 {
                    self.offset[channel] = 0.0;
                }
                (input - self.offset[channel]) as f32
            }
        })
    }
}

/// Passband-only group-delay surrogate for the WM8731 normal-mode Type1
/// converters (datasheet Table32). It supplies no stop-band filtering.
pub struct PassbandDelay {
    adc: [[f32; 2]; 20],
    dac: [[f32; 2]; 18],
    adc_index: usize,
    dac_index: usize,
}
impl Default for PassbandDelay {
    fn default() -> Self {
        Self {
            adc: [[0.0; 2]; 20],
            dac: [[0.0; 2]; 18],
            adc_index: 0,
            dac_index: 0,
        }
    }
}
impl PassbandDelay {
    pub fn adc(&mut self, sample: [f32; 2]) -> [f32; 2] {
        let old = std::mem::replace(&mut self.adc[self.adc_index], sample);
        self.adc_index = (self.adc_index + 1) % 20;
        old
    }
    pub fn dac(&mut self, sample: [f32; 2]) -> [f32; 2] {
        let old = std::mem::replace(&mut self.dac[self.dac_index], sample);
        self.dac_index = (self.dac_index + 1) % 18;
        old
    }
}
