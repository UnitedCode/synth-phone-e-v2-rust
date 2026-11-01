//! STM32H750 QUADSPI functional command/FIFO model with a separate IS25LP064A.
//! Serial shifting is a functional service scenario, not pin-edge simulation.
use crate::nor::NorFlash;
use std::collections::VecDeque;

const MAPPED_DISABLED: &str = "QSPI memory-mapped read while disabled or in another mode";
const MAPPED_RANGE: &str = "QSPI mapped read exceeds FSIZE";
/// RM0433 section23.3.14 requires a bus-error response for these two cases.
/// Protocols, missing models and stopped-clock behavior stay unsupported.
pub fn mapped_bus_error(error: &str) -> bool {
    matches!(error, MAPPED_DISABLED | MAPPED_RANGE)
}

#[derive(Default)]
pub struct Qspi {
    registers: [u32; 13],
    fifo: VecDeque<u8>,
    pending: VecDeque<u8>,
    command: u8,
    address: u32,
    expected: u64,
    written: u64,
    received: u64,
    busy: bool,
    complete: bool,
    error: bool,
    receiving: bool,
    transmitting: bool,
    waiting_address: bool,
    now: u64,
    next_poll: u64,
    polling: bool,
    matched: bool,
    poll_ready: bool,
    last_status: u32,
    kernel_clock: Option<crate::clock::Rate>,
    clock_configured: bool,
    ignored_busy_writes: u64,
    mapped: bool,
    timeout_at: Option<u64>,
    timeout_clocks_left: Option<u64>,
    timed_out: bool,
    schedule_changed: bool,
    chip: NorFlash,
}
impl Qspi {
    pub fn watches_memory(&self) -> bool {
        self.mapped && self.registers[0] & 9 == 9
    }
    pub fn take_schedule_changed(&mut self) -> bool {
        std::mem::take(&mut self.schedule_changed)
    }
    pub fn synchronize_chip(&mut self, now: u64) -> Result<(), &'static str> {
        self.now = self.now.max(now);
        self.chip.advance_time(self.now)
    }
    pub fn configure_clock(
        &mut self,
        rate: Option<crate::clock::Rate>,
    ) -> Result<(), &'static str> {
        if rate.is_some_and(|r| r.numerator == 0 || r.denominator == 0) {
            return Err("invalid QSPI kernel rate");
        }
        let changed = !self.clock_configured || self.kernel_clock != rate;
        if changed {
            if let (Some(deadline), Some(previous)) = (self.timeout_at, self.kernel_clock) {
                let ns = deadline.saturating_sub(self.now);
                let divider = u128::from((self.registers[0] >> 24) + 1);
                self.timeout_clocks_left = Some(
                    u64::try_from(
                        (u128::from(ns) * u128::from(previous.numerator))
                            .div_ceil(u128::from(previous.denominator) * 1_000_000_000 * divider),
                    )
                    .map_err(|_| "QSPI remaining timeout overflow")?,
                );
                self.timeout_at = None;
            }
        }
        self.kernel_clock = rate;
        self.clock_configured = true;
        if changed && self.polling && rate.is_some() {
            self.next_poll = self
                .now
                .checked_add(self.cycles_ns(self.frame_clocks())?)
                .ok_or("QSPI poll time overflow")?;
        }
        if let (true, Some(clocks)) = (changed && rate.is_some(), self.timeout_clocks_left) {
            self.timeout_clocks_left = None;
            self.timeout_at = Some(
                self.now
                    .checked_add(self.cycles_ns(clocks)?)
                    .ok_or("QSPI resumed timeout overflow")?,
            );
            self.schedule_changed = true;
        }
        Ok(())
    }
    pub fn next_event(&self) -> Option<u64> {
        if self.clock_configured && self.kernel_clock.is_none() {
            return None;
        }
        self.polling
            .then_some(self.next_poll)
            .into_iter()
            .chain(self.timeout_at)
            .min()
    }
    pub fn advance_time(&mut self, now: u64) -> Result<(), &'static str> {
        self.chip.advance_time(now)?;
        self.now = now;
        if self.kernel_clock.is_some() && self.timeout_at.is_some_and(|time| now >= time) {
            self.timeout_at = None;
            self.busy = false;
            self.timed_out = true;
        }
        if self.polling && self.next_event().is_some_and(|next| now >= next) {
            self.poll_once()?;
        }
        Ok(())
    }
    pub fn set_maximum(&mut self, maximum: bool) -> Result<(), &'static str> {
        self.chip.set_maximum(maximum)
    }
    pub fn reset_controller(&mut self) {
        if self.transmitting && self.written != 0 {
            // Already-transmitted bytes form a shorter valid page packet when
            // controller reset releases CE. Chip programming then continues.
            self.finish_write()
                .expect("validated QSPI packet must finish on reset");
        }
        let chip = std::mem::take(&mut self.chip);
        *self = Self {
            chip,
            now: self.now,
            kernel_clock: self.kernel_clock,
            clock_configured: self.clock_configured,
            ..Self::default()
        };
    }
    fn status(&self) -> u32 {
        let threshold = ((self.registers[0] >> 8) & 31) as usize + 1;
        let write_threshold = self.registers[0] & 1 != 0 && self.registers[5] >> 26 & 3 == 0;
        let threshold_met = self.poll_ready
            || write_threshold
            || (self.receiving
                && (self.fifo.len() >= threshold
                    || (self.received == self.expected && !self.fifo.is_empty())));
        u32::from(self.error)
            | (u32::from(self.complete) << 1)
            | (u32::from(threshold_met) << 2)
            | (u32::from(self.matched) << 3)
            | (u32::from(self.timed_out) << 4)
            | (u32::from(self.busy) << 5)
            | ((self.fifo.len() as u32) << 8)
    }
    pub fn irq(&self) -> bool {
        self.status() & (self.registers[0] >> 16) & 0x1f != 0
    }
    pub fn read(&mut self, offset: u64, width: usize) -> Result<u32, &'static str> {
        if ![1, 2, 4].contains(&width) {
            return Err("invalid QSPI access width");
        }
        if offset == 0x20 {
            if self.registers[5] >> 26 & 3 == 2 {
                self.poll_ready = false;
                return Ok(if width == 4 {
                    self.last_status
                } else {
                    self.last_status & ((1 << (width * 8)) - 1)
                });
            }
            if !self.receiving || self.fifo.is_empty() {
                return Err("QSPI receive FIFO underflow");
            }
            let mut value = 0;
            for index in 0..width {
                if let Some(byte) = self.fifo.pop_front() {
                    value |= u32::from(byte) << (index * 8);
                } else {
                    break;
                }
            }
            self.fill_fifo()?;
            self.busy = !self.fifo.is_empty();
            return Ok(value);
        }
        if width != 4 || !offset.is_multiple_of(4) || offset > 0x30 {
            return Err("unsupported QSPI register access");
        }
        match offset {
            8 => Ok(self.status()),
            0xc => Ok(0), // FCR is write-only.
            _ => Ok(self.registers[offset as usize / 4]),
        }
    }
    pub fn write(&mut self, offset: u64, value: u32, width: usize) -> Result<(), &'static str> {
        if ![1, 2, 4].contains(&width) {
            return Err("invalid QSPI access width");
        }
        if offset == 0x20 {
            if !self.transmitting {
                return Err("unexpected QSPI transmit data");
            }
            if self.written == 0 {
                self.validate_header()?;
                if !self.address_valid() {
                    return Ok(());
                }
            }
            self.busy = true;
            let accepted = width.min((self.expected - self.written) as usize);
            // RM0433: bytes beyond DLR are flushed after the last data byte.
            for byte in value.to_le_bytes().into_iter().take(accepted) {
                if self.pending.len() == 256 {
                    self.pending.pop_front();
                }
                self.pending.push_back(byte);
                self.written += 1;
            }
            if self.written == self.expected {
                self.finish_write()?;
            }
            return Ok(());
        }
        if width != 4 || !offset.is_multiple_of(4) || offset > 0x30 {
            return Err("unsupported QSPI register access");
        }
        if offset == 8 {
            return Err("QSPI status is read-only");
        }
        if offset == 0xc {
            if value & 1 != 0 {
                self.error = false;
            }
            if value & 2 != 0 {
                self.complete = false;
            }
            if value & 8 != 0 {
                self.matched = false;
            }
            if value & 16 != 0 {
                self.timed_out = false;
            }
            return Ok(());
        }
        if offset == 0 {
            self.registers[0] = value & !2;
            if value & 1 == 0 && self.polling {
                self.polling = false;
                self.busy = false;
            }
            if value & 2 != 0 {
                if self.transmitting && self.written != 0 {
                    self.finish_write()?;
                }
                let aborted = self.busy || self.waiting_address;
                self.fifo.clear();
                self.pending.clear();
                self.receiving = false;
                self.transmitting = false;
                self.waiting_address = false;
                self.busy = false;
                self.polling = false;
                self.poll_ready = false;
                self.mapped = false;
                self.timeout_at = None;
                self.timeout_clocks_left = None;
                self.complete |= aborted;
            }
            return Ok(());
        }
        if self.busy {
            self.ignored_busy_writes += 1;
            return Ok(()); // Fields restricted to BUSY=0 retain their value.
        }
        self.registers[offset as usize / 4] = value;
        if offset == 0x14 {
            self.command = value as u8;
            self.expected = if value >> 24 & 3 != 0 {
                u64::from(self.registers[4]) + 1
            } else {
                0
            };
            self.fifo.clear();
            self.pending.clear();
            self.written = 0;
            self.received = 0;
            self.receiving = false;
            self.transmitting = false;
            self.poll_ready = false;
            self.waiting_address = value >> 10 & 3 != 0;
            if value >> 26 & 3 == 3 {
                self.waiting_address = false;
                self.start(0)?;
            } else if !self.waiting_address {
                self.start(0)?;
            }
        } else if offset == 0x18 && self.waiting_address {
            self.waiting_address = false;
            self.start(value)?;
        }
        Ok(())
    }
    fn validate_header(&self) -> Result<(), &'static str> {
        if self.registers[0] & 1 == 0 {
            return Err("QSPI transfer while disabled");
        }
        if self.registers[0] & 0xc0 != 0 {
            return Err("Daisy has one flash on QSPI bank1");
        }
        let ccr = self.registers[5];
        if ccr & (1 << 31) != 0 {
            return Err("QSPI DDR protocol is not implemented");
        }
        if ccr & (1 << 28) != 0 {
            return Err("QSPI instruction-once continuous-read mode is not implemented");
        }
        if ccr >> 26 & 3 == 2 && self.expected > 4 {
            return Err("QSPI polling reads at most four status bytes");
        }
        let instruction = (ccr >> 8) & 3;
        let address = (ccr >> 10) & 3;
        let data = (ccr >> 24) & 3;
        let read = ccr >> 26 & 3 != 0;
        let mode = if self.chip.is_qpi() { 3 } else { 1 };
        if instruction != mode {
            return Err("IS25LP instruction lane mode mismatch");
        }
        if address != 0 && (ccr >> 12 & 3 != 2) {
            return Err("IS25LP requires a 24-bit address phase");
        }
        match self.command {
            0 | 4 | 6 | 0x35 | 0xf5 | 0x66 | 0x99 | 0x60 | 0xc7 | 0x75 | 0xb0 | 0x7a | 0x30
            | 0xb9 | 0xab
                if address == 0 && data == 0 && !read => {}
            1 | 0x42 | 0xc0 if address == 0 && data == mode && !read && self.expected == 1 => {}
            2 if address == mode && data == mode && !read => {}
            0x20 | 0xd7 | 0x52 | 0xd8 if address == mode && data == 0 && !read => {}
            5 | 0x48 | 0x9f | 0xaf | 0xab if address == 0 && data == mode && read => {}
            0x90 if address == mode && data == mode && read => {}
            3 | 0x0b if address == mode && data == mode && read => {}
            0xeb if address == 3
                && data == 3
                && read
                && (self.chip.is_qpi() || self.chip.quad_enabled()) => {}
            _ => return Err("unsupported IS25LP command or QSPI phase configuration"),
        }
        if read {
            if let (Some(rate), Some(maximum)) =
                (self.kernel_clock, self.chip.array_read_max_hz(self.command))
            {
                let divisor = u128::from((self.registers[0] >> 24) + 1);
                if u128::from(rate.numerator)
                    > u128::from(maximum) * u128::from(rate.denominator) * divisor
                {
                    return Err("IS25LP array read exceeds the 3.3V datasheet frequency limit");
                }
            }
            let alternate_mode = (ccr >> 14) & 3;
            let mode_clocks = if alternate_mode == 0 {
                0
            } else {
                (8 * (((ccr >> 16) & 3) + 1)) / (1 << (alternate_mode - 1))
            };
            if mode_clocks + ((ccr >> 18) & 31) != self.chip.read_dummy_clocks(self.command)? {
                return Err("IS25LP read dummy/mode clock count mismatch");
            }
        }
        Ok(())
    }
    fn start(&mut self, address: u32) -> Result<(), &'static str> {
        self.address = address;
        if self.registers[5] >> 26 & 3 == 3 {
            self.validate_header()?;
            self.mapped = true;
            return Ok(()); // AHB/AXI read initiates the serial transaction.
        }
        if self.registers[5] >> 26 & 3 == 0 && self.expected != 0 {
            self.transmitting = true;
            self.busy = false;
            return Ok(()); // The serial write starts only on its first data.
        }
        self.validate_header()?;
        if !self.address_valid() {
            return Ok(());
        }
        if self.registers[5] >> 26 & 3 == 2 {
            self.polling = true;
            self.busy = true;
            if !self.clock_configured {
                self.poll_once()?;
            } else if self.kernel_clock.is_some() {
                self.next_poll = self
                    .now
                    .checked_add(self.cycles_ns(self.frame_clocks())?)
                    .ok_or("QSPI poll time overflow")?;
            }
        } else if self.registers[5] >> 26 & 3 == 1 {
            let bytes = self.chip.transact(
                self.command,
                address & 0xffffff,
                &[],
                self.expected.min(32) as usize,
                self.chip.is_qpi(),
            )?;
            self.received = bytes.len() as u64;
            self.fifo.extend(bytes);
            self.receiving = true;
            self.busy = !self.fifo.is_empty();
            self.complete |= self.received == self.expected;
        } else if self.expected != 0 {
            self.transmitting = true;
            self.busy = false;
        } else {
            self.chip
                .transact(self.command, address & 0xffffff, &[], 0, self.chip.is_qpi())?;
            self.complete = true;
        }
        Ok(())
    }
    fn address_valid(&mut self) -> bool {
        if self.registers[5] >> 10 & 3 == 0 {
            return true;
        }
        let maximum = 1_u64 << (((self.registers[1] >> 16) & 31) + 1);
        if u64::from(self.address) + self.expected.max(1) > maximum {
            self.error = true;
            self.busy = false;
            self.transmitting = false;
            self.receiving = false;
            false
        } else {
            true
        }
    }
    fn fill_fifo(&mut self) -> Result<(), &'static str> {
        let was_complete = self.received == self.expected;
        // Functional 32-byte FIFO with four-byte refill groups. RM0433's
        // accompanying FLEVEL<=11 example is inconsistent with its 32-byte
        // depth; this uses the stated four-vacant-byte rule, not that example.
        if !was_complete && self.fifo.len() > 28 {
            return Ok(());
        }
        while self.fifo.len() < 32 && self.received < self.expected {
            self.fifo.push_back(self.chip.read_command_byte(
                self.command,
                self.address,
                self.received,
            )?);
            self.received += 1;
        }
        if !was_complete && self.received == self.expected {
            self.complete = true;
        }
        Ok(())
    }
    fn poll_once(&mut self) -> Result<(), &'static str> {
        let bytes = self.chip.transact(
            self.command,
            self.address & 0xffffff,
            &[],
            self.expected as usize,
            self.chip.is_qpi(),
        )?;
        self.last_status = bytes
            .iter()
            .enumerate()
            .fold(0, |value, (i, &byte)| value | (u32::from(byte) << (i * 8)));
        self.poll_ready = true;
        let mask = self.registers[9];
        let equal = !(self.last_status ^ self.registers[10]) & mask;
        let matched = if self.registers[0] & (1 << 23) == 0 {
            equal == mask
        } else {
            equal != 0
        };
        self.matched |= matched;
        if matched && self.registers[0] & (1 << 22) != 0 {
            self.polling = false;
            self.busy = false;
        }
        let interval = u64::from(self.registers[11] & 0xffff);
        let delay = if self.clock_configured {
            let cs_high = u64::from((self.registers[1] >> 8) & 7) + 1;
            self.cycles_ns(self.frame_clocks() + interval.max(cs_high))?
        } else {
            interval + 1
        }; // Untimed standalone protocol tests.
        self.next_poll = self
            .now
            .checked_add(delay)
            .ok_or("QSPI poll time overflow")?;
        Ok(())
    }
    fn frame_clocks(&self) -> u64 {
        self.frame_clocks_for(self.expected)
    }
    fn frame_clocks_for(&self, bytes: u64) -> u64 {
        let ccr = self.registers[5];
        let phase = |bits: u64, mode: u32| {
            if mode == 0 {
                0
            } else {
                bits / (1 << (mode - 1))
            }
        };
        phase(8, (ccr >> 8) & 3)
            + phase(8 * u64::from(((ccr >> 12) & 3) + 1), (ccr >> 10) & 3)
            + phase(8 * u64::from(((ccr >> 16) & 3) + 1), (ccr >> 14) & 3)
            + u64::from((ccr >> 18) & 31)
            + phase(bytes * 8, (ccr >> 24) & 3)
    }
    pub fn peek_mapped(&self, address: u32, bytes: usize) -> Vec<u8> {
        (0..bytes)
            .map(|i| self.chip.peek(address.wrapping_add(i as u32)))
            .collect()
    }
    pub fn mapped_bus_fault(&self, address: u32, bytes: usize) -> bool {
        !self.mapped
            || self.registers[0] & 1 == 0
            || bytes == 0
            || u64::from(address) + bytes as u64 > 1_u64 << (((self.registers[1] >> 16) & 31) + 1)
    }
    fn check_mapped(&self, address: u32, bytes: usize) -> Result<(), &'static str> {
        if !self.mapped || self.registers[0] & 1 == 0 {
            return Err(MAPPED_DISABLED);
        }
        self.validate_header()?;
        let maximum = 1_u64 << (((self.registers[1] >> 16) & 31) + 1);
        if bytes == 0 || u64::from(address) + bytes as u64 > maximum {
            return Err(MAPPED_RANGE);
        }
        if self.clock_configured && self.kernel_clock.is_none() {
            return Err("QSPI mapped read with stopped kernel clock");
        }
        Ok(())
    }
    /// One serial transaction per system-bus request. Prefetch beyond a cache
    /// line and nCS overlap are not inferred; timing is an explicit scenario.
    pub fn mapped_service_ns(&self, address: u32, bytes: usize) -> Result<u64, &'static str> {
        self.check_mapped(address, bytes)?;
        self.cycles_ns(
            self.frame_clocks_for(bytes as u64) + u64::from((self.registers[1] >> 8) & 7) + 1,
        )
    }
    pub fn read_mapped(&mut self, address: u32, bytes: usize) -> Result<Vec<u8>, &'static str> {
        self.check_mapped(address, bytes)?;
        let data = self.chip.transact(
            self.command,
            address & 0xffffff,
            &[],
            bytes,
            self.chip.is_qpi(),
        )?;
        self.busy = true;
        if self.registers[0] & 8 != 0 {
            // Timeout begins after the 32-byte prefetch buffer has filled.
            self.timeout_at = Some(
                self.now
                    .checked_add(self.cycles_ns(
                        self.frame_clocks_for(bytes.max(32) as u64)
                            + u64::from(self.registers[12] & 0xffff),
                    )?)
                    .ok_or("QSPI timeout overflow")?,
            );
            self.schedule_changed = true;
        }
        Ok(data)
    }
    fn cycles_ns(&self, cycles: u64) -> Result<u64, &'static str> {
        let rate = self.kernel_clock.ok_or("QSPI kernel clock stopped")?;
        let divider = u128::from((self.registers[0] >> 24) + 1);
        u64::try_from(
            (u128::from(cycles) * divider * u128::from(rate.denominator) * 1_000_000_000)
                .div_ceil(u128::from(rate.numerator)),
        )
        .map_err(|_| "QSPI time overflow")
    }
    fn finish_write(&mut self) -> Result<(), &'static str> {
        let address = if self.command == 2 {
            (self.address & !255)
                | ((u64::from(self.address) + self.written - self.pending.len() as u64) as u32
                    & 255)
        } else {
            self.address
        };
        let bytes: Vec<_> = self.pending.drain(..).collect();
        self.chip.transact(
            self.command,
            address & 0xffffff,
            &bytes,
            0,
            self.chip.is_qpi(),
        )?;
        self.transmitting = false;
        self.busy = false;
        self.complete = true;
        Ok(())
    }
}
