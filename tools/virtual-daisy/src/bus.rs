//! RM0433 programmed SDRAM timing bounds. These are not CPU stall estimates.
use crate::clock::Rate;
pub struct SdramTiming {
    clock: Rate,
    divider: u64,
    width: u64,
    cas: u64,
    pipe: u64,
    trcd: u64,
    trp: u64,
    trc: u64,
    tmrd: u64,
    refresh: u64,
}
impl SdramTiming {
    pub fn decode(
        control: u32,
        timing: u32,
        refresh: u32,
        kernel: Rate,
    ) -> Result<Self, &'static str> {
        let divider = match (control >> 10) & 3 {
            2 => 2,
            3 => 3,
            _ => return Err("SDRAM clock disabled or reserved divider"),
        };
        let width = match (control >> 4) & 3 {
            0 => 1,
            1 => 2,
            2 => 4,
            _ => return Err("reserved SDRAM bus width"),
        };
        let cas = u64::from((control >> 7) & 3);
        let pipe = u64::from((control >> 13) & 3);
        if cas == 0 || pipe == 3 || kernel.numerator == 0 || kernel.denominator == 0 {
            return Err("invalid SDRAM CAS, pipeline or kernel clock");
        }
        let field = |shift: u32| u64::from(((timing >> shift) & 15_u32) + 1);
        let refresh = u64::from((refresh >> 1) & 8191);
        if (1..41).contains(&refresh) {
            return Err("SDRAM COUNT must be zero or at least 41");
        }
        Ok(Self {
            divider,
            clock: Rate {
                numerator: kernel.numerator,
                denominator: kernel.denominator * divider,
            },
            width,
            cas,
            pipe,
            trcd: field(24),
            trp: field(20),
            trc: field(12),
            tmrd: field(0),
            refresh,
        })
    }
    fn ns(&self, cycles: u64) -> u64 {
        (u128::from(cycles) * u128::from(self.clock.denominator) * 1_000_000_000)
            .div_ceil(u128::from(self.clock.numerator)) as u64
    }
    pub fn read_min_ns(
        &self,
        bytes: u64,
        open_row: bool,
        conflicting_row: bool,
    ) -> Result<u64, &'static str> {
        if bytes == 0 || (open_row && conflicting_row) {
            return Err("invalid SDRAM transfer");
        }
        let cycles = self.cas + bytes.div_ceil(self.width) - 1
            + if open_row { 0 } else { self.trcd }
            + if conflicting_row { self.trp } else { 0 };
        // RM0433 SDCR.RPIPE is in fmc_ker_ck cycles; other terms use SDCLK.
        Ok((u128::from(cycles * self.divider + self.pipe)
            * u128::from(self.clock.denominator / self.divider)
            * 1_000_000_000)
            .div_ceil(u128::from(self.clock.numerator)) as u64)
    }
    pub fn refresh_interval_ns(&self) -> u64 {
        self.ns(self.refresh)
    }
    pub fn refresh_busy_ns(&self, count: u32) -> u64 {
        self.ns(self.trc * u64::from(count))
    }
    pub fn report(&self) -> String {
        format!("scope=PROGRAMMED_TIMING_LOWER_BOUNDS\nsdram_clock_hz={}\nbus_width_bytes={}\ncas_cycles={}\nread_pipe_kernel_cycles={}\ntrcd_cycles={}\ntrp_cycles={}\ntrc_cycles={}\ntmrd_cycles={}\nread_word_open_row_min_ns={}\nread_word_precharged_row_min_ns={}\nread_word_conflicting_row_min_ns={}\nread_cacheline_precharged_row_min_ns={}\nrefresh_interval_ns={}\nrefresh_busy_min_ns={}\nmode_register_quiet_min_ns={}\ncpu_stalls_and_bus_contention=UNMODELED\n",self.clock.hz(),self.width,self.cas,self.pipe,self.trcd,self.trp,self.trc,self.tmrd,self.read_min_ns(4,true,false).unwrap(),self.read_min_ns(4,false,false).unwrap(),self.read_min_ns(4,false,true).unwrap(),self.read_min_ns(32,false,false).unwrap(),self.refresh_interval_ns(),self.refresh_busy_ns(1),self.ns(self.tmrd))
    }
}
