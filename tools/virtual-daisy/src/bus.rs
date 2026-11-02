//! RM0433 programmed SDRAM timing bounds. These are not CPU stall estimates.
use crate::clock::Rate;
const SDRAM_RANGE: &str = "SDRAM access outside configured geometry";
pub fn sdram_bus_error(error: &str) -> bool {
    error == SDRAM_RANGE
}

struct Transaction {
    id: u64,
    ini: usize,
    target: usize,
    write: bool,
    service: u64,
    finish: u64,
    flash: Option<(u32, u64)>,
    sdram: Option<(u32, usize)>,
}
/// Event-driven target service and backpressure. 1..7 are AXI targets;
/// 8..12 are local SRAM slaves on AHB matrices. Service costs are supplied
/// explicitly; this does not reconstruct an unpublished M7 pipeline.
#[derive(Default)]
pub struct Fabric {
    pub axi: Axi,
    now: u64,
    next: u64,
    waiting: std::collections::VecDeque<Transaction>,
    active: Vec<Transaction>,
    done: std::collections::BTreeSet<u64>,
    last_ahb: [usize; 13],
    pub contests: u64,
    pub completed: u64,
    flash_buffer: Option<u32>,
    pub flash_buffer_hits: u64,
    pub flash_buffer_misses: u64,
    sdram: Option<Sdram>,
}
impl Fabric {
    fn lane(target: usize, write: bool) -> usize {
        // RM0433 22.9.3, figures 119/120: FMC SDRAM transfers use the
        // same DATA pins in both directions. AXI's independent channels do
        // not provide two physical SDRAM ports. Reserve the service window;
        // this does not model FMC FIFO acceptance or command pipelining.
        (target - 1) * 2 + usize::from(write && target <= 7 && target != 5)
    }
    pub fn submit(
        &mut self,
        ini: usize,
        target: usize,
        write: bool,
        service: u64,
    ) -> Result<Option<u64>, &'static str> {
        if !(1..=6).contains(&ini) || !(1..=13).contains(&target) || service == 0 {
            return Err("invalid fabric transaction");
        }
        // RM0433 4.3.8: one active and two waiting reads per flash bank.
        if target == 3
            && !write
            && self
                .waiting
                .iter()
                .chain(&self.active)
                .filter(|t| t.target == 3 && !t.write)
                .count()
                >= 3
        {
            return Ok(None);
        }
        if target <= 7 && !self.axi.reserve(ini, write)? {
            return Ok(None);
        }
        self.next += 1;
        self.waiting.push_back(Transaction {
            id: self.next,
            ini,
            target,
            write,
            service,
            finish: 0,
            flash: None,
            sdram: None,
        });
        Ok(Some(self.next))
    }
    /// Bank-buffer lookup happens when arbitration grants the read, not when
    /// its master queues it. A previous queued read can replace the buffer.
    pub fn submit_flash(
        &mut self,
        ini: usize,
        address: u32,
        cold: u64,
        hit: u64,
    ) -> Result<Option<u64>, &'static str> {
        if !crate::memory::SystemMemory::is_flash(address, 1) || hit == 0 {
            return Err("invalid flash-buffer transaction");
        }
        let id = self.submit(ini, 3, false, cold)?;
        if id.is_some() {
            self.waiting.back_mut().unwrap().flash = Some((address & !31, hit));
        }
        Ok(id)
    }
    pub fn invalidate_flash_buffer(&mut self) {
        self.flash_buffer = None;
    }
    pub fn configure_sdram(&mut self, timing: Option<SdramTiming>) {
        match (&mut self.sdram, timing) {
            (Some(s), Some(t)) => {
                if s.timing != t {
                    s.read_ahead.clear();
                }
                if s.timing.refresh != t.refresh || s.timing.clock != t.clock {
                    s.epoch = self.now;
                    s.refresh_index = 1;
                }
                if (
                    s.timing.columns,
                    s.timing.rows,
                    s.timing.banks,
                    s.timing.width,
                ) != (t.columns, t.rows, t.banks, t.width)
                {
                    s.rows = [SdramRow::default(); 4];
                }
                s.timing = t;
            }
            (slot, t) => *slot = t.map(|timing| Sdram::new(timing, self.now)),
        }
    }
    pub fn sdram_refresh_error(&self) -> bool {
        self.sdram.as_ref().is_some_and(|s| s.refresh_error)
    }
    pub fn sdram_status(&self) -> u32 {
        self.sdram
            .as_ref()
            .map_or(0, |s| (s.mode << 1) | u32::from(s.refresh_error))
    }
    pub fn sdram_command(&mut self, command: u32) -> Result<(), &'static str> {
        if command & 16 == 0 {
            return Ok(());
        } // Seed bank2 is unpopulated.
        self.sdram
            .as_mut()
            .ok_or("SDRAM command without a running clock")?
            .commands
            .push_back(command);
        Ok(())
    }
    pub fn clear_sdram_refresh_error(&mut self) {
        if let Some(s) = self.sdram.as_mut() {
            s.refresh_error = false;
        }
    }
    pub(crate) fn invalidate_sdram_fifo(&mut self) {
        if let Some(s) = self.sdram.as_mut() {
            s.read_ahead.clear();
        }
    }
    pub fn sdram_report(&self) -> String {
        self.sdram.as_ref().map_or_else(|| "sdram_runtime=DISABLED\n".into(), |s|
            format!("sdram_runtime=ROW_REFRESH_COMMAND_AND_READ_AHEAD_SERVICE_SCENARIO\nsdram_row_hits={}\nsdram_activations={}\nsdram_precharges={}\nsdram_refreshes={}\nsdram_refresh_error={}\nsdram_mode={}\nsdram_mode_register={}\nsdram_fifo_hits={}\nsdram_fifo_ack=ONE_KERNEL_CYCLE_SCENARIO\n",s.row_hits,s.activations,s.precharges,s.refreshes,u32::from(s.refresh_error),s.mode,s.mode_register,s.fifo_hits))
    }
    pub fn submit_sdram(
        &mut self,
        ini: usize,
        address: u32,
        bytes: usize,
        write: bool,
    ) -> Result<Option<u64>, &'static str> {
        self.validate_sdram(address, bytes)?;
        let id = self.submit(ini, 5, write, 1)?;
        if id.is_some() {
            self.waiting.back_mut().unwrap().sdram = Some((address, bytes));
        }
        Ok(id)
    }
    pub(crate) fn validate_sdram(&self, address: u32, bytes: usize) -> Result<(), &'static str> {
        self.sdram
            .as_ref()
            .ok_or("SDRAM configuration unavailable")?
            .validate(address, bytes)
    }
    fn grant_idle(&mut self) -> Result<(), &'static str> {
        let sdram_active = self.active.iter().any(|t| t.target == 5);
        let sdram_waiting = self.waiting.iter().any(|t| t.target == 5);
        if let Some(sdram) = self.sdram.as_mut() {
            sdram.poll(self.now, sdram_active, sdram_waiting)?;
        }
        if self.waiting.is_empty() {
            return Ok(());
        }
        for lane in 0..26 {
            if lane == Self::lane(5, false)
                && self
                    .sdram
                    .as_ref()
                    .is_some_and(|s| s.busy_until > self.now || s.pending)
            {
                continue;
            }
            if self
                .active
                .iter()
                .any(|t| Self::lane(t.target, t.write) == lane)
            {
                continue;
            }
            let candidates: Vec<_> = self
                .waiting
                .iter()
                .filter(|t| Self::lane(t.target, t.write) == lane)
                .collect();
            let Some(first) = candidates.first() else {
                continue;
            };
            let target = first.target;
            let write = first.write;
            // At a shared port, the oldest queued direction wins, then use
            // that direction's AXI QoS/LRU arbiter. Cross-direction scheduling
            // is a deterministic scenario rule, not a silicon priority claim.
            let mask = candidates
                .iter()
                .filter(|t| target != 5 || t.write == write)
                .fold(0, |m, t| m | (1 << (t.ini - 1)));
            if candidates.len() > 1 {
                self.contests += 1;
            }
            let winner = if target <= 7 {
                self.axi.grant(target, write, mask)?.unwrap()
            } else {
                (1..=6)
                    .map(|step| (self.last_ahb[target - 1] + step - 1) % 6 + 1)
                    .find(|&ini| mask & (1 << (ini - 1)) != 0)
                    .unwrap()
            };
            let index = self
                .waiting
                .iter()
                .position(|t| {
                    Self::lane(t.target, t.write) == lane
                        && (target != 5 || t.write == write)
                        && t.ini == winner
                })
                .unwrap();
            let mut t = self.waiting.remove(index).unwrap();
            if let Some((address, bytes)) = t.sdram {
                t.service = self
                    .sdram
                    .as_mut()
                    .ok_or("SDRAM clock stopped with queued transfer")?
                    .service(self.now, address, bytes, t.write)?;
            }
            if let Some((word, hit)) = t.flash {
                if self.flash_buffer == Some(word) {
                    t.service = hit;
                    self.flash_buffer_hits += 1;
                } else {
                    self.flash_buffer_misses += 1;
                }
            }
            t.finish = self
                .now
                .checked_add(t.service)
                .ok_or("fabric time overflow")?;
            self.last_ahb[target - 1] = winner;
            self.active.push(t);
        }
        Ok(())
    }
    pub fn advance(&mut self, now: u64) -> Result<(), &'static str> {
        if self.sdram.is_none()
            && self
                .active
                .iter()
                .chain(&self.waiting)
                .any(|t| t.sdram.is_some())
        {
            return Err("stopping the SDRAM clock with in-flight data is unsupported");
        }
        if now < self.now {
            return Err("fabric clock moved backwards");
        }
        loop {
            self.grant_idle()?;
            let next = self
                .active
                .iter()
                .map(|t| t.finish)
                .chain(self.sdram.as_ref().and_then(|s| s.next_event(self.now)))
                .min();
            let Some(next) = next.filter(|&t| t <= now) else {
                break;
            };
            self.now = next;
            let mut i = 0;
            while i < self.active.len() {
                if self.active[i].finish == next {
                    let t = self.active.remove(i);
                    if t.target == 3 && !t.write {
                        self.flash_buffer = t.flash.map(|(word, _)| word);
                    }
                    if t.target <= 7 {
                        self.axi.complete(t.ini, t.write)?;
                    }
                    self.done.insert(t.id);
                    self.completed += 1;
                } else {
                    i += 1;
                }
            }
        }
        self.now = now;
        Ok(())
    }
    pub fn take_ready(&mut self, id: u64) -> bool {
        self.done.remove(&id)
    }
    pub fn wait(&self, id: u64) -> u64 {
        if self.done.contains(&id) {
            return 0;
        }
        self.active.iter().find(|t| t.id == id).map_or_else(
            || self.next_event(),
            |t| t.finish.saturating_sub(self.now).max(1),
        )
    }
    pub fn next_event(&self) -> u64 {
        self.active
            .iter()
            .map(|t| t.finish.saturating_sub(self.now))
            .chain(
                self.sdram
                    .as_ref()
                    .and_then(|s| s.next_event(self.now))
                    .map(|t| t - self.now),
            )
            .min()
            .unwrap_or(1)
            .max(1)
    }
}

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
#[derive(Clone, Debug, PartialEq, Eq)]
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
    columns: u32,
    rows: u32,
    banks: u32,
    tras: u64,
    twr: u64,
    txsr: u64,
    write_protected: bool,
    read_burst: bool,
}
impl SdramTiming {
    pub(crate) fn write_protected(&self) -> bool {
        self.write_protected
    }
    pub fn write_min_ns(&self, bytes: u64) -> u64 {
        self.ns(bytes.div_ceil(self.width))
    }
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
        if control & (1 << 12) != 0 && width != 4 {
            return Err("SDRAM read-ahead currently requires the Seed 32-bit data bus");
        }
        if (control >> 2) & 3 == 3 {
            return Err("reserved SDRAM row address width");
        }
        let field = |shift: u32| u64::from(((timing >> shift) & 15_u32) + 1);
        // RM0433 SDTR: controller constraints are additional to the external
        // chip's tWR. Invalid programming has no certified data/timing result.
        if field(16) + field(24) < field(8) || field(16) + field(24) + field(20) < field(12) {
            return Err("SDRAM TWR violates TRAS/TRCD or TRC/TRCD/TRP recovery constraints");
        }
        let refresh = u64::from((refresh >> 1) & 8191);
        if (1..41).contains(&refresh) {
            return Err("SDRAM COUNT must be zero or at least 41");
        }
        if refresh == field(16) + field(20) + field(12) + field(24) + 4 {
            return Err("forbidden SDRAM refresh COUNT timing combination");
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
            columns: 8 + (control & 3),
            rows: 11 + ((control >> 2) & 3),
            banks: if control & (1 << 6) == 0 { 2 } else { 4 },
            tras: field(8),
            twr: field(16),
            txsr: field(4),
            write_protected: control & (1 << 9) != 0,
            read_burst: control & (1 << 12) != 0,
        })
    }
    fn ns(&self, cycles: u64) -> u64 {
        (u128::from(cycles) * u128::from(self.clock.denominator) * 1_000_000_000)
            .div_ceil(u128::from(self.clock.numerator)) as u64
    }
    fn kernel_ns(&self, cycles: u64) -> u64 {
        (u128::from(cycles) * u128::from(self.clock.denominator / self.divider) * 1_000_000_000)
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
        if self.refresh == 0 {
            0
        } else {
            self.ns(self.refresh + 1)
        }
    }
    pub fn refresh_busy_ns(&self, count: u32) -> u64 {
        self.ns(self.trc * u64::from(count))
    }
    pub fn report(&self) -> String {
        format!("scope=PROGRAMMED_TIMING_LOWER_BOUNDS\nsdram_clock_hz={}\nbus_width_bytes={}\ncas_cycles={}\nread_pipe_kernel_cycles={}\ntrcd_cycles={}\ntrp_cycles={}\ntrc_cycles={}\ntmrd_cycles={}\nread_word_open_row_min_ns={}\nread_word_precharged_row_min_ns={}\nread_word_conflicting_row_min_ns={}\nread_cacheline_precharged_row_min_ns={}\nrefresh_interval_ns={}\nrefresh_busy_min_ns={}\nmode_register_quiet_min_ns={}\nlive_stalls=SEE_MEMORY_REPORT\n",self.clock.hz(),self.width,self.cas,self.pipe,self.trcd,self.trp,self.trc,self.tmrd,self.read_min_ns(4,true,false).unwrap(),self.read_min_ns(4,false,false).unwrap(),self.read_min_ns(4,false,true).unwrap(),self.read_min_ns(32,false,false).unwrap(),self.refresh_interval_ns(),self.refresh_busy_ns(1),self.ns(self.tmrd))
    }
}

#[derive(Clone, Copy, Default)]
struct SdramRow {
    number: Option<u32>,
    activated: u64,
    last_write: Option<u64>,
}
struct Sdram {
    timing: SdramTiming,
    rows: [SdramRow; 4],
    epoch: u64,
    refresh_index: u64,
    pending: bool,
    refresh_start: Option<u64>,
    busy_until: u64,
    refresh_error: bool,
    refreshes: u64,
    row_hits: u64,
    activations: u64,
    precharges: u64,
    mode: u32,
    commands: std::collections::VecDeque<u32>,
    mode_register: u32,
    // Six64-bit entries per RM revision history. Per-word validity is a
    // declared narrow-AXI-read packing scenario, pending independent traces.
    read_ahead: std::collections::VecDeque<(u32, [Option<u64>; 2])>,
    port_tail: u64,
    fifo_hits: u64,
}
impl Sdram {
    fn new(timing: SdramTiming, now: u64) -> Self {
        Self {
            timing,
            rows: [SdramRow::default(); 4],
            epoch: now,
            refresh_index: 1,
            pending: false,
            refresh_start: None,
            busy_until: 0,
            refresh_error: false,
            refreshes: 0,
            row_hits: 0,
            activations: 0,
            precharges: 0,
            mode: 0,
            commands: std::collections::VecDeque::new(),
            mode_register: 0,
            read_ahead: std::collections::VecDeque::new(),
            port_tail: 0,
            fifo_hits: 0,
        }
    }
    fn deadline(&self) -> Option<u64> {
        (self.timing.refresh != 0 && self.mode != 1).then(|| {
            self.epoch.saturating_add(
                self.timing
                    .ns(self.refresh_index.saturating_mul(self.timing.refresh + 1)),
            )
        })
    }
    fn next_event(&self, now: u64) -> Option<u64> {
        self.deadline()
            .into_iter()
            .chain(self.refresh_start)
            .chain([self.busy_until])
            .filter(|&time| time > now)
            .min()
    }
    fn poll(
        &mut self,
        now: u64,
        port_active: bool,
        data_waiting: bool,
    ) -> Result<(), &'static str> {
        // Complete command issuance before evaluating a new timer pulse on
        // the same edge. Precharge/recovery can keep an older request pending.
        if self.refresh_start.is_some_and(|time| time <= now) {
            self.refresh_start = None;
            self.pending = false;
            self.refreshes += 1;
        }
        if self.deadline().is_some_and(|time| time <= now) {
            if self.pending {
                self.refresh_error = true;
            }
            self.pending = true;
            self.refresh_index += 1;
        }
        if self.pending && !port_active && self.busy_until <= now {
            let mut start = now.max(self.port_tail);
            let open = self.rows.iter().any(|row| row.number.is_some());
            for &row in &self.rows {
                if row.number.is_some() {
                    start = start.max(self.precharge_ready(row, now));
                }
            }
            if open {
                start += self.timing.ns(self.timing.trp);
                self.precharges += 1;
            }
            self.rows = [SdramRow::default(); 4];
            self.read_ahead.clear();
            self.busy_until = start + self.timing.ns(self.timing.trc);
            if start == now {
                self.pending = false;
                self.refreshes += 1;
            } else {
                self.refresh_start = Some(start);
            }
        }
        while !port_active && self.busy_until <= now {
            let Some(command) = self.commands.pop_front() else {
                break;
            };
            match command & 7 {
                0 => self.exit_low_power(now),
                1 => {} // External device startup delay is firmware-controlled.
                2 => self.busy_until = self.close_all(now, true),
                3 => {
                    if self.rows.iter().any(|r| r.number.is_some()) {
                        return Err("manual SDRAM auto-refresh requires a preceding PALL command");
                    }
                    let count = u64::from(((command >> 5) & 15) + 1);
                    self.busy_until = now + self.timing.ns(self.timing.trc * count);
                    self.refreshes += count;
                }
                4 => {
                    self.mode_register = (command >> 9) & 0x3fff;
                    self.busy_until = now + self.timing.ns(self.timing.tmrd);
                }
                5 => {
                    self.busy_until = self.close_all(now, true) + self.timing.ns(self.timing.tras);
                    self.mode = 1;
                    self.pending = false;
                    self.refresh_start = None;
                }
                6 => {
                    self.mode = 2;
                }
                _ => return Err("reserved SDRAM command"),
            }
        }
        if !port_active && data_waiting && self.commands.is_empty() && self.mode != 0 {
            self.exit_low_power(now);
        }
        Ok(())
    }
    fn close_all(&mut self, now: u64, always: bool) -> u64 {
        let mut ready = now.max(self.port_tail);
        let open = self.rows.iter().any(|r| r.number.is_some());
        for &row in &self.rows {
            if row.number.is_some() {
                ready = ready.max(self.precharge_ready(row, now));
            }
        }
        self.rows = [SdramRow::default(); 4];
        self.read_ahead.clear();
        if open || always {
            ready += self.timing.ns(self.timing.trp);
            self.precharges += 1;
        }
        ready
    }
    fn exit_low_power(&mut self, now: u64) {
        if self.mode == 1 {
            let start = now.max(self.busy_until);
            self.busy_until = start + self.timing.ns(self.timing.txsr);
            self.epoch = start;
            self.refresh_index = 1;
        } else if self.mode == 2 {
            self.busy_until = now.max(self.busy_until) + self.timing.ns(1);
        }
        self.mode = 0;
    }
    fn validate(&self, address: u32, bytes: usize) -> Result<(), &'static str> {
        let capacity = (1_u64 << (self.timing.columns + self.timing.rows))
            * self.timing.width
            * u64::from(self.timing.banks);
        if bytes == 0
            || address < 0xc0000000
            || u64::from(address - 0xc0000000) + bytes as u64 > capacity
        {
            return Err(SDRAM_RANGE);
        }
        Ok(())
    }
    fn precharge_ready(&self, row: SdramRow, now: u64) -> u64 {
        now.max(row.activated + self.timing.ns(self.timing.tras))
            .max(
                row.last_write
                    .map_or(0, |w| w + self.timing.ns(self.timing.twr)),
            )
    }
    fn service(
        &mut self,
        now: u64,
        mut address: u32,
        mut bytes: usize,
        write: bool,
    ) -> Result<u64, &'static str> {
        self.validate(address, bytes)?;
        if write {
            self.read_ahead.clear();
        }
        // RM0433 SDCR.WP: write accesses are ignored, not bus faults or
        // physical SDRAM WRITE commands. AXI acknowledgement latency is not
        // independently specified; no SDRAM service is charged here.
        if write && self.timing.write_protected {
            return Ok(0);
        }
        let t = &self.timing;
        let row_bytes = t.width << t.columns;
        let mut time = now;
        while bytes != 0 {
            let offset = u64::from(address - 0xc0000000);
            let bank = (((offset / row_bytes) >> t.rows) & u64::from(t.banks - 1)) as usize;
            let number = ((offset / row_bytes) & ((1 << t.rows) - 1)) as u32;
            let count = bytes.min((row_bytes - offset % row_bytes) as usize);
            let mut prefix = 0;
            if !write && t.read_burst {
                while prefix < count {
                    let word = (address + prefix as u32) & !3;
                    let half = ((word >> 2) & 1) as usize;
                    let Some(index) = self
                        .read_ahead
                        .iter()
                        .position(|&(tag, ready)| tag == word & !7 && ready[half].is_some())
                    else {
                        break;
                    };
                    let ready = self.read_ahead[index].1[half].take().unwrap();
                    if self.read_ahead[index].1.iter().all(Option::is_none) {
                        self.read_ahead.remove(index);
                    }
                    time = time.max(ready) + t.kernel_ns(1);
                    prefix += (4 - ((address + prefix as u32) & 3) as usize).min(count - prefix);
                    self.fifo_hits += 1;
                }
                if prefix == count {
                    bytes -= count;
                    if bytes != 0 {
                        address += count as u32;
                    }
                    continue;
                }
            }
            time = time.max(self.port_tail);
            let mut row = self.rows[bank];
            if row.number != Some(number) {
                if row.number.is_some() {
                    time = (self.precharge_ready(row, time) + t.ns(t.trp))
                        .max(row.activated + t.ns(t.trc));
                    self.precharges += 1;
                    self.read_ahead.clear();
                }
                self.activations += 1;
                row = SdramRow {
                    number: Some(number),
                    activated: time,
                    last_write: None,
                };
                time += t.ns(t.trcd);
            } else {
                self.row_hits += 1;
            }
            if write {
                row.last_write = Some(time + t.ns((count as u64).div_ceil(t.width) - 1));
                time += t.write_min_ns(count as u64);
            } else {
                time += t.read_min_ns((count - prefix) as u64, true, false)?;
                if t.read_burst {
                    for (tag, words) in &mut self.read_ahead {
                        for (half, word) in words.iter_mut().enumerate() {
                            let start = u64::from(*tag) + half as u64 * 4;
                            if start < u64::from(address) + count as u64
                                && start + 4 > u64::from(address)
                            {
                                *word = None;
                            }
                        }
                    }
                    self.read_ahead
                        .retain(|(_, words)| words.iter().any(Option::is_some));
                    let last = (address + count as u32 - 1) & !3;
                    let anticipated = t.cas + 1 + t.pipe / 2;
                    let mut fetched = 0;
                    for index in 1..=anticipated {
                        let next = last + 4 * index as u32;
                        if u64::from(next - 0xc0000000) / row_bytes != offset / row_bytes {
                            break;
                        }
                        let tag = next & !7;
                        let half = ((next >> 2) & 1) as usize;
                        if let Some(line) =
                            self.read_ahead.iter_mut().find(|(base, _)| *base == tag)
                        {
                            line.1[half] = Some(time + t.ns(index));
                        } else {
                            if self.read_ahead.len() == 6 {
                                self.read_ahead.pop_front();
                            }
                            let mut words = [None; 2];
                            words[half] = Some(time + t.ns(index));
                            self.read_ahead.push_back((tag, words));
                        }
                        fetched = index;
                    }
                    self.port_tail = time + t.ns(fetched);
                }
            }
            self.rows[bank] = row;
            bytes -= count;
            if bytes != 0 {
                address += count as u32;
            }
        }
        Ok(time - now)
    }
}
