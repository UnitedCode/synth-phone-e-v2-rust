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

pub struct SystemMemory {
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
    /// Firmware image loader, never a CPU/DMA programming operation.
    pub fn load_flash(&mut self, address: u32, width: usize, value: u32) -> Result<()> {
        Self::check(address, width)?;
        if !Self::is_flash(address, width) {
            return Err("not user flash");
        }
        for byte in 0..width {
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
    fn clean(&mut self, index: usize) {
        if self.cache[index].valid && self.cache[index].dirty {
            let line = self.cache[index].clone();
            for (offset, value) in line.data.into_iter().enumerate() {
                self.store_byte(line.address + offset as u32, value);
            }
            self.cache[index].dirty = false;
            self.writebacks += 1;
        }
    }
    fn find(&self, address: u32) -> Option<usize> {
        let base = address & !(LINE - 1);
        let set = (base / LINE) as usize % SETS;
        (set * WAYS..(set + 1) * WAYS)
            .find(|&index| self.cache[index].valid && self.cache[index].address == base)
    }
    fn allocate(&mut self, address: u32) -> usize {
        if let Some(index) = self.find(address) {
            return index;
        }
        let base = address & !(LINE - 1);
        let set = (base / LINE) as usize % SETS;
        let index = (set * WAYS..(set + 1) * WAYS)
            .find(|&i| !self.cache[i].valid)
            .unwrap_or(set * WAYS + self.next_way[set]);
        self.next_way[set] = (index % WAYS + 1) % WAYS;
        self.clean(index);
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
        index
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
                let line = self.allocate(addr);
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
        for line in &block.lines {
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
                self.runtime_retranslations += 1;
                return Ok(true);
            }
        }
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
            return Err("flash programming is not a RAM store");
        }
        for byte in 0..width {
            let addr = address + byte as u32;
            let b = (value >> (byte * 8)) as u8;
            let policy = self.policy(addr)?;
            let line = if policy == Policy::WriteAllocate {
                Some(self.allocate(addr))
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
            return Err("DMA cannot program flash");
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
        format!("data_cache_enabled={}\ncache_line_fills={}\ncache_line_writebacks={}\ndirty_lines={}\ndma_cacheable_bytes={}\ninstruction_cache_enabled={}\ninstruction_fetch_reads={}\ninstruction_cache_fills={}\nruntime_blocks={}\nruntime_instruction_slots={}\nruntime_retranslations={}\n", u32::from(self.ccr & (1 << 16) != 0), self.fills, self.writebacks, self.cache.iter().filter(|line| line.valid && line.dirty).count(), self.dma_cacheable_bytes,u32::from(self.ccr&(1<<17)!=0),self.instruction_reads,self.instruction_fills,self.runtime_blocks,self.runtime_instructions,self.runtime_retranslations)
    }
    pub fn write_scb(&mut self, offset: u64, value: u32) -> Result<()> {
        self.blocks.clear();
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
                    self.clean(index);
                }
            }
            0xf5c => {
                if let Some(index) = self.find(value) {
                    self.cache[index].valid = false;
                }
            }
            0xf70 => {
                if let Some(index) = self.find(value) {
                    self.clean(index);
                    self.cache[index].valid = false;
                }
            }
            0xf60 | 0xf6c | 0xf74 => {
                let index = (((value >> 5) & 127) as usize) * WAYS + ((value >> 30) as usize);
                if offset != 0xf60 {
                    self.clean(index);
                }
                if offset != 0xf6c {
                    self.cache[index].valid = false;
                }
            }
            0xf64 => {
                if let Some(index) = self.find(value) {
                    self.clean(index);
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
