use crate::wave_tables::{SAW_TABLE, SINE_TABLE, SQUARE_TABLE, TRIANGLE_TABLE};

#[derive(Copy, Clone)]
pub enum Waveform {
    Sine,
    Saw,
    Square,
    Triangle,
}

pub struct Oscillator {
    phase: u32,     // 0x0000_0000..=0xFFFF_FFFF maps to 0..2π
    phase_inc: u32, // Δphase per sample
    sample_rate: f32,
    waveform: Waveform,
    freq: f32, // keep for reference/debug
}

impl Oscillator {
    pub fn new(freq: f32, sample_rate: f32, waveform: Waveform) -> Self {
        let mut o = Self {
            phase: 0,
            phase_inc: 0,
            sample_rate,
            waveform,
            freq,
        };
        o.set_freq(freq);
        o
    }

    #[inline]
    pub fn set_waveform(&mut self, w: Waveform) {
        self.waveform = w;
    }

    #[inline]
    pub fn set_freq(&mut self, freq: f32) {
        self.freq = freq;
        // phase_inc = freq / Fs * 2^32
        self.phase_inc = ((freq / self.sample_rate) * (u32::MAX as f32)) as u32;
    }

    /// Return next sample
    #[inline]
    pub fn next(&mut self) -> f32 {
        self.phase = self.phase.wrapping_add(self.phase_inc);

        match self.waveform {
            Waveform::Sine => self.lut(&SINE_TABLE),
            Waveform::Saw => self.lut(&SAW_TABLE),
            Waveform::Square => self.lut(&SQUARE_TABLE),
            Waveform::Triangle => self.lut(&TRIANGLE_TABLE),
        }
    }

    /// Linear-interpolated lookup
    #[inline(always)]
    fn lut(&self, table: &[f32; 1024]) -> f32 {
        const TABLE_BITS: u32 = 10; // 2^10 = 1024
        const FRAC_MASK: u32 = (1 << (32 - TABLE_BITS)) - 1; // 0x003F_FFFF

        let idx = (self.phase >> (32 - TABLE_BITS)) as usize;
        let frac = (self.phase & FRAC_MASK) as f32 * (1.0 / FRAC_MASK as f32);

        let a = table[idx];
        let b = table[(idx + 1) & 1023]; // wrap
        a + (b - a) * frac
    }
}
