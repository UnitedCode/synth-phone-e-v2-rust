//! Rust register models hosted by Renode through a small native bridge.
use std::collections::BTreeMap;
pub mod board;
pub mod bus;
pub mod clock;
mod codec;
mod dma;
mod gpio;
pub mod memory;
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
    codec_reset_high: bool,
    memory: memory::SystemMemory,
    clock: clock::ClockTree,
    line_stage: codec::LineStage,
    passband_delay: codec::PassbandDelay,
}

impl Peripheral {
    pub fn codec_report(&self, ideal_line: bool, rate: f64) -> Result<String, &'static str> {
        let (name, adc, dac) = match self.kind {
            4 => ("WM8731", 20.0, 18.0),
            9 => ("AK4556", 18.0, 21.0),
            10 => ("PCM3060", 17.4, 20.0),
            _ => return Err("not a Seed codec"),
        };
        Ok(format!("codec={name}\naudio_boundary={}\nnominal_adc_group_delay_frames={adc}\nnominal_dac_group_delay_frames={dac}\nnominal_adc_group_delay_us={}\nnominal_dac_group_delay_us={}\npassband_delay_surrogate_applied={}\ncomplete_decimation_and_interpolation_response=UNMODELED\ninternal_coefficients_and_rounding=UNVERIFIED\n",if ideal_line { "ideal-line" } else { "serial" },adc/rate*1_000_000.0,dac/rate*1_000_000.0,u32::from(ideal_line&&self.kind==4)))
    }
    pub fn nominal_adc_passband(&mut self, sample: [f32; 2]) -> Result<[f32; 2], &'static str> {
        let sample = self.ideal_line_input(sample)?;
        Ok(self.passband_delay.adc(sample))
    }
    pub fn nominal_dac_passband(&mut self, sample: [f32; 2]) -> Result<[f32; 2], &'static str> {
        if self.kind != 4 {
            return Err("nominal passband delay only modeled for WM8731");
        }
        self.read(0x100)?;
        Ok(self.passband_delay.dac(sample))
    }
    /// Idealized normalized line input before ADC gain/DC removal. Serial PCM
    /// injection remains a separate boundary and never runs this stage twice.
    pub fn ideal_line_input(&mut self, sample: [f32; 2]) -> Result<[f32; 2], &'static str> {
        if self.kind != 4 {
            return Err("ideal line stage only implemented for WM8731");
        }
        let mask = self.read(0x100)?;
        if mask & 1 == 0 {
            return Ok([0.0; 2]);
        }
        let reg = |index| self.registers.get(&index).copied().unwrap_or(0);
        if reg(4) & 0x0c != 0 {
            return Err("WM8731 microphone or analog bypass route is not modeled");
        }
        Ok(self.line_stage.process(sample, [reg(0), reg(1)], reg(5)))
    }
    pub fn configure_revision_straps(&mut self, grounded: u32) -> Result<(), &'static str> {
        if self.kind != 103 || grounded > 7 {
            return Err("invalid port D revision straps");
        }
        for bit in 0..3 {
            self.gpio.release(3 + bit);
            if grounded & (1 << bit) != 0 {
                self.gpio.drive(3 + bit, false);
            }
        }
        Ok(())
    }
    pub fn codec_control(&mut self, high: bool) -> Result<(), &'static str> {
        if self.kind != 9 {
            return Err("this codec has no modeled GPIO reset connection");
        }
        self.codec_reset_high = high;
        Ok(())
    }
    pub fn configure_audio(&mut self, rx_channel: u32) -> Result<(), &'static str> {
        if self.kind != 7 || rx_channel > 1 {
            return Err("invalid SAI wiring");
        }
        self.sai.rx_channel = rx_channel as usize;
        Ok(())
    }
    pub fn new(kind: u32) -> Self {
        let mut result = Self {
            kind,
            registers: BTreeMap::new(),
            dma: dma::Dma::default(),
            sai: sai::Sai::default(),
            qspi: qspi::Qspi::default(),
            gpio: gpio::Gpio::default(),
            codec_reset_high: false,
            memory: memory::SystemMemory::default(),
            clock: clock::ClockTree::default(),
            line_stage: codec::LineStage::default(),
            passband_delay: codec::PassbandDelay::default(),
        };
        result.reset();
        result
    }
    pub fn reset(&mut self) {
        self.memory.reset_registers();
        self.clock = clock::ClockTree::default();
        self.line_stage = codec::LineStage::default();
        self.passband_delay = codec::PassbandDelay::default();
        self.registers.clear();
        self.dma = dma::Dma::default();
        self.qspi = qspi::Qspi::default();
        if self.kind >= 100 {
            self.gpio.reset(self.kind - 100);
        }
        let input = std::mem::take(&mut self.sai.input);
        let rx_channel = self.sai.rx_channel;
        let drop_rx = self.sai.drop_rx;
        self.sai = sai::Sai::default();
        self.sai.input = input;
        self.sai.rx_channel = rx_channel;
        self.sai.drop_rx = drop_rx;
        if self.kind == 0 {
            self.registers
                .extend([(0, 0xF000_C000), (0x0c, 2), (0x18, 0xc000)]);
        }
        if self.kind == 4 {
            self.registers.extend(
                [0x97, 0x97, 0x79, 0x79, 0x0a, 8, 0x9f, 0x0a, 0, 0]
                    .into_iter()
                    .enumerate()
                    .map(|(index, value)| (index as u64, value)),
            );
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
                if reg(7) != 9 {
                    return Err("unsupported active WM8731 digital format");
                }
                if reg(8) != 0 {
                    return Err("unsupported WM8731 sample clock ratio");
                }
                if reg(5) & 6 != 0 {
                    return Err("WM8731 de-emphasis filter is not implemented");
                }
                if reg(4) & 0x2c != 0 {
                    return Err(
                        "WM8731 microphone, bypass and sidetone routing is not implemented",
                    );
                }
                if reg(6) & 0x80 != 0 {
                    return Ok(0);
                }
                let adc = u32::from(reg(6) & 5 == 0);
                let dac = u32::from(reg(6) & 0x18 == 0 && reg(5) & 8 == 0 && reg(4) & 0x10 != 0);
                Ok(adc | (dac << 1) | ((reg(0) & 0x80) >> 5) | ((reg(1) & 0x80) >> 4))
            }
            6 => self.dma.read(offset),
            7 => self.sai.read(offset),
            8 => self.qspi.read(offset, 4),
            9 if offset == 0x100 => Ok(if self.codec_reset_high { 3 } else { 0 }),
            10 if offset == 0x100 => Ok(3), // Rev7 hardware mode, fixed LJ24, no I2C.
            12 => self.memory.read_scb(offset),
            13 => Ok(self.clock.read(offset)),
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
                    self.reset();
                } else {
                    let value = ((bytes[0] as u32 & 1) << 8) | bytes[1] as u32;
                    self.registers.insert(register as u64, value);
                    if register <= 3 && value & 0x100 != 0 {
                        let opposite = u64::from(register ^ 1);
                        let previous = self.registers.get(&opposite).copied().unwrap_or(0);
                        self.registers
                            .insert(opposite, (previous & 0x100) | (value & 0xff));
                    }
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
        if self.kind == 12 {
            return self.memory.write_scb(offset, value);
        }
        if self.kind == 6 {
            return self.dma.write(offset, value);
        }
        if self.kind == 7 {
            return self.sai.write(offset, value);
        }
        if self.kind == 8 {
            return self.qspi.write(offset, value, 4);
        }
        if self.kind == 13 {
            self.clock.write(offset, value);
            return Ok(());
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
/// `state` is a live, serialized architecture state; width is 1, 2, or 4.
#[no_mangle]
pub unsafe extern "C" fn daisy_memory_read(
    state: *mut Peripheral,
    address: u32,
    width: u32,
    cpu: u32,
) -> u64 {
    let result = if cpu == 2 {
        (*state).memory.read_instruction(address, width as usize)
    } else if cpu != 0 {
        (*state).memory.read_cpu(address, width as usize)
    } else {
        (*state).memory.read_dma(address, width as usize)
    };
    match result {
        Ok(value) => u64::from(value),
        Err(_) => 1_u64 << 32,
    }
}

/// # Safety
/// `state` is a live, exclusively accessed architecture state; width is 1, 2, or 4.
#[no_mangle]
pub unsafe extern "C" fn daisy_memory_write(
    state: *mut Peripheral,
    address: u32,
    width: u32,
    value: u32,
    cpu: u32,
) -> u32 {
    u32::from(
        if cpu != 0 {
            (*state).memory.write_cpu(address, width as usize, value)
        } else {
            (*state).memory.write_dma(address, width as usize, value)
        }
        .is_err(),
    )
}

#[no_mangle]
pub extern "C" fn daisy_memory_is_ram(address: u32, width: u32) -> u32 {
    u32::from(memory::SystemMemory::is_ram(address, width as usize))
}

/// # Safety
/// `state` is a live, serialized clock state.
#[no_mangle]
pub unsafe extern "C" fn daisy_clock_advance(
    state: *mut Peripheral,
    cr1: u32,
    frcr: u32,
    ns: u64,
) -> u64 {
    (*state)
        .clock
        .advance(cr1, frcr, ns)
        .map_or(1_u64 << 32, u64::from)
}

/// # Safety
/// `state` is a live, serialized clock state.
#[no_mangle]
pub unsafe extern "C" fn daisy_clock_rate(state: *mut Peripheral, cr1: u32, frcr: u32) -> f64 {
    (*state)
        .clock
        .sample_rate(cr1, frcr)
        .map_or(f64::NAN, clock::Rate::hz)
}

/// # Safety
/// `state` is a live, serialized clock state.
#[no_mangle]
pub unsafe extern "C" fn daisy_clock_domain(state: *mut Peripheral, domain: u32) -> u64 {
    (*state).clock.domain_rate(domain).map_or(u64::MAX, |rate| {
        rate.map_or(0, |rate| rate.numerator / rate.denominator)
    })
}

/// # Safety
/// `state` is a live, serialized clock state; use write=2 to synchronize before RCC writes.
#[no_mangle]
pub unsafe extern "C" fn daisy_clock_dwt(
    state: *mut Peripheral,
    offset: u64,
    value: u32,
    write: u32,
    now: u64,
) -> u64 {
    if write == 3 {
        return u64::from((*state).clock.trace_enable(value != 0, now).is_err()) << 32;
    }
    if write == 2 {
        return u64::from((*state).clock.sync_cycles(now).is_err()) << 32;
    }
    (*state)
        .clock
        .dwt(offset, if write == 0 { None } else { Some(value) }, now)
        .map_or(1_u64 << 32, u64::from)
}

/// # Safety
/// `state` is live/serialized; `path` is a valid NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn daisy_clock_save(
    state: *mut Peripheral,
    cr1: u32,
    frcr: u32,
    path: *const std::ffi::c_char,
) -> u32 {
    let Ok(path) = std::ffi::CStr::from_ptr(path).to_str() else {
        return 1;
    };
    let Ok(stats) = (*state).clock.stats(cr1, frcr) else {
        return 1;
    };
    u32::from(std::fs::write(path, stats).is_err())
}

/// # Safety
/// `state` must be live/serialized and `path` a valid NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn daisy_memory_save(
    state: *mut Peripheral,
    path: *const std::ffi::c_char,
) -> u32 {
    let Ok(path) = std::ffi::CStr::from_ptr(path).to_str() else {
        return 1;
    };
    u32::from(std::fs::write(path, (*state).memory.stats()).is_err())
}

/// # Safety
/// Distinct live/serialized FMC and clock states; path is NUL-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn daisy_fmc_save(
    fmc: *mut Peripheral,
    clock: *mut Peripheral,
    path: *const std::ffi::c_char,
) -> u32 {
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let path = std::ffi::CStr::from_ptr(path).to_str()?;
        let reg = |offset| (*fmc).registers.get(&offset).copied().unwrap_or(0);
        let rate = (*clock).clock.fmc_rate()?.ok_or("FMC clock disabled")?;
        let timing = bus::SdramTiming::decode(reg(0x140), reg(0x148), reg(0x154), rate)?;
        std::fs::write(path, timing.report())?;
        Ok(())
    })();
    u32::from(result.is_err())
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
    (*state).sai.tick_mask(codec_active);
}

/// # Safety
/// Both pointers are distinct live states with serialized access.
#[no_mangle]
pub unsafe extern "C" fn daisy_sai_tick_codec(
    state: *mut Peripheral,
    codec: *mut Peripheral,
    ideal_line: u32,
) -> u32 {
    let Ok(mask) = (*codec).read(0x100) else {
        return 1;
    };
    let sample = (*state).sai.input_sample();
    let sample = if ideal_line != 0 {
        match (*codec).nominal_adc_passband(sample) {
            Ok(sample) => sample,
            Err(_) => return 1,
        }
    } else {
        sample
    };
    (*state).sai.tick_sample(mask, sample);
    if ideal_line != 0 {
        if let Some(frame) = (*state).sai.output.last_mut() {
            let Ok(delayed) = (*codec).nominal_dac_passband(*frame) else {
                return 1;
            };
            *frame = if mask & 2 != 0 { delayed } else { [0.0; 2] };
        }
    }
    0
}

/// # Safety
/// `state` must be a live, exclusive SAI state. Configure before CPU execution.
#[no_mangle]
pub unsafe extern "C" fn daisy_sai_configure(state: *mut Peripheral, rx_channel: u32) -> u32 {
    u32::from((*state).configure_audio(rx_channel).is_err())
}

/// # Safety
/// `state` must be a live, serialized SAI state. Rate comes from the modeled clock tree.
#[no_mangle]
pub unsafe extern "C" fn daisy_sai_rate(state: *mut Peripheral, rate: f64) -> u32 {
    if !rate.is_finite() || !(1.0..=384000.0).contains(&rate) {
        return 1;
    }
    if (*state).sai.frames != 0
        && (*state).sai.sample_rate != 0.0
        && (*state).sai.sample_rate != rate
    {
        return 1;
    } // A WAV cannot label a variable-rate capture.
    (*state).sai.sample_rate = rate;
    0
}

/// # Safety
/// `state` must be a live, exclusively accessed codec state.
#[no_mangle]
pub unsafe extern "C" fn daisy_codec_control(state: *mut Peripheral, high: u32) -> u32 {
    u32::from((*state).codec_control(high != 0).is_err())
}

/// # Safety
/// `state` is a live/serialized codec state and `path` is NUL-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn daisy_codec_save(
    state: *mut Peripheral,
    path: *const std::ffi::c_char,
    ideal_line: u32,
    rate: f64,
) -> u32 {
    if !rate.is_finite() || rate <= 0.0 {
        return 1;
    }
    let Ok(path) = std::ffi::CStr::from_ptr(path).to_str() else {
        return 1;
    };
    let Ok(report) = (*state).codec_report(ideal_line != 0, rate) else {
        return 1;
    };
    u32::from(std::fs::write(path, report).is_err())
}

/// # Safety
/// `state` must be a live, exclusively accessed GPIO port D state.
#[no_mangle]
pub unsafe extern "C" fn daisy_revision_straps(state: *mut Peripheral, grounded: u32) -> u32 {
    u32::from((*state).configure_revision_straps(grounded).is_err())
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
    sample_rate: u32,
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
                sample_rate,
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
