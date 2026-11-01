//! GPIO register behavior with pull resistors and external drive retained across RCC reset.
#[derive(Default)]
pub struct Gpio {
    registers: [u32; 10],
    external_mask: u16,
    external_levels: u16,
    alternate: [u16; 16],
    lock_phase: u8,
    lock_candidate: u16,
    locked: u16,
    lock_active: bool,
}
impl Gpio {
    pub fn release(&mut self, pin: u32) {
        if pin < 16 {
            self.external_mask &= !(1 << pin);
            self.external_levels &= !(1 << pin);
        }
    }
    pub fn reset(&mut self, port: u32) {
        self.registers = [0; 10];
        self.lock_phase = 0;
        self.lock_candidate = 0;
        self.locked = 0;
        self.lock_active = false;
        self.registers[0] = if port == 0 {
            0xabff_ffff
        } else if port == 1 {
            0xffff_febf
        } else {
            u32::MAX
        };
        self.registers[3] = if port == 0 {
            0x6400_0000
        } else if port == 1 {
            0x0000_0100
        } else {
            0
        };
    }
    pub fn input(&self) -> u32 {
        let mut value = 0;
        for pin in 0..16 {
            let mode = (self.registers[0] >> (pin * 2)) & 3;
            let output = if mode == 1 {
                Some(self.registers[5] & (1 << pin) != 0)
            } else if mode == 2 {
                let af = (self.registers[8 + pin / 8] >> ((pin % 8) * 4)) & 15;
                Some(self.alternate[af as usize] & (1 << pin) != 0)
            } else {
                None
            };
            let high = if mode == 3 || output == Some(false) {
                false
            } else if output == Some(true) && self.registers[1] & (1 << pin) == 0 {
                true
            } else if self.external_mask & (1 << pin) != 0 {
                self.external_levels & (1 << pin) != 0
            } else {
                (self.registers[3] >> (pin * 2)) & 3 == 1
            };
            if high {
                value |= 1 << pin;
            }
        }
        value
    }
    pub fn alternate(&mut self, pin: u32, af: u32, high: bool) {
        if pin < 16 && af < 16 {
            if high {
                self.alternate[af as usize] |= 1 << pin;
            } else {
                self.alternate[af as usize] &= !(1 << pin);
            }
        }
    }
    pub fn drive(&mut self, pin: u32, high: bool) {
        if pin < 16 {
            self.external_mask |= 1 << pin;
            if high {
                self.external_levels |= 1 << pin;
            } else {
                self.external_levels &= !(1 << pin);
            }
        }
    }
    pub fn read(&mut self, offset: u64) -> Result<u32, &'static str> {
        if !offset.is_multiple_of(4) || offset > 0x24 {
            return Err("unsupported GPIO register");
        }
        Ok(match offset {
            0x10 => self.input(),
            0x18 => 0,
            0x1c => {
                if self.lock_phase == 3 {
                    self.locked = self.lock_candidate;
                    self.lock_active = true;
                }
                self.lock_phase = 0;
                u32::from(self.lock_candidate) | (u32::from(self.lock_active) << 16)
            }
            _ => self.registers[offset as usize / 4],
        })
    }
    pub fn write(&mut self, offset: u64, value: u32) -> Result<(), &'static str> {
        if !offset.is_multiple_of(4) || offset > 0x24 {
            return Err("unsupported GPIO register");
        }
        match offset {
            0x10 => return Err("GPIO input is read-only"),
            0x18 => {
                self.registers[5] = (self.registers[5] & !(value >> 16)) | (value & 0xffff);
            }
            0x1c => {
                if self.lock_active {
                    return Ok(());
                }
                let mask = value as u16;
                let key = value & (1 << 16) != 0;
                self.lock_phase = match self.lock_phase {
                    0 if key => {
                        self.lock_candidate = mask;
                        1
                    }
                    1 if !key && mask == self.lock_candidate => 2,
                    2 if key && mask == self.lock_candidate => 3,
                    _ => 0,
                };
                if self.lock_phase == 0 {
                    self.lock_candidate = mask;
                }
            }
            0x14 => self.registers[5] = value & 0xffff,
            _ => {
                let index = offset as usize / 4;
                let mut locked_bits = 0;
                let (first, width) = if index >= 8 {
                    ((index - 8) * 8, 4)
                } else if index == 1 {
                    (0, 1)
                } else {
                    (0, 2)
                };
                for pin in first..(first + 32 / width).min(16) {
                    if self.locked & (1 << pin) != 0 {
                        locked_bits |= ((1 << width) - 1) << ((pin - first) * width);
                    }
                }
                let value = if index == 1 { value & 0xffff } else { value };
                self.registers[index] =
                    (self.registers[index] & locked_bits) | (value & !locked_bits);
            }
        }
        Ok(())
    }
}
