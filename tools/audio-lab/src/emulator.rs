//! Build a real no_std ARM guest, execute it in QEMU, and grade returned samples.
use crate::{constants::*, estimate_pitch, metrics, render, steady, vocal_frame};
use std::{
    error::Error,
    fs::{self, File},
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[path = "../../arm-audio-tests/src/cases.rs"]
pub mod cases;

type Result<T> = std::result::Result<T, Box<dyn Error>>;
pub const MAX_SAMPLE_ERROR: f64 = 1e-4;

pub struct Capture {
    pub audio: Vec<Vec<f32>>,
    pub interrupt_count: u32,
}

fn word(cursor: &mut Cursor<&[u8]>) -> Result<u32> {
    let mut bytes = [0; 4];
    cursor.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

/// Reject incomplete or malformed captures instead of treating a crashed guest as silence.
pub fn decode(bytes: &[u8]) -> Result<Capture> {
    let mut cursor = Cursor::new(bytes);
    let mut magic = [0; 8];
    cursor.read_exact(&mut magic)?;
    if &magic != cases::MAGIC || word(&mut cursor)? != cases::COUNT as u32 {
        return Err("Invalid ARM audio capture header".into());
    }
    let total = (cases::INPUT_SAMPLES + FFT_SIZE * 2).div_ceil(BLOCK_SIZE) * BLOCK_SIZE;
    let mut audio = Vec::new();
    for case in 0..cases::COUNT {
        if word(&mut cursor)? != case as u32 || word(&mut cursor)? != total as u32 {
            return Err("Invalid ARM case ID/sample count".into());
        }
        let mut samples = Vec::with_capacity(total);
        for _ in 0..total {
            let sample = f32::from_bits(word(&mut cursor)?);
            if !sample.is_finite() {
                return Err("Non-finite sample from ARM guest".into());
            }
            samples.push(sample);
        }
        audio.push(samples);
    }
    let interrupt_count = word(&mut cursor)?;
    if interrupt_count == 0 || cursor.position() != bytes.len() as u64 {
        return Err("Missing interrupt evidence or trailing ARM capture data".into());
    }
    Ok(Capture {
        audio,
        interrupt_count,
    })
}

pub fn sample_error(actual: &[f32], expected: &[f32]) -> Result<(f64, f64)> {
    if actual.len() != expected.len() || actual.is_empty() {
        return Err("Audio length mismatch".into());
    }
    let mut peak: f64 = 0.0;
    let mut squares = 0.0;
    for (&a, &b) in actual.iter().zip(expected) {
        if !a.is_finite() || !b.is_finite() {
            return Err("Non-finite comparison sample".into());
        }
        let difference = (a as f64 - b as f64).abs();
        peak = peak.max(difference);
        squares += difference * difference;
    }
    Ok((peak, (squares / actual.len() as f64).sqrt()))
}

fn build_guest(root: &Path, omit_frame: bool) -> Result<PathBuf> {
    let manifest = root.join("tools/arm-audio-tests/Cargo.toml");
    let target = root.join("tools/arm-audio-tests/target");
    let mut command = Command::new("cargo");
    command
        .current_dir(root)
        .args([
            "build",
            "--release",
            "--locked",
            "--target",
            "thumbv7em-none-eabihf",
            "--manifest-path",
        ])
        .arg(manifest)
        .env("CARGO_TARGET_DIR", &target)
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("RUSTFLAGS", "-C link-arg=-Tlink.x -C target-cpu=cortex-m7");
    if omit_frame {
        command.args(["--features", "omit-frame"]);
    }
    if !command.status()?.success() {
        return Err("ARM guest build failed".into());
    }
    Ok(target.join("thumbv7em-none-eabihf/release/arm-audio-tests"))
}

fn execute(qemu: &std::ffi::OsStr, elf: &Path, directory: &Path, label: &str) -> Result<Capture> {
    let output = directory.join(format!("{label}.bin"));
    let log = directory.join(format!("{label}.log"));
    let mut child = Command::new(qemu)
        .args([
            "-machine",
            "mps2-an500",
            "-cpu",
            "cortex-m7",
            "-display",
            "none",
            "-monitor",
            "none",
            "-serial",
            "none",
            "-semihosting-config",
            "enable=on,target=native",
            "-kernel",
        ])
        .arg(elf)
        .stdin(Stdio::null())
        .stdout(File::create(&output)?)
        .stderr(File::create(&log)?)
        .spawn()
        .map_err(|e| {
            format!("Cannot start QEMU: {e}. Install qemu-system-arm or set AUDIO_LAB_QEMU.")
        })?;
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() > Duration::from_secs(30) {
            child.kill()?;
            child.wait()?;
            return Err(format!("ARM guest timed out; see {}", log.display()).into());
        }
        thread::sleep(Duration::from_millis(20));
    };
    if !status.success() {
        return Err(format!("ARM guest failed ({status}); see {}", log.display()).into());
    }
    decode(&fs::read(output)?)
}

fn save_wav(path: &Path, samples: &[f32]) -> Result<()> {
    let mut writer = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE.round() as u32,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )?;
    for &sample in samples {
        writer.write_sample(sample)?;
    }
    writer.finalize()?;
    Ok(())
}

pub fn run(directory: &Path) -> Result<()> {
    fs::create_dir_all(directory)?;
    let directory = fs::canonicalize(directory)?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let qemu = std::env::var_os("AUDIO_LAB_QEMU").unwrap_or_else(|| "qemu-system-arm".into());
    let version = Command::new(&qemu).arg("--version").output().map_err(|e| {
        format!("QEMU is unavailable: {e}. Install qemu-system-arm or set AUDIO_LAB_QEMU.")
    })?;
    if !version.status.success() {
        return Err("QEMU version check failed".into());
    }
    fs::write(directory.join("qemu-version.txt"), &version.stdout)?;
    println!("Building and executing no_std Cortex-M7 audio guest...");
    let elf = build_guest(&root, false)?;
    let captured = execute(&qemu, &elf, &directory, "arm-audio")?;
    let mut report = File::create(directory.join("arm-comparison.csv"))?;
    writeln!(
        report,
        "case,max_sample_error,rms_error,arm_pitch_hz,arm_rms,arm_silent_hops,host_match"
    )?;
    let mut references = Vec::new();
    let mut failures = 0;
    for (case, actual) in captured.audio.iter().enumerate() {
        let input: Vec<_> = (0..cases::INPUT_SAMPLES)
            .map(|i| cases::input(case, i))
            .collect();
        let carrier: Vec<_> = (0..cases::INPUT_SAMPLES)
            .map(|i| cases::carrier(case, i))
            .collect();
        let expected = render(
            &input,
            &carrier,
            &vocal_frame::default_config(),
            &cases::settings(case),
        )
        .audio;
        let (peak, rms) = sample_error(actual, &expected)?;
        let pass = peak <= MAX_SAMPLE_ERROR;
        failures += usize::from(!pass);
        let pitch = estimate_pitch(steady(actual));
        let m = metrics(steady(actual));
        writeln!(
            report,
            "{},{peak:.9},{rms:.9},{},{:.6},{},{pass}",
            cases::NAMES[case],
            pitch.map(|p| format!("{p:.6}")).unwrap_or_default(),
            m.rms,
            m.silent_hops
        )?;
        save_wav(
            &directory.join(format!("{}-arm.wav", cases::NAMES[case])),
            actual,
        )?;
        references.push(expected);
    }
    if failures != 0 {
        return Err(format!("{failures} ARM/host mismatches; see arm-comparison.csv").into());
    }

    println!("All nine ARM cases match. Checking an injected lost frame...");
    let broken_elf = build_guest(&root, true)?;
    let broken = execute(&qemu, &broken_elf, &directory, "arm-lost-frame")?;
    let (fault_error, _) = sample_error(&broken.audio[4], &references[4])?;
    if fault_error <= MAX_SAMPLE_ERROR {
        return Err("ARM lost-frame negative control was not detected".into());
    }
    let summary = format!("9/9 Cortex-M7 audio cases match host output within {MAX_SAMPLE_ERROR} peak sample error.\n\
        SysTick interrupts occurred during the ARM workload ({} observed).\n\
        Injected lost frame was detected (peak sample error {fault_error:.6}).\n\
        ARM target: thumbv7em-none-eabihf; QEMU board: mps2-an500; CPU: cortex-m7.\n\
        This executes the DSP/replay test guest, not the complete Daisy firmware.\n\
        STM32 SAI/DMA, codec, cache timing and real-time deadline accuracy are not modeled.\n\
        Matching host output does not fix or approve the known pitch-quality failures.\n", captured.interrupt_count);
    fs::write(directory.join("emulator-summary.txt"), &summary)?;
    print!("{summary}");
    Ok(())
}
