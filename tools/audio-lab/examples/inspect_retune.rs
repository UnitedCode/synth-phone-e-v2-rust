//! Pitch audit of captured vocals. PCM distance alone is phase-sensitive.
use audio_lab::{cents, constants::SAMPLE_RATE, estimate_pitch, metrics};
use std::{env, error::Error, path::Path};

fn counter(path: &Path, name: &str) -> Result<u64, Box<dyn Error>> {
    let text = std::fs::read_to_string(path)?;
    let prefix = format!("{name}=");
    let mut values = text.lines().filter_map(|s| s.strip_prefix(&prefix));
    let value = values.next().ok_or("Missing counter")?.parse()?;
    if values.next().is_some() {
        return Err("Duplicate counter".into());
    }
    Ok(value)
}

fn verify_native(directory: &Path) -> Result<(), Box<dyn Error>> {
    let result = directory.join("native-audit.txt");
    if result.exists() {
        std::fs::remove_file(&result)?;
    }
    let manifest = std::fs::read_to_string(directory.join("firmware.txt"))?;
    let mut report = String::new();
    for name in ["before", "after-slow", "after-fast"] {
        let field = if name == "before" {
            "before_sha256="
        } else {
            "after_sha256="
        };
        let hash = manifest
            .lines()
            .find_map(|s| s.strip_prefix(field))
            .ok_or("Missing image hash")?;
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Invalid image hash".into());
        }
        let completion = std::fs::read_to_string(directory.join(format!("{name}.completed.txt")))?;
        if !completion.lines().any(|s| s == "exit=0")
            || !completion
                .lines()
                .any(|s| s.strip_prefix("firmware_sha256=") == Some(hash))
        {
            return Err("Wrong image or missing process completion".into());
        }
        let host = std::fs::read_to_string(directory.join(format!("{name}.host.log")))?;
        for guard in [
            "Python execution guard verified",
            "Native instruction-fetch tag verified",
            "IRQ-aware wait pacing adapter verified",
            "NVIC active-count adapter verified",
            "PC: ",
        ] {
            if !host.contains(guard) {
                return Err("Missing CPU/guard evidence".into());
            }
        }
        let rtt = std::fs::read_to_string(directory.join(format!("{name}.rtt")))?;
        if !rtt.contains("Startup complete, transitioning away from splash screen")
            || [
                "panicked",
                "HardFault",
                "ERROR",
                "underrun error",
                "Dropped MIDI event",
            ]
            .iter()
            .any(|s| rtt.contains(s))
        {
            return Err("Incomplete boot or firmware fault".into());
        }
        let reader = hound::WavReader::open(directory.join(format!("{name}.wav")))?;
        let spec = reader.spec();
        if spec.channels != 2
            || spec.sample_format != hound::SampleFormat::Float
            || spec.bits_per_sample != 32
        {
            return Err("Expected stereo float capture".into());
        }
        let samples = reader
            .into_samples::<f32>()
            .collect::<Result<Vec<_>, _>>()?;
        if metrics(&samples).rms < 0.001
            || samples.len() % 2 != 0
            || samples
                .chunks_exact(2)
                .any(|p| !p[0].is_finite() || p[0] != p[1] || p[0].abs() >= 0.79999)
        {
            return Err("Nonfinite, unequal stereo, or output ceiling hit".into());
        }
        let stats = directory.join(format!("{name}.wav.stats"));
        let clock = directory.join(format!("{name}.wav.clock.txt"));
        let denominator = counter(&clock, "sai_rate_denominator")?;
        if denominator == 0 {
            return Err("Invalid SAI clock".into());
        }
        let rate = counter(&clock, "sai_rate_numerator")? as f64 / denominator as f64;
        let frames = samples.len() / 2;
        if counter(&stats, "frames")? != frames as u64
            || counter(&stats, "underruns")? != 0
            || counter(&stats, "overruns")? != 0
            || rate.round() as u32 != spec.sample_rate
            || (frames as f64) < rate * 1.95
            || (frames as f64) > rate * 2.0
        {
            return Err("DMA/rate/length mismatch".into());
        }
        report.push_str(&format!("{name}: exit=0, frames={frames}, peak={}, finite matching stereo, zero DMA errors and no ceiling hits\n",metrics(&samples).peak));
    }
    report.push_str("PASS: functional production ARM captures. Two-second runs do not test settings persistence; audibility and musical quality require listening.\n");
    std::fs::write(result, report)?;
    Ok(())
}

fn read(path: &Path) -> Result<(u32, Vec<f32>), Box<dyn Error>> {
    let reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    if ![1, 2].contains(&spec.channels) || spec.sample_format != hound::SampleFormat::Float {
        return Err("Expected mono/stereo float WAV".into());
    }
    let samples = reader
        .into_samples::<f32>()
        .collect::<Result<Vec<_>, _>>()?;
    if samples.iter().any(|s| !s.is_finite()) {
        return Err("Nonfinite capture".into());
    }
    Ok((
        spec.sample_rate,
        samples
            .chunks_exact(spec.channels as usize)
            .map(|s| s[0])
            .collect(),
    ))
}

fn scan(path: &Path) -> Result<(), Box<dyn Error>> {
    let (rate, audio) = read(path)?;
    let window = (rate as f64 * 0.12) as usize;
    let hop = (rate as f64 * 0.04) as usize;
    let mut spans = Vec::new();
    let mut run: Option<(usize, usize, f64, f64, usize)> = None;
    for start in (0..audio.len().saturating_sub(window)).step_by(hop) {
        let samples = &audio[start..start + window];
        let pitch = estimate_pitch(samples).map(|p| p * rate as f64 / SAMPLE_RATE as f64);
        let pitch = pitch.filter(|_| metrics(samples).rms >= 0.01);
        if let (Some((first, _, initial, sum, count)), Some(p)) = (run, pitch) {
            if cents(p, initial).abs() <= 25.0 {
                run = Some((first, start + window, initial, sum + p, count + 1));
                continue;
            }
        }
        if let Some((first, end, _, sum, count)) = run.take() {
            if end - first >= (rate as f64 * 0.6) as usize {
                spans.push((first, end, sum / count as f64));
            }
        }
        run = pitch.map(|p| (start, start + window, p, p, 1));
    }
    if let Some((first, end, _, sum, count)) = run {
        if end - first >= (rate as f64 * 0.6) as usize {
            spans.push((first, end, sum / count as f64));
        }
    }
    spans.sort_by_key(|(start, end, _)| std::cmp::Reverse(end - start));
    println!("start_seconds,end_seconds,duration,mean_estimated_hz");
    for (start, end, pitch) in spans.into_iter().take(12) {
        println!(
            "{:.4},{:.4},{:.4},{pitch:.4}",
            start as f64 / rate as f64,
            end as f64 / rate as f64,
            (end - start) as f64 / rate as f64
        );
    }
    Ok(())
}

fn host_audition(input: &Path, out: &Path) -> Result<(), Box<dyn Error>> {
    let (rate, input) = read(input)?;
    if rate != SAMPLE_RATE.round() as u32 {
        return Err("Use a 48014 Hz fixture".into());
    }
    std::fs::create_dir_all(out)?;
    for (name, speed) in [("after-slow", 1), ("after-fast", 10)] {
        let result = audio_lab::render(
            &input,
            &[],
            &audio_lab::vocal_frame::config_for_speed(speed),
            &synthphone_e_vocal_dsp::MusicalSettings::default(),
        );
        let mut writer = hound::WavWriter::create(
            out.join(format!("{name}.wav")),
            hound::WavSpec {
                channels: 2,
                sample_rate: rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )?;
        for sample in result.audio {
            writer.write_sample(sample)?;
            writer.write_sample(sample)?;
        }
        writer.finalize()?;
    }
    eprintln!("Host shared-DSP preflight only; these files are not native firmware captures.");
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut directory = env::args_os()
        .nth(1)
        .ok_or("Usage: inspect_retune CAPTURE_DIRECTORY")?;
    let native = directory == "native";
    if native {
        directory = env::args_os()
            .nth(2)
            .ok_or("native requires capture directory")?;
        verify_native(Path::new(&directory))?;
    }
    if directory == "phase-control" {
        let a = audio_lab::tone(220.0, 0.5, &[(1, 0.2)]);
        let b: Vec<f32> = (0..a.len())
            .map(|i| {
                (0.2 * (std::f64::consts::TAU * 220.0 * i as f64 / SAMPLE_RATE as f64).cos()) as f32
            })
            .collect();
        let delta = cents(
            estimate_pitch(&a).ok_or("Missing sine pitch")?,
            estimate_pitch(&b).ok_or("Missing cosine pitch")?,
        )
        .abs();
        let difference: Vec<_> = a.iter().zip(&b).map(|(x, y)| x - y).collect();
        let pcm = metrics(&difference).rms / metrics(&a).rms;
        println!("90-degree phase control: relative PCM difference={pcm:.4}, pitch difference={delta:.4} cents");
        if delta > 3.0 || pcm < 1.3 {
            return Err("Phase control failed".into());
        }
        return Ok(());
    }
    if directory == "scan" {
        return scan(Path::new(
            &env::args_os().nth(2).ok_or("scan requires a WAV")?,
        ));
    }
    if directory == "host" {
        return host_audition(
            Path::new(&env::args_os().nth(2).ok_or("host requires input WAV")?),
            Path::new(
                &env::args_os()
                    .nth(3)
                    .ok_or("host requires output directory")?,
            ),
        );
    }
    let directory = Path::new(&directory);
    let first_window: f64 = match env::args_os().nth(if native { 3 } else { 2 }) {
        Some(value) => value
            .to_str()
            .ok_or("Start time must be numeric UTF-8")?
            .parse()?,
        None => {
            if native {
                0.2
            } else {
                0.6
            }
        }
    };
    if !first_window.is_finite() || first_window < 0.0 {
        return Err("Invalid start time".into());
    }
    let (rate, fast) = read(&directory.join("after-fast.wav"))?;
    let (slow_rate, slow) = read(&directory.join("after-slow.wav"))?;
    if rate != slow_rate {
        return Err("Capture rates differ".into());
    }
    let window = (rate as f64 * 0.12) as usize;
    let hop = (rate as f64 * 0.02) as usize;
    let mut differences = Vec::new();
    let mut candidates = 0;
    let mut ambiguous = 0;
    println!("center_seconds,fast_hz,slow_hz,fast_minus_slow_cents,fast_rms,slow_rms");
    for start in ((rate as f64 * first_window) as usize
        ..fast.len().min(slow.len()).saturating_sub(window))
        .step_by(hop)
    {
        let a = &fast[start..start + window];
        let b = &slow[start..start + window];
        let (Some(f), Some(s)) = (estimate_pitch(a), estimate_pitch(b)) else {
            continue;
        };
        let (f, s) = (
            f * rate as f64 / SAMPLE_RATE as f64,
            s * rate as f64 / SAMPLE_RATE as f64,
        );
        let delta = cents(f, s);
        candidates += 1;
        if delta.abs() > 300.0 {
            ambiguous += 1;
        } else {
            differences.push(delta.abs());
        }
        println!(
            "{:.4},{f:.4},{s:.4},{delta:.3},{:.6},{:.6}",
            (start as f64 + window as f64 / 2.0) / rate as f64,
            metrics(a).rms,
            metrics(b).rms
        );
    }
    differences.sort_by(f64::total_cmp);
    eprintln!("paired pitch estimates={candidates}; >300-cent ambiguous windows={ambiguous}; comparable={}", differences.len());
    if !differences.is_empty() {
        eprintln!(
            "absolute difference: median={:.2} cents, p95={:.2} cents; >=25 cents in {} windows",
            differences[differences.len() / 2],
            differences[differences.len() * 95 / 100],
            differences.iter().filter(|&&d| d >= 25.0).count()
        );
    }
    eprintln!("Overlapping 120ms windows at 20ms hops; estimator misses/ambiguities are not musical accuracy grades.");
    Ok(())
}
