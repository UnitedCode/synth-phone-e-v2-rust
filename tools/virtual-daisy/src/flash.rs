//! STM32H750 bank-one user flash (RM0433 4.3/4.7/4.9).
//! Durations select DS12556 typical/max scenarios, not measured board latency.
use std::collections::VecDeque;
pub const EOP: u32 = 1 << 16;
pub const PGSERR: u32 = 1 << 18;
pub const STRBERR: u32 = 1 << 19;
pub const INCERR: u32 = 1 << 21;
pub enum Action {
    Program(u32, [u8; 32]),
    Erase,
}
struct Pending {
    action: Action,
    finish: u64,
}
pub struct Flash {
    acr: u32,
    cr: u32,
    flags: u32,
    key: u8,
    blocked: bool,
    data: [u8; 32],
    mask: u32,
    address: Option<u32>,
    master: u32,
    pending: VecDeque<Pending>,
    maximum: bool,
    pub programs: u64,
    pub erases: u64,
}
impl Default for Flash {
    fn default() -> Self {
        Self {
            acr: 0x37,
            cr: 0x31,
            flags: 0,
            key: 0,
            blocked: false,
            data: [0xff; 32],
            mask: 0,
            address: None,
            master: 0,
            pending: VecDeque::new(),
            maximum: false,
            programs: 0,
            erases: 0,
        }
    }
}
impl Flash {
    pub fn program_enabled(&self) -> bool {
        self.cr & 2 != 0 && self.flags & (PGSERR | INCERR) == 0
    }
    pub fn pending_program(&self, address: u32) -> bool {
        self.pending
            .iter()
            .any(|op| matches!(op.action,Action::Program(base,_) if base==address))
    }
    pub fn pending_erase(&self) -> bool {
        self.pending
            .iter()
            .any(|op| matches!(op.action, Action::Erase))
    }
    pub fn stats(&self) -> String {
        format!("flash_programs={}\nflash_erases={}\nflash_timing_scenario={}\necc_reprogramming=UNSUPPORTED\n",self.programs,self.erases,if self.maximum {"DS12556_MAXIMUM"} else {"DS12556_TYPICAL"})
    }
    pub fn reset(&mut self) {
        let maximum = self.maximum;
        *self = Self::default();
        self.maximum = maximum;
    }
    pub fn set_maximum(&mut self, maximum: bool) -> Result<(), &'static str> {
        if !self.pending.is_empty() {
            return Err("flash timing profile change during operation");
        }
        self.maximum = maximum;
        Ok(())
    }
    pub fn wait(&self, write: bool, now: u64) -> u64 {
        if write && self.pending.len() < 2 {
            return 0;
        }
        let operation = if write {
            self.pending.front()
        } else {
            self.pending.back()
        };
        operation.map_or(0, |op| op.finish.saturating_sub(now))
    }
    pub fn read(&self, offset: u64) -> Result<u32, &'static str> {
        Ok(match offset {
            0 => self.acr,
            4 => 0,
            0xc => self.cr,
            0x10 => {
                self.flags
                    | if self.mask != 0 { 2 } else { 0 }
                    | if !self.pending.is_empty() { 5 } else { 0 }
            }
            0x14 => 0,
            _ => return Err("unmodeled H750 flash register"),
        })
    }
    pub fn irq(&self) -> bool {
        self.flags & self.cr & 0x0fef0000 != 0
    }
    fn discard_buffer(&mut self) {
        self.mask = 0;
        self.address = None;
        self.data = [0xff; 32];
    }
    fn queue(&mut self, action: Action, now: u64, duration: u64) {
        let start = self.pending.back().map_or(now, |op| op.finish.max(now));
        self.pending.push_back(Pending {
            action,
            finish: start + duration,
        });
    }
    fn program_buffer(&mut self, now: u64) {
        if let Some(address) = self.address {
            let ns = [290_000, 180_000, 130_000, 100_000][((self.cr >> 4) & 3) as usize]
                * (if self.maximum { 2 } else { 1 });
            self.queue(Action::Program(address, self.data), now, ns);
            self.discard_buffer();
        }
    }
    pub fn write(
        &mut self,
        offset: u64,
        value: u32,
        width: usize,
        now: u64,
    ) -> Result<(), &'static str> {
        if offset == 4 {
            if width != 4 || self.blocked || self.cr & 1 == 0 {
                self.blocked = true;
                self.cr |= 1;
                return Err("FLASH key sequence bus error");
            }
            match (self.key, value) {
                (0, 0x45670123) => self.key = 1,
                (1, 0xcdef89ab) => {
                    self.key = 0;
                    self.cr &= !1;
                }
                _ => {
                    self.blocked = true;
                    self.cr |= 1;
                    return Err("FLASH key sequence bus error");
                }
            }
            return Ok(());
        }
        if self.key == 1 {
            self.blocked = true;
            self.cr |= 1;
            return Err("interrupted FLASH key sequence bus error");
        }
        if width != 4 {
            return Err("unsupported flash register access width");
        }
        match offset {
            0 => {
                if !self.pending.is_empty() && (value ^ self.acr) & 0x30 != 0 {
                    return Err("WRHIGHFREQ change during flash operation is undefined");
                }
                self.acr = value & 0x3f;
            }
            0xc => {
                if self.cr & 1 != 0 {
                    return Ok(());
                }
                if value & 1 != 0 {
                    self.cr |= 1;
                    return Ok(());
                }
                if value & 0xc0 != 0 && self.wait(true, now) > 0 {
                    return Err("flash command queue full; retry required");
                }
                if value & (1 << 15) != 0 {
                    return Err("flash CRC operation is not modeled");
                }
                if !self.pending.is_empty() && (value ^ self.cr) & 0x30 != 0 {
                    return Err("PSIZE change during flash operation is undefined");
                }
                self.cr = value & 0x0fef07ff & !0xc0;
                if value & 2 == 0 {
                    self.discard_buffer();
                }
                if value & 0x80 != 0 {
                    if value & 12 == 0 {
                        return Err("START requires sector/bank erase selection");
                    }
                    if value & 8 == 0 && value & 0x700 != 0 {
                        return Err("H750 only has user sector zero");
                    }
                    let parallel = ((self.cr >> 4) & 3) as usize;
                    // Bank erase uses the published mass-erase timing scenario;
                    // sector erase uses tERASE128KB. Exact bank latency is uncalibrated.
                    let duration = if value & 8 != 0 {
                        [13_000_000_000, 8_000_000_000, 6_000_000_000, 5_000_000_000][parallel]
                    } else {
                        [2_000_000_000, 1_800_000_000, 1_100_000_000, 1_000_000_000][parallel]
                    } * (if self.maximum { 2 } else { 1 });
                    self.queue(Action::Erase, now, duration);
                }
                // RM0433: FW has no effect when no buffered bit is zero.
                if value & 0x40 != 0 && self.data.iter().any(|&byte| byte != 0xff) {
                    self.program_buffer(now);
                }
            }
            0x14 => self.flags &= !(value & 0x0fef0000),
            _ => return Err("unmodeled H750 flash register write"),
        }
        Ok(())
    }
    pub fn program(
        &mut self,
        address: u32,
        width: usize,
        value: u64,
        master: u32,
        now: u64,
    ) -> Result<(), &'static str> {
        if ![1, 2, 4, 8].contains(&width) || !crate::memory::SystemMemory::is_flash(address, width)
        {
            return Err("invalid flash programming address/width");
        }
        if self.wait(true, now) > 0 {
            return Err("flash write queue full; retry required");
        }
        if self.cr & 2 == 0 || self.flags & (PGSERR | INCERR) != 0 {
            self.flags |= PGSERR;
            self.discard_buffer();
            return Ok(());
        }
        let base = address & !31;
        if !(address as usize).is_multiple_of(width) || (address & 31) as usize + width > 32 {
            return Err("unaligned/cross-word flash bus transactions are unmodeled");
        }
        if self
            .address
            .is_some_and(|previous| previous != base || self.master != master)
        {
            self.flags |= INCERR;
            return Ok(());
        }
        self.address = Some(base);
        self.master = master;
        for byte in 0..width {
            let offset = ((address & 31) as usize) + byte;
            if self.mask & (1 << offset) != 0 {
                self.flags |= STRBERR;
            }
            self.mask |= 1 << offset;
            self.data[offset] = (value >> (byte * 8)) as u8;
        }
        if self.mask == u32::MAX {
            self.program_buffer(now);
        }
        Ok(())
    }
    pub fn advance(&mut self, now: u64) -> Vec<Action> {
        let mut completed = Vec::new();
        while self.pending.front().is_some_and(|op| op.finish <= now) {
            let op = self.pending.pop_front().unwrap();
            match op.action {
                Action::Program(..) => self.programs += 1,
                Action::Erase => self.erases += 1,
            }
            completed.push(op.action);
            self.flags |= EOP;
        }
        completed
    }
}
