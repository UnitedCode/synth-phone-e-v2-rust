//! Prepare external real-vocal fixtures and audit production ARM captures.
//! Song files and generated audio stay in the caller's output directory.
use audio_lab::{constants::SAMPLE_RATE, metrics};
use std::{error::Error, fs, io::Write, path::Path};
type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn read(path: &Path) -> Result<(u32, Vec<f32>)> {
    let r = hound::WavReader::open(path)?;
    let spec = r.spec();
    if spec.sample_format != hound::SampleFormat::Float || ![1, 2].contains(&spec.channels) {
        return Err("expected mono or stereo float WAV".into());
    }
    let raw = r
        .into_samples::<f32>()
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if raw.len() % spec.channels as usize != 0 || raw.iter().any(|x| !x.is_finite()) {
        return Err("invalid PCM".into());
    }
    Ok((
        spec.sample_rate,
        raw.chunks_exact(spec.channels as usize)
            .map(|s| s[0])
            .collect(),
    ))
}

fn prepare(source: &Path, out: &Path) -> Result<()> {
    let (rate, source) = read(source)?;
    if rate != SAMPLE_RATE as u32 || source.len() < rate as usize / 2 {
        return Err("need at least half a second of 48014 Hz vocals".into());
    }
    fs::create_dir_all(out)?;
    let mut input = vec![0.0; rate as usize * 21 / 10];
    let start = rate as usize * 3 / 4;
    for (to, &sample) in input[start..].iter_mut().zip(&source) {
        *to = sample * 0.35;
    }
    let mut pcm = fs::File::create(out.join("input.f32le"))?;
    let mut wav = hound::WavWriter::create(
        out.join("input.wav"),
        hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )?;
    for sample in input {
        pcm.write_all(&sample.to_le_bytes())?;
        pcm.write_all(&sample.to_le_bytes())?;
        wav.write_sample(sample)?;
    }
    wav.finalize()?;
    for (name, lead, count) in [
        ("before", 127, 4),
        ("after", 127, 4),
        ("after-midi-only", 0, 4),
        ("after-eight", 0, 8),
    ] {
        let mut events = Vec::new();
        // Each bridge step is ten milliseconds. Keep UART bytes on separate
        // steps so every receive interrupt can be serviced.
        let mut midi = |ms: usize, status, key, value| {
            for (i, byte) in [status, key, value].into_iter().enumerate() {
                events.push((ms + i, format!("{},uart,usart1,{byte}\n", ms + i)));
            }
        };
        midi(5, 176, 44, 64); // Voice Level 10: unity.
        midi(8, 176, 45, 0); // Synth mute; only vocal harmonies audible.
        midi(11, 176, 47, lead);
        let notes = [57, 59, 60, 62, 64, 65, 67, 69];
        for (i, &key) in notes[..count].iter().enumerate() {
            midi(57 + i * 3, 144, key, 100);
        }
        let down = 57 + count * 3;
        midi(down, 176, 64, 127);
        // Hold the same complete chord throughout the matched A/B vocal.
        // The MIDI-only cases separately exercise sustain and key release.
        let release = if name == "before" || name == "after" {
            175
        } else {
            115
        };
        for (i, &key) in notes[..count].iter().enumerate() {
            midi(release + i * 3, 128, key, 0);
        }
        let up = if name == "before" || name == "after" {
            190
        } else if count == 4 {
            150
        } else {
            160
        };
        midi(up, 176, 64, 0);
        for ms in [13, 21, 29, 37] {
            events.push((ms, format!("{ms},link,gpioPortC,1,gpioPortA,7\n")));
            events.push((
                ms + 4,
                format!("{},unlink,gpioPortC,1,gpioPortA,7\n", ms + 4),
            ));
        }
        events.extend([
            (45, "45,gpio,gpioPortC,10,0\n".into()),
            (53, "53,gpio,gpioPortC,10,1\n".into()),
        ]);
        events.sort_by_key(|e| e.0);
        fs::write(
            out.join(format!("{name}.stimulus")),
            events.into_iter().map(|e| e.1).collect::<String>(),
        )?;
    }
    Ok(())
}

fn counter(path: &Path, key: &str) -> Result<u64> {
    let text = fs::read_to_string(path)?;
    let prefix = format!("{key}=");
    let mut values = text.lines().filter_map(|s| s.strip_prefix(&prefix));
    let n = values
        .next()
        .ok_or_else(|| format!("missing {key}"))?
        .parse()?;
    if values.next().is_some() {
        return Err("duplicate counter".into());
    }
    Ok(n)
}

fn listen(input: &Path, output: &Path) -> Result<()> {
    listen_at(input, output, 0.1)
}

fn listen_at(input: &Path, output: &Path, target_rms: f64) -> Result<()> {
    let (rate, samples) = read(input)?;
    let m = metrics(&samples);
    if m.rms < 1e-7 || output.exists() {
        return Err("need audible PCM and a new output path".into());
    }
    // Playback only: a constant gain, without compression, filtering or a
    // firmware change. Preserve the raw capture for level comparisons.
    let gain = (target_rms / m.rms).min(0.9 / f64::from(m.peak));
    let mut writer = hound::WavWriter::create(
        output,
        hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )?;
    for sample in samples {
        writer.write_sample((f64::from(sample) * gain) as f32)?;
    }
    writer.finalize()?;
    println!(
        "{}: playback gain {gain:.6} ({:.3} dB), raw input preserved",
        output.display(),
        20.0 * gain.log10()
    );
    Ok(())
}

fn inspect(out: &Path) -> Result<()> {
    let manifest = fs::read_to_string(out.join("firmware.txt"))?;
    let mut report = String::new();
    for name in ["before", "after", "after-midi-only", "after-eight"] {
        let field = if name == "before" {
            "before_sha256="
        } else {
            "after_sha256="
        };
        let hash = manifest
            .lines()
            .find_map(|s| s.strip_prefix(field))
            .ok_or("missing image hash")?;
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("invalid image hash".into());
        }
        let done = fs::read_to_string(out.join(format!("{name}.completed.txt")))?;
        if !done.lines().any(|s| s == "exit=0")
            || !done.lines().any(|s| s == format!("firmware_sha256={hash}"))
        {
            return Err("missing successful image-specific completion".into());
        }
        let host = fs::read_to_string(out.join(format!("{name}.host.log")))?;
        for guard in [
            "Python execution guard verified",
            "Native instruction-fetch tag verified",
            "IRQ-aware wait pacing adapter verified",
            "NVIC active-count adapter verified",
            "PC: ",
        ] {
            if !host.contains(guard) {
                return Err(format!("{name}: missing {guard}").into());
            }
        }
        let rtt = fs::read_to_string(out.join(format!("{name}.rtt")))?;
        let faults = fs::read_to_string(out.join(format!("{name}.cpu-faults.txt")))?;
        for address in ["0xE000ED28", "0xE000ED2C"] {
            if !faults
                .lines()
                .any(|line| line == format!("{address}=0x00000000"))
            {
                return Err(format!("{name}: Cortex-M fault status is not clear").into());
            }
        }
        if !rtt.contains("Startup complete, transitioning away from splash screen")
            || [
                "panicked",
                "HardFault",
                "ERROR",
                "underrun error",
                "Dropped MIDI event",
                "Failed to spawn",
            ]
            .iter()
            .any(|s| rtt.contains(s))
        {
            return Err(format!("{name}: boot/fault/task failure").into());
        }
        let path = out.join(format!("{name}.wav"));
        let (rate, samples) = read(&path)?;
        let reader = hound::WavReader::open(&path)?;
        if reader.spec().channels != 2 {
            return Err("expected stereo native capture".into());
        }
        let stereo = reader
            .into_samples::<f32>()
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if stereo.chunks_exact(2).any(|s| s[0] != s[1]) {
            return Err("native channels differ".into());
        }
        let clock = out.join(format!("{name}.wav.clock.txt"));
        let denominator = counter(&clock, "sai_rate_denominator")?;
        if denominator == 0
            || (counter(&clock, "sai_rate_numerator")? as f64 / denominator as f64).round() as u32
                != rate
        {
            return Err("capture rate differs from modeled SAI clock".into());
        }
        let emulator_log = fs::read_to_string(out.join(format!("{name}.log")))?;
        for line in emulator_log.lines() {
            if line.contains("[ERROR]") {
                return Err(format!("{name}: emulator error: {line}").into());
            }
            if let Some((_, message)) = line.split_once("[WARNING] ") {
                if !include_str!("../../virtual-daisy/known-warnings.txt")
                    .lines()
                    .any(|known| !known.is_empty() && known == message)
                {
                    return Err(format!("{name}: unexpected emulator warning: {message}").into());
                }
            }
        }
        let stats = out.join(format!("{name}.wav.stats"));
        let m = metrics(&samples);
        if counter(&stats, "frames")? != samples.len() as u64
            || counter(&stats, "underruns")? != 0
            || counter(&stats, "overruns")? != 0
            || samples.len() < rate as usize * 19 / 10
            || samples.len() > rate as usize * 2
            || m.peak >= 0.79999
            || m.rms < 0.0001
        {
            return Err(format!("{name}: audio length/DMA/ceiling failure").into());
        }
        let segment = |a: f64, b: f64| {
            metrics(&samples[(a * rate as f64) as usize..(b * rate as f64) as usize]).rms
        };
        report.push_str(&format!("{name}: frames={} peak={:.6} rms={:.6} held={:.6} after_key_up={:.6} after_pedal_up={:.6}; boot/CPU guards/DMA passed\n", samples.len(),m.peak,m.rms,segment(0.95,1.05),segment(1.35,1.45),segment(1.60,1.65)));
        if name == "after-midi-only" && (segment(1.35, 1.45) < 0.001 || segment(1.60, 1.65) > 1e-5)
        {
            return Err("sustain/gating not reflected in production audio".into());
        }
    }
    report.push_str("Functional model evidence only; physical CPU deadlines and musical acceptance remain hardware/listening gates.\n");
    fs::write(out.join("native-audit.txt"), &report)?;
    print!("{report}");
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [mode,input] if mode == "levels" => { let (_,pcm)=read(Path::new(input))?; println!("{:?}",metrics(&pcm)); Ok(()) },
        [mode, source, out] if mode == "prepare" => prepare(Path::new(source), Path::new(out)),
        [mode, out] if mode == "inspect" => inspect(Path::new(out)),
        [mode, input, output] if mode == "listen" => listen(Path::new(input),Path::new(output)),
        [mode, a,b,out_a,out_b] if mode == "match" => {
            let (rate_a,pcm_a)=read(Path::new(a))?;
            let (rate_b,pcm_b)=read(Path::new(b))?;
            if rate_a!=rate_b || pcm_a.len()!=pcm_b.len() {return Err("matching requires the same rate and duration".into());}
            let (ma,mb)=(metrics(&pcm_a),metrics(&pcm_b));
            if ma.rms<1e-7 || mb.rms<1e-7 {return Err("need audible inputs".into());}
            let target=0.1_f64.min(0.9*ma.rms/ma.peak as f64).min(0.9*mb.rms/mb.peak as f64);
            println!("common playback RMS={target}");
            listen_at(Path::new(a),Path::new(out_a),target)?;
            listen_at(Path::new(b),Path::new(out_b),target)
        }
        _ => Err("usage: harmony_capture prepare VOCAL.wav OUTPUT | inspect OUTPUT | levels INPUT.wav | listen INPUT.wav OUTPUT.wav | match A.wav B.wav OUT_A.wav OUT_B.wav".into()),
    }
}
