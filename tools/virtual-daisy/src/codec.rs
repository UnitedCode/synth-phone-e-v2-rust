//! Documented WM8731 ideal line gain and DC-removal stage, not a complete analog ADC.
use crate::clock::Rate;

/// WM8731 Tables18/20/21/22: exact ratios, without rounding USB rates to 44.1k.
#[derive(Debug)]
pub struct SampleClocks {
    pub adc: Rate,
    pub dac: Rate,
    pub clkout: Rate,
    pub filter: u32,
}
pub struct Sampling {
    base: u64,
    input_div: u64,
    output_div: u64,
    adc: (u64, u64),
    dac: (u64, u64),
    pub filter: u32,
}
impl Sampling {
    pub fn decode(value: u32) -> Result<Self, &'static str> {
        if value & !255 != 0 {
            return Err("reserved WM8731 sampling bit");
        }
        let usb = value & 1 != 0;
        let bosr = value & 2 != 0;
        let sr = (value >> 2) & 15;
        let valid = if usb {
            if bosr {
                [8, 9, 10, 11, 15].contains(&sr)
            } else {
                [0, 1, 2, 3, 6, 7].contains(&sr)
            }
        } else {
            [0, 1, 2, 3, 6, 7, 8, 9, 10, 11, 15].contains(&sr)
        };
        if !valid {
            return Err("reserved WM8731 sample clock ratio");
        }
        let base = match (usb, bosr) {
            (false, false) => 256,
            (false, true) => 384,
            (true, false) => 250,
            (true, true) => 272,
        };
        let low = if sr & 8 != 0 { (2, 11) } else { (1, 6) };
        let (adc, dac) = match sr & 7 {
            0 => ((1, 1), (1, 1)),
            1 => ((1, 1), low),
            2 => (low, (1, 1)),
            3 => (low, low),
            6 => ((2, 3), (2, 3)),
            7 => ((2, 1), (2, 1)),
            _ => unreachable!(),
        };
        let filter = if sr == 7 {
            if usb {
                3
            } else {
                2
            }
        } else if sr == 15 {
            2
        } else if usb && !bosr {
            0
        } else {
            1
        };
        Ok(Self {
            base,
            input_div: if value & 64 != 0 { 2 } else { 1 },
            output_div: if value & 128 != 0 { 2 } else { 1 },
            adc,
            dac,
            filter,
        })
    }
    pub fn clocks(&self, mclk: Rate) -> Result<SampleClocks, &'static str> {
        if mclk.numerator == 0 || mclk.denominator == 0 {
            return Err("WM8731 MCLK stopped or invalid");
        }
        let scale = |n: u64, d: u64| -> Result<Rate, &'static str> {
            let numerator = mclk
                .numerator
                .checked_mul(n)
                .ok_or("codec clock overflow")?;
            let denominator = mclk
                .denominator
                .checked_mul(d)
                .ok_or("codec clock overflow")?;
            Ok(Rate::new(numerator, denominator))
        };
        Ok(SampleClocks {
            adc: scale(self.adc.0, self.base * self.input_div * self.adc.1)?,
            dac: scale(self.dac.0, self.base * self.input_div * self.dac.1)?,
            clkout: scale(1, self.input_div * self.output_div)?,
            filter: self.filter,
        })
    }
    pub fn delays(&self) -> (usize, usize) {
        match self.filter {
            0 => (12, 11),
            1 => (20, 18),
            2 => (3, 5),
            3 => (6, 5),
            _ => unreachable!(),
        }
    }
}
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

/// Passband-only group-delay surrogate for all four WM8731 converter filter
/// types (Table32). It supplies no stop-band filtering or transition waveform.
pub struct PassbandDelay {
    adc: [[f32; 2]; 20],
    dac: [[f32; 2]; 18],
    adc_index: usize,
    dac_index: usize,
    adc_length: usize,
    dac_length: usize,
}
impl Default for PassbandDelay {
    fn default() -> Self {
        Self {
            adc: [[0.0; 2]; 20],
            dac: [[0.0; 2]; 18],
            adc_index: 0,
            dac_index: 0,
            adc_length: 20,
            dac_length: 18,
        }
    }
}
impl PassbandDelay {
    pub fn adc(&mut self, sample: [f32; 2], frames: usize) -> [f32; 2] {
        if self.adc_length != frames {
            self.adc.fill([0.0; 2]);
            self.adc_index = 0;
            self.adc_length = frames;
        }
        let old = std::mem::replace(&mut self.adc[self.adc_index], sample);
        self.adc_index = (self.adc_index + 1) % frames;
        old
    }
    pub fn dac(&mut self, sample: [f32; 2], frames: usize) -> [f32; 2] {
        if self.dac_length != frames {
            self.dac.fill([0.0; 2]);
            self.dac_index = 0;
            self.dac_length = frames;
        }
        let old = std::mem::replace(&mut self.dac[self.dac_index], sample);
        self.dac_index = (self.dac_index + 1) % frames;
        old
    }
}
