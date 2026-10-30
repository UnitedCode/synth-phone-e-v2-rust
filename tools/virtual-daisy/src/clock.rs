//! RM0433 clock arithmetic. PLL lock/startup transients and bus wait states are not simulated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rate {
    pub numerator: u64,
    pub denominator: u64,
}
impl Rate {
    fn new(numerator: u64, denominator: u64) -> Self {
        fn gcd(mut a: u64, mut b: u64) -> u64 {
            while b != 0 {
                (a, b) = (b, a % b);
            }
            a
        }
        let divisor = gcd(numerator, denominator);
        Self {
            numerator: numerator / divisor,
            denominator: denominator / divisor,
        }
    }
    pub fn hz(self) -> f64 {
        self.numerator as f64 / self.denominator as f64
    }
    fn divided(self, divisor: u64) -> Self {
        Self::new(self.numerator, self.denominator * divisor)
    }
}

pub struct ClockTree {
    registers: [u32; 0x100 / 4],
    fractions: [u32; 3],
    phase: u128,
    last_rate: Option<Rate>,
    cycle_time: u64,
    cycle_fraction: u128,
    cycle_count: u32,
    dwt_control: u32,
    dwt_locked: bool,
    cycle_rate: Option<Rate>,
    trace_enabled: bool,
}
impl Default for ClockTree {
    fn default() -> Self {
        let mut registers = [0; 64];
        registers[0] = 1;
        registers[0x28 / 4] = 0x02020200;
        registers[0x2c / 4] = 0x01ff0000;
        for offset in [0x30, 0x38, 0x40] {
            registers[offset / 4] = 0x01010280;
        }
        Self {
            registers,
            fractions: [0; 3],
            phase: 0,
            last_rate: None,
            cycle_time: 0,
            cycle_fraction: 0,
            cycle_count: 0,
            dwt_control: 0,
            dwt_locked: true,
            cycle_rate: None,
            trace_enabled: false,
        }
    }
}
impl ClockTree {
    pub fn trace_enable(&mut self, enabled: bool, now: u64) -> Result<(), &'static str> {
        self.sync_cycles(now)?;
        self.trace_enabled = enabled;
        Ok(())
    }
    pub fn sync_cycles(&mut self, now: u64) -> Result<(), &'static str> {
        let elapsed = now
            .checked_sub(self.cycle_time)
            .ok_or("virtual clock moved backwards")?;
        self.cycle_time = now;
        if self.dwt_control & 1 != 0 && self.trace_enabled {
            let rate = self.core_rate()?.ok_or("DWT has no core clock")?;
            if self.cycle_rate != Some(rate) {
                self.cycle_fraction = 0;
                self.cycle_rate = Some(rate);
            }
            let denominator = u128::from(rate.denominator) * 1_000_000_000;
            self.cycle_fraction += u128::from(elapsed) * u128::from(rate.numerator);
            self.cycle_count = self
                .cycle_count
                .wrapping_add((self.cycle_fraction / denominator) as u32);
            self.cycle_fraction %= denominator;
        }
        Ok(())
    }
    pub fn dwt(&mut self, offset: u64, value: Option<u32>, now: u64) -> Result<u32, &'static str> {
        self.sync_cycles(now)?;
        match (offset, value) {
            (0xfb0, Some(v)) => {
                self.dwt_locked = v != 0xc5acce55;
                return Ok(0);
            }
            (0xfb4, None) => return Ok(1 | (u32::from(self.dwt_locked) << 1)),
            (0, Some(v)) if !self.dwt_locked => self.dwt_control = v,
            (4, Some(v)) if !self.dwt_locked => {
                self.cycle_count = v;
                self.cycle_fraction = 0;
            }
            (0 | 4, Some(_)) => {}
            (0 | 4, None) => {}
            _ => return Err("DWT register delegated to Renode"),
        }
        Ok(if offset == 0 {
            self.dwt_control
        } else {
            self.cycle_count
        })
    }
    pub fn read(&self, offset: u64) -> u32 {
        self.registers
            .get(offset as usize / 4)
            .copied()
            .unwrap_or(0)
    }
    pub fn write(&mut self, offset: u64, mut value: u32) {
        let index = offset as usize / 4;
        if index >= self.registers.len() {
            return;
        }
        let old = self.registers[index];
        if offset == 0x28 {
            for pll in 0..3 {
                if self.read(0) & (1 << (24 + pll * 2)) != 0 {
                    let mask = 63 << (4 + pll * 8);
                    value = (value & !mask) | (old & mask);
                }
            }
            if self.read(0) & 0x15000000 != 0 {
                value = (value & !3) | (old & 3);
            }
        }
        if [0x30, 0x38, 0x40].contains(&offset) {
            let pll = (offset - 0x30) / 8;
            if self.read(0) & (1 << (24 + pll * 2)) != 0 {
                return;
            }
            value &= 0x7f7fffff;
        }
        if [0x34, 0x3c, 0x44].contains(&offset) {
            // Writes update FRACR; only the rising edge loads SH_REG.
            value &= 0xfff8;
        }
        if offset == 0x2c {
            value &= 0x01ff0fff;
            for pll in 0..3 {
                if self.read(0) & (1 << (24 + pll * 2)) != 0 {
                    let locked = (0xe << (pll * 4)) | (7 << (16 + pll * 3));
                    value = (value & !locked) | (old & locked);
                }
                let bit = 1 << (pll * 4);
                if old & bit == 0 && value & bit != 0 {
                    self.fractions[pll] = self.read(0x34 + pll as u64 * 8) >> 3;
                }
            }
        }
        self.registers[index] = value;
    }
    fn hsi(&self) -> u64 {
        64_000_000 >> ((self.read(0) >> 3) & 3)
    }
    fn pll(&self, pll: usize, output: usize) -> Result<Option<Rate>, &'static str> {
        let cr = self.read(0);
        let config = self.read(0x2c);
        if cr & (1 << (24 + pll * 2)) == 0 || config & (1 << (16 + pll * 3 + output)) == 0 {
            return Ok(None);
        }
        let select = self.read(0x28);
        let source = match select & 3 {
            0 if cr & 1 != 0 => self.hsi(),
            1 if cr & (1 << 7) != 0 => 4_000_000,
            2 if cr & (1 << 16) != 0 => 16_000_000,
            _ => return Ok(None),
        };
        let m = (select >> (4 + pll * 8)) & 63;
        if m == 0 {
            return Ok(None);
        }
        let div = self.read(0x30 + pll as u64 * 8);
        let n = (div & 511) + 1;
        if n < 4 {
            return Err("PLL multiplier must be 4..512");
        }
        let shift = [9, 16, 24][output];
        let divisor = ((div >> shift) & 127) + 1;
        let fraction = self.fractions[pll];
        Ok(Some(Rate::new(
            source * (u64::from(n) * 8192 + u64::from(fraction)),
            u64::from(m) * 8192 * u64::from(divisor),
        )))
    }
    pub fn core_rate(&self) -> Result<Option<Rate>, &'static str> {
        let rate = match self.read(0x10) & 7 {
            0 => Some(Rate::new(self.hsi(), 1)),
            1 => Some(Rate::new(4_000_000, 1)),
            2 => Some(Rate::new(16_000_000, 1)),
            3 => self.pll(0, 0)?,
            _ => return Err("reserved SYSCLK source"),
        };
        let divisor = [1, 1, 1, 1, 1, 1, 1, 1, 2, 4, 8, 16, 64, 128, 256, 512]
            [((self.read(0x18) >> 8) & 15) as usize];
        Ok(rate.map(|r| r.divided(divisor)))
    }
    /// 0=core, 1=HCLK, 2..5=PCLK1..4, 6..7=APB1/2 timer kernel clocks.
    pub fn domain_rate(&self, domain: u32) -> Result<Option<Rate>, &'static str> {
        let core = self.core_rate()?;
        if domain == 0 {
            return Ok(core);
        }
        let hpre = [1, 1, 1, 1, 1, 1, 1, 1, 2, 4, 8, 16, 64, 128, 256, 512]
            [(self.read(0x18) & 15) as usize];
        let hclk = core.map(|rate| rate.divided(hpre));
        if domain == 1 {
            return Ok(hclk);
        }
        let code = match domain {
            2 | 6 => (self.read(0x1c) >> 4) & 7,
            3 | 7 => (self.read(0x1c) >> 8) & 7,
            4 => (self.read(0x18) >> 4) & 7,
            5 => (self.read(0x20) >> 4) & 7,
            _ => return Err("unknown clock domain"),
        };
        let apb = [1, 1, 1, 1, 2, 4, 8, 16][code as usize];
        let divisor = if domain < 6 {
            apb
        } else if self.read(0x10) & (1 << 15) != 0 {
            (apb / 4).max(1)
        } else {
            (apb / 2).max(1)
        };
        Ok(hclk.map(|rate| rate.divided(divisor)))
    }
    pub fn sample_rate(&self, cr1: u32, frcr: u32) -> Result<Rate, &'static str> {
        let kernel = match self.read(0x50) & 7 {
            0 => self.pll(0, 1)?,
            1 => self.pll(1, 0)?,
            2 => self.pll(2, 0)?,
            3 => return Err("external I2S_CKIN is not connected by this profile"),
            4 => match (self.read(0x4c) >> 28) & 3 {
                0 if self.read(0) & 3 != 0 => Some(Rate::new(self.hsi(), 1)),
                1 if self.read(0) & ((1 << 7) | (1 << 9)) != 0 => Some(Rate::new(4_000_000, 1)),
                2 if self.read(0) & (1 << 16) != 0 => Some(Rate::new(16_000_000, 1)),
                0..=2 => None,
                _ => return Err("reserved CKPER source"),
            },
            _ => return Err("reserved SAI kernel source"),
        };
        let kernel = kernel.unwrap_or(Rate::new(0, 1));
        let divider = u64::from(((cr1 >> 20) & 63).max(1));
        let frame = if cr1 & (1 << 19) == 0 {
            256 * (1 + u64::from((cr1 >> 26) & 1))
        } else {
            u64::from((frcr & 255) + 1)
        };
        Ok(kernel.divided(divider * frame))
    }
    pub fn advance(&mut self, cr1: u32, frcr: u32, nanoseconds: u64) -> Result<u32, &'static str> {
        if cr1 & (1 << 16) == 0 || self.read(0xf0) & (1 << 22) == 0 {
            self.phase = 0;
            self.last_rate = None;
            return Ok(0);
        }
        let rate = self.sample_rate(cr1, frcr)?;
        if self.last_rate != Some(rate) {
            self.phase = 0;
            self.last_rate = Some(rate);
        }
        let denominator = u128::from(rate.denominator) * 1_000_000_000;
        self.phase += u128::from(rate.numerator) * u128::from(nanoseconds);
        let frames = self.phase / denominator;
        self.phase %= denominator;
        u32::try_from(frames).map_err(|_| "clock advance too large")
    }
    pub fn stats(&self, cr1: u32, frcr: u32) -> Result<String, &'static str> {
        let rate = self.sample_rate(cr1, frcr)?;
        let mut stats=format!("hse_hz=16000000\ncore_hz={}\nsai_rate_numerator={}\nsai_rate_denominator={}\nsai_hz={:.9}\nevent_quantum_ns=1000\npll_lock_transient=UNMODELED\nrcc_cr={:#010x}\nrcc_pllckselr={:#010x}\nrcc_pllcfgr={:#010x}\nrcc_pll3divr={:#010x}\nrcc_pll3fracr={:#010x}\nsai_cr1={cr1:#010x}\nsai_frcr={frcr:#010x}\n",self.core_rate()?.map_or(0.0,Rate::hz),rate.numerator,rate.denominator,rate.hz(),self.read(0),self.read(0x28),self.read(0x2c),self.read(0x40),self.read(0x44));
        for (name, domain) in [
            ("hclk", 1),
            ("pclk1", 2),
            ("pclk2", 3),
            ("pclk3", 4),
            ("pclk4", 5),
            ("apb1_timer", 6),
            ("apb2_timer", 7),
        ] {
            stats.push_str(&format!(
                "{name}_hz={}\n",
                self.domain_rate(domain)?.map_or(0.0, Rate::hz)
            ));
        }
        Ok(stats)
    }
}
