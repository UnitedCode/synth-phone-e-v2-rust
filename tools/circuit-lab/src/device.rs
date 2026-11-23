//! Unmodified ARM firmware connected to project circuits, contacts, OLED RAM and serial PCM.
//! CPU time is never skipped. ADC voltages come from the evolving RC network.
use crate::{
    carrier::{shared, SynthphoneCarrier},
    electrical::KeyboardCircuit,
    fixture::Fixture,
    netlist::Graph,
    oled::Oled,
    sha256, Result,
};
use std::{collections::BTreeMap, fs, io::Write, path::Path, time::Instant};
use virtual_daisy::{
    board::Board,
    machine::{Carrier, Error, Machine},
};
const MS: u64 = 1_000_000;
const SDK: &str = "f7892c73b36960d729e6c3bf8bbd0ce3e65498a0";
#[derive(Clone, Debug)]
enum Change {
    Voltage(String, u32),
    Key(usize, usize, bool),
    Pin(usize, u8, bool),
}
#[derive(Clone, Debug)]
struct Event {
    at: u64,
    change: Change,
}
pub struct Device {
    keyboard: Option<KeyboardCircuit>,
    phone: Option<SynthphoneCarrier>,
    pub oled: Oled,
    events: Vec<Event>,
    next: usize,
    now: u64,
    pins: BTreeMap<(usize, u8), bool>,
    pub error: Option<String>,
    fault: String,
    pub adc: Vec<(u64, u8, u32)>,
    gpio: Vec<(u64, [u16; 11])>,
    outputs: [u16; 11],
    pub oled_frames: Vec<(u64, [u8; 512])>,
    scans: u64,
    last_channel: Option<u16>,
    last_scan: Option<u64>,
    pub max_scan_gap: u64,
}
impl Device {
    fn new(kind: &str, g: &Graph, f: &Fixture, scenario: &str) -> Result<Self> {
        if kind != "keyboard" {
            crate::profiles::synth_wiring(g)?;
        }
        let (keyboard, phone) = if kind == "keyboard" {
            (Some(KeyboardCircuit::new(g, f)?), None)
        } else {
            (None, Some(SynthphoneCarrier::new(g)?))
        };
        let mut d = Self {
            keyboard,
            phone,
            oled: Oled::default(),
            events: Vec::new(),
            next: 0,
            now: 0,
            pins: BTreeMap::new(),
            error: None,
            fault: scenario.into(),
            adc: Vec::new(),
            gpio: Vec::new(),
            outputs: [0; 11],
            oled_frames: Vec::new(),
            scans: 0,
            last_channel: None,
            last_scan: None,
            max_scan_gap: 0,
        };
        if kind == "keyboard" {
            d.keyboard_events(scenario);
            if scenario == "reset" {
                d.voltage(2000, "HE1", 2_200_000);
                d.voltage(2050, "HE1", 1_650_000);
            }
        } else {
            d.phone_events(scenario);
        }
        d.events.sort_by_key(|e| e.at);
        if scenario == "open-hall" {
            d.keyboard.as_mut().unwrap().open_control = Some("HE1".into());
        }
        Ok(d)
    }
    fn voltage(&mut self, ms: u64, name: &str, uv: u32) {
        self.events.push(Event {
            at: ms * MS,
            change: Change::Voltage(name.into(), uv),
        });
    }
    fn key(&mut self, ms: u64, row: usize, col: usize, down: bool) {
        self.events.push(Event {
            at: ms * MS,
            change: Change::Key(row, col, down),
        });
    }
    fn pin(&mut self, ms: u64, port: usize, pin: u8, high: bool) {
        self.events.push(Event {
            at: ms * MS,
            change: Change::Pin(port, pin, high),
        });
    }
    fn keyboard_events(&mut self, scenario: &str) {
        if scenario == "boot" {
            return;
        }
        if scenario == "open-hall" {
            self.voltage(170, "HE1", 2_200_000);
            self.voltage(220, "HE1", 1_650_000);
            return;
        }
        // Every musical key, split into groups to avoid duplicate-note ambiguities and wire overload.
        for i in 1..=70 {
            let group = (i - 1) / 14;
            self.voltage(170 + group * 85, &format!("HE{i}"), 2_200_000);
            self.voltage(205 + group * 85, &format!("HE{i}"), 1_650_000);
        }
        for i in 81..=100 {
            self.voltage(620, &format!("HE{i}"), 2_200_000);
            self.voltage(675, &format!("HE{i}"), 1_650_000);
        }
        for i in 1..=12 {
            self.voltage(740, &format!("RV{i}"), 3_300_000);
        }
        // Program, pitch-bend, modulation, all-notes-off and recalibration controls.
        for (time, key) in [
            (810, 71),
            (865, 72),
            (920, 73),
            (975, 78),
            (1030, 77),
            (1085, 76),
            (1140, 75),
        ] {
            self.voltage(time, &format!("HE{key}"), 2_200_000);
            self.voltage(time + 25, &format!("HE{key}"), 1_650_000);
        }
        // Test the settings screen after the unmodified three-second splash timer.
        self.voltage(3150, "HE74", 2_200_000);
        self.voltage(3180, "HE74", 1_650_000);
        self.voltage(3220, "HE80", 2_200_000);
        self.voltage(3250, "HE80", 1_650_000);
        self.voltage(3290, "HE74", 2_200_000);
        self.voltage(3320, "HE74", 1_650_000);
    }
    fn phone_events(&mut self, scenario: &str) {
        // Open hangup contact -> debounced "not pressed" -> firmware selects the direct right jack.
        self.pins.insert((2, 11), true);
        self.pins.insert((2, 9), false);
        self.pins.insert((2, 8), false);
        if ["midi", "open-midi", "open-jack", "open-display", "boot"].contains(&scenario) {
            return;
        }
        if scenario == "profiles" {
            // Startup already opens Effects. Pressing the encoder would switch to note-playing.
            for i in 0..6 {
                self.key(130 + i * 80, 3, 1, true);
                self.key(150 + i * 80, 3, 1, false);
            }
            return;
        }
        if scenario == "contention" {
            self.key(80, 0, 0, true);
            self.key(80, 0, 1, true);
            return;
        }
        if scenario == "open-key" {
            self.key(80, 0, 0, true);
            self.key(115, 0, 0, false);
            return;
        }
        // All twelve physical contacts; physical columns are reversed in the firmware scanner.
        for i in 0..12 {
            let time = 80 + i as u64 * 30;
            self.key(time, i / 3, i % 3, true);
            self.key(time + 15, i / 3, i % 3, false);
        }
        // Real encoder push and quadrature contacts, followed by a double press.
        self.pin(460, 2, 10, false);
        self.pin(490, 2, 10, true);
        for (time, dt, clk) in [
            (510, false, true),
            (515, true, true),
            (520, true, false),
            (525, false, false),
        ] {
            self.pin(time, 2, 9, dt);
            self.pin(time, 2, 8, clk);
        }
        for (time, high) in [(560, false), (585, true), (620, false), (645, true)] {
            self.pin(time, 2, 10, high);
        }
        // Exercise both jack / handset route switch positions; handset input is explicitly silent.
        self.pin(690, 2, 11, false);
        self.pin(730, 2, 11, true);
    }
    fn advance(&mut self, now: u64, outputs: &[u16; 11]) -> Result<()> {
        while self.next < self.events.len() && self.events[self.next].at <= now {
            let event = self.events[self.next].clone();
            if let Some(c) = &mut self.keyboard {
                c.observe(event.at, &self.outputs)?;
            }
            match event.change {
                Change::Voltage(name, value) => self
                    .keyboard
                    .as_mut()
                    .ok_or("voltage stimulus on phone")?
                    .set_microvolts(event.at, &name, value)?,
                Change::Key(r, c, down) => {
                    if self.fault != "open-key" {
                        self.phone.as_mut().unwrap().contact(r, c, down)?;
                    }
                }
                Change::Pin(p, n, high) => {
                    self.pins.insert((p, n), high);
                }
            }
            self.next += 1;
        }
        if let Some(c) = &mut self.keyboard {
            c.observe(now, outputs)?;
        }
        if *outputs != self.outputs {
            if self.gpio.len() > 500_000 {
                return Err("GPIO trace limit exceeded".into());
            }
            self.gpio.push((now, *outputs));
            self.outputs = *outputs;
        }
        self.now = now;
        Ok(())
    }
}
impl Carrier for Device {
    fn pins(&mut self, outputs: &[u16; 11], now: u64) -> [(u16, u16); 11] {
        // Most CPU synchronizations have no electrical event. Do not integrate at instruction frequency.
        if *outputs != self.outputs || self.events.get(self.next).is_some_and(|e| e.at <= now) {
            if let Err(e) = self.advance(now, outputs) {
                self.error = Some(e.to_string());
            }
        }
        self.now = now;
        let mut inputs = if let Some(phone) = &self.phone {
            match phone.row_contacts(outputs) {
                Ok(p) => p,
                Err(e) => {
                    self.error = Some(e.to_string());
                    [(0, 0); 11]
                }
            }
        } else {
            [(0, 0); 11]
        };
        for (&(port, pin), &high) in &self.pins {
            inputs[port].0 |= 1 << pin;
            if high {
                inputs[port].1 |= 1 << pin;
            } else {
                inputs[port].1 &= !(1 << pin);
            }
        }
        inputs
    }
    fn adc_input(
        &mut self,
        adc: u8,
        channel: u8,
        outputs: &[u16; 11],
        now: u64,
    ) -> std::result::Result<u32, Error> {
        if adc != 1 {
            return Err("device carrier only connects ADC1".into());
        }
        self.advance(now, outputs)
            .map_err(|_| Error::from("circuit advancement failed"))?;
        let value = self
            .keyboard
            .as_mut()
            .ok_or("phone has no external ADC route")?
            .sample(now, channel, outputs)
            .map_err(|_| Error::from("analog acquisition failed"))?;
        if self.adc.len() >= 2_000_000 {
            return Err("ADC trace bound exceeded".into());
        }
        self.adc.push((now, channel, value));
        if channel == 10 {
            let ch = ((outputs[6] >> 10) & 1)
                | (((outputs[6] >> 11) & 1) << 1)
                | (((outputs[1] >> 4) & 1) << 2);
            if ch == 0 && self.last_channel == Some(7) {
                self.scans += 1;
                if let Some(last) = self.last_scan {
                    self.max_scan_gap = self.max_scan_gap.max(now - last);
                }
                self.last_scan = Some(now);
            }
            self.last_channel = Some(ch);
        }
        Ok(value)
    }
    fn i2c_write(&mut self, bus: u8, address: u8, bytes: &[u8]) -> std::result::Result<(), Error> {
        if bus != 1 || address != 0x3c {
            return Err("unconnected external I2C device".into());
        }
        if self.fault == "open-display" {
            return Err("display physically disconnected".into());
        }
        let before = self.oled.data_bytes;
        self.oled.write(bytes).map_err(|e| {
            self.error = Some(e.to_string());
            Error::from("OLED protocol error")
        })?;
        if self.oled.data_bytes / 512 != before / 512 {
            if self.oled_frames.len() > 10_000 {
                return Err("OLED trace bound exceeded".into());
            }
            self.oled_frames.push((self.now, self.oled.ram));
        }
        Ok(())
    }
    fn next_event(&self) -> Option<u64> {
        self.events.get(self.next).map(|e| e.at)
    }
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes.get(at..at + 4).ok_or("truncated ELF")?.try_into()?,
    ))
}
fn rtt_symbol(bytes: &[u8]) -> Result<u32> {
    let table = u32_at(bytes, 32)? as usize;
    let packed = u32_at(bytes, 44)?;
    let size = (packed >> 16) as usize;
    let count = (u32_at(bytes, 48)? & 0xffff) as usize;
    if size < 40 {
        return Err("ELF has no section table".into());
    }
    for index in 0..count {
        let h = table + index * size;
        if u32_at(bytes, h + 4)? != 2 {
            continue;
        }
        let offset = u32_at(bytes, h + 16)? as usize;
        let len = u32_at(bytes, h + 20)? as usize;
        let strings = table + u32_at(bytes, h + 24)? as usize * size;
        let string_offset = u32_at(bytes, strings + 16)? as usize;
        for sym in (offset..offset + len).step_by(16) {
            let at = string_offset + u32_at(bytes, sym)? as usize;
            if bytes.get(at..at + 12) == Some(b"_SEGGER_RTT\0") {
                return u32_at(bytes, sym + 4);
            }
        }
    }
    Err("firmware lacks RTT symbol".into())
}
fn read_byte(machine: &mut Machine, address: u32) -> Result<u8> {
    Ok((machine.read32(address & !3)? >> ((address & 3) * 8)) as u8)
}
fn drain_rtt(machine: &mut Machine, address: u32, log: &mut Vec<u8>) -> Result<()> {
    if machine.read32(address)? != u32::from_le_bytes(*b"SEGG") {
        return Ok(());
    }
    let descriptor = address + 24;
    let buffer = machine.read32(descriptor + 4)?;
    let size = machine.read32(descriptor + 8)?;
    let write = machine.read32(descriptor + 12)?;
    let mut read = machine.read32(descriptor + 16)?;
    if size == 0 || size > 65_536 || write >= size || read >= size {
        return Err("invalid RTT descriptor".into());
    }
    while read != write {
        log.push(read_byte(machine, buffer + read)?);
        read = (read + 1) % size;
    }
    // Only the debugger-consumer pointer changes. No application timer, state or execution is patched.
    machine.write32(descriptor + 16, read)?;
    Ok(())
}
fn write_wav(path: &Path, frames: &[[f32; 2]], hz: u32) -> Result<()> {
    let mut w = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 2,
            sample_rate: hz,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )?;
    for frame in frames {
        for sample in frame {
            w.write_sample(*sample)?;
        }
    }
    w.finalize()?;
    Ok(())
}
fn rms(frames: &[[f32; 2]]) -> f64 {
    if frames.is_empty() {
        return 0.;
    }
    (frames
        .iter()
        .flat_map(|f| f.iter())
        .map(|v| f64::from(*v).powi(2))
        .sum::<f64>()
        / (frames.len() * 2) as f64)
        .sqrt()
}

/// Profile, baseline, fixture, production ELF, board, fresh output, scenario.
pub fn run(args: &[String]) -> Result<()> {
    if args.len() != 7 {
        return Err("device PROFILE BASELINE FIXTURE ELF BOARD FRESH_OUTPUT SCENARIO".into());
    }
    let profile = &args[0];
    let kind = if profile == "keyboard-current" {
        "keyboard"
    } else if profile == "synthphone-v2" {
        "phone"
    } else {
        return Err("device requires current keyboard or Synthphone-v2 profile".into());
    };
    let scenario = &args[6];
    if ![
        "baseline",
        "open-hall",
        "open-key",
        "open-display",
        "open-jack",
        "contention",
        "reset",
        "profiles",
        "midi",
        "open-midi",
        "boot",
    ]
    .contains(&scenario.as_str())
    {
        return Err("unknown device scenario".into());
    }
    let output = Path::new(&args[5]);
    fs::create_dir(output)?;
    let bytes = fs::read(&args[3])?;
    fs::write(output.join("firmware.elf"), &bytes)?;
    fs::copy(&args[1], output.join("baseline.net"))?;
    fs::copy(&args[2], output.join("assumptions.fixture"))?;
    fs::write(
        output.join("runner-sources.sha256"),
        env!("CIRCUIT_WORKBENCH_SOURCES").replace(';', "\n"),
    )?;
    let baseline = Graph::parse(&fs::read_to_string(&args[1])?)?;
    let fixture = Fixture::parse(&fs::read_to_string(&args[2])?)?;
    let mut metadata=format!("profile={profile}\nscenario={scenario}\nfirmware_sha256={}\nbaseline_sha256={}\nfixture_sha256={}\nvirtual_daisy_revision={SDK}\nPHYSICAL_MEASURED=NOT_RUN\n",sha256(Path::new(&args[3]))?,sha256(Path::new(&args[1]))?,sha256(Path::new(&args[2]))?);
    let device = Device::new(kind, &baseline, &fixture, scenario)?;
    let mut events = String::from("time_ns,change\n");
    for e in &device.events {
        events.push_str(&format!("{},\"{:?}\"\n", e.at, e.change));
    }
    fs::write(output.join("stimulus.csv"), events)?;
    let (handle, attachment) = shared(device);
    let board = args[4].parse::<Board>().map_err(|e| e.to_string())?;
    let mut machine = Machine::from_elf(board, &bytes)?;
    machine.attach(attachment);
    let rtt = rtt_symbol(&bytes)?;
    let end_ms = if scenario == "profiles" {
        650
    } else if scenario == "baseline" || scenario == "reset" {
        if kind == "keyboard" {
            3400
        } else {
            820
        }
    } else {
        260
    };
    let start = Instant::now();
    let mut frames = Vec::new();
    let mut serial = Vec::new();
    let mut log = Vec::new();
    let mut errors = Vec::new();
    let mut input_frame = 0_u64;
    let mut reset = false;
    let mut metrics = String::from("time_ns,steps,audio_frames,adc_samples,oled_bytes,cfsr,hfsr\n");
    let mut scheduled_midi = [false; 4];
    for ms in 1..=end_ms {
        if kind == "phone" {
            if machine.queued_input_frames() < 128 {
                let samples: Vec<_> = (0..480)
                    .map(|_| {
                        let sample =
                            if ["open-jack", "midi", "open-midi"].contains(&scenario.as_str()) {
                                0.
                            } else {
                                0.12 * (std::f64::consts::TAU * 220. * input_frame as f64 / 48014.)
                                    .sin() as f32
                            };
                        input_frame += 1;
                        [0., sample]
                    })
                    .collect();
                machine.input_at_rate(&samples, 48014)?;
            }
            for (i, (time, message)) in [
                (
                    40,
                    [
                        if scenario == "midi" || scenario == "open-midi" {
                            0x91
                        } else {
                            0x90
                        },
                        69,
                        100,
                    ],
                ),
                (
                    65,
                    [
                        if scenario == "midi" || scenario == "open-midi" {
                            0x81
                        } else {
                            0x80
                        },
                        69,
                        0,
                    ],
                ),
                (750, [0x99, 36, 110]),
                (770, [0x89, 36, 0]),
            ]
            .iter()
            .enumerate()
            {
                if machine.time_ns() >= *time * MS && !scheduled_midi[i] && scenario != "open-midi"
                {
                    machine.receive_serial(message)?;
                    scheduled_midi[i] = true;
                }
            }
        }
        if scenario == "reset" && !reset && ms == 500 {
            machine.write32(0xe000ed0c, 0x05fa0004)?;
            reset = true;
        }
        if let Err(error) = machine.run_until(ms * MS, 3_000_000) {
            errors.push(error.to_string());
            break;
        }
        frames.extend(machine.take_audio());
        serial.extend(machine.take_serial());
        if ms % 5 == 0 {
            drain_rtt(&mut machine, rtt, &mut log)?;
        }
        let cfsr = machine.read32(0xe000ed28)?;
        let hfsr = machine.read32(0xe000ed2c)?;
        if ms % 10 == 0 {
            let d = handle.borrow();
            metrics.push_str(&format!(
                "{},{},{},{},{},{cfsr},{hfsr}\n",
                machine.time_ns(),
                machine.steps(),
                frames.len(),
                d.adc.len(),
                d.oled.data_bytes
            ));
        }
        if cfsr != 0 || hfsr != 0 {
            errors.push(format!("Cortex-M fault CFSR={cfsr:#x} HFSR={hfsr:#x}"));
            break;
        }
        if let Some(error) = &handle.borrow().error {
            errors.push(error.clone());
            break;
        }
        if ms % 50 == 0 {
            eprintln!(
                "{profile}/{scenario}: {} virtual ms, {} scheduler steps, {} wall seconds",
                ms,
                machine.steps(),
                start.elapsed().as_secs()
            );
        }
    }
    drain_rtt(&mut machine, rtt, &mut log)?;
    let log = String::from_utf8_lossy(&log);
    fs::write(output.join("firmware.rtt"), log.as_bytes())?;
    if log.contains("panicked") || log.contains("HardFault") {
        errors.push("firmware panic/fault RTT output".into());
    }
    let d = handle.borrow();
    let rate = machine
        .sample_rate()
        .ok_or("firmware did not start codec")?
        .hz();
    let mut failures = grade(kind, scenario, &d, &serial, &frames, &log, rate);
    let sai = [machine.read32(0x40015818)?, machine.read32(0x40015838)?];
    if (sai[0] | sai[1]) & 1 != 0 {
        failures.push("SAI overrun/underrun flag set".into());
    }
    if errors.is_empty() && (frames.len() as f64) < rate * machine.time_ns() as f64 * 1e-9 * 0.90 {
        failures.push("audio clock did not sustain the elapsed run".into());
    }
    drop(d); // GPIO debugger reads invoke the carrier; release its observation borrow first.
    let mut pin_report =
        String::from("port,pin,expected_mode,actual_mode,alternate,pull,open_drain\n");
    let mut modes = vec![(1, 6, 2), (1, 7, 2), (1, 8, 2), (1, 9, 2)];
    if kind == "keyboard" {
        modes.extend([
            (6, 10, 1),
            (6, 11, 1),
            (1, 4, 1),
            (2, 8, 1),
            (2, 9, 1),
            (2, 10, 1),
            (2, 0, 3),
            (0, 3, 3),
            (1, 1, 3),
            (0, 7, 3),
            (0, 6, 3),
            (2, 1, 3),
            (2, 4, 3),
            (0, 5, 3),
            (0, 4, 3),
            (0, 1, 3),
            (0, 0, 3),
            (0, 2, 3),
        ]);
    } else {
        modes.extend([
            (2, 4, 1),
            (2, 1, 1),
            (0, 6, 1),
            (2, 0, 0),
            (0, 3, 0),
            (1, 1, 0),
            (0, 7, 0),
            (2, 8, 0),
            (2, 9, 0),
            (2, 10, 0),
            (2, 11, 0),
        ]);
    }
    for (port, pin, expected) in modes {
        let base = 0x58020000 + port * 0x400;
        let mode = (machine.read32(base)? >> (pin * 2)) & 3;
        let af = (machine.read32(base + 0x20 + (pin / 8) * 4)? >> ((pin % 8) * 4)) & 15;
        let pull = (machine.read32(base + 0xc)? >> (pin * 2)) & 3;
        let od = (machine.read32(base + 4)? >> pin) & 1;
        pin_report.push_str(&format!(
            "{port},{pin},{expected},{mode},{af},{pull},{od}\n"
        ));
        if mode != expected {
            failures.push(format!("GPIO mode mismatch port {port} pin {pin}"));
        }
        if port == 1 && (6..=9).contains(&pin) && af != if pin < 8 { 7 } else { 4 } {
            failures.push("serial/I2C pin alternate function mismatch".into());
        }
        if port == 1 && pin >= 8 && od != 1 {
            failures.push("I2C pin is not open drain".into());
        }
    }
    fs::write(output.join("pin-config.csv"), pin_report)?;
    let d = handle.borrow();
    metadata.push_str(&format!("sai_status={sai:x?}\n"));
    if scenario == "reset"
        && (machine.resets() != 1
            || log
                .matches(if kind == "keyboard" {
                    "keyboard_keyboard ready:"
                } else {
                    "Startup done!!"
                })
                .count()
                < 2)
    {
        failures.push("reset did not reboot production firmware".into());
    }
    failures.extend(errors);
    metadata.push_str(&format!("board={}\ncodec={}\nvirtual_time_ns={}\nscheduler_steps={}\nwall_seconds={}\naudio_frames={}\nserial_bytes={}\nadc_samples={}\nscans={}\nmax_scan_gap_ns={}\noled_transactions={}\noled_data_bytes={}\noled_frames={}\naudio_rms={}\nsample_rate_hz={rate}\nresets={}\nfinal_registers={:x?}\n{}",board.id(),board.codec(),machine.time_ns(),machine.steps(),start.elapsed().as_secs_f64(),frames.len(),serial.len(),d.adc.len(),d.scans,d.max_scan_gap,d.oled.writes,d.oled.data_bytes,d.oled_frames.len(),rms(&frames),machine.resets(),machine.registers(),machine.memory_report()));
    metadata.push_str(&format!(
        "ARM_DEVICE={}\nfailures={:?}\n",
        if failures.is_empty() { "PASS" } else { "FAIL" },
        failures
    ));
    fs::write(output.join("report.txt"), &metadata)?;
    fs::write(output.join("progress.csv"), metrics)?;
    let mut adc = fs::File::create(output.join("adc.csv"))?;
    writeln!(adc, "time_ns,adc_channel,microvolts")?;
    for (t, c, v) in &d.adc {
        writeln!(adc, "{t},{c},{v}")?;
    }
    let mut gpio = fs::File::create(output.join("gpio.csv"))?;
    writeln!(gpio, "time_ns,port_a,port_b,port_c,port_g")?;
    for (t, p) in &d.gpio {
        writeln!(gpio, "{t},{},{},{},{}", p[0], p[1], p[2], p[6])?;
    }
    fs::write(output.join("oled-final.pbm"), d.oled.pbm())?;
    fs::write(output.join("oled-final.svg"), d.oled.svg())?;
    fs::write(output.join("oled-final.bin"), d.oled.ram)?;
    let mut snapshots = Vec::new();
    let mut times = String::from("index,time_ns\n");
    for (index, (time, ram)) in d.oled_frames.iter().enumerate() {
        snapshots.extend_from_slice(ram);
        times.push_str(&format!("{index},{time}\n"));
    }
    fs::write(output.join("oled-frames.bin"), snapshots)?;
    fs::write(output.join("oled-frames.csv"), times)?;
    let mut midi = String::from("time_ns,byte\n");
    for (t, b) in &serial {
        midi.push_str(&format!("{t},{b}\n"));
    }
    fs::write(output.join("midi.csv"), midi)?;
    write_wav(&output.join("audio.wav"), &frames, rate.round() as u32)?;
    fs::write(output.join("settings.vdnor"), machine.nonvolatile_image())?;
    if machine.read32(0xe000ed28)? != 0 || machine.read32(0xe000ed2c)? != 0 {
        let regs = machine.registers();
        let mut stack = String::new();
        for address in (regs[13]..regs[13].saturating_add(128)).step_by(4) {
            if let Ok(value) = machine.read32(address) {
                stack.push_str(&format!("{address:08x} {value:08x}\n"));
            }
        }
        fs::write(output.join("fault-stack.txt"), stack)?;
    }
    println!("{metadata}");
    if !failures.is_empty() {
        return Err("device acceptance failed; retained report and traces".into());
    }
    Ok(())
}

fn grade(
    kind: &str,
    scenario: &str,
    d: &Device,
    serial: &[(u64, u8)],
    frames: &[[f32; 2]],
    log: &str,
    rate: f64,
) -> Vec<String> {
    let mut failures = Vec::new();
    if !(47_000. ..49_000.).contains(&rate) {
        failures.push("unexpected audio clock".into());
    }
    if frames
        .iter()
        .flat_map(|f| f.iter())
        .any(|v| !v.is_finite() || v.abs() > 1.)
    {
        failures.push("invalid/clipped audio".into());
    }
    if !d.oled.enabled || d.oled.data_bytes < 512 || d.oled.ram.iter().all(|v| *v == 0) {
        failures.push("OLED did not display a frame".into());
    }
    if kind == "keyboard" {
        if !log.contains("keyboard_keyboard ready:") {
            failures.push("keyboard startup marker missing".into());
        }
        if d.adc.len() < 7000 || d.scans < 20 {
            failures.push("keyboard stopped scanning".into());
        }
        let messages = midi_messages(serial);
        if scenario != "boot"
            && !messages
                .iter()
                .any(|(t, msg)| *t >= 170 * MS && msg == &[0x90, 54, 127])
            && !messages.iter().any(|(t, msg)| {
                *t >= 170 * MS && msg.len() == 3 && msg[0] == 0x90 && msg[1] == 54 && msg[2] > 0
            })
        {
            failures.push("HE1 note-on missing".into());
        }
        if scenario == "baseline" {
            let on = messages
                .iter()
                .filter(|(_, m)| m.len() == 3 && m[0] & 0xf0 == 0x90 && m[2] > 0)
                .count();
            let off = messages
                .iter()
                .filter(|(_, m)| {
                    m.len() == 3 && (m[0] & 0xf0 == 0x80 || (m[0] & 0xf0 == 0x90 && m[2] == 0))
                })
                .count();
            if on != 90 || off != 90 {
                failures.push(format!("expected 90 note-on/off pairs, got {on}/{off}"));
            }
            let melody: [u8; 70] = [
                78, 80, 82, 84, 86, 88, 90, 92, 94, 96, 98, 100, 102, 104, 71, 73, 75, 77, 79, 81,
                83, 85, 87, 89, 91, 93, 95, 97, 99, 66, 68, 70, 72, 74, 76, 78, 80, 82, 84, 86, 88,
                90, 92, 59, 61, 63, 65, 67, 69, 71, 73, 75, 77, 79, 81, 83, 85, 87, 54, 58, 60, 62,
                64, 66, 68, 70, 72, 74, 76, 80,
            ];
            for (i, note) in melody.iter().enumerate() {
                let start = (170 + (i as u64 / 14) * 85) * MS;
                let end = start + 80 * MS;
                let note = note - 24;
                if !messages.iter().any(|(t, m)| {
                    (*t >= start && *t < end)
                        && m.len() == 3
                        && m[0] == 0x90
                        && m[1] == note
                        && m[2] > 0
                }) {
                    failures.push(format!("HE{} expected note {note} missing", i + 1));
                }
            }
            for note in 36..=55 {
                if !messages.iter().any(|(t, m)| {
                    *t >= 620 * MS
                        && *t < 740 * MS
                        && m.len() == 3
                        && m[0] == 0x99
                        && m[1] == note
                        && m[2] > 0
                }) {
                    failures.push(format!("drum note {note} missing"));
                }
            }
            for (time, pc) in [(810, 0), (865, 1), (920, 2)] {
                if !messages
                    .iter()
                    .any(|(t, m)| *t >= time * MS && *t < (time + 50) * MS && m == &[0xc0, pc])
                {
                    failures.push(format!("program change {pc} missing"));
                }
            }
            if !messages
                .iter()
                .any(|(t, m)| *t >= 975 * MS && m.first() == Some(&0xe0) && m != &[0xe0, 0, 64])
            {
                failures.push("pitch bend missing".into());
            }
            if !messages.iter().any(|(t, m)| {
                *t >= 1030 * MS && m.len() == 3 && m[0] == 0xb0 && m[1] == 1 && m[2] > 0
            }) {
                failures.push("modulation missing".into());
            }
            for channel in 0..16 {
                if !messages
                    .iter()
                    .any(|(t, m)| *t >= 1085 * MS && m == &[0xb0 | channel, 123, 0])
                {
                    failures.push(format!("all notes off channel {channel} missing"));
                }
            }
            if !d
                .oled_frames
                .windows(2)
                .any(|w| w[0].0 >= 3000 * MS && w[0].1 != w[1].1)
            {
                failures.push("post-splash display did not change".into());
            }
            for cc in [7, 10, 11, 74, 36, 37, 38, 39, 40, 41, 42, 43] {
                if !messages
                    .iter()
                    .any(|(t, m)| *t > 740 * MS && m == &[0xb0, cc, 0])
                {
                    failures.push(format!("pot CC{cc}=0 missing"));
                }
            }
            if d.max_scan_gap > 2 * MS {
                failures.push("scan deadline exceeded".into());
            }
        }
    } else {
        if !log.contains("Startup complete, transitioning away from splash screen") {
            failures.push("Synthphone startup marker missing".into());
        }
        let tail = &frames[frames.len().saturating_sub(2400)..];
        if !["midi", "open-midi", "contention", "open-display"].contains(&scenario)
            && (frames.len() < 6000 || rms(tail) < 0.005)
        {
            failures.push("jack audio not processed".into());
        }
        let expected = if scenario == "baseline" {
            12
        } else if scenario == "open-key" {
            1
        } else {
            0
        };
        let presses = log.matches("Button pressed - Row:").count();
        if presses < expected {
            failures.push(format!(
                "keypad presses missing: expected {expected}, observed {presses}"
            ));
        }
        if scenario == "baseline" && !log.contains("Encoder single press detected!") {
            failures.push("encoder input not observed".into());
        }
        if scenario == "baseline" {
            for row in 0..4 {
                for column in 0..3 {
                    let marker = format!(
                        "Row: {row}, Col: {}, Key: {}",
                        2 - column,
                        row * 3 + column + 1
                    );
                    if !log.contains(&marker) {
                        failures.push(format!("keypad contact {row}/{column} not decoded"));
                    }
                }
            }
            if !log.contains("Encoder double press detected!") {
                failures.push("encoder double press not observed".into());
            }
        }
    }
    if kind != "keyboard" && (scenario == "midi" || scenario == "open-midi") {
        let start = (rate * 0.046) as usize;
        let end = ((rate * 0.060) as usize).min(frames.len());
        let note = frames.get(start..end).unwrap_or(&[]);
        if rms(note) < 0.005 {
            failures.push("MIDI note did not produce audio".into());
        }
    }
    if scenario == "profiles" {
        if log.matches("Key: 11").count() < 6 {
            failures.push("not all six profile-cycle contacts reached firmware".into());
        }
        for label in [
            "Pitch Ctrl",
            "Vocode",
            "Synth+Vox",
            "Synth+Tune",
            "Harmony",
            "Percussion",
        ] {
            if !d
                .oled_frames
                .iter()
                .any(|(_, ram)| Oled::has_profile_label(ram, label))
            {
                failures.push(format!("profile label {label} absent from OLED RAM"));
            }
        }
    }
    failures
}
fn midi_messages(bytes: &[(u64, u8)]) -> Vec<(u64, Vec<u8>)> {
    let mut out = Vec::new();
    let mut pending = Vec::new();
    let mut status = 0;
    for &(time, byte) in bytes {
        if byte & 0x80 != 0 {
            if byte >= 0xf8 {
                continue;
            }
            status = byte;
            pending.clear();
        } else if status >= 0x80 {
            pending.push(byte);
            let n = if status & 0xe0 == 0xc0 { 1 } else { 2 };
            if pending.len() == n {
                let mut m = vec![status];
                m.extend_from_slice(&pending);
                out.push((time, m));
                pending.clear();
            }
        }
    }
    out
}

/// Re-evaluate corrected observable expectations. Never edit the original execution report.
pub fn recheck(directory: &Path) -> Result<String> {
    let report = fs::read_to_string(directory.join("report.txt"))?;
    let analysis = analyze(directory)?;
    let scenario = report
        .lines()
        .find_map(|l| l.strip_prefix("scenario="))
        .ok_or("scenario missing")?;
    let negative = match scenario {
        "open-hall" => Some("HE1 note-on missing"),
        "open-key" => Some("keypad presses missing"),
        "open-jack" => Some("jack audio not processed"),
        "open-midi" => Some("MIDI note did not produce audio"),
        "open-display" => Some("display physically disconnected"),
        "contention" => Some("opposing push-pull columns"),
        _ => None,
    };
    let (pass, kind) = if let Some(marker) = negative {
        (
            report.contains("ARM_DEVICE=FAIL")
                && report.contains(marker)
                && !report.contains("Cortex-M fault"),
            "NEGATIVE_CONTROL",
        )
    } else if scenario == "baseline" && report.contains("profile=keyboard-current") {
        let controllers = [7, 10, 11, 74, 36, 37, 38, 39, 40, 41, 42, 43];
        let old: Vec<_> = controllers
            .iter()
            .map(|cc| format!("pot CC{cc}=127 missing"))
            .collect();
        let failures = report
            .lines()
            .find_map(|l| l.strip_prefix("failures="))
            .ok_or("missing original gates")?;
        // Only the known wrong-polarity expectation can be corrected from this historical capture.
        let original_other_gates_passed = failures == format!("{old:?}") || failures == "[]";
        let text = fs::read_to_string(directory.join("midi.csv"))?;
        let mut bytes = Vec::new();
        for line in text.lines().skip(1) {
            let (t, b) = line.split_once(',').ok_or("bad serial trace")?;
            bytes.push((t.parse::<u64>()?, b.parse::<u8>()?));
        }
        let messages = midi_messages(&bytes);
        let correct = controllers.iter().all(|cc| {
            messages
                .iter()
                .any(|(t, m)| *t > 740 * MS && m == &[0xb0, *cc, 0])
        });
        let balanced = analysis.contains("note_on=90\n")
            && analysis.contains("note_off=90\n")
            && analysis.contains("unmatched_releases=0\n")
            && analysis.contains("held_notes=0\n");
        (
            original_other_gates_passed && correct && balanced,
            "ARM_DEVICE_OBSERVATIONS",
        )
    } else {
        (report.contains("ARM_DEVICE=PASS"), "ORIGINAL_ACCEPTANCE")
    };
    let verdict=format!("{kind}={}\noriginal_report_sha256={}\nmidi_trace_sha256={}\noriginal_report_preserved=true\npolicy=observable-only; no CPU execution or circuit-state changes\n",if pass {"PASS"}else{"FAIL"},sha256(&directory.join("report.txt"))?,sha256(&directory.join("midi.csv"))?);
    fs::write(directory.join("recheck.txt"), &verdict)?;
    if !pass {
        return Err(verdict.into());
    }
    Ok(verdict)
}

/// Retain independent positive/negative captures even if an earlier scenario fails.
pub fn batch(args: &[String]) -> Result<()> {
    if args.len() != 7 {
        return Err(
            "device-batch PROFILE BASELINE FIXTURE ELF BOARD FRESH_OUTPUT SCENARIOS_CSV".into(),
        );
    }
    let out = Path::new(&args[5]);
    fs::create_dir(out)?;
    let mut summary = String::new();
    let mut failed = false;
    for scenario in args[6].split(',') {
        let mut run_args = args[..6].to_vec();
        run_args[5] = out.join(scenario).display().to_string();
        run_args.push(scenario.into());
        let result = run(&run_args);
        let report = fs::read_to_string(out.join(scenario).join("report.txt")).unwrap_or_default();
        let expected = match scenario {
            "open-hall" => Some("HE1 note-on missing"),
            "open-key" => Some("keypad presses missing"),
            "open-jack" => Some("jack audio not processed"),
            "open-midi" => Some("MIDI note did not produce audio"),
            "open-display" => Some("display physically disconnected"),
            "contention" => Some("opposing push-pull columns"),
            _ => None,
        };
        let pass = if let Some(marker) = expected {
            result.is_err()
                && report.contains(marker)
                && !report.contains("Cortex-M fault")
                && (!report.contains("unsupported") || scenario == "open-display")
        } else {
            result.is_ok()
        };
        summary.push_str(&format!(
            "{scenario}={}\n",
            if pass { "PASS" } else { "FAIL" }
        ));
        failed |= !pass;
        fs::write(out.join("suite.txt"), &summary)?;
    }
    println!("{summary}");
    if failed {
        Err("one or more device scenarios failed; captures retained".into())
    } else {
        Ok(())
    }
}

/// Inspect saved observations without executing or changing firmware.
pub fn analyze(directory: &Path) -> Result<String> {
    if let Ok(pixels) = fs::read(directory.join("oled-final.bin")) {
        if pixels.len() == 512 {
            let mut oled = Oled::default();
            oled.ram.copy_from_slice(&pixels);
            fs::write(directory.join("oled-final.svg"), oled.svg())?;
        }
    }
    let data = fs::read_to_string(directory.join("midi.csv"))?;
    let mut bytes = Vec::new();
    for line in data.lines().skip(1) {
        let (t, b) = line.split_once(',').ok_or("invalid MIDI trace")?;
        bytes.push((t.parse::<u64>()?, b.parse::<u8>()?));
    }
    let messages = midi_messages(&bytes);
    let mut active = BTreeMap::<(u8, u8), usize>::new();
    let mut on = 0;
    let mut off = 0;
    let mut unmatched = 0;
    let mut controls = std::collections::BTreeSet::new();
    let mut programs = std::collections::BTreeSet::new();
    for (_, m) in &messages {
        match m[0] & 0xf0 {
            0x90 if m[2] > 0 => {
                on += 1;
                *active.entry((m[0] & 15, m[1])).or_default() += 1;
            }
            0x80 | 0x90 => {
                off += 1;
                let count = active.entry((m[0] & 15, m[1])).or_default();
                if *count == 0 {
                    unmatched += 1;
                } else {
                    *count -= 1;
                }
            }
            0xb0 => {
                controls.insert((m[0] & 15, m[1], m[2]));
            }
            0xc0 => {
                programs.insert((m[0] & 15, m[1]));
            }
            _ => {}
        }
    }
    let mut report=format!("note_on={on}\nnote_off={off}\nunmatched_releases={unmatched}\nheld_notes={}\nobserved_cc={controls:?}\nobserved_programs={programs:?}\n",active.values().sum::<usize>());
    if let Ok(images) = fs::read(directory.join("oled-frames.bin")) {
        for label in [
            "Pitch Ctrl",
            "Vocode",
            "Synth+Vox",
            "Synth+Tune",
            "Harmony",
            "Percussion",
        ] {
            let present = images
                .chunks_exact(512)
                .any(|chunk| Oled::has_profile_label(chunk.try_into().unwrap(), label));
            report.push_str(&format!("oled_label_{label}={present}\n"));
            if let Some(chunk) = images
                .chunks_exact(512)
                .find(|&chunk| Oled::has_profile_label(chunk.try_into().unwrap(), label))
            {
                let mut oled = Oled::default();
                oled.ram.copy_from_slice(chunk);
                fs::write(
                    directory.join(format!("oled-{}.svg", label.replace(' ', "-"))),
                    oled.svg(),
                )?;
            }
        }
    }
    let reader = hound::WavReader::open(directory.join("audio.wav"))?;
    let samples: Vec<f32> = reader
        .into_samples()
        .collect::<std::result::Result<_, _>>()?;
    report.push_str(&format!(
        "non_finite_audio={}\npeak_audio={}\n",
        samples.iter().filter(|v| !v.is_finite()).count(),
        samples.iter().map(|v| v.abs()).fold(0_f32, f32::max)
    ));
    fs::write(directory.join("observations.txt"), &report)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn midi_observer_handles_running_status_realtime_and_program_messages() {
        let events: Vec<_> = [0x90, 60, 100, 0xf8, 61, 0, 0xc0, 2]
            .into_iter()
            .enumerate()
            .map(|(i, b)| (i as u64, b))
            .collect();
        let decoded = midi_messages(&events);
        assert_eq!(
            decoded,
            vec![
                (2, vec![0x90, 60, 100]),
                (5, vec![0x90, 61, 0]),
                (7, vec![0xc0, 2])
            ]
        );
    }
    #[test]
    fn incomplete_elf_cannot_supply_a_fake_ready_marker() {
        assert!(rtt_symbol(&[]).is_err());
        assert!(rtt_symbol(&[0; 64]).is_err());
    }
}
