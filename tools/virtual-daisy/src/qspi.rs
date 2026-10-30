//! Functional STM32 QSPI + IS25LP064 startup command protocol.
use std::collections::VecDeque;
#[derive(Default)]
pub struct Qspi {
    registers: [u32; 13],
    fifo: VecDeque<u8>,
    pending: Vec<u8>,
    command: u8,
    expected: usize,
    busy: bool,
    complete: bool,
    write_enable: bool,
    quad: bool,
    status: u8,
    read_parameters: u8,
}
impl Qspi {
    pub fn read(&mut self, offset: u64, width: usize) -> Result<u32, &'static str> {
        if offset == 0x20 {
            let mut result = 0;
            for i in 0..width {
                result |=
                    (self.fifo.pop_front().ok_or("QSPI receive FIFO underflow")? as u32) << (i * 8);
            }
            return Ok(result);
        }
        if !offset.is_multiple_of(4) || offset > 0x30 {
            return Err("unsupported QSPI register");
        }
        if offset == 8 {
            let threshold = ((self.registers[0] >> 8) & 31) as usize + 1;
            return Ok((u32::from(self.busy) << 5)
                | (u32::from(self.complete) << 1)
                | (u32::from(self.fifo.len() >= threshold) << 2)
                | ((self.fifo.len() as u32) << 8));
        }
        Ok(self.registers[offset as usize / 4])
    }
    pub fn write(&mut self, offset: u64, value: u32, width: usize) -> Result<(), &'static str> {
        if offset == 0x20 {
            if ![1, 0xc0].contains(&self.command) || !self.busy {
                return Err("unexpected QSPI transmit data");
            }
            for byte in value.to_le_bytes().into_iter().take(width) {
                self.pending.push(byte);
            }
            if self.pending.len() >= self.expected {
                if self.pending.len() != 1 || !self.write_enable {
                    return Err("invalid IS25LP register write");
                }
                if self.command == 1 {
                    self.status = self.pending[0] & 0xfc;
                } else {
                    self.read_parameters = self.pending[0];
                }
                self.write_enable = false;
                self.busy = false;
                self.complete = true;
            }
            return Ok(());
        }
        if !offset.is_multiple_of(4) || offset > 0x30 {
            return Err("unsupported QSPI register");
        }
        if offset == 8 {
            return Err("QSPI status is read-only");
        }
        if offset == 0xc {
            if value & 2 != 0 {
                self.complete = false;
            }
            return Ok(());
        }
        self.registers[offset as usize / 4] = value;
        if offset == 0 && value & 2 != 0 {
            self.fifo.clear();
            self.pending.clear();
            self.busy = false;
            self.registers[0] &= !2;
        }
        if offset != 0x14 || value & (3 << 8) == 0 {
            return Ok(());
        }
        if self.registers[0] & 1 == 0 {
            return Err("QSPI transfer while controller disabled");
        }
        let instruction_mode = (value >> 8) & 3;
        if instruction_mode != if self.quad { 3 } else { 1 } {
            return Err("IS25LP instruction lane mode mismatch");
        }
        self.command = value as u8;
        self.expected = self.registers[4] as usize + 1;
        if self.expected > 256 {
            return Err("QSPI startup transfer too large");
        }
        self.fifo.clear();
        self.pending.clear();
        self.complete = false;
        match self.command {
            6 => self.write_enable = true,
            4 => self.write_enable = false,
            0x35 => {
                if !self.write_enable {
                    return Err("QPI entry needs write enable");
                }
                self.quad = true;
                self.write_enable = false;
            }
            0xf5 => self.quad = false,
            0x9f => self
                .fifo
                .extend([0x9d, 0x60, 0x17].into_iter().take(self.expected)),
            5 => self.fifo.extend(std::iter::repeat_n(
                self.status | (u8::from(self.write_enable) << 1),
                self.expected,
            )),
            1 | 0xc0 => {
                if !self.write_enable {
                    return Err("IS25LP register write needs write enable");
                }
                self.busy = true;
                return Ok(());
            }
            _ => return Err("unsupported IS25LP command"),
        }
        self.busy = false;
        self.complete = true;
        Ok(())
    }
}
