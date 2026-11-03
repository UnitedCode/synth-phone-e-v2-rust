//! Physical MIDI/GPIO stimuli and audits for the complete production ELF.
use audio_lab::metrics;
use std::{error::Error, fs, path::Path};
type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn prepare(dir: &Path) -> Result<()> {
    fs::create_dir(dir)?;
    for (name, level, count) in [
        ("equal", 127, 3),
        ("focus", 32, 3),
        ("mute", 0, 3),
        ("eight", 32, 7),
    ] {
        let mut events = Vec::new();
        let mut midi = |step: usize, status, key, value| {
            for (i, byte) in [status, key, value].into_iter().enumerate() {
                events.push((step + i, format!("{},uart,usart1,{byte}\n", step + i)));
            }
        };
        midi(5, 176, 44, 64);
        midi(8, 176, 45, 0);
        midi(11, 179, 47, 0);
        midi(14, 179, 48, level);
        midi(57, 147, 57, 100); // A3 lead, below all backing notes.
        let notes = [64, 67, 71, 60, 62, 65, 69];
        for (i, &key) in notes[..count].iter().enumerate() {
            midi(60 + i * 3, 144, key, 100);
        }
        midi(100, 179, 64, 127);
        midi(110, 131, 57, 0);
        midi(120, 179, 64, 0);
        midi(140, 147, 57, 100);
        midi(174, 131, 57, 0);
        for (i, &key) in notes[..count].iter().enumerate() {
            midi(177 + i * 3, 128, key, 0);
        }
        if name == "eight" {
            midi(95, 179, 48, 16);
        }
        for step in [13, 21, 29, 37] {
            events.push((step, format!("{step},link,gpioPortC,1,gpioPortA,7\n")));
            events.push((
                step + 4,
                format!("{},unlink,gpioPortC,1,gpioPortA,7\n", step + 4),
            ));
        }
        events.extend([
            (45, "45,gpio,gpioPortC,10,0\n".into()),
            (53, "53,gpio,gpioPortC,10,1\n".into()),
        ]);
        events.sort_by_key(|event| event.0);
        fs::write(
            dir.join(format!("{name}.stimulus")),
            events.into_iter().map(|event| event.1).collect::<String>(),
        )?;
    }
    Ok(())
}

fn number(path: &Path, key: &str) -> Result<u64> {
    let text = fs::read_to_string(path)?;
    let prefix = format!("{key}=");
    let mut values = text.lines().filter_map(|line| line.strip_prefix(&prefix));
    let value = values.next().ok_or("missing counter")?.parse()?;
    if values.next().is_some() {
        return Err("duplicate counter".into());
    }
    Ok(value)
}

fn projection(x: &[f32], rate: u32, hz: f64) -> f64 {
    let (mut re, mut im, mut weight) = (0.0, 0.0, 0.0);
    for (i, &sample) in x.iter().enumerate() {
        let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / x.len() as f64).cos();
        let phase = std::f64::consts::TAU * hz * i as f64 / rate as f64;
        re += sample as f64 * w * phase.cos();
        im += sample as f64 * w * phase.sin();
        weight += w;
    }
    2.0 * (re * re + im * im).sqrt() / weight
}

fn inspect(dir: &Path) -> Result<()> {
    let manifest = fs::read_to_string(dir.join("firmware.txt"))?;
    let hash = manifest
        .lines()
        .find_map(|line| line.strip_prefix("firmware_sha256="))
        .ok_or("missing ELF identity")?;
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid ELF identity".into());
    }
    let mut report = String::new();
    let mut ratios = Vec::new();
    for name in ["equal", "focus", "mute", "eight"] {
        let done = fs::read_to_string(dir.join(format!("{name}.completed.txt")))?;
        if !done.lines().any(|l| l == "exit=0")
            || !done.lines().any(|l| l == format!("firmware_sha256={hash}"))
        {
            return Err("missing image-specific completion".into());
        }
        let host = fs::read_to_string(dir.join(format!("{name}.host.log")))?;
        for guard in [
            "Python execution guard verified",
            "Native instruction-fetch tag verified",
            "IRQ-aware wait pacing adapter verified",
            "NVIC active-count adapter verified",
            "PC: ",
        ] {
            if !host.contains(guard) {
                return Err(format!("{name}: missing CPU guard").into());
            }
        }
        let faults = fs::read_to_string(dir.join(format!("{name}.cpu-faults.txt")))?;
        for address in ["0xE000ED28", "0xE000ED2C"] {
            if !faults.lines().any(|l| l == format!("{address}=0x00000000")) {
                return Err("Cortex-M fault status not clear".into());
            }
        }
        let rtt = fs::read_to_string(dir.join(format!("{name}.rtt")))?;
        if !rtt.contains("Startup complete, transitioning away from splash screen")
            || [
                "Dropped",
                "Failed to spawn",
                "panicked",
                "HardFault",
                "ERROR",
                "underrun error",
            ]
            .iter()
            .any(|s| rtt.contains(s))
        {
            return Err("firmware boot/task failure".into());
        }
        let log = fs::read_to_string(dir.join(format!("{name}.log")))?;
        for line in log.lines() {
            if line.contains("[ERROR]") {
                return Err("emulator error".into());
            }
            if let Some((_, warning)) = line.split_once("[WARNING] ") {
                if !include_str!("../../virtual-daisy/known-warnings.txt")
                    .lines()
                    .any(|known| !known.is_empty() && known == warning)
                {
                    return Err(format!("unexpected warning: {warning}").into());
                }
            }
        }
        let reader = hound::WavReader::open(dir.join(format!("{name}.wav")))?;
        let spec = reader.spec();
        if spec.channels != 2 || spec.sample_format != hound::SampleFormat::Float {
            return Err("expected stereo float capture".into());
        }
        let stereo = reader
            .into_samples::<f32>()
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if stereo.len() % 2 != 0
            || stereo
                .chunks_exact(2)
                .any(|s| !s[0].is_finite() || s[0] != s[1] || s[0].abs() >= 0.79999)
        {
            return Err("invalid, unequal or limited stereo".into());
        }
        let audio: Vec<_> = stereo.chunks_exact(2).map(|s| s[0]).collect();
        let stats = dir.join(format!("{name}.wav.stats"));
        let clock = dir.join(format!("{name}.wav.clock.txt"));
        let denominator = number(&clock, "sai_rate_denominator")?;
        if denominator == 0 {
            return Err("invalid sample clock".into());
        }
        let actual_rate = number(&clock, "sai_rate_numerator")? as f64 / denominator as f64;
        if actual_rate.round() as u32 != spec.sample_rate
            || number(&stats, "frames")? != audio.len() as u64
            || number(&stats, "underruns")? != 0
            || number(&stats, "overruns")? != 0
            || audio.len() < (actual_rate * 1.9) as usize
            || audio.len() > (actual_rate * 2.0) as usize
        {
            return Err("DMA/rate/length mismatch".into());
        }
        let segment = |a: f64, b: f64| {
            &audio[(a * spec.sample_rate as f64) as usize..(b * spec.sample_rate as f64) as usize]
        };
        let (held, sustained, rest, returned) = (
            metrics(segment(0.93, 1.05)).rms,
            metrics(segment(1.14, 1.18)).rms,
            metrics(segment(1.27, 1.35)).rms,
            metrics(segment(1.49, 1.59)).rms,
        );
        if held < 0.0001
            || sustained < 0.0001
            || returned < 0.0001
            || (name == "mute" && rest > 1e-5)
            || (name != "mute" && rest < 0.0001)
        {
            return Err(format!("{name}: lead/sustain/rest control not reflected in PCM: {held},{sustained},{rest},{returned}").into());
        }
        if name == "equal" || name == "focus" {
            let shift = spec.sample_rate as f64 / 48000.0;
            let ratio = projection(segment(0.93, 1.05), spec.sample_rate, 220.0 * shift)
                / projection(segment(0.93, 1.05), spec.sample_rate, 329.62756 * shift);
            ratios.push(ratio);
        }
        report.push_str(&format!("{name}: frames={} held={held:.6} key-up-under-pedal={sustained:.6} lead-rest={rest:.6} returned={returned:.6}; boot/fault/stereo/DMA passed\n",audio.len()));
    }
    let change = 20.0 * (ratios[1] / ratios[0]).log10();
    let expected = 20.0 * (127.0_f64 / 32.0).log10();
    if !change.is_finite() || (change - expected).abs() > 1.5 {
        return Err(
            format!("lead/backing control response {change} dB; expected {expected}").into(),
        );
    }
    report.push_str(&format!("lead/backing spectral-ratio change {change:.3} dB, expected {expected:.3} dB. Functional model only; physical timing and listening acceptance unverified.\n"));
    fs::write(dir.join("native-audit.txt"), &report)?;
    print!("{report}");
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [mode, dir] if mode == "prepare" => prepare(Path::new(dir)),
        [mode, dir] if mode == "inspect" => inspect(Path::new(dir)),
        _ => {
            Err("usage: midi_lead_capture prepare NEW_DIRECTORY | inspect CAPTURE_DIRECTORY".into())
        }
    }
}
