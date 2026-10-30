//! RM0433 programmed SDRAM timing bounds. These are not CPU stall estimates.
use crate::clock::Rate;

#[derive(Debug, PartialEq, Eq)]
pub enum FlashAdmission {
    Queued,
    BufferHit,
    Backpressure,
}
/// RM0433 4.3.8 bank-one command model in flash-interface clocks. One active
/// read and two waiting; consecutive reads can reuse a 256-bit buffer. This is
/// an earliest-service model, not a reconstructed AXI/NVM pipeline waveform.
pub struct FlashReadPipeline {
    queue: std::collections::VecDeque<u32>,
    buffer: Option<u32>,
    remaining: u64,
    service: u64,
}
impl FlashReadPipeline {
    pub fn new(latency: u32) -> Result<Self, &'static str> {
        if latency > 7 {
            return Err("reserved flash read latency");
        }
        Ok(Self {
            queue: std::collections::VecDeque::new(),
            buffer: None,
            remaining: 0,
            service: u64::from(latency) + 1,
        })
    }
    pub fn submit(&mut self, address: u32) -> Result<FlashAdmission, &'static str> {
        if !crate::memory::SystemMemory::is_flash(address, 1) {
            return Err("outside H750 user flash");
        }
        if self.queue.len() == 3 {
            return Ok(FlashAdmission::Backpressure);
        }
        if self.queue.is_empty() && self.buffer == Some(address & !31) {
            return Ok(FlashAdmission::BufferHit);
        }
        if self.queue.is_empty() {
            self.remaining = self.service;
        }
        self.queue.push_back(address);
        Ok(FlashAdmission::Queued)
    }
    pub fn advance(&mut self, mut clocks: u64) -> Vec<u32> {
        let mut completed = Vec::new();
        while let Some(&address) = self.queue.front() {
            if self.buffer != Some(address & !31) {
                if clocks < self.remaining {
                    self.remaining -= clocks;
                    break;
                }
                clocks -= self.remaining;
                self.buffer = Some(address & !31);
            }
            completed.push(self.queue.pop_front().unwrap());
            self.remaining = self.service;
        }
        completed
    }
}

/// RM0433 2.2 functional AXI admission/arbitration. Callers provide coincident
/// requests; this is not a source of CPU arrival times or cycle-accurate stalls.
#[derive(Default)]
pub struct Axi {
    qos: [[u32; 2]; 6],
    overrides: [u32; 6],
    outstanding: [[u32; 2]; 6],
    last: [[[u64; 6]; 2]; 7],
    sequence: u64,
}
impl Axi {
    fn register(offset: u64) -> Result<(usize, usize), &'static str> {
        if offset < 0x42100 {
            return Err("unmodeled AXI register");
        }
        let ini = ((offset - 0x42100) / 0x1000) as usize;
        let field = (offset - 0x42100) % 0x1000;
        if ini >= 6 || ![0, 4, 8].contains(&field) {
            return Err("unmodeled AXI register");
        }
        Ok((ini, (field / 4) as usize))
    }
    pub fn read(&self, offset: u64) -> Result<u32, &'static str> {
        let (ini, field) = Self::register(offset)?;
        Ok(if field == 2 {
            self.overrides[ini]
        } else {
            self.qos[ini][field]
        })
    }
    pub fn write(&mut self, offset: u64, value: u32) -> Result<(), &'static str> {
        let (ini, field) = Self::register(offset)?;
        if field == 2 {
            self.overrides[ini] = value & 3;
        } else {
            self.qos[ini][field] = value & 15;
        }
        Ok(())
    }
    pub fn reserve(&mut self, ini: usize, write: bool) -> Result<bool, &'static str> {
        let i = ini
            .checked_sub(1)
            .filter(|&i| i < 6)
            .ok_or("invalid AXI initiator")?;
        let channel = usize::from(write);
        let limits = [[1, 4], [7, 32], [1, 4], [4, 1], [2, 1], [1, 1]];
        let limit = if self.overrides[i] & (1 << channel) != 0 {
            1
        } else {
            limits[i][channel]
        };
        if self.outstanding[i][channel] >= limit {
            return Ok(false);
        }
        self.outstanding[i][channel] += 1;
        Ok(true)
    }
    pub fn complete(&mut self, ini: usize, write: bool) -> Result<(), &'static str> {
        let i = ini
            .checked_sub(1)
            .filter(|&i| i < 6)
            .ok_or("invalid AXI initiator")?;
        let count = &mut self.outstanding[i][usize::from(write)];
        *count = count
            .checked_sub(1)
            .ok_or("completion without AXI transaction")?;
        Ok(())
    }
    /// Numbered INI/TARG ports follow RM0433 Figure2. Reset ties have no
    /// published initial ranking; lower port number is a deterministic choice.
    pub fn grant(
        &mut self,
        target: usize,
        write: bool,
        pending: u32,
    ) -> Result<Option<usize>, &'static str> {
        let target = target
            .checked_sub(1)
            .filter(|&i| i < 7)
            .ok_or("invalid AXI target")?;
        if pending & !63 != 0 {
            return Err("invalid AXI request mask");
        }
        let channel = usize::from(write);
        let winner = (0..6)
            .filter(|&i| pending & (1 << i) != 0)
            .min_by_key(|&i| (15 - self.qos[i][channel], self.last[target][channel][i], i));
        if let Some(i) = winner {
            self.sequence += 1;
            self.last[target][channel][i] = self.sequence;
        }
        Ok(winner.map(|i| i + 1))
    }
}
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
