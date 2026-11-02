//! Real-vocal speed regression through unmodified production ARM images.
use super::{bounded_with_limit, grade, Result, RATE};
use sha2::{Digest, Sha256};
use std::{env, fs, io::Write, path::Path, process::Command, time::Duration};

const CASES: [(&str, bool, u8); 5] = [
    ("before-slow", false, 1),
    ("before-fast", false, 10),
    ("after-slow", true, 1),
    ("after-medium", true, 5),
    ("after-fast", true, 10),
];

fn hash(path: &Path) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(fs::read(path)?)))
}

pub fn run(
    runtime: &Path,
    prepared: &Path,
    before: &Path,
    after: &Path,
    input: &Path,
    out: &Path,
) -> Result<()> {
    let cwd = env::current_dir()?;
    let paths = [runtime, prepared, before, after, input, out].map(|p| {
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            cwd.join(p)
        }
    });
    let [runtime, prepared, before, after, input, out] = paths.each_ref().map(|p| p.as_path());
    let conformance = fs::read_to_string(prepared.join("conformance.txt"))?;
    if !conformance.lines().any(|s| s == "board=rev5")
        || !conformance
            .lines()
            .any(|s| s == "audio_input_boundary=serial")
    {
        return Err("Prepare Rev5 serial-PCM input first".into());
    }
    fs::create_dir_all(out)?;
    for name in ["result.txt", "comparison.csv"] {
        if out.join(name).exists() {
            fs::remove_file(out.join(name))?;
        }
    }
    super::wav_input_limit(input, &out.join("source.f32le"), (RATE as usize * 41) / 10)?;
    let source = fs::read(out.join("source.f32le"))?;
    let samples: Vec<_> = source
        .chunks_exact(8)
        .map(|s| f32::from_le_bytes(s[..4].try_into().unwrap()))
        .collect();
    if samples.len() < RATE as usize * 4 {
        return Err("Need at least four seconds of vocals".into());
    }
    let peak = samples.iter().fold(0.0_f32, |p, x| p.max(x.abs()));
    if peak < 0.001 {
        return Err("Need audible vocals".into());
    }
    let gain = 0.35 / peak;
    let pcm = out.join("vocal.f32le");
    let mut file = fs::File::create(&pcm)?;
    for sample in samples {
        let sample = sample * gain;
        file.write_all(&sample.to_le_bytes())?;
        file.write_all(&sample.to_le_bytes())?;
    }
    let before_hash = hash(before)?;
    let after_hash = hash(after)?;
    if before_hash == after_hash {
        return Err("A/B requires different images".into());
    }
    fs::write(out.join("firmware.txt"), format!(
        "before_sha256={before_hash}\nafter_sha256={after_hash}\nboard=rev5\ncodec=WM8731\naudio_boundary=serial\nsource_peak={peak}\ninput_gain={gain}\ninput_peak=0.35\nvirtual_seconds=4\ncomparison_window_seconds=0.6..3.8\n"))?;
    let mut plan = fs::File::create(out.join("capture-plan.csv"))?;
    writeln!(
        plan,
        "case,firmware_sha256,requested_speed,voice_level,synth_level,virtual_seconds"
    )?;
    for (label, updated, speed) in CASES {
        writeln!(
            plan,
            "{label},{},{speed},10,0,4",
            if updated { &after_hash } else { &before_hash }
        )?;
    }
    let dotnet = env::var_os("VIRTUAL_DAISY_DOTNET").unwrap_or_else(|| "dotnet".into());
    for chunk in CASES.chunks(3) {
        let results = std::thread::scope(|scope| {
            let mut workers = Vec::new();
            for &(label, updated, speed) in chunk {
                let (pcm, dotnet) = (&pcm, &dotnet);
                workers.push(scope.spawn(move || -> std::result::Result<(), String> {
                    let capture = || -> Result<()> {
                        let elf = if updated { after } else { before };
                        let image_hash = hash(elf)?;
                        let prefix = out.join(label);
                        for suffix in [".wav", ".wav.stats", ".host.log", ".rtt", ".completed.txt"] {
                            let file = grade::artifact(&prefix, suffix);
                            if file.exists() { fs::remove_file(file)?; }
                        }
                        let script = grade::artifact(&prefix, ".stimulus");
                        let cc = (u16::from(speed - 1) * 127).div_ceil(9);
                        // Physical MIDI: voice unity, synth mute, speed 1..10.
                        fs::write(&script, format!("5,uart,usart1,176\n6,uart,usart1,44\n7,uart,usart1,64\n8,uart,usart1,176\n9,uart,usart1,45\n10,uart,usart1,0\n11,uart,usart1,176\n12,uart,usart1,46\n13,uart,usart1,{cc}\n"))?;
                        bounded_with_limit(Command::new(dotnet)
                            .arg(prepared.join("managed/DaisyHost.dll"))
                            .arg(runtime).arg(prepared.join("daisy.repl")).arg(elf).arg(&prefix).arg(pcm)
                            .args(["none", "rev5", "1", "serial", "typical"]).arg(&script)
                            .env("VIRTUAL_DAISY_INFRASTRUCTURE", prepared.join("managed/Infrastructure.dll"))
                            .env("VIRTUAL_DAISY_AUDIO_STEPS", "400"),
                            &grade::artifact(&prefix, ".host.log"), Duration::from_secs(600))?;
                        if hash(elf)? != image_hash { return Err("Image changed during capture".into()); }
                        fs::write(grade::artifact(&prefix, ".completed.txt"), format!("exit=0\nfirmware_sha256={image_hash}\n"))?;
                        println!("Completed autotune capture: {label}");
                        Ok(())
                    };
                    capture().map_err(|e| e.to_string())
                }));
            }
            workers
                .into_iter()
                .map(|w| w.join().unwrap())
                .collect::<Vec<_>>()
        });
        for result in results {
            result?;
        }
    }
    if hash(before)? != before_hash || hash(after)? != after_hash {
        return Err("Image changed during A/B".into());
    }
    analyze(out)
}

fn read_capture(out: &Path, label: &str, expected_hash: &str) -> Result<Vec<f32>> {
    let prefix = out.join(label);
    grade::execution_evidence(&prefix)?;
    let completed = fs::read_to_string(grade::artifact(&prefix, ".completed.txt"))?;
    if !completed.lines().any(|s| s == "exit=0")
        || !completed
            .lines()
            .any(|s| s.strip_prefix("firmware_sha256=") == Some(expected_hash))
    {
        return Err(format!("{label}: missing exit evidence or wrong image").into());
    }
    let rtt = fs::read_to_string(grade::artifact(&prefix, ".rtt"))?;
    let host = fs::read_to_string(grade::artifact(&prefix, ".host.log"))?;
    if !rtt.contains("Startup complete, transitioning away from splash screen")
        || !rtt.contains("settings saved")
        || ["panicked", "HardFault", "ERROR", "underrun error", "Dropped MIDI event"]
            .iter()
            .any(|s| rtt.contains(s))
        || !host.lines().any(|s| s.starts_with("PC: "))
    {
        return Err(format!("{label}: incomplete boot/save or firmware fault").into());
    }
    if grade::stat(&prefix, "underruns")? != 0 || grade::stat(&prefix, "overruns")? != 0 {
        return Err(format!("{label}: DMA stream fault").into());
    }
    let reader = hound::WavReader::open(grade::artifact(&prefix, ".wav"))?;
    let spec = reader.spec();
    if spec.channels != 2
        || spec.bits_per_sample != 32
        || spec.sample_format != hound::SampleFormat::Float
    {
        return Err("Expected stereo float PCM".into());
    }
    let clock = grade::artifact(&prefix, ".wav.clock.txt");
    let denominator = grade::counter(&clock, "sai_rate_denominator")?;
    if denominator == 0 {
        return Err("Invalid SAI clock".into());
    }
    let rate = grade::counter(&clock, "sai_rate_numerator")? as f64 / denominator as f64;
    let samples = reader
        .into_samples::<f32>()
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let frames = samples.len() / 2;
    if samples.len() % 2 != 0
        || frames as u64 != grade::stat(&prefix, "frames")?
        || rate.round() as u32 != spec.sample_rate
        || frames as f64 > rate * 4.0
        || (frames as f64) < rate * 3.95
    {
        return Err(format!("{label}: truncated/misclocked PCM").into());
    }
    let mut mono = Vec::with_capacity(frames);
    for pair in samples.chunks_exact(2) {
        if !pair[0].is_finite() || pair[0] != pair[1] || pair[0].abs() >= 0.79999 {
            return Err(format!("{label}: nonfinite, stereo mismatch or ceiling hit").into());
        }
        mono.push(pair[0]);
    }
    let window = mono[(rate * 0.6) as usize..(rate * 3.8) as usize].to_vec();
    if audio_lab::metrics(&window).rms < 0.001 {
        return Err("Missing vocal output".into());
    }
    Ok(window)
}

/// Relative PCM difference after at most 64 samples of constant alignment.
/// This handles a small capture-start offset, without time warping or gain changes.
fn difference(a: &[f32], b: &[f32]) -> (f64, i32) {
    let n = a.len().min(b.len()) - 128;
    let mut best = (f64::INFINITY, 0);
    for lag in -64_i32..=64 {
        let ai = (64 - lag.min(0)) as usize;
        let bi = (64 + lag.max(0)) as usize;
        let mut error = 0.0;
        let mut power = 0.0;
        for i in 0..n {
            let x = f64::from(a[ai + i]);
            let y = f64::from(b[bi + i]);
            error += (x - y).powi(2);
            power += (x * x + y * y) * 0.5;
        }
        let ratio = (error / power.max(1e-30)).sqrt();
        if ratio < best.0 {
            best = (ratio, lag);
        }
    }
    best
}

pub fn analyze(out: &Path) -> Result<()> {
    if out.join("result.txt").exists() {
        fs::remove_file(out.join("result.txt"))?;
    }
    let firmware = fs::read_to_string(out.join("firmware.txt"))?;
    let image_hash = |key: &str| -> Result<&str> {
        let value = firmware
            .lines()
            .find_map(|s| s.strip_prefix(key))
            .ok_or("Missing image hash")?;
        if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Invalid image hash".into());
        }
        Ok(value)
    };
    let before = image_hash("before_sha256=")?;
    let after = image_hash("after_sha256=")?;
    if before == after {
        return Err("A/B requires different images".into());
    }
    let mut audio = std::collections::BTreeMap::new();
    for (label, updated, _) in CASES {
        audio.insert(
            label,
            read_capture(out, label, if updated { after } else { before })?,
        );
    }
    let mut csv = fs::File::create(out.join("comparison.csv"))?;
    writeln!(
        csv,
        "first,second,relative_pcm_difference,best_constant_lag_samples"
    )?;
    let mut differences = Vec::new();
    for (a, b) in [
        ("before-slow", "before-fast"),
        ("after-slow", "after-fast"),
        ("after-slow", "after-medium"),
        ("after-medium", "after-fast"),
        ("before-fast", "after-fast"),
    ] {
        let (delta, lag) = difference(&audio[a], &audio[b]);
        writeln!(csv, "{a},{b},{delta},{lag}")?;
        differences.push(delta);
    }
    if differences[0] > 0.01 {
        return Err("Legacy reference captures disagree between input sequences".into());
    }
    if differences[1] < 0.05 || differences[2] < 0.01 || differences[3] < 0.01 {
        return Err("Updated firmware speed settings do not change vocal PCM sufficiently".into());
    }
    fs::write(out.join("result.txt"), "PASS: five actual ARM firmware real-vocal captures; legacy reference captures match (legacy firmware ignores CC46), updated slow/medium/fast settings produce distinct PCM; saved controls, no ceiling hits, finite matching stereo, correct SAI-clock lengths and zero DMA underruns/overruns.\nPCM differences prove control response, not intended-note accuracy or musical quality. Physical board equivalence remains unverified.\n")?;
    println!("{}", fs::read_to_string(out.join("comparison.csv"))?);
    Ok(())
}
