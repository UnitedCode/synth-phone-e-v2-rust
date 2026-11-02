use std::{path::PathBuf, process::Command};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "audio-lab-wav-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        Self(directory)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn pcm_recording_renders_to_a_finite_float_wav() {
    let scratch = Scratch::new();
    let input = scratch.0.join("input.wav");
    let output = scratch.0.join("output.wav");
    let mut writer = hound::WavWriter::create(
        &input,
        hound::WavSpec {
            channels: 1,
            sample_rate: 48000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for sample in audio_lab::tone(220.0, 0.4, &[(1, 0.2)]) {
        writer.write_sample((sample * 32767.0) as i16).unwrap();
    }
    writer.finalize().unwrap();
    let command = Command::new(env!("CARGO_BIN_EXE_audio-lab"))
        .arg("render")
        .arg(&input)
        .arg(&output)
        .arg("dry")
        .output()
        .unwrap();
    assert!(
        command.status.success(),
        "{}",
        String::from_utf8_lossy(&command.stderr)
    );
    let mut wav = hound::WavReader::open(output).unwrap();
    assert_eq!(wav.spec().sample_rate, 48014);
    assert_eq!(wav.spec().sample_format, hound::SampleFormat::Float);
    let samples: Vec<f32> = wav.samples().collect::<Result<_, _>>().unwrap();
    let m = audio_lab::metrics(&samples);
    assert_eq!(m.non_finite, 0);
    assert!(m.rms > 0.01);
    assert!(m.peak <= 0.800001);
}

#[test]
fn stereo_input_is_rejected_without_creating_output() {
    let scratch = Scratch::new();
    let input = scratch.0.join("stereo.wav");
    let output = scratch.0.join("output.wav");
    hound::WavWriter::create(
        &input,
        hound::WavSpec {
            channels: 2,
            sample_rate: 48000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap()
    .finalize()
    .unwrap();
    let command = Command::new(env!("CARGO_BIN_EXE_audio-lab"))
        .arg("render")
        .arg(input)
        .arg(&output)
        .output()
        .unwrap();
    assert!(!command.status.success());
    assert!(String::from_utf8_lossy(&command.stderr).contains("mono"));
    assert!(!output.exists());
}

#[test]
fn recorded_voice_ab_keeps_voice_unchanged_when_only_synth_level_changes() {
    let scratch = Scratch::new();
    let input = scratch.0.join("recording.wav");
    let output = scratch.0.join("comparison");
    let mut wav = hound::WavWriter::create(
        &input,
        hound::WavSpec {
            channels: 1,
            sample_rate: 48014,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    for x in audio_lab::tone(224.492958, 0.45, &[(1, 0.2), (2, 0.05)]) {
        wav.write_sample(x).unwrap();
    }
    wav.finalize().unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_audio-lab"))
        .arg("levels-recording")
        .arg(&input)
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let samples = |name: &str| {
        hound::WavReader::open(output.join(name))
            .unwrap()
            .into_samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    assert_eq!(
        samples("before-autotune.wav"),
        samples("after-balanced-autotune.wav")
    );
    let rms = |version: &str, source: &str| {
        audio_lab::metrics(audio_lab::steady(&samples(&format!(
            "{version}-{source}.wav"
        ))))
        .rms
    };
    // Reproduce the user's settings attempt: half voice, maximum notes. The
    // older settings path scales keypad with voice, so its ratio cannot change.
    let ratio = |version: &str, source: &str| {
        audio_lab::levels::db_ratio(rms(version, source), rms(version, "autotune"))
    };
    assert!(
        (ratio("before-settings", "keypad") - ratio("before-settings-adjusted", "keypad")).abs()
            < 0.01
    );
    // With independent controls, the same attempt can move both note sources
    // from quieter than the voice to louder, without changing the input file.
    for source in ["keypad", "midi"] {
        assert!(ratio("before-settings", source) < -3.0);
        assert!(ratio("after-adjusted", source) > 6.0);
    }
    let before = audio_lab::metrics(&samples("before-midi.wav")).rms;
    let after = audio_lab::metrics(&samples("after-balanced-midi.wav")).rms;
    assert!(after > before * 1.5 && after < before * 3.0);
    assert!(output.join("before-comparison.wav").is_file());
    assert!(output.join("after-balanced-comparison.wav").is_file());
}
