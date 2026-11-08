use std::{path::PathBuf, process::Command};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "audio-lab-wav-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
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

fn crossing_pitch(samples: &[f32], rate: u32) -> f64 {
    let crossings: Vec<_> = samples
        .windows(2)
        .enumerate()
        .filter(|(_, s)| s[0] <= 0.0 && s[1] > 0.0)
        .map(|(i, s)| i as f64 + (-s[0] / (s[1] - s[0])) as f64)
        .collect();
    assert!(crossings.len() > 2);
    rate as f64 * (crossings.len() - 1) as f64 / (crossings.last().unwrap() - crossings[0])
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
    assert_eq!(wav.spec().sample_rate, 48000);
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
fn wav_clock_and_dry_pitch_are_preserved_at_common_sample_rates() {
    let scratch = Scratch::new();
    for rate in [44100, 48000, 48014, 96000] {
        let input = scratch.0.join(format!("input-{rate}.wav"));
        let output = scratch.0.join(format!("output-{rate}.wav"));
        let mut writer = hound::WavWriter::create(
            &input,
            hound::WavSpec {
                channels: 1,
                sample_rate: rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();
        for i in 0..rate {
            writer
                .write_sample(
                    (0.2 * (std::f64::consts::TAU * 440.0 * i as f64 / rate as f64).sin()) as f32,
                )
                .unwrap();
        }
        writer.finalize().unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_audio-lab"))
            .arg("render")
            .arg(&input)
            .arg(&output)
            .arg("dry")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{rate} Hz: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let wav = hound::WavReader::open(&output).unwrap();
        assert_eq!(
            wav.spec().sample_rate,
            rate,
            "the WAV clock must describe its actual samples"
        );
        let samples = wav
            .into_samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(samples.len() >= rate as usize && samples.len() <= rate as usize + 2050);
        let window = &samples[rate as usize / 3..rate as usize * 2 / 3];
        let frequency = crossing_pitch(window, rate);
        assert!(
            (frequency - 440.0).abs() < 0.01,
            "{rate} Hz file played at {frequency} Hz"
        );
    }
}

#[test]
fn synthesized_midi_notes_are_in_tune_when_played_from_wav_files() {
    use synthphone_e_vocal_dsp::audio::{Oscillator, Waveform};
    let scratch = Scratch::new();
    let path = scratch.0.join("note.wav");
    for rate in [44100, 48000, 48014, 96000] {
        for note in 0..=127 {
            let expected = 440.0 * 2_f64.powf((note as f64 - 69.0) / 12.0);
            let count = (rate as f64 * (6.0 / expected).max(0.25)).ceil() as usize;
            let mut osc = Oscillator::new(
                audio_lab::midi_notes::frequency(note),
                rate as f32,
                Waveform::Sine,
            );
            let samples: Vec<_> = (0..count).map(|_| osc.next_value() * 0.2).collect();
            audio_lab::wav::write(&path, &samples, rate).unwrap();
            let reader = hound::WavReader::open(&path).unwrap();
            assert_eq!(reader.spec().sample_rate, rate);
            let samples = reader
                .into_samples::<f32>()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            let measured = crossing_pitch(&samples, rate);
            let error = audio_lab::cents(measured, expected).abs();
            assert!(
                error < 0.5,
                "MIDI {note} at {rate}Hz: {measured}Hz, expected {expected}Hz ({error} cents)"
            );
        }
    }
}

#[test]
fn file_autotune_reaches_musical_targets_at_each_sample_rate() {
    let scratch = Scratch::new();
    let input = scratch.0.join("detuned.wav");
    let output = scratch.0.join("tuned.wav");
    for rate in [44100, 48000, 48014, 96000] {
        for (key, note, detune) in [(0, 69, 35.0), (0, 59, 35.0), (7, 60, -35.0), (18, 60, 35.0)] {
            let target = 440.0 * 2_f64.powf((note as f64 - 69.0) / 12.0);
            let frequency = target * 2_f64.powf(detune / 1200.0);
            let samples: Vec<_> = (0..rate)
                .map(|i| {
                    (0.2 * (std::f64::consts::TAU * frequency * i as f64 / rate as f64).sin())
                        as f32
                })
                .collect();
            audio_lab::wav::write(&input, &samples, rate).unwrap();
            let result = Command::new(env!("CARGO_BIN_EXE_audio-lab"))
                .arg("render")
                .arg(&input)
                .arg(&output)
                .arg("pitch")
                .arg(key.to_string())
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            let wav = hound::WavReader::open(&output).unwrap();
            assert_eq!(wav.spec().sample_rate, rate);
            let samples = wav
                .into_samples::<f32>()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            let measured = audio_lab::estimate_pitch_at_rate(
                &samples[rate as usize * 4 / 10..rate as usize * 9 / 10],
                rate as f64,
            )
            .unwrap();
            let error = audio_lab::cents(measured, target).abs();
            assert!(
                error < 10.0,
                "{rate}Hz key={key} MIDI={note}: {measured}Hz vs {target}Hz ({error} cents)"
            );
        }
    }
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
    let after = audio_lab::metrics(&samples("after-midi.wav")).rms;
    assert!(after > before * 1.5 && after < before * 3.0);
    assert!(output.join("before-comparison.wav").is_file());
    assert!(output.join("after-balanced-comparison.wav").is_file());
}
