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
