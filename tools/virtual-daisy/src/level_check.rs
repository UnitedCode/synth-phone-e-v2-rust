//! Hot/quiet real-vocal regression through production ELFs and physical inputs.
use super::{bounded_with_limit, grade, Result, RATE};
use sha2::{Digest, Sha256};
use std::{env, fs, io::Write, path::Path, process::Command};

fn controls(voice: u8, synth: u8) -> String {
    // The firmware maps floor(CC * 20 / 127); choose the first exact CC for a level.
    let voice = (u16::from(voice) * 127).div_ceil(20);
    let synth = (u16::from(synth) * 127).div_ceil(20);
    format!("5,uart,usart1,176\n6,uart,usart1,44\n7,uart,usart1,{voice}\n8,uart,usart1,176\n9,uart,usart1,45\n10,uart,usart1,{synth}\n")
}

fn tuned_setup() -> &'static str {
    "13,link,gpioPortC,1,gpioPortA,7\n17,unlink,gpioPortC,1,gpioPortA,7\n21,link,gpioPortC,1,gpioPortA,7\n25,unlink,gpioPortC,1,gpioPortA,7\n29,link,gpioPortC,1,gpioPortA,7\n33,unlink,gpioPortC,1,gpioPortA,7\n37,gpio,gpioPortC,10,0\n45,gpio,gpioPortC,10,1\n"
}

fn tuned_note() -> String {
    tuned_setup().to_owned() + "52,link,gpioPortC,4,gpioPortA,3\n"
}

fn demo() -> String {
    // Same vocal continues throughout: voice alone, voice+keypad, voice+MIDI.
    tuned_setup().to_owned()
        + concat!(
            "100,link,gpioPortC,4,gpioPortA,3\n165,unlink,gpioPortC,4,gpioPortA,3\n",
            "200,uart,usart1,145\n201,uart,usart1,45\n202,uart,usart1,127\n",
            "275,uart,usart1,129\n276,uart,usart1,45\n277,uart,usart1,0\n"
        )
}
fn notes(keypad: bool) -> &'static str {
    if keypad {
        // Two mode changes, enter Processing, hold key6 (A2).
        "13,link,gpioPortC,1,gpioPortA,7\n17,unlink,gpioPortC,1,gpioPortA,7\n21,link,gpioPortC,1,gpioPortA,7\n25,unlink,gpioPortC,1,gpioPortA,7\n29,gpio,gpioPortC,10,0\n37,gpio,gpioPortC,10,1\n44,link,gpioPortC,4,gpioPortA,3\n"
    } else {
        "30,uart,usart1,145\n31,uart,usart1,45\n32,uart,usart1,127\n"
    }
}

/// Uses an already prepared rev5 serial-PCM emulator; never changes guest RAM,
/// instructions or settings storage. All changed settings arrive over UART.
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
    for path in [runtime, prepared, before, after, input] {
        if !path.exists() {
            return Err(format!("Missing {}", path.display()).into());
        }
    }
    let conformance = fs::read_to_string(prepared.join("conformance.txt"))?;
    if !conformance.lines().any(|x| x == "board=rev5")
        || !conformance
            .lines()
            .any(|x| x == "audio_input_boundary=serial")
    {
        return Err(
            "Prepare rev5 with serial PCM first; this fixture explicitly tests that boundary"
                .into(),
        );
    }
    // Reuse the normal WAV decoder and its strict sample-rate/range checks.
    fs::create_dir_all(out)?;
    for name in ["result.txt", "levels.csv"] {
        let path = out.join(name);
        if path.exists() {
            fs::remove_file(path)?;
        }
    }
    super::wav_input_limit(
        input,
        &out.join("source.f32le"),
        RATE as usize * 3 + RATE as usize / 10,
    )?;
    let source = fs::read(out.join("source.f32le"))?;
    let samples: Vec<f32> = source
        .chunks_exact(8)
        .map(|x| f32::from_le_bytes(x[..4].try_into().unwrap()))
        .collect();
    if samples.len() < RATE as usize * 3 {
        return Err("Volume demos need at least three seconds of vocals".into());
    }
    let peak = samples.iter().fold(0.0_f32, |p, x| p.max(x.abs()));
    if peak < 0.001 {
        return Err("Need an audible vocal recording".into());
    }
    let mut fixture_report = fs::File::create(out.join("preamp.csv"))?;
    writeln!(
        fixture_report,
        "fixture,source_peak,linear_gain,pre_adc_peak,clipped_frames,total_frames"
    )?;
    for (name, target) in [
        ("hot", 0.95_f32),
        ("quiet", 0.05_f32),
        ("overload", 2.0_f32),
        ("silence", 0.0_f32),
    ] {
        let gain = target / peak;
        let mut raw = fs::File::create(out.join(format!("{name}.f32le")))?;
        let mut wav = hound::WavWriter::create(
            out.join(format!("{name}-adc.wav")),
            hound::WavSpec {
                channels: 1,
                sample_rate: RATE,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )?;
        let mut clipped = 0;
        for &sample in &samples {
            let amplified = sample * gain;
            clipped += usize::from(amplified.abs() > 1.0);
            // A specified overload fixture, not a measured analog transfer curve.
            let adc = amplified.clamp(-1.0, 1.0 - 1.0 / 8_388_608.0);
            wav.write_sample(adc)?;
            raw.write_all(&adc.to_le_bytes())?;
            raw.write_all(&adc.to_le_bytes())?;
        }
        wav.finalize()?;
        writeln!(
            fixture_report,
            "{name},{peak},{gain},{target},{clipped},{}",
            samples.len()
        )?;
    }
    let before_hash = format!("{:x}", Sha256::digest(fs::read(before)?));
    let after_hash = format!("{:x}", Sha256::digest(fs::read(after)?));
    if before_hash == after_hash {
        return Err("A/B requires different firmware binaries".into());
    }
    fs::write(out.join("firmware.txt"),format!("before_sha256={before_hash}\nafter_sha256={after_hash}\nboard=rev5\ncodec=WM8731\naudio_boundary=serial\nmeasurement_window_seconds=0.65..0.95\npreamp_model=linear gain plus explicit hard saturation before serial ADC boundary; analog transfer curve unverified\n"))?;
    let cases = [
        ("quiet-voice", after, "quiet", controls(20, 10)),
        (
            "quiet-midi-low",
            after,
            "quiet",
            controls(0, 1) + notes(false),
        ),
        (
            "quiet-midi-high",
            after,
            "quiet",
            controls(0, 20) + notes(false),
        ),
        ("before-voice", before, "hot", String::new()),
        ("before-keypad", before, "silence", notes(true).into()),
        ("before-midi", before, "silence", notes(false).into()),
        ("after-voice-hot", after, "hot", controls(10, 10)),
        ("after-voice-pad", after, "hot", controls(5, 10)),
        (
            "after-midi-low",
            after,
            "hot",
            controls(0, 1) + notes(false),
        ),
        (
            "after-midi-high",
            after,
            "hot",
            controls(0, 20) + notes(false),
        ),
        (
            "after-keypad-low",
            after,
            "hot",
            controls(0, 1) + notes(true),
        ),
        (
            "after-keypad-high",
            after,
            "hot",
            controls(0, 20) + notes(true),
        ),
        (
            "quiet-keypad-low",
            after,
            "quiet",
            controls(0, 1) + notes(true),
        ),
        (
            "quiet-keypad-high",
            after,
            "quiet",
            controls(0, 20) + notes(true),
        ),
        (
            "tuned-keypad-low",
            after,
            "hot",
            controls(0, 1) + &tuned_note(),
        ),
        (
            "tuned-keypad-high",
            after,
            "hot",
            controls(0, 20) + &tuned_note(),
        ),
        (
            "after-overload-pad",
            after,
            "overload",
            controls(5, 10) + notes(false),
        ),
        ("before-hot-demo", before, "hot", demo()),
        ("after-hot-demo", after, "hot", controls(5, 7) + &demo()),
        ("before-quiet-demo", before, "quiet", demo()),
        (
            "after-quiet-demo",
            after,
            "quiet",
            controls(20, 3) + &demo(),
        ),
    ];
    let dotnet = env::var_os("VIRTUAL_DAISY_DOTNET").unwrap_or_else(|| "dotnet".into());
    let mut manifest = fs::File::create(out.join("capture-plan.csv"))?;
    writeln!(
        manifest,
        "case,firmware_sha256,input_fixture,virtual_seconds"
    )?;
    for (label, elf, fixture, _) in &cases {
        writeln!(
            manifest,
            "{label},{},{fixture},{}",
            if *elf == before {
                &before_hash
            } else {
                &after_hash
            },
            if label.ends_with("-demo") { 3 } else { 1 }
        )?;
    }
    // Bounded parallelism keeps the matrix local and avoids CI or extra caches.
    for chunk in cases.chunks(3) {
        let results = std::thread::scope(|scope| {
            let mut workers = Vec::new();
            for (label, elf, fixture, stimulus) in chunk {
                let dotnet = &dotnet;
                workers.push(scope.spawn(move || -> std::result::Result<(), String> {
                    let execute = || -> Result<()> {
                        let prefix = out.join(label);
                        let image_hash = format!("{:x}", Sha256::digest(fs::read(elf)?));
                        let script = out.join(format!("{label}.stimulus"));
                        fs::write(&script, stimulus)?;
                        // Remove stale capture evidence before launching.
                        for suffix in [".wav", ".wav.stats", ".rtt", ".host.log", ".completed.txt"]
                        {
                            let artifact = grade::artifact(&prefix, suffix);
                            if artifact.exists() {
                                fs::remove_file(artifact)?;
                            }
                        }
                        bounded_with_limit(
                            Command::new(dotnet)
                                .arg(prepared.join("managed/DaisyHost.dll"))
                                .arg(runtime)
                                .arg(prepared.join("daisy.repl"))
                                .arg(elf)
                                .arg(&prefix)
                                .arg(out.join(format!("{fixture}.f32le")))
                                .args(["none", "rev5", "1", "serial", "typical"])
                                .arg(&script)
                                .env(
                                    "VIRTUAL_DAISY_INFRASTRUCTURE",
                                    prepared.join("managed/Infrastructure.dll"),
                                )
                                .env(
                                    "VIRTUAL_DAISY_AUDIO_STEPS",
                                    if label.ends_with("-demo") {
                                        "300"
                                    } else {
                                        "100"
                                    },
                                ),
                            &grade::artifact(&prefix, ".host.log"),
                            std::time::Duration::from_secs(600),
                        )?;
                        grade::execution_evidence(&prefix)?;
                        let rtt = fs::read_to_string(grade::artifact(&prefix, ".rtt"))?;
                        if !rtt.contains("Startup complete, transitioning away from splash screen")
                            || ["panicked", "HardFault", "ERROR"]
                                .iter()
                                .any(|bad| rtt.contains(bad))
                        {
                            return Err(format!("{label}: startup/fault evidence failed").into());
                        }
                        if grade::stat(&prefix, "underruns")? != 0
                            || grade::stat(&prefix, "overruns")? != 0
                            || grade::stat(&prefix, "frames")? < 47000
                        {
                            return Err(format!("{label}: DMA/audio stream failed").into());
                        }
                        if format!("{:x}", Sha256::digest(fs::read(elf)?)) != image_hash {
                            return Err("Firmware changed during capture".into());
                        }
                        fs::write(
                            grade::artifact(&prefix, ".completed.txt"),
                            format!("exit=0\nfirmware_sha256={image_hash}\n"),
                        )?;
                        println!("Completed firmware capture: {label}");
                        Ok(())
                    };
                    execute().map_err(|e| e.to_string())
                }));
            }
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .collect::<Vec<_>>()
        });
        for result in results {
            result?;
        }
        if chunk[0].0 == "quiet-voice" {
            let voice = capture_rms(out, "quiet-voice")?;
            let note = capture_rms(out, "quiet-midi-low")?;
            fs::write(
                out.join("quiet-floor.txt"),
                format!(
                    "voice_rms={voice}\nminimum_note_rms={note}\nminimum_note_db_vs_voice={}\n",
                    audio_lab::levels::db_ratio(note, voice)
                ),
            )?;
            if note >= voice {
                return Err("Quiet-mic minimum synth level cannot go below the voice".into());
            }
        }
    }
    if format!("{:x}", Sha256::digest(fs::read(before)?)) != before_hash
        || format!("{:x}", Sha256::digest(fs::read(after)?)) != after_hash
    {
        return Err("Firmware changed during the A/B capture".into());
    }
    analyze(out)
}

fn capture_rms(out: &Path, label: &str) -> Result<f64> {
    let reader = hound::WavReader::open(out.join(format!("{label}.wav")))?;
    let rate = f64::from(reader.spec().sample_rate);
    let samples = reader
        .into_samples::<f32>()
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mono: Vec<_> = samples.chunks_exact(2).map(|x| x[0]).collect();
    Ok(audio_lab::metrics(&mono[(rate * 0.65) as usize..(rate * 0.95) as usize]).rms)
}

/// Recheck saved PCM and boot/stream evidence without rerunning the emulator.
/// Successful process exits are recorded by `run`, along with the image hashes.
pub fn analyze(out: &Path) -> Result<()> {
    let result = out.join("result.txt");
    if result.exists() {
        fs::remove_file(result)?;
    }
    let firmware = fs::read_to_string(out.join("firmware.txt"))?;
    let image_hash = |key: &str| -> Result<&str> {
        let hash = firmware
            .lines()
            .find_map(|line| line.strip_prefix(key))
            .ok_or("Missing firmware image hash")?;
        if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("Invalid firmware image hash".into());
        }
        Ok(hash)
    };
    let before_hash = image_hash("before_sha256=")?;
    let after_hash = image_hash("after_sha256=")?;
    if before_hash == after_hash {
        return Err("A/B requires different firmware binaries".into());
    }
    let mut report = fs::File::create(out.join("levels.csv"))?;
    writeln!(
        report,
        "case,window_rms,window_peak,full_peak,full_samples_at_output_ceiling,non_finite"
    )?;
    let mut rms = std::collections::BTreeMap::new();
    for label in [
        "quiet-voice",
        "quiet-midi-low",
        "quiet-midi-high",
        "before-voice",
        "before-keypad",
        "before-midi",
        "after-voice-hot",
        "after-voice-pad",
        "after-midi-low",
        "after-midi-high",
        "after-keypad-low",
        "after-keypad-high",
        "quiet-keypad-low",
        "quiet-keypad-high",
        "tuned-keypad-low",
        "tuned-keypad-high",
        "after-overload-pad",
        "before-hot-demo",
        "after-hot-demo",
        "before-quiet-demo",
        "after-quiet-demo",
    ] {
        let prefix = out.join(label);
        grade::execution_evidence(&prefix)?;
        let host = fs::read_to_string(grade::artifact(&prefix, ".host.log"))?;
        let rtt = fs::read_to_string(grade::artifact(&prefix, ".rtt"))?;
        let completed = fs::read_to_string(grade::artifact(&prefix, ".completed.txt"))?;
        if !completed.lines().any(|line| line == "exit=0") {
            return Err("Capture did not exit successfully".into());
        }
        let expected_hash = if label.starts_with("before-") {
            before_hash
        } else {
            after_hash
        };
        if !completed
            .lines()
            .any(|line| line.strip_prefix("firmware_sha256=") == Some(expected_hash))
        {
            return Err(format!("{label}: capture uses a different firmware image").into());
        }
        if !host.lines().any(|line| line.starts_with("PC: "))
            || !rtt.contains("Startup complete, transitioning away from splash screen")
            || ["panicked", "HardFault", "ERROR"]
                .iter()
                .any(|bad| rtt.contains(bad))
        {
            return Err(format!("{label}: incomplete boot/capture or firmware fault").into());
        }
        if label.starts_with("after-")
            && label.ends_with("-demo")
            && !rtt.contains("settings saved")
        {
            return Err(format!("{label}: source settings were not saved").into());
        }
        if grade::stat(&prefix, "underruns")? != 0
            || grade::stat(&prefix, "overruns")? != 0
            || grade::stat(&prefix, "frames")? < 47000
        {
            return Err(format!("{label}: stream counters failed").into());
        }
        let reader = hound::WavReader::open(out.join(format!("{label}.wav")))?;
        let spec = reader.spec();
        if spec.channels != 2
            || spec.sample_format != hound::SampleFormat::Float
            || spec.bits_per_sample != 32
        {
            return Err("Expected stereo float capture".into());
        }
        let mut mono = Vec::new();
        let samples = reader
            .into_samples::<f32>()
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let clock = grade::artifact(&prefix, ".wav.clock.txt");
        let denominator = grade::counter(&clock, "sai_rate_denominator")?;
        if denominator == 0 {
            return Err("Invalid SAI clock denominator".into());
        }
        let rate = grade::counter(&clock, "sai_rate_numerator")? as f64 / denominator as f64;
        let frames = grade::stat(&prefix, "frames")?;
        let seconds = if label.ends_with("-demo") { 3.0 } else { 1.0 };
        if rate.round() as u32 != spec.sample_rate
            || samples.len() % 2 != 0
            || samples.len() / 2 != frames as usize
            || (frames as f64) < rate * (seconds - 0.05)
            || (frames as f64) > rate * seconds
        {
            return Err(format!("{label}: WAV length/rate disagrees with SAI evidence").into());
        }
        for pair in samples.chunks_exact(2) {
            if !pair[0].is_finite() || pair[0] != pair[1] {
                return Err(format!("{label}: nonfinite/stereo mismatch").into());
            }
            mono.push(pair[0]);
        }
        let start = (rate * 0.65) as usize;
        let end = (rate * 0.95) as usize;
        if mono.len() < end {
            return Err("Truncated firmware capture".into());
        }
        let window = &mono[start..end];
        let m = audio_lab::metrics(window);
        let full = audio_lab::metrics(&mono);
        let ceiling = mono.iter().filter(|x| x.abs() >= 0.79999).count();
        writeln!(
            report,
            "{label},{},{},{},{ceiling},{}",
            m.rms, m.peak, full.peak, full.non_finite
        )?;
        if full.peak > 0.80001 || m.rms < 0.0001 {
            return Err(format!("{label}: missing signal or output above ceiling").into());
        }
        if label.starts_with("after-") && label.ends_with("-demo") && ceiling != 0 {
            return Err(format!("{label}: balanced demonstration hit the output ceiling").into());
        }
        rms.insert(label, m.rms);
    }
    let pad = audio_lab::levels::db_ratio(rms["after-voice-pad"], rms["after-voice-hot"]);
    // Input trim is linear (covered by the shared-source tone tests), but
    // pitch tracking, thresholds and frame history make real-vocal output
    // nonlinear. Require useful attenuation rather than an exact output ratio.
    if !pad.is_finite() || pad > -3.0 {
        return Err(format!("Voice trim did not attenuate independently: {pad}dB").into());
    }
    for source in ["keypad", "midi"] {
        let low = rms[format!("after-{source}-low").as_str()];
        let high = rms[format!("after-{source}-high").as_str()];
        if low >= rms["after-voice-pad"] || high <= rms["after-voice-pad"] {
            return Err(format!("{source}: settings do not straddle the hot vocal level").into());
        }
        if (audio_lab::levels::db_ratio(high, low) - 46.0206).abs() > 0.25 {
            return Err(format!("{source}: source settings range incorrect").into());
        }
    }
    for source in ["keypad", "midi"] {
        let low = rms[format!("quiet-{source}-low").as_str()];
        let high = rms[format!("quiet-{source}-high").as_str()];
        if low >= rms["quiet-voice"]
            || high <= rms["quiet-voice"]
            || (audio_lab::levels::db_ratio(high, low) - 46.0206).abs() > 0.25
        {
            return Err(format!("Quiet {source} range does not cover the voice").into());
        }
        if audio_lab::levels::db_ratio(low, rms[format!("after-{source}-low").as_str()]).abs()
            > 0.25
        {
            return Err(format!("{source}: microphone mute leaked between input regimes").into());
        }
    }
    for bound in ["low", "high"] {
        if audio_lab::levels::db_ratio(
            rms[format!("tuned-keypad-{bound}").as_str()],
            rms[format!("after-keypad-{bound}").as_str()],
        )
        .abs()
            > 0.25
        {
            return Err("Tuned keypad calibration differs from Dry".into());
        }
    }
    fs::write(out.join("result.txt"),format!("PASS: actual firmware captures with hot and quiet real vocals, hot-vocal trim {pad:.4}dB, Dry/Tuned keypad and MIDI span below/above voice, 46.02dB note range, independent microphone mute, balanced demos below output ceiling, saved source settings, finite matching stereo and zero DMA underruns/overruns.\nInput overload is explicit hard saturation, not a measured preamp transfer curve; clipped peaks cannot be recovered.\nHardware equivalence remains unverified.\n"))?;
    println!("{}", fs::read_to_string(out.join("levels.csv"))?);
    Ok(())
}
