//! IS25LP064A serial NOR chip, independent of its STM32 QSPI controller.
//! Protocol and timing reference: ISSI 25LP032-64A-B, Rev A, 2015-11-06.
use std::collections::BTreeMap;
pub const CAPACITY: u32 = 8 * 1024 * 1024;
enum Action {
    Program(Vec<(u32, u8)>),
    Erase(u32, u32),
    Status(u8),
    Function(u8),
}
struct Operation {
    finish: u64,
    suspend_after: u64,
    action: Action,
}
struct Page {
    data: [u8; 256],
    programmed: [u64; 4],
}
pub struct NorFlash {
    pages: BTreeMap<u32, Box<Page>>,
    now: u64,
    pending: Option<Operation>,
    suspended: Option<Operation>,
    suspend_at: Option<u64>,
    power_down: bool,
    write_enable: bool,
    qpi: bool,
    status: u8,
    function: u8,
    read_parameters: u8,
    reset_enable: bool,
    recovery_until: u64,
    write_protect_low: bool,
    maximum: bool,
    continuous: Option<u8>,
}
impl Default for NorFlash {
    fn default() -> Self {
        Self {
            pages: BTreeMap::new(),
            now: 0,
            pending: None,
            suspended: None,
            suspend_at: None,
            power_down: false,
            write_enable: false,
            qpi: false,
            status: 0,
            function: 0,
            read_parameters: 0xe0,
            reset_enable: false,
            recovery_until: 0,
            write_protect_low: false,
            maximum: false,
            continuous: None,
        }
    }
}
impl NorFlash {
    pub fn continuous_opcode(&self) -> Option<u8> {
        self.continuous
    }
    /// Decode an SDR read packet. Mode-less dummy phases retain the existing
    /// functional non-AX scenario; explicit mode bytes govern continuous reads.
    pub fn framed_read(
        &mut self,
        opcode: u8,
        address: u32,
        read: usize,
        instruction: bool,
        mode: Option<u8>,
    ) -> Result<Vec<u8>, &'static str> {
        if (instruction && self.continuous.is_some())
            || (!instruction && self.continuous != Some(opcode))
        {
            return Err("IS25LP continuous-read instruction phase mismatch");
        }
        let result = self.execute(opcode, address, &[], read, self.qpi)?;
        if matches!(opcode, 0xbb | 0xeb) {
            self.continuous = mode.filter(|&byte| byte >> 4 == 0xa).map(|_| opcode);
        }
        Ok(result)
    }
    /// Translation/debug backing view: no serial command, time or cache effects.
    pub fn peek(&self, address: u32) -> u8 {
        self.byte(address)
    }
    pub fn is_qpi(&self) -> bool {
        self.qpi
    }
    pub fn quad_enabled(&self) -> bool {
        self.status & 0x40 != 0
    }
    /// Table 6.10 includes the I/O-read mode clocks in this total.
    pub fn read_dummy_clocks(&self, opcode: u8) -> Result<u32, &'static str> {
        match opcode {
            3 if !self.qpi => Ok(0),
            0x0b if !self.qpi => Ok(8),
            0x3b | 0x6b if !self.qpi => Ok(8),
            0xbb if !self.qpi => Ok([4, 4, 8, 8][usize::from((self.read_parameters >> 3) & 3)]),
            0x0b | 0xeb => Ok([6, 4, 8, 10][usize::from((self.read_parameters >> 3) & 3)]),
            5 | 0x48 | 0x9f | 0xaf | 0x90 => Ok(0),
            0xab => Ok(if self.qpi { 6 } else { 24 }),
            _ => Err("unsupported IS25LP064A read latency"),
        }
    }
    pub fn array_read_max_hz(&self, opcode: u8) -> Option<u64> {
        match opcode {
            3 => Some(50_000_000),
            0x0b if !self.qpi => Some(133_000_000),
            0x3b | 0x6b if !self.qpi => Some(133_000_000),
            0xbb if !self.qpi => Some(if self.read_parameters & 0x10 == 0 {
                104_000_000
            } else {
                133_000_000
            }),
            0x0b | 0xeb => Some(
                [104_000_000, 84_000_000, 133_000_000, 133_000_000]
                    [usize::from((self.read_parameters >> 3) & 3)],
            ),
            _ => None,
        }
    }
    /// Continue a previously accepted read command without issuing another opcode.
    pub fn read_command_byte(
        &self,
        opcode: u8,
        address: u32,
        index: u64,
    ) -> Result<u8, &'static str> {
        match opcode {
            5 => Ok(self.status
                | (u8::from(self.write_enable) << 1)
                | u8::from(self.pending.is_some())),
            0x48 => Ok(self.function),
            0x9f | 0xaf => Ok([0x9d, 0x60, 0x17][(index % 3) as usize]),
            0xab => Ok(0x16),
            0x90 => Ok([0x9d, 0x16][((u64::from(address) + index) & 1) as usize]),
            3 | 0x0b | 0xeb | 0x3b | 0x6b | 0xbb if self.pending.is_none() => {
                self.array_byte(self.read_address(address, index as u32))
            }
            _ => Err("unsupported or unavailable IS25LP064A read stream"),
        }
    }
    pub fn set_write_protect(&mut self, low: bool) {
        self.write_protect_low = low;
    }
    pub fn set_maximum(&mut self, maximum: bool) -> Result<(), &'static str> {
        if self.pending.is_some() || self.suspended.is_some() {
            return Err("cannot change timing scenario during a NOR operation");
        }
        self.maximum = maximum;
        Ok(())
    }
    fn duration(&self, typical: u64, maximum: u64) -> u64 {
        if self.maximum {
            maximum
        } else {
            typical
        }
    }
    pub fn transact(
        &mut self,
        opcode: u8,
        address: u32,
        data: &[u8],
        read: usize,
        qpi: bool,
    ) -> Result<Vec<u8>, &'static str> {
        if self.continuous.is_some() {
            return Err("IS25LP continuous-read mode requires an address without instruction");
        }
        self.execute(opcode, address, data, read, qpi)
    }
    fn execute(
        &mut self,
        opcode: u8,
        address: u32,
        data: &[u8],
        read: usize,
        qpi: bool,
    ) -> Result<Vec<u8>, &'static str> {
        if qpi != self.qpi {
            return Err("IS25LP064A instruction lane mode mismatch");
        }
        if address >= 1 << 24 {
            return Err("IS25LP064A uses a 24-bit address");
        }
        if self.now < self.recovery_until {
            return if read == 0 {
                Ok(Vec::new())
            } else {
                Err("IS25LP064A output is undefined during reset/power recovery")
            };
        }
        if self.power_down {
            if opcode == 0xab {
                if read != 0 {
                    return Err("combined deep-power release and ID read is not implemented");
                }
                self.power_down = false;
                self.recovery_until = self.now.checked_add(3000).ok_or("NOR wake time overflow")?;
                return Ok(Vec::new());
            }
            return if read == 0 {
                Ok(Vec::new())
            } else {
                Err("IS25LP064A output is high impedance in deep power down")
            };
        }
        let reset_enabled = self.reset_enable;
        self.reset_enable = opcode == 0x66;
        if opcode == 0x99 && reset_enabled && (self.pending.is_some() || self.suspended.is_some()) {
            return Err("reset during a NOR nonvolatile operation has indeterminate data effects");
        }
        if matches!(opcode, 0x75 | 0xb0) {
            if self.pending.as_ref().is_some_and(|operation| {
                matches!(
                    operation.action,
                    Action::Program(_) | Action::Erase(_, 1..CAPACITY)
                )
            }) && self.suspend_at.is_none()
            {
                // Section8.21 requires at least 400us of resumed operation
                // before another suspend. Behavior outside that limit is not
                // specified; reject it rather than inventing a chip response.
                if self.now < self.pending.as_ref().unwrap().suspend_after {
                    return Err("IS25LP suspend requires 400us since resume");
                }
                self.suspend_at = Some(
                    self.now
                        .checked_add(100_000)
                        .ok_or("NOR suspend time overflow")?,
                );
            }
            return Ok(Vec::new());
        }
        if self.pending.is_some() && opcode != 5 {
            return if read == 0 {
                Ok(Vec::new())
            } else {
                Err("IS25LP064A read output is undefined while busy")
            };
        }
        if self.suspended.is_some()
            && !matches!(
                opcode,
                0 | 3
                    | 5
                    | 0x0b
                    | 0xeb
                    | 0x3b
                    | 0x6b
                    | 0xbb
                    | 0x48
                    | 0x7a
                    | 0x30
                    | 0xab
                    | 0xc0
                    | 0x9f
                    | 0xaf
                    | 0x90
                    | 0x66
                    | 0x99
            )
        {
            return if read == 0 {
                Ok(Vec::new())
            } else {
                Err("command unavailable while NOR operation suspended")
            };
        }
        match opcode {
            0 | 0x66 => {}
            0xb9 => {
                self.power_down = true;
                self.recovery_until = self
                    .now
                    .checked_add(3000)
                    .ok_or("NOR power-down time overflow")?;
            }
            0x7a | 0x30 => {
                if let Some(mut operation) = self.suspended.take() {
                    operation.suspend_after = self
                        .now
                        .checked_add(400_000)
                        .ok_or("NOR resume-to-suspend time overflow")?;
                    operation.finish = self
                        .now
                        .checked_add(operation.finish)
                        .ok_or("NOR resume time overflow")?;
                    self.pending = Some(operation);
                    self.function &= !12;
                }
            }
            0x99 => {
                if reset_enabled {
                    self.qpi = false;
                    self.read_parameters = 0xe0;
                    self.write_enable = false;
                    self.recovery_until = self
                        .now
                        .checked_add(100_000)
                        .ok_or("NOR reset time overflow")?;
                }
            }
            6 => self.write_enable = true,
            4 => self.write_enable = false,
            0x35 => self.qpi = true,
            0xf5 => self.qpi = false,
            0xc0 => {
                if data.len() != 1 {
                    return Err("IS25LP064A read parameters require one byte");
                }
                self.read_parameters = data[0];
            }
            5 => {
                return Ok(vec![
                    self.status
                        | (u8::from(self.write_enable) << 1)
                        | u8::from(self.pending.is_some());
                    read
                ])
            }
            0x9f | 0xaf if opcode == 0x9f || qpi => {
                return Ok((0..read).map(|i| [0x9d, 0x60, 0x17][i % 3]).collect())
            }
            0xab => return Ok(vec![0x16; read]),
            0x90 => {
                return Ok((0..read)
                    .map(|i| [0x9d, 0x16][(address as usize + i) & 1])
                    .collect())
            }
            0x48 => return Ok(vec![self.function; read]),
            3 if !qpi => {
                return (0..read)
                    .map(|i| self.array_byte(self.read_address(address, i as u32)))
                    .collect::<Result<Vec<_>, _>>()
            }
            0x3b | 0x6b | 0xbb if !qpi && (opcode != 0x6b || self.quad_enabled()) => {
                return (0..read)
                    .map(|i| self.array_byte(self.read_address(address, i as u32)))
                    .collect()
            }
            0x0b | 0xeb if opcode != 0xeb || qpi || self.quad_enabled() => {
                return (0..read)
                    .map(|i| self.array_byte(self.read_address(address, i as u32)))
                    .collect::<Result<Vec<_>, _>>()
            }
            2 | 0x32 | 0x38 if opcode == 2 || (!qpi && self.quad_enabled()) => {
                if self.write_enable && !data.is_empty() && !self.protected(address, 1) {
                    let base = address & !(255) & (CAPACITY - 1);
                    let bytes: Vec<_> = data
                        .iter()
                        .enumerate()
                        .skip(data.len().saturating_sub(256))
                        .map(|(i, &value)| (base | ((address.wrapping_add(i as u32)) & 255), value))
                        .collect();
                    // Sections8.8–8.10 require erase before programming the
                    // same byte again, even if its previous data was FF. Fail
                    // this unsupported sequence before changing any cell.
                    if bytes.iter().any(|&(address, _)| {
                        self.pages.get(&(address >> 8)).is_some_and(|page| {
                            let byte = (address & 255) as usize;
                            page.programmed[byte / 64] & (1 << (byte % 64)) != 0
                        })
                    }) {
                        return Err("IS25LP reprogramming a byte requires erase");
                    }
                    self.pending = Some(Operation {
                        suspend_after: 0,
                        finish: self
                            .now
                            .checked_add(if data.len() == 1 {
                                self.duration(8_000, 40_000)
                            } else {
                                self.duration(200_000, 800_000)
                            })
                            .ok_or("NOR operation time overflow")?,
                        action: Action::Program(bytes),
                    });
                }
            }
            1 | 0x42 => {
                if data.len() != 1 {
                    return Err("IS25LP064A status write requires one byte");
                }
                let hardware_lock =
                    opcode == 1 && self.write_protect_low && !qpi && self.status & 0xc0 == 0x80;
                if self.write_enable && !hardware_lock {
                    self.pending = Some(Operation {
                        suspend_after: 0,
                        finish: self
                            .now
                            .checked_add(self.duration(2_000_000, 15_000_000))
                            .ok_or("NOR operation time overflow")?,
                        action: if opcode == 1 {
                            Action::Status(data[0] & 0xfc)
                        } else {
                            Action::Function(data[0] & 0xf2)
                        },
                    });
                }
            }
            0x20 | 0xd7 | 0x52 | 0xd8 | 0x60 | 0xc7 => {
                if self.write_enable {
                    let (size, typical, maximum) = match opcode {
                        0x20 | 0xd7 => (4096, 70_000_000, 300_000_000),
                        0x52 => (32768, 100_000_000, 500_000_000),
                        0xd8 => (65536, 150_000_000, 1_000_000_000),
                        _ => (CAPACITY, 16_000_000_000, 45_000_000_000),
                    };
                    let duration = self.duration(typical, maximum);
                    let base = address & (CAPACITY - 1) & !(size - 1);
                    if !self.protected(base, size) {
                        self.pending = Some(Operation {
                            suspend_after: 0,
                            finish: self
                                .now
                                .checked_add(duration)
                                .ok_or("NOR operation time overflow")?,
                            action: Action::Erase(base, size),
                        });
                    }
                }
            }
            _ => return Err("unsupported IS25LP064A command"),
        }
        Ok(Vec::new())
    }
    fn byte(&self, address: u32) -> u8 {
        let address = address & (CAPACITY - 1);
        self.pages
            .get(&(address >> 8))
            .map_or(0xff, |page| page.data[(address & 255) as usize])
    }
    fn array_byte(&self, address: u32) -> Result<u8, &'static str> {
        let physical = address & (CAPACITY - 1);
        // Partial charge/program state inside the suspended target is not
        // specified. Do not make its old bytes a false deterministic oracle.
        if self
            .suspended
            .as_ref()
            .is_some_and(|operation| match &operation.action {
                Action::Program(bytes) => bytes
                    .iter()
                    .any(|&(address, _)| address & !255 == physical & !255),
                Action::Erase(base, size) => (*base..(*base + *size)).contains(&physical),
                _ => false,
            })
        {
            return Err("read of suspended NOR target has unmodeled intermediate data");
        }
        Ok(self.byte(address))
    }
    fn read_address(&self, start: u32, offset: u32) -> u32 {
        let mask = if self.read_parameters & 4 != 0 {
            (8 << (self.read_parameters & 3)) - 1
        } else {
            CAPACITY - 1
        };
        (start & !mask) | (start.wrapping_add(offset) & mask)
    }
    fn protected(&self, address: u32, length: u32) -> bool {
        let bp = (self.status >> 2) & 15;
        if bp == 0 {
            return false;
        }
        let size = if bp >= 8 { CAPACITY } else { 65536 << (bp - 1) };
        if self.function & 2 != 0 {
            address & (CAPACITY - 1) < size
        } else {
            u64::from(address & (CAPACITY - 1)) + u64::from(length) > u64::from(CAPACITY - size)
        }
    }
    pub fn advance_time(&mut self, now: u64) -> Result<(), &'static str> {
        if now < self.now {
            return Err("NOR clock moved backwards");
        }
        self.now = now;
        if self.suspend_at.is_some_and(|at| at <= now) {
            let at = self.suspend_at.take().unwrap();
            if self
                .pending
                .as_ref()
                .is_some_and(|operation| operation.finish > at)
            {
                let mut operation = self.pending.take().unwrap();
                operation.finish -= at; // Explicit scenario: work continues during tSUS.
                self.function |= if matches!(operation.action, Action::Program(_)) {
                    4
                } else {
                    8
                };
                self.write_enable = false;
                self.suspended = Some(operation);
            }
        }
        if self.pending.as_ref().is_some_and(|op| op.finish <= now) {
            let op = self.pending.take().unwrap();
            match op.action {
                Action::Program(bytes) => {
                    for (address, value) in bytes {
                        let page = self.pages.entry(address >> 8).or_insert_with(|| {
                            Box::new(Page {
                                data: [0xff; 256],
                                programmed: [0; 4],
                            })
                        });
                        let byte = (address & 255) as usize;
                        page.data[byte] &= value;
                        page.programmed[byte / 64] |= 1 << (byte % 64);
                    }
                }
                Action::Erase(base, size) => self
                    .pages
                    .retain(|&page, _| !((base >> 8)..((base + size) >> 8)).contains(&page)),
                Action::Status(value) => self.status = value,
                Action::Function(value) => self.function |= value,
            }
            self.write_enable = false;
        }
        Ok(())
    }
}
