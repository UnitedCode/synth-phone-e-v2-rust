//! Rust register models hosted by Renode through a small native bridge.
use std::collections::BTreeMap;
mod dma;
mod gpio;
mod qspi;
mod sai;
#[cfg(test)]
mod tests;

pub struct Peripheral {
    kind: u32,
    registers: BTreeMap<u64, u32>,
    dma: dma::Dma,
    sai: sai::Sai,
    qspi: qspi::Qspi,
    gpio: gpio::Gpio,
}

impl Peripheral {
    pub fn new(kind: u32) -> Self {
        let mut result = Self {
            kind,
            registers: BTreeMap::new(),
            dma: dma::Dma::default(),
            sai: sai::Sai::default(),
            qspi: qspi::Qspi::default(),
            gpio: gpio::Gpio::default(),
        };
        result.reset();
        result
    }
    pub fn reset(&mut self) {
        self.registers.clear();
        self.dma = dma::Dma::default();
        self.qspi = qspi::Qspi::default();
        if self.kind >= 100 {
            self.gpio.reset(self.kind - 100);
        }
        let input = std::mem::take(&mut self.sai.input);
        self.sai = sai::Sai::default();
        self.sai.input = input;
        if self.kind == 0 {
            self.registers
                .extend([(0, 0xF000_C000), (0x0c, 2), (0x18, 0xc000)]);
        }
    }
    pub fn read(&mut self, offset: u64) -> Result<u32, &'static str> {
        match self.kind {
            0 => match offset {
                0 | 8 | 0xc | 0x10 | 0x14 | 0x20 | 0x24 | 0x28 | 0x30 | 0x34 | 0x38 => {
                    let value = self.registers.get(&offset).copied().unwrap_or(0);
                    Ok(if offset == 8 && value & 1 != 0 {
                        value | (1 << 16)
                    } else {
                        value
                    })
                }
                4 => Ok((self.registers[&0x18] & 0xc000) | (1 << 13)),
                0x18 => Ok(self.registers[&0x18] | (1 << 13)),
                _ => Err("unsupported PWR register"),
            },
            1 => match offset {
                0 | 4 | 8 | 0xc | 0x10 | 0x14 | 0x18 | 0x1c | 0x140 | 0x144 | 0x148 | 0x14c
                | 0x150 | 0x154 | 0x158 => Ok(self.registers.get(&offset).copied().unwrap_or(0)),
                _ => Err("unsupported FMC register"),
            },
            2 => match offset {
                0x20 => {
                    let value = self.registers.get(&offset).copied().unwrap_or(0);
                    Ok(value | if value & 1 != 0 { 1 << 8 } else { 0 })
                }
                0x2c => Ok(self.registers.get(&offset).copied().unwrap_or(0)),
                _ => Err("register delegated to the built-in SYSCFG model"),
            },
            3 => match offset {
                0x100 => {
                    let cr = self.registers.get(&0x108).copied().unwrap_or(0);
                    Ok(self.registers.get(&offset).copied().unwrap_or(0)
                        | if cr & (1 << 28) != 0 { 1 << 12 } else { 0 })
                }
                0x104..=0x150 | 0x160..=0x180 | 0x1b0..=0x1c8 if offset.is_multiple_of(4) => {
                    Ok(self.registers.get(&offset).copied().unwrap_or(0))
                }
                _ => Err("register delegated to primary ADC model"),
            },
            4 if offset < 16 => Ok(self.registers.get(&offset).copied().unwrap_or(0)),
            // Host-only codec boundary query. Unsupported active digital formats
            // fail explicitly instead of silently feeding valid-looking PCM.
            4 if offset == 0x100 => {
                let reg = |index| self.registers.get(&index).copied().unwrap_or(0);
                if reg(9) & 1 == 0 {
                    return Ok(0);
                }
                if reg(7) & 0x4f != 9 {
                    return Err("unsupported active WM8731 digital format");
                }
                Ok(u32::from(reg(6) & 0x9d == 0 && reg(5) & 8 == 0))
            }
            6 => self.dma.read(offset),
            7 => self.sai.read(offset),
            8 => self.qspi.read(offset, 4),
            100..=110 => self.gpio.read(offset),
            _ => Err("unsupported peripheral"),
        }
    }
    pub fn i2c_write(&mut self, bytes: &[u8]) -> Result<(), &'static str> {
        match self.kind {
            4 => {
                if bytes.len() != 2 {
                    return Err("WM8731 expects a two-byte register write");
                }
                let register = bytes[0] >> 1;
                if register > 9 && register != 15 {
                    return Err("unsupported WM8731 register");
                }
                if register == 15 {
                    self.registers.clear();
                } else {
                    self.registers.insert(
                        register as u64,
                        ((bytes[0] as u32 & 1) << 8) | bytes[1] as u32,
                    );
                }
            }
            5 => {
                if bytes.is_empty() || ![0x00, 0x40, 0x80].contains(&bytes[0]) {
                    return Err("invalid SSD1306 control byte");
                }
                *self.registers.entry(0).or_default() += 1;
            }
            _ => return Err("not an I2C slave"),
        }
        Ok(())
    }
    pub fn write(&mut self, offset: u64, value: u32) -> Result<(), &'static str> {
        if self.kind == 6 {
            return self.dma.write(offset, value);
        }
        if self.kind == 7 {
            return self.sai.write(offset, value);
        }
        if self.kind == 8 {
            return self.qspi.write(offset, value, 4);
        }
        if self.kind >= 100 {
            return self.gpio.write(offset, value);
        }
        self.read(offset)?;
        if self.kind == 0 && offset == 4 {
            return Err("CSR1 is read-only");
        }
        if self.kind == 1 && offset == 0x158 {
            return Err("SDSR is read-only");
        }
        if self.kind == 1 && offset == 0x150 && value & 7 > 6 {
            return Err("invalid SDRAM command");
        }
        if self.kind == 3 && offset == 0x108 {
            if value & (1 << 31) != 0 && (value & (1 << 28) == 0 || value & ((1 << 29) | 1) != 0) {
                return Err("invalid ADC calibration state");
            }
            self.registers.insert(offset, value & !(1 << 31));
            return Ok(());
        }
        self.registers.insert(
            offset,
            if self.kind == 0 && offset == 0x18 {
                value & !0x2000
            } else {
                value
            },
        );
        Ok(())
    }
}

#[no_mangle]
pub extern "C" fn daisy_new(kind: u32) -> *mut Peripheral {
    Box::into_raw(Box::new(Peripheral::new(kind)))
}

/// # Safety
/// `state` must be the live pointer returned by daisy_new, with serialized access.
#[no_mangle]
pub unsafe extern "C" fn daisy_read(state: *mut Peripheral, offset: u64) -> u64 {
    match (*state).read(offset) {
        Ok(value) => value as u64,
        Err(_) => 1_u64 << 32,
    }
}

/// # Safety
/// `state` must be the live pointer returned by daisy_new, with serialized access.
#[no_mangle]
pub unsafe extern "C" fn daisy_write(state: *mut Peripheral, offset: u64, value: u32) -> u32 {
    u32::from((*state).write(offset, value).is_err())
}

/// # Safety
/// `state` must be the live pointer returned by daisy_new, with serialized access.
#[no_mangle]
pub unsafe extern "C" fn daisy_reset(state: *mut Peripheral) {
    (*state).reset();
}

/// # Safety
/// `state` must be live; `bytes` must point to `length` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn daisy_i2c_write(
    state: *mut Peripheral,
    bytes: *const u8,
    length: usize,
) -> u32 {
    if length == 0 {
        return 1;
    }
    u32::from(
        (*state)
            .i2c_write(std::slice::from_raw_parts(bytes, length))
            .is_err(),
    )
}

#[no_mangle]
/// # Safety
/// `state` must be live and exclusively accessed for this call.
pub unsafe extern "C" fn daisy_dma_request(state: *mut Peripheral, channel: u32) -> u64 {
    match (*state).dma.request(channel as usize) {
        Ok(Some((src, dst))) => ((src as u64) << 32) | dst as u64,
        Ok(None) => 0,
        Err(_) => u64::MAX,
    }
}
#[no_mangle]
/// # Safety
/// `state` must be live with serialized access.
pub unsafe extern "C" fn daisy_dma_irqs(state: *mut Peripheral) -> u32 {
    (*state).dma.irqs()
}
#[no_mangle]
/// # Safety
/// `state` must be live and exclusively accessed for this call.
pub unsafe extern "C" fn daisy_sai_tick(state: *mut Peripheral, codec_active: u32) {
    (*state).sai.tick(codec_active != 0);
}

/// # Safety
/// `state` must be live and exclusively accessed for this call.
#[no_mangle]
pub unsafe extern "C" fn daisy_sai_fault(state: *mut Peripheral, drop_rx: u32) {
    (*state).sai.drop_rx = drop_rx != 0;
}
#[no_mangle]
/// # Safety
/// `state` must be live with serialized access.
pub unsafe extern "C" fn daisy_sai_requests(state: *mut Peripheral) -> u32 {
    (*state).sai.requests()
}
#[no_mangle]
/// # Safety
/// `state` must be exclusive; `samples` must reference `frames * 2` valid floats.
pub unsafe extern "C" fn daisy_sai_input(
    state: *mut Peripheral,
    samples: *const f32,
    frames: usize,
) {
    if frames == 0 {
        (*state).sai.input.clear();
        return;
    }
    (*state).sai.input = std::slice::from_raw_parts(samples, frames * 2)
        .as_chunks::<2>()
        .0
        .iter()
        .map(|s| [s[0], s[1]])
        .collect();
}
#[no_mangle]
/// # Safety
/// `state` must be exclusive; `path` must be a valid NUL-terminated UTF-8 string.
pub unsafe extern "C" fn daisy_sai_save(
    state: *mut Peripheral,
    path: *const std::ffi::c_char,
) -> u32 {
    let path = match std::ffi::CStr::from_ptr(path).to_str() {
        Ok(path) => path,
        Err(_) => return 1,
    };
    let audio = &(*state).sai;
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let mut wav = hound::WavWriter::create(
            path,
            hound::WavSpec {
                channels: 2,
                sample_rate: 48014,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )?;
        for frame in &audio.output {
            for sample in frame {
                wav.write_sample(*sample)?;
            }
        }
        wav.finalize()?;
        std::fs::write(
            format!("{path}.stats"),
            format!(
                "frames={}\nunderruns={}\noverruns={}\n",
                audio.frames, audio.underruns, audio.overruns
            ),
        )?;
        Ok(())
    })();
    u32::from(result.is_err())
}

#[no_mangle]
/// # Safety
/// `state` must be live and exclusively accessed for this call.
pub unsafe extern "C" fn daisy_read_width(state: *mut Peripheral, offset: u64, width: u32) -> u64 {
    match (*state).qspi.read(offset, width as usize) {
        Ok(value) => value as u64,
        Err(_) => 1_u64 << 32,
    }
}
#[no_mangle]
/// # Safety
/// `state` must be live and exclusively accessed for this call.
pub unsafe extern "C" fn daisy_write_width(
    state: *mut Peripheral,
    offset: u64,
    value: u32,
    width: u32,
) -> u32 {
    u32::from((*state).qspi.write(offset, value, width as usize).is_err())
}
#[no_mangle]
/// # Safety
/// `state` must be live and exclusively accessed for this call.
pub unsafe extern "C" fn daisy_gpio_drive(state: *mut Peripheral, pin: u32, high: u32) {
    (*state).gpio.drive(pin, high != 0);
}

#[no_mangle]
/// # Safety
/// `state` must be live and exclusively accessed for this call.
pub unsafe extern "C" fn daisy_gpio_alternate(
    state: *mut Peripheral,
    pin: u32,
    af: u32,
    high: u32,
) {
    (*state).gpio.alternate(pin, af, high != 0);
}
