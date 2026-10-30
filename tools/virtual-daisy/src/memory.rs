//! Functional H750 Harvard caches and RAM/flash views. No cycle-timing claim.
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

type Result<T> = std::result::Result<T, &'static str>;
const LINE: u32 = 32;
const SETS: usize = 128;
const WAYS: usize = 4;

#[derive(Clone, Default)]
struct CacheLine {
    valid: bool,
    dirty: bool,
    address: u32,
    data: [u8; 32],
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Policy {
    Noncacheable,
    WriteThrough,
    WriteBack,
    WriteAllocate,
}
struct FetchLine {
    address: u32,
    start: usize,
    end: usize,
    expected: [u8; 32],
    cacheable: bool,
}
struct FetchBlock {
    lines: Vec<FetchLine>,
    instructions: u64,
}
#[derive(Clone)]
struct CpuAccess {
    address: u32,
    width: usize,
    write: bool,
    value: u64,
}
struct Writeback {
    line: CacheLine,
    ticket: Option<u64>,
}

pub struct SystemMemory {
    flash: crate::flash::Flash,
    now_ns: u64,
    flash_written: std::collections::BTreeSet<u32>,
    pub fabric: crate::bus::Fabric,
    hclk: Option<crate::clock::Rate>,
    pclk: Option<crate::clock::Rate>,
    sdram: Option<crate::bus::SdramTiming>,
    cpu_requests: [Option<(u32, usize, bool, u64)>; 2],
    cpu_debt: u64,
    pub wait_interruptible: bool,
    pending_writebacks: Vec<Writeback>,
    memory_order_fence: bool,
    entry_stores: std::collections::VecDeque<CpuAccess>,
    entry_vector: Option<(u32, u32)>,
    entry_active: bool,
    pub abandoned_waits: u64,
    pub abandoned_wait_ns: u64,
    return_frame: Option<(u32, u32)>,
    return_reads: Vec<(u32, u32)>,
    pub bypass_exception_waits: bool,
    pub entry_wait_yields: u64,
    pub return_wait_yields: u64,
    pub exception_stack_words: u64,
    pub vector_reads: u64,
    pub vector_cache_hits: u64,
    pub writeback_drain_yields: u64,
    runtime_progress: Option<(u32, u32, usize)>,
    pub cpu_wait_ns: u64,
    pub cpu_wait_yields: u64,
    pub dma_wait_yields: u64,
    replay: BTreeMap<u64, Vec<CpuAccess>>,
    access_context: Option<u64>,
    access_log: Vec<CpuAccess>,
    access_index: usize,
    pages: BTreeMap<u32, Box<[u8; 4096]>>,
    cache: Vec<CacheLine>,
    instructions: Vec<CacheLine>,
    instruction_way: [usize; 256],
    instruction_fills: u64,
    instruction_reads: u64,
    decoded: BTreeMap<u32, u8>,
    blocks: HashMap<(u32, u32), Arc<FetchBlock>>,
    runtime_blocks: u64,
    runtime_instructions: u64,
    runtime_retranslations: u64,
    next_way: [usize; SETS],
    ccr: u32,
    mpu_ctrl: u32,
    region_number: usize,
    regions: [(u32, u32); 16],
    cache_selection: u32,
    fills: u64,
    writebacks: u64,
    dma_cacheable_bytes: u64,
}
impl Default for SystemMemory {
    fn default() -> Self {
        Self {
            flash: crate::flash::Flash::default(),
            now_ns: 0,
            flash_written: std::collections::BTreeSet::new(),
            fabric: crate::bus::Fabric::default(),
            hclk: None,
            pclk: None,
            sdram: None,
            cpu_requests: [None; 2],
            cpu_debt: 0,
            wait_interruptible: true,
            pending_writebacks: Vec::new(),
            memory_order_fence: false,
            entry_stores: std::collections::VecDeque::new(),
            entry_vector: None,
            entry_active: false,
            abandoned_waits: 0,
            abandoned_wait_ns: 0,
            return_frame: None,
            return_reads: Vec::new(),
            bypass_exception_waits: false,
            entry_wait_yields: 0,
            return_wait_yields: 0,
            exception_stack_words: 0,
            vector_reads: 0,
            vector_cache_hits: 0,
            writeback_drain_yields: 0,
            runtime_progress: None,
            cpu_wait_ns: 0,
            cpu_wait_yields: 0,
            dma_wait_yields: 0,
            replay: BTreeMap::new(),
            access_context: None,
            access_log: Vec::new(),
            access_index: 0,
            pages: BTreeMap::new(),
            cache: vec![CacheLine::default(); SETS * WAYS],
            instructions: vec![CacheLine::default(); 512],
            instruction_way: [0; 256],
            instruction_fills: 0,
            instruction_reads: 0,
            decoded: BTreeMap::new(),
            blocks: HashMap::new(),
            runtime_blocks: 0,
            runtime_instructions: 0,
            runtime_retranslations: 0,
            next_way: [0; SETS],
            ccr: 0,
            mpu_ctrl: 0,
            region_number: 0,
            regions: [(0, 0); 16],
            cache_selection: 0,
            fills: 0,
            writebacks: 0,
            dma_cacheable_bytes: 0,
        }
    }
}
impl SystemMemory {
    pub fn reset_registers(&mut self) {
        self.flash.reset();
        self.fabric = crate::bus::Fabric::default();
        self.fabric
            .advance(self.now_ns)
            .expect("empty reset fabric");
        self.cpu_requests = [None; 2];
        self.cpu_debt = 0;
        self.wait_interruptible = true;
        self.pending_writebacks.clear();
        self.memory_order_fence = false;
        self.entry_stores.clear();
        self.entry_vector = None;
        self.entry_active = false;
        self.return_frame = None;
        self.return_reads.clear();
        self.replay.clear();
        self.begin_cpu_block();
        self.ccr = 0;
        self.mpu_ctrl = 0;
        self.region_number = 0;
        self.regions = [(0, 0); 16];
        self.cache_selection = 0;
        self.cache.fill(CacheLine::default());
        self.instructions.fill(CacheLine::default());
        self.instruction_way = [0; 256];
        self.next_way = [0; SETS];
        self.decoded.clear();
        self.blocks.clear();
        self.runtime_progress = None;
    }
    pub fn read_scb(&self, offset: u64) -> Result<u32> {
        match offset {
            0xd14 => Ok(self.ccr),
            0xd78 => Ok(0x09000003),
            0xd7c => Ok(0x8303c003),
            0xd80 => {
                if self.cache_selection == 0 {
                    Ok(0xf00fe019)
                } else if self.cache_selection == 1 {
                    Ok(0xf01fe009)
                } else {
                    Err("no cache at selected level")
                }
            }
            0xd84 => Ok(self.cache_selection),
            _ => Err("SCB read delegated to NVIC"),
        }
    }
    pub fn is_ram(address: u32, width: usize) -> bool {
        let end = u64::from(address) + width as u64;
        [
            (0x2400_0000_u32, 0x80000_u32),
            (0x3000_0000, 0x48000),
            (0x3800_0000, 0x10000),
            (0x3880_0000, 0x1000),
            (0xc000_0000, 0x4000000),
        ]
        .iter()
        .any(|&(base, size)| address >= base && end <= u64::from(base) + u64::from(size))
    }
    fn check(address: u32, width: usize) -> Result<()> {
        if ![1, 2, 4].contains(&width)
            || !(Self::is_ram(address, width) || Self::is_flash(address, width))
        {
            return Err("access outside H750/Daisy memory");
        }
        Ok(())
    }
    pub fn is_flash(address: u32, width: usize) -> bool {
        address >= 0x08000000 && u64::from(address) + width as u64 <= 0x08020000
    }
    pub fn advance_time(&mut self, now: u64) -> Result<()> {
        if now < self.now_ns {
            return Err("memory clock moved backwards");
        }
        self.now_ns = now;
        self.fabric.advance(now)?;
        let mut index = 0;
        while index < self.pending_writebacks.len() {
            if self.pending_writebacks[index]
                .ticket
                .is_some_and(|id| self.fabric.take_ready(id))
            {
                let line = self.pending_writebacks.remove(index).line;
                for (offset, value) in line.data.into_iter().enumerate() {
                    self.store_byte(line.address + offset as u32, value);
                }
                self.writebacks += 1;
            } else {
                index += 1;
            }
        }
        self.issue_writebacks()?;
        for action in self.flash.advance(now) {
            self.fabric.invalidate_flash_buffer();
            match action {
                crate::flash::Action::Program(base, data) => {
                    self.flash_written.insert(base);
                    for (offset, byte) in data.into_iter().enumerate() {
                        self.store_byte(
                            base + offset as u32,
                            self.physical_byte(base + offset as u32) & byte,
                        );
                    }
                }
                crate::flash::Action::Erase => {
                    self.pages
                        .retain(|&page, _| !(0x08000..0x08020).contains(&page));
                    self.flash_written.clear();
                }
            }
        }
        Ok(())
    }
    pub fn now(&self) -> u64 {
        self.now_ns
    }
    pub fn configure_bus(
        &mut self,
        hclk: crate::clock::Rate,
        pclk: crate::clock::Rate,
        sdram: Option<crate::bus::SdramTiming>,
    ) {
        self.hclk = Some(hclk);
        self.pclk = Some(pclk);
        self.sdram = sdram;
    }
    fn ns(rate: crate::clock::Rate, cycles: u64) -> u64 {
        (u128::from(cycles) * u128::from(rate.denominator) * 1_000_000_000)
            .div_ceil(u128::from(rate.numerator)) as u64
    }
    fn target(&self, address: u32, bytes: usize, write: bool) -> Result<Option<(usize, u64)>> {
        let Some(hclk) = self.hclk else {
            return Ok(None);
        };
        let (target, rate, cycles) = match address {
            0x08000000..=0x0801ffff => (
                3,
                hclk,
                if write {
                    1
                } else {
                    u64::from((self.flash.read(0)? & 15) + 1)
                } + (bytes as u64).div_ceil(8)
                    - 1,
            ),
            0x24000000..=0x2407ffff => (7, hclk, (bytes as u64).div_ceil(8)),
            0x30000000..=0x3001ffff => (8, hclk, (bytes as u64).div_ceil(4)),
            0x30020000..=0x3003ffff => (9, hclk, (bytes as u64).div_ceil(4)),
            0x30040000..=0x30047fff => (10, hclk, (bytes as u64).div_ceil(4)),
            0x38000000..=0x3800ffff => (11, hclk, (bytes as u64).div_ceil(4)),
            0x38800000..=0x38800fff => (12, hclk, (bytes as u64).div_ceil(4)),
            0x40015820 | 0x40015840 => (13, self.pclk.ok_or("SAI bus clock stopped")?, 2),
            0xc0000000..=0xc3ffffff => {
                let timing = self
                    .sdram
                    .as_ref()
                    .ok_or("SDRAM clock/configuration unavailable")?;
                return Ok(Some((
                    5,
                    if write {
                        timing.write_min_ns(bytes as u64)
                    } else {
                        timing.read_min_ns(bytes as u64, true, false)?
                    },
                )));
            }
            _ => return Ok(None),
        };
        Ok(Some((target, Self::ns(rate, cycles.max(1)))))
    }
    fn cpu_wait(&mut self, ns: u64) -> u64 {
        let ns = ns.max(1);
        self.cpu_debt = self.cpu_debt.max(ns);
        self.cpu_wait_yields += 1;
        ns
    }
    pub fn take_cpu_debt(&mut self) -> u64 {
        let ns = std::mem::take(&mut self.cpu_debt);
        self.cpu_wait_ns += ns;
        ns
    }
    pub fn abandon_wait(&mut self, ns: u64) {
        self.cpu_wait_ns = self.cpu_wait_ns.saturating_sub(ns);
        self.abandoned_waits += 1;
        self.abandoned_wait_ns += ns;
    }
    pub fn has_cpu_debt(&self) -> bool {
        self.cpu_debt != 0
    }
    pub fn begin_cpu_block(&mut self) {
        self.access_context = None;
        self.access_log.clear();
        self.access_index = 0;
    }
    /// Precise memory retries must replay completed accesses of an STM/STRD,
    /// rather than issue their writes twice. Context identifies guest PC+SP.
    pub fn cpu_access(
        &mut self,
        context: u64,
        address: u32,
        width: usize,
        write: bool,
        value: &mut u64,
    ) -> Result<u64> {
        if ![1, 2, 4, 8].contains(&width)
            || !(Self::is_ram(address, width) || Self::is_flash(address, width))
        {
            return Err("invalid CPU transaction");
        }
        if context == u64::MAX - 1 {
            if !write || width != 4 {
                return Err("unsupported exception stack transaction");
            }
            self.entry_stores.push_back(CpuAccess {
                address,
                width,
                write,
                value: *value,
            });
            self.exception_stack_words += 1;
            return Ok(0);
        }
        if context == u64::MAX - 2 {
            if write || width != 4 {
                return Err("unsupported exception unstack transaction");
            }
            *value = u64::from(
                self.return_reads
                    .iter()
                    .find(|&&(ready, _)| ready == address)
                    .ok_or("unstack access was not prepared")?
                    .1,
            );
            return Ok(0);
        }
        if context == u64::MAX {
            if write || width != 4 {
                return Err("invalid exception vector access");
            }
            if let Some((ready, data)) = self.entry_vector.filter(|&(ready, _)| ready == address) {
                let _ = ready;
                *value = u64::from(data);
            } else {
                if Self::is_flash(address, width)
                    && self.flash_wait(false) > 0
                    && !self.vector_cached(address)
                {
                    return Err("vector read bypassed pending flash service");
                }
                *value = u64::from(self.read_vector(address)?);
            }
            return Ok(0);
        }
        if context == 0 && self.hclk.is_some() && self.target(address, width, write)?.is_some() {
            return Err("bus stalls outside a translated memory instruction are unmodeled");
        }
        if self.access_context != Some(context) {
            self.access_context = Some(context);
            self.access_log = self.replay.remove(&context).unwrap_or_default();
            self.access_index = 0;
        }
        if let Some(previous) = self.access_log.get(self.access_index) {
            if (previous.address, previous.width, previous.write) != (address, width, write)
                || (write && previous.value != *value)
            {
                return Err("CPU retry transaction sequence changed");
            }
            *value = previous.value;
            self.access_index += 1;
            return Ok(0);
        }
        let ns = self.access_wait(address, width, write, false)?;
        if ns != 0 {
            self.replay.insert(context, self.access_log.clone());
            return Ok(ns);
        }
        if write {
            if width == 8 {
                if Self::is_flash(address, 8) {
                    self.program_flash(address, 8, *value, 0)?;
                } else {
                    self.write_cpu(address, 4, *value as u32)?;
                    self.write_cpu(address + 4, 4, (*value >> 32) as u32)?;
                }
            } else {
                self.write_cpu(address, width, *value as u32)?;
            }
        } else {
            *value = if width == 8 {
                u64::from(self.read_cpu(address, 4)?)
                    | (u64::from(self.read_cpu(address + 4, 4)?) << 32)
            } else {
                u64::from(self.read_cpu(address, width)?)
            };
        }
        self.access_log.push(CpuAccess {
            address,
            width,
            write,
            value: *value,
        });
        self.access_index += 1;
        Ok(0)
    }
    pub fn access_wait(
        &mut self,
        address: u32,
        width: usize,
        write: bool,
        instruction: bool,
    ) -> Result<u64> {
        self.access_wait_kind(address, width, write, instruction, false)
    }
    fn access_wait_kind(
        &mut self,
        address: u32,
        width: usize,
        write: bool,
        instruction: bool,
        vector: bool,
    ) -> Result<u64> {
        let owner = usize::from(!instruction);
        if self.hclk.is_none() {
            return Ok(0);
        }
        if !instruction && self.memory_order_fence {
            let wait = self.barrier(4)?;
            if wait != 0 {
                return Ok(wait);
            }
            self.memory_order_fence = false;
        }
        self.wait_interruptible = instruction || (!write && self.normal_memory(address));
        let policy = if vector {
            self.vector_policy(address)
        } else {
            self.access_policy(address, instruction)?
        };
        let cached = if instruction {
            let base = address & !31;
            let set = ((base / 32) % 256) as usize;
            policy != Policy::Noncacheable
                && (set * 2..set * 2 + 2)
                    .any(|i| self.instructions[i].valid && self.instructions[i].address == base)
        } else {
            policy != Policy::Noncacheable && self.find(address).is_some()
        };
        if cached && (!write || matches!(policy, Policy::WriteAllocate | Policy::WriteBack)) {
            return Ok(0);
        }
        // A refill must not fetch older physical bytes while the same line is
        // still being written back. This scenario serializes that hazard.
        if !instruction
            && self
                .pending_writebacks
                .iter()
                .any(|op| op.line.address == address & !31)
        {
            let wait = self.fabric.next_event();
            return Ok(self.cpu_wait(wait));
        }
        if Self::is_flash(address, width) {
            let wait = self.flash_wait(write);
            if wait > 0 {
                return Ok(self.cpu_wait(wait));
            }
        }
        let allocate_write = write && policy == Policy::WriteAllocate && !cached;
        let bytes = if (instruction && !vector)
            || ((!write || allocate_write) && policy != Policy::Noncacheable)
        {
            32
        } else {
            width
        };
        let key_address = if bytes == 32 { address & !31 } else { address };
        if let Some((previous, previous_width, previous_write, id)) = self.cpu_requests[owner] {
            if self.fabric.take_ready(id) {
                self.cpu_requests[owner] = None;
                if (previous, previous_width, previous_write) == (key_address, bytes, write) {
                    return Ok(0);
                }
            } else {
                let ns = self.fabric.wait(id);
                return Ok(self.cpu_wait(ns));
            }
        }
        let bus_write = write && !allocate_write;
        let Some((target, service)) = self.target(address, bytes, bus_write)? else {
            return Ok(0);
        };
        let Some(id) = self.submit_bus(2, address, bytes, bus_write, target, service)? else {
            let ns = self.fabric.next_event();
            return Ok(self.cpu_wait(ns));
        };
        self.cpu_requests[owner] = Some((key_address, bytes, write, id));
        Ok(self.cpu_wait(1))
    }
    fn vector_policy(&self, address: u32) -> Policy {
        // ARMv7-M MPUCheck: vectors use the default map, ignoring MPU/XN.
        if self.ccr & (1 << 17) != 0
            && (address < 0x40000000 || (0x60000000..0xa0000000).contains(&address))
        {
            Policy::WriteThrough
        } else {
            Policy::Noncacheable
        }
    }
    fn normal_memory(&self, address: u32) -> bool {
        if self.mpu_ctrl & 1 != 0 {
            for &(base, attributes) in self.regions.iter().rev() {
                if attributes & 1 == 0 {
                    continue;
                }
                let size = 1_u64 << (((attributes >> 1) & 31) + 1);
                if u64::from(address) < u64::from(base)
                    || u64::from(address) >= u64::from(base) + size
                {
                    continue;
                }
                let subregion = (u64::from(address - base) * 8 / size) as u32;
                if size >= 256 && attributes & (1 << (8 + subregion)) != 0 {
                    continue;
                }
                return !matches!(
                    ((attributes >> 19) & 7, (attributes >> 16) & 3),
                    (0, 0 | 1) | (2, 0)
                );
            }
        }
        address < 0x40000000 || (0x60000000..0xa0000000).contains(&address)
    }
    fn vector_cached(&self, address: u32) -> bool {
        let base = address & !31;
        let set = ((base / 32) % 256) as usize;
        self.vector_policy(address) != Policy::Noncacheable
            && (set * 2..set * 2 + 2)
                .any(|i| self.instructions[i].valid && self.instructions[i].address == base)
    }
    fn read_vector(&mut self, address: u32) -> Result<u32> {
        Self::check(address, 4)?;
        if address & 3 != 0 {
            return Err("unaligned exception vector");
        }
        self.vector_reads += 1;
        self.vector_cache_hits += u64::from(self.vector_cached(address));
        let index = if self.vector_policy(address) != Policy::Noncacheable {
            Some(self.instruction_index(address & !31))
        } else {
            None
        };
        Ok((0..4).fold(0, |value, byte| {
            value
                | (u32::from(index.map_or_else(
                    || self.physical_byte(address + byte),
                    |i| self.instructions[i].data[((address + byte) & 31) as usize],
                )) << (byte * 8))
        }))
    }
    /// Suspend before handler execution; frame stores and vector fetch can
    /// progress on separate ports. Entry never drains unrelated writebacks.
    pub fn exception_entry(&mut self, address: u32) -> Result<u64> {
        if !self.entry_active {
            self.entry_active = true;
            self.runtime_progress = None;
        }
        if self.bypass_exception_waits {
            while let Some(op) = self.entry_stores.pop_front() {
                self.write_cpu(op.address, op.width, op.value as u32)?;
            }
            let data = self.read_vector(address)?;
            self.entry_vector = Some((address, data));
            return Ok(0);
        }
        let old = self.cpu_debt;
        self.cpu_debt = 0;
        let fence = std::mem::take(&mut self.memory_order_fence);
        let result = (|| {
            let vector_wait = if self.entry_vector.is_some_and(|(ready, _)| ready == address) {
                0
            } else if (0..0x10000).contains(&address) || (0x20000000..0x20020000).contains(&address)
            {
                self.entry_vector = Some((address, 0));
                0
            } else {
                let wait = self.access_wait_kind(address, 4, false, true, true)?;
                if wait == 0 {
                    let data = self.read_vector(address)?;
                    self.entry_vector = Some((address, data));
                }
                wait
            };
            let mut stack_wait = 0;
            while let Some(op) = self.entry_stores.front().cloned() {
                stack_wait = self.access_wait(op.address, op.width, true, false)?;
                if stack_wait != 0 {
                    break;
                }
                self.write_cpu(op.address, op.width, op.value as u32)?;
                self.entry_stores.pop_front();
            }
            Ok(match (vector_wait, stack_wait) {
                (0, wait) | (wait, 0) => wait,
                (a, b) => a.min(b),
            })
        })();
        self.memory_order_fence = fence;
        self.cpu_debt = old;
        result.map(|wait| {
            if wait != 0 {
                self.entry_wait_yields += 1;
                self.cpu_wait(wait)
            } else {
                0
            }
        })
    }
    pub fn finish_exception_entry(&mut self) -> Result<()> {
        if !self.entry_stores.is_empty() {
            return Err("handler started before stack stores completed");
        }
        self.entry_vector = None;
        self.entry_active = false;
        Ok(())
    }
    pub fn exception_return(&mut self, address: u32, bytes: u32) -> Result<u64> {
        if ![32, 104].contains(&bytes) {
            return Err("unsupported exception frame size");
        }
        if (0x20000000..0x20020000).contains(&address) {
            return Ok(0);
        } // Native DTCM.
        if !Self::is_ram(address, bytes as usize) {
            return Err("unsupported exception stack memory");
        }
        if self.return_frame != Some((address, bytes)) {
            self.return_frame = Some((address, bytes));
            self.return_reads.clear();
        }
        let fence = std::mem::take(&mut self.memory_order_fence);
        let result = (|| {
            while self.return_reads.len() < (bytes / 4) as usize {
                let current = address + self.return_reads.len() as u32 * 4;
                let wait = self.access_wait(current, 4, false, false)?;
                if wait != 0 {
                    return Ok(wait);
                }
                let value = self.read_cpu(current, 4)?;
                self.return_reads.push((current, value));
            }
            Ok(0)
        })();
        self.memory_order_fence = fence;
        result.inspect(|&wait| {
            self.return_wait_yields += u64::from(wait != 0);
        })
    }
    pub fn finish_exception_return(&mut self) {
        self.return_frame = None;
        self.return_reads.clear();
    }
    pub fn dma_submit(&mut self, address: u32, write: bool) -> Result<Option<u64>> {
        if Self::is_flash(address, 4) && self.flash_wait(write) > 0 {
            self.dma_wait_yields += 1;
            return Ok(None);
        }
        let Some((target, service)) = self.target(address, 4, write)? else {
            return Err("unmodeled DMA bus target");
        };
        let ticket = self.submit_bus(1, address, 4, write, target, service)?;
        // Submission is asynchronous even when service finishes before the
        // next 1µs poll; the bridge yields without committing NDTR/IRQ yet.
        self.dma_wait_yields += 1;
        Ok(ticket)
    }
    fn submit_bus(
        &mut self,
        ini: usize,
        address: u32,
        bytes: usize,
        write: bool,
        target: usize,
        service: u64,
    ) -> Result<Option<u64>> {
        if target == 3 && !write {
            let beat = Self::ns(
                self.hclk.ok_or("flash clock stopped")?,
                (bytes as u64).div_ceil(8),
            );
            self.fabric.submit_flash(ini, address, service, beat)
        } else {
            self.fabric.submit(ini, target, write, service)
        }
    }
    pub fn dma_ready(&mut self, id: u64) -> bool {
        let ready = self.fabric.take_ready(id);
        if !ready {
            self.dma_wait_yields += 1;
        }
        ready
    }
    pub fn inject_bus_load(&mut self, target: usize, ns: u64) -> Result<()> {
        self.inject_bus_load_kind(target, ns, false)
    }
    pub fn inject_bus_load_kind(&mut self, target: usize, ns: u64, write: bool) -> Result<()> {
        self.fabric
            .submit(4, target, write, ns)?
            .ok_or("bus injector admission failed")?;
        Ok(())
    }
    pub fn flash_read_reg(&self, offset: u64) -> Result<u32> {
        self.flash.read(offset)
    }
    pub fn flash_write_reg(&mut self, offset: u64, value: u32, width: usize) -> Result<()> {
        self.flash.write(offset, value, width, self.now_ns)?;
        if self.flash_wait(false) > 0 {
            self.fabric.invalidate_flash_buffer();
        }
        Ok(())
    }
    pub fn flash_wait(&self, write: bool) -> u64 {
        self.flash.wait(write, self.now_ns)
    }
    pub fn flash_irq(&self) -> bool {
        self.flash.irq()
    }
    pub fn set_flash_maximum(&mut self, maximum: bool) -> Result<()> {
        self.flash.set_maximum(maximum)
    }
    pub fn flash_stats(&self) -> String {
        self.flash.stats()
    }
    fn program_flash(&mut self, address: u32, width: usize, value: u64, master: u32) -> Result<()> {
        if master == 0 {
            if matches!(
                self.policy(address)?,
                Policy::WriteAllocate | Policy::WriteBack
            ) {
                return Err("write-back cached flash programming is unmodeled");
            }
            if let Some(index) = self.find(address) {
                for byte in 0..width {
                    self.cache[index].data[((address + byte as u32) & 31) as usize] =
                        (value >> (byte * 8)) as u8;
                }
            }
        }
        if self.flash.program_enabled()
            && ((self.flash_written.contains(&(address & !31)) && !self.flash.pending_erase())
                || self.flash.pending_program(address & !31))
        {
            return Err("reprogramming nonvirgin flash ECC words is unmodeled; erase first");
        }
        self.flash
            .program(address, width, value, master, self.now_ns)?;
        if self.flash_wait(false) > 0 {
            self.fabric.invalidate_flash_buffer();
        }
        Ok(())
    }
    /// Firmware image loader, never a CPU/DMA programming operation.
    pub fn load_flash(&mut self, address: u32, width: usize, value: u32) -> Result<()> {
        Self::check(address, width)?;
        if !Self::is_flash(address, width) {
            return Err("not user flash");
        }
        self.fabric.invalidate_flash_buffer();
        for byte in 0..width {
            self.flash_written.insert((address + byte as u32) & !31);
            self.store_byte(address + byte as u32, (value >> (byte * 8)) as u8);
        }
        Ok(())
    }
    fn physical_byte(&self, address: u32) -> u8 {
        self.pages
            .get(&(address >> 12))
            .map_or(if Self::is_flash(address, 1) { 0xff } else { 0 }, |page| {
                page[(address & 4095) as usize]
            })
    }
    fn store_byte(&mut self, address: u32, value: u8) {
        self.pages.entry(address >> 12).or_insert_with(|| {
            Box::new([if Self::is_flash(address, 1) { 0xff } else { 0 }; 4096])
        })[(address & 4095) as usize] = value;
    }
    fn clean(&mut self, index: usize) -> Result<()> {
        if self.cache[index].valid && self.cache[index].dirty {
            let line = self.cache[index].clone();
            if self.hclk.is_some() {
                self.pending_writebacks
                    .push(Writeback { line, ticket: None });
                self.issue_writebacks()?;
            } else {
                for (offset, value) in line.data.into_iter().enumerate() {
                    self.store_byte(line.address + offset as u32, value);
                }
                self.writebacks += 1;
            }
            self.cache[index].dirty = false;
        }
        Ok(())
    }
    fn issue_writebacks(&mut self) -> Result<()> {
        for index in 0..self.pending_writebacks.len() {
            if self.pending_writebacks[index].ticket.is_none() {
                let address = self.pending_writebacks[index].line.address;
                let (target, service) = self
                    .target(address, 32, true)?
                    .ok_or("unmodeled writeback target")?;
                let ticket = self.fabric.submit(2, target, true, service)?;
                self.pending_writebacks[index].ticket = ticket;
                if ticket.is_none() {
                    break;
                }
            }
        }
        Ok(())
    }
    /// DSB waits for completion. DMB orders subsequent memory accesses while
    /// permitting intervening non-memory instructions and exception entry.
    pub fn barrier(&mut self, kind: u32) -> Result<u64> {
        match kind {
            5 => {
                self.memory_order_fence = true;
                Ok(0)
            }
            4 => {
                self.wait_interruptible = true;
                self.issue_writebacks()?;
                if self.pending_writebacks.is_empty() {
                    Ok(0)
                } else {
                    self.writeback_drain_yields += 1;
                    let wait = self.fabric.next_event();
                    Ok(self.cpu_wait(wait))
                }
            }
            _ => Err("unsupported memory barrier"),
        }
    }
    fn find(&self, address: u32) -> Option<usize> {
        let base = address & !(LINE - 1);
        let set = (base / LINE) as usize % SETS;
        (set * WAYS..(set + 1) * WAYS)
            .find(|&index| self.cache[index].valid && self.cache[index].address == base)
    }
    fn allocate(&mut self, address: u32) -> Result<usize> {
        if let Some(index) = self.find(address) {
            return Ok(index);
        }
        let base = address & !(LINE - 1);
        let set = (base / LINE) as usize % SETS;
        let index = (set * WAYS..(set + 1) * WAYS)
            .find(|&i| !self.cache[i].valid)
            .unwrap_or(set * WAYS + self.next_way[set]);
        self.next_way[set] = (index % WAYS + 1) % WAYS;
        self.clean(index)?;
        let mut data = [0; 32];
        for (offset, byte) in data.iter_mut().enumerate() {
            *byte = self.physical_byte(base + offset as u32);
        }
        self.cache[index] = CacheLine {
            valid: true,
            dirty: false,
            address: base,
            data,
        };
        self.fills += 1;
        Ok(index)
    }
    fn policy(&self, address: u32) -> Result<Policy> {
        self.access_policy(address, false)
    }
    fn access_policy(&self, address: u32, instruction: bool) -> Result<Policy> {
        let enabled = self.ccr & (1 << if instruction { 17 } else { 16 }) != 0;
        if !enabled && !instruction {
            return Ok(Policy::Noncacheable);
        }
        if self.mpu_ctrl & 1 != 0 {
            for &(base, attributes) in self.regions.iter().rev() {
                if attributes & 1 == 0 {
                    continue;
                }
                let size = 1_u64 << (((attributes >> 1) & 31) + 1);
                if u64::from(address) < u64::from(base)
                    || u64::from(address) >= u64::from(base) + size
                {
                    continue;
                }
                let subregion = (u64::from(address - base) * 8 / size) as u32;
                if size >= 256 && attributes & (1 << (8 + subregion)) != 0 {
                    continue;
                }
                if instruction && attributes & (1 << 28) != 0 {
                    return Err("instruction fetch from MPU execute-never region");
                }
                let tex = (attributes >> 19) & 7;
                let cb = (attributes >> 16) & 3;
                if instruction && matches!((tex, cb), (0, 0 | 1) | (2, 0)) {
                    return Err("instruction fetch from Device or Strongly-ordered region");
                }
                if attributes & (1 << 18) != 0 {
                    return Ok(Policy::Noncacheable);
                }
                if !enabled {
                    return Ok(Policy::Noncacheable);
                }
                return match (tex, cb) {
                    (0, 0 | 1) | (1 | 2, 0) => Ok(Policy::Noncacheable),
                    (0, 2) => Ok(Policy::WriteThrough),
                    (0, 3) => Ok(Policy::WriteBack),
                    (1, 3) => Ok(Policy::WriteAllocate),
                    (4..=7, 0) => Ok(Policy::Noncacheable),
                    (4..=7, 1) => Ok(Policy::WriteAllocate),
                    (4..=7, 2) => Ok(Policy::WriteThrough),
                    (4..=7, 3) => Ok(Policy::WriteBack),
                    _ => Err("reserved MPU memory attribute encoding"),
                };
            }
            if self.mpu_ctrl & 4 == 0 {
                return Err("no MPU region and no privileged background map");
            }
        }
        if instruction
            && !(0..=0x3fff_ffff).contains(&address)
            && !(0x6000_0000..=0x9fff_ffff).contains(&address)
        {
            return Err("instruction fetch from default execute-never Device map");
        }
        if !enabled {
            return Ok(Policy::Noncacheable);
        }
        Ok(match address {
            0..=0x1fff_ffff | 0x8000_0000..=0x9fff_ffff => Policy::WriteThrough,
            0x2000_0000..=0x3fff_ffff | 0x6000_0000..=0x7fff_ffff => Policy::WriteAllocate,
            _ => Policy::Noncacheable,
        })
    }
    pub fn read_cpu(&mut self, address: u32, width: usize) -> Result<u32> {
        Self::check(address, width)?;
        let mut value = 0;
        for byte in 0..width {
            let addr = address + byte as u32;
            let b = if self.policy(addr)? != Policy::Noncacheable {
                let line = self.allocate(addr)?;
                self.cache[line].data[(addr & 31) as usize]
            } else {
                self.physical_byte(addr)
            };
            value |= u32::from(b) << (byte * 8);
        }
        Ok(value)
    }
    /// Harvard instruction view: never forwards dirty data-cache contents.
    /// Translation is a side-effect-free peek: it must not populate the cache.
    pub fn decode_instruction(&mut self, address: u32, width: usize) -> Result<u32> {
        Self::check(address, width)?;
        self.blocks.clear();
        let mut value = 0;
        for byte in 0..width {
            let addr = address + byte as u32;
            let base = addr & !31;
            let set = ((base / 32) % 256) as usize;
            let cached = if self.access_policy(addr, true)? != Policy::Noncacheable {
                (set * 2..set * 2 + 2)
                    .find(|&i| self.instructions[i].valid && self.instructions[i].address == base)
            } else {
                None
            };
            let b = cached.map_or_else(
                || self.physical_byte(addr),
                |i| self.instructions[i].data[(addr & 31) as usize],
            );
            value |= u32::from(b) << (byte * 8);
            self.decoded.insert(addr, b);
        }
        Ok(value)
    }
    /// Check a linear Thumb translation immediately before it executes. Returns
    /// true when its decoded bytes differ, requiring a safe translation restart.
    /// This is block-granular functional fetching, not pipeline/speculation timing.
    pub fn runtime_fetch(&mut self, address: u32, bytes: u32) -> Result<bool> {
        if !Self::is_ram(address, 2) && !Self::is_flash(address, 2) {
            return Ok(false);
        }
        let key = (address, bytes);
        let block = if let Some(block) = self.blocks.get(&key) {
            Arc::clone(block)
        } else {
            let end = address.checked_add(bytes).ok_or("invalid block extent")?;
            if bytes == 0 || bytes > 8192 || address & 1 != 0 || bytes & 1 != 0 {
                return Err("invalid Thumb block");
            }
            let mut lines = Vec::new();
            let mut pc = address;
            let mut instructions = 0;
            while pc < end {
                let half = u16::from_le_bytes([
                    *self.decoded.get(&pc).ok_or("missing decoded byte")?,
                    *self.decoded.get(&(pc + 1)).ok_or("missing decoded byte")?,
                ]);
                pc += if half >> 11 >= 0b11101 { 4 } else { 2 };
                instructions += 1;
            }
            let mut current = address;
            while current < end {
                let base = current & !31;
                let stop = end.min(base + 32);
                let mut expected = [0; 32];
                for addr in current..stop {
                    expected[(addr - base) as usize] =
                        *self.decoded.get(&addr).ok_or("missing decoded byte")?;
                }
                lines.push(FetchLine {
                    address: base,
                    start: (current - base) as usize,
                    end: (stop - base) as usize,
                    expected,
                    cacheable: self.access_policy(current, true)? != Policy::Noncacheable,
                });
                current = stop;
            }
            let block = Arc::new(FetchBlock {
                lines,
                instructions,
            });
            self.blocks.insert(key, Arc::clone(&block));
            block
        };
        self.runtime_blocks += 1;
        self.runtime_instructions += block.instructions;
        let first = self
            .runtime_progress
            // Renode may trim the same PC's block as its remaining execution
            // budget changes. Already-serviced prefix lines survive that retry.
            .filter(|&(pc, _, _)| pc == address)
            .map_or(0, |(_, _, i)| i.min(block.lines.len()));
        for (line_index, line) in block.lines.iter().enumerate().skip(first) {
            let set = ((line.address / 32) % 256) as usize;
            let present = line.cacheable
                && (set * 2..set * 2 + 2).any(|i| {
                    self.instructions[i].valid && self.instructions[i].address == line.address
                });
            if !present
                && self.access_wait(
                    line.address + line.start as u32,
                    line.end - line.start,
                    false,
                    true,
                )? != 0
            {
                self.runtime_progress = Some((address, bytes, line_index));
                return Ok(false);
            }
            self.instruction_reads += 1;
            let matches = if line.cacheable {
                let index = self.instruction_index(line.address);
                self.instructions[index].data[line.start..line.end]
                    == line.expected[line.start..line.end]
            } else {
                (line.start..line.end).all(|offset| {
                    self.physical_byte(line.address + offset as u32) == line.expected[offset]
                })
            };
            if !matches {
                self.runtime_progress = None;
                self.runtime_retranslations += 1;
                return Ok(true);
            }
        }
        self.runtime_progress = None;
        Ok(false)
    }
    fn instruction_index(&mut self, base: u32) -> usize {
        let set = ((base / 32) % 256) as usize;
        if let Some(index) = (set * 2..set * 2 + 2)
            .find(|&i| self.instructions[i].valid && self.instructions[i].address == base)
        {
            return index;
        }
        let index = (set * 2..set * 2 + 2)
            .find(|&i| !self.instructions[i].valid)
            .unwrap_or(set * 2 + self.instruction_way[set]);
        let data = std::array::from_fn(|offset| self.physical_byte(base + offset as u32));
        self.instructions[index] = CacheLine {
            valid: true,
            dirty: false,
            address: base,
            data,
        };
        self.instruction_way[set] = (index % 2 + 1) % 2;
        self.instruction_fills += 1;
        index
    }
    pub fn read_instruction(&mut self, address: u32, width: usize) -> Result<u32> {
        Self::check(address, width)?;
        self.instruction_reads += 1;
        let mut value = 0;
        for byte in 0..width {
            let addr = address + byte as u32;
            let b = if self.access_policy(addr, true)? != Policy::Noncacheable {
                let index = self.instruction_index(addr & !31);
                self.instructions[index].data[(addr & 31) as usize]
            } else {
                self.physical_byte(addr)
            };
            value |= u32::from(b) << (byte * 8);
        }
        Ok(value)
    }
    pub fn write_cpu(&mut self, address: u32, width: usize, value: u32) -> Result<()> {
        Self::check(address, width)?;
        if !Self::is_ram(address, width) {
            return self.program_flash(address, width, u64::from(value), 0);
        }
        for byte in 0..width {
            let addr = address + byte as u32;
            let b = (value >> (byte * 8)) as u8;
            let policy = self.policy(addr)?;
            let line = if policy == Policy::WriteAllocate {
                Some(self.allocate(addr)?)
            } else if policy != Policy::Noncacheable {
                self.find(addr)
            } else {
                None
            };
            if let Some(line) = line {
                self.cache[line].data[(addr & 31) as usize] = b;
                self.cache[line].dirty = policy != Policy::WriteThrough;
            }
            if line.is_none() || matches!(policy, Policy::Noncacheable | Policy::WriteThrough) {
                self.store_byte(addr, b);
            }
        }
        Ok(())
    }
    pub fn read_dma(&mut self, address: u32, width: usize) -> Result<u32> {
        Self::check(address, width)?;
        if self
            .policy(address)
            .is_ok_and(|policy| policy != Policy::Noncacheable)
        {
            self.dma_cacheable_bytes += width as u64;
        }
        Ok((0..width).fold(0, |value, byte| {
            value | (u32::from(self.physical_byte(address + byte as u32)) << (byte * 8))
        }))
    }
    pub fn write_dma(&mut self, address: u32, width: usize, value: u32) -> Result<()> {
        Self::check(address, width)?;
        if !Self::is_ram(address, width) {
            return self.program_flash(address, width, u64::from(value), 1);
        }
        if self
            .policy(address)
            .is_ok_and(|policy| policy != Policy::Noncacheable)
        {
            self.dma_cacheable_bytes += width as u64;
        }
        for byte in 0..width {
            self.store_byte(address + byte as u32, (value >> (byte * 8)) as u8);
        }
        Ok(())
    }
    pub fn stats(&self) -> String {
        let cache=format!("data_cache_enabled={}\ncache_line_fills={}\ncache_line_writebacks={}\ndirty_lines={}\ndma_cacheable_bytes={}\ninstruction_cache_enabled={}\ninstruction_fetch_reads={}\ninstruction_cache_fills={}\nruntime_blocks={}\nruntime_instruction_slots={}\nruntime_retranslations={}\n", u32::from(self.ccr & (1 << 16) != 0), self.fills, self.writebacks, self.cache.iter().filter(|line| line.valid && line.dirty).count(), self.dma_cacheable_bytes,u32::from(self.ccr&(1<<17)!=0),self.instruction_reads,self.instruction_fills,self.runtime_blocks,self.runtime_instructions,self.runtime_retranslations);
        let timing=format!("abandoned_cpu_waits={}\nabandoned_cpu_wait_ns={}\nflash_buffer_hits={}\nflash_array_reads={}\nwriteback_pending_lines={}\nwriteback_drain_yields={}\nexception_entry_wait_yields={}\nexception_return_wait_yields={}\nexception_stack_words={}\nvector_reads={}\nvector_cache_hits={}\ncache_writeback_timing=DEFERRED_FABRIC_COMPLETION\nexception_bus_timing=RESUMABLE_PORT_SERVICE_SCENARIO\nstore_buffer_and_pipeline_overlap=UNMODELED\n",self.abandoned_waits,self.abandoned_wait_ns,self.fabric.flash_buffer_hits,self.fabric.flash_buffer_misses,self.pending_writebacks.len(),self.writeback_drain_yields,self.entry_wait_yields,self.return_wait_yields,self.exception_stack_words,self.vector_reads,self.vector_cache_hits);
        format!("{cache}{timing}cpu_bus_wait_ns={}\ncpu_bus_wait_yields={}\ndma_bus_wait_yields={}\nfabric_completed={}\nfabric_contests={}\nbus_stall_scope=COUPLED_FUNCTIONAL_SERVICE_SCENARIO\ndma_service_grid_ns=1000\n{}",self.cpu_wait_ns,self.cpu_wait_yields,self.dma_wait_yields,self.fabric.completed,self.fabric.contests,self.flash.stats())
    }
    pub fn write_scb(&mut self, offset: u64, value: u32) -> Result<()> {
        self.blocks.clear();
        self.runtime_progress = None;
        match offset {
            0xd14 => self.ccr = value,
            0xd84 => self.cache_selection = value & 1,
            0xd94 => self.mpu_ctrl = value & 7,
            0xd98 => {
                if value > 15 {
                    return Err("H750 has 16 MPU regions");
                }
                self.region_number = value as usize;
            }
            0xd9c | 0xda4 | 0xdac | 0xdb4 => {
                if value & 16 != 0 {
                    self.region_number = (value & 15) as usize;
                }
                self.regions[self.region_number].0 = value & !31;
            }
            0xda0 | 0xda8 | 0xdb0 | 0xdb8 => {
                let bits = (value >> 1) & 31;
                if value & 1 != 0 && (bits < 4 || (bits < 7 && value & 0xff00 != 0)) {
                    return Err("invalid MPU region size/subregions");
                }
                self.regions[self.region_number].1 = value;
            }
            0xf68 => {
                if let Some(index) = self.find(value) {
                    self.clean(index)?;
                }
            }
            0xf5c => {
                if let Some(index) = self.find(value) {
                    self.cache[index].valid = false;
                }
            }
            0xf70 => {
                if let Some(index) = self.find(value) {
                    self.clean(index)?;
                    self.cache[index].valid = false;
                }
            }
            0xf60 | 0xf6c | 0xf74 => {
                let index = (((value >> 5) & 127) as usize) * WAYS + ((value >> 30) as usize);
                if offset != 0xf60 {
                    self.clean(index)?;
                }
                if offset != 0xf6c {
                    self.cache[index].valid = false;
                }
            }
            0xf64 => {
                if let Some(index) = self.find(value) {
                    self.clean(index)?;
                }
            }
            0xf50 => self.instructions.fill(CacheLine::default()),
            0xf58 => {
                for line in &mut self.instructions {
                    if line.address == value & !31 {
                        line.valid = false;
                    }
                }
            }
            _ => return Err("unsupported cache/MPU register"),
        }
        Ok(())
    }
}
