//! GPIO register behavior with pull resistors and external drive retained across RCC reset.
#[derive(Default)]
pub struct Gpio {
    registers: [u32; 10],
    external_mask: u16,
    external_levels: u16,
    alternate: [u16; 16],
}
impl Gpio {
    pub fn reset(&mut self, port: u32) {
        self.registers = [0; 10];
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
            let high = if mode == 1 {
                self.registers[5] & (1 << pin) != 0
            } else if mode == 2 {
                let af = (self.registers[8 + pin / 8] >> ((pin % 8) * 4)) & 15;
                self.alternate[af as usize] & (1 << pin) != 0
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
    pub fn read(&self, offset: u64) -> Result<u32, &'static str> {
        if !offset.is_multiple_of(4) || offset > 0x24 {
            return Err("unsupported GPIO register");
        }
        Ok(match offset {
            0x10 => self.input(),
            0x18 => 0,
            _ => self.registers[offset as usize / 4],
        })
    }
    pub fn write(&mut self, offset: u64, value: u32) -> Result<(), &'static str> {
        self.read(offset)?;
        match offset {
            0x10 => return Err("GPIO input is read-only"),
            0x18 => {
                self.registers[5] = (self.registers[5] | (value & 0xffff)) & !(value >> 16);
            }
            0x1c if value != 0 => return Err("GPIO configuration lock is not implemented"),
            _ => self.registers[offset as usize / 4] = value,
        }
        Ok(())
    }
}
