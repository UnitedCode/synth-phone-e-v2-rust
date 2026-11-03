//! Held-note controls: isolate vocal DSP from an external song arrangement.
//! Audio and generated diagnostic MIDI are written outside the repository.
use audio_lab::{constants::*, estimate_pitch, metrics, replay::Replay, vocal_frame};
use std::{error::Error, fs, io::Write, path::Path};
use synthphone_e_vocal_dsp::{dsp::normalize_sample, MusicalSettings, ProcessingMode};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn writer(path: &Path) -> Result<hound::WavWriter<std::io::BufWriter<fs::File>>> {
    Ok(hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE as u32,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )?)
}

fn held_midi(seconds: f64, keys: &[u8]) -> Result<Vec<u8>> {
    use midly::{
        Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind,
    };
    let mut track = vec![TrackEvent {
        delta: 0.into(),
        kind: TrackEventKind::Meta(MetaMessage::Tempo(500_000.into())),
    }];
    for &key in keys {
        track.push(TrackEvent {
            delta: 0.into(),
            kind: TrackEventKind::Midi {
                channel: 0.into(),
                message: MidiMessage::NoteOn {
                    key: key.into(),
                    vel: 100.into(),
                },
            },
        });
    }
    let mut delta = (seconds * 960.0).round() as u32;
    for &key in keys {
        track.push(TrackEvent {
            delta: delta.into(),
            kind: TrackEventKind::Midi {
                channel: 0.into(),
                message: MidiMessage::NoteOff {
                    key: key.into(),
                    vel: 0.into(),
                },
            },
        });
        delta = 0;
    }
    track.push(TrackEvent {
        delta: 0.into(),
        kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
    });
    let smf = Smf {
        header: Header::new(Format::SingleTrack, Timing::Metrical(480.into())),
        tracks: vec![track],
    };
    let mut bytes = Vec::new();
    smf.write(&mut bytes)
        .map_err(|e| format!("MIDI write failed: {e:?}"))?;
    Ok(bytes)
}

fn hz(key: u8) -> f32 {
    440.0 * 2_f32.powf((key as f32 - 69.0) / 12.0)
}

fn render(source: &[f32], out: &Path, seconds: f64, polyphony: bool) -> Result<()> {
    let count = (seconds * SAMPLE_RATE as f64) as usize;
    let sample = |i: usize| {
        let j = i % source.len();
        let fade = (j.min(source.len() - 1 - j) as f32 / (SAMPLE_RATE * 0.01)).min(1.0);
        source[j] * fade * 0.35
    };
    let mut input = writer(&out.join("input.wav"))?;
    for i in 0..count {
        input.write_sample(sample(i))?;
    }
    input.finalize()?;
    let mut report = fs::File::create(out.join("levels.csv"))?;
    writeln!(report, "case,seconds,rms,peak,ceiling_samples,nonfinite")?;
    let baseline = [
        ("dry", &[][..]),
        ("unison-g3", &[55][..]),
        ("one-a3", &[57][..]),
        ("two-a3-e4", &[57, 64][..]),
    ];
    let chords = [
        ("two-a3-e4", &[57, 64][..]),
        ("four-am7", &[57, 60, 64, 67][..]),
        ("eight-am7", &[48, 52, 55, 57, 60, 64, 67, 69][..]),
    ];
    for (name, keys) in if polyphony {
        &chords[..]
    } else {
        &baseline[..]
    } {
        let mut settings = MusicalSettings {
            mode: if keys.is_empty() {
                ProcessingMode::Dry
            } else {
                ProcessingMode::Harmony
            },
            ..Default::default()
        };
        for (target, &key) in settings.midi_frequencies.iter_mut().zip(*keys) {
            *target = hz(key);
        }
        // Keep exactly the production/default configuration used in the earlier
        // full arrangement. The synthetic probes below match the clocks instead.
        let mut config = vocal_frame::default_config();
        config.harmony_lead_gain = 0.0;
        let mut replay = Replay::new();
        let mut file = writer(&out.join(format!("{name}.wav")))?;
        let (mut energy, mut peak, mut ceiling, mut nonfinite) =
            (0.0_f64, 0.0_f32, 0_usize, 0_usize);
        for block in (0..count).step_by(BLOCK_SIZE) {
            for i in block..(block + BLOCK_SIZE).min(count) {
                let y = normalize_sample(replay.sample(sample(i)), 0.8);
                energy += (y as f64).powi(2);
                peak = peak.max(y.abs());
                ceiling += usize::from(y.abs() >= 0.79999);
                nonfinite += usize::from(!y.is_finite());
                file.write_sample(y)?;
            }
            replay.finish_block(&[0.0; HOP_SIZE], &config, &settings, None);
        }
        file.finalize()?;
        if !keys.is_empty() {
            fs::write(out.join(format!("{name}.mid")), held_midi(seconds, keys)?)?;
        }
        writeln!(
            report,
            "{name},{seconds},{},{peak},{ceiling},{nonfinite}",
            (energy / count as f64).sqrt()
        )?;
        eprintln!("rendered {name}: {seconds} seconds");
    }
    // Record what the production pitch detector sees in this actual sample,
    // rather than assuming that a short real vowel is a stationary oscillator.
    let config = vocal_frame::default_config();
    let mut track = fs::File::create(out.join("input-pitch.csv"))?;
    writeln!(
        track,
        "seconds,production_yin_hz,independent_autocorrelation_hz,rms"
    )?;
    for start in (0..source.len()).step_by(HOP_SIZE) {
        let x: Vec<_> = (start..start + 8192).map(sample).collect();
        let yin = synthphone_e_vocal_dsp::dsp::frequency_analysis::find_pitch_yin(
            &x[..FFT_SIZE],
            &config,
        );
        writeln!(
            track,
            "{},{yin},{},{}",
            start as f64 / SAMPLE_RATE as f64,
            estimate_pitch(&x)
                .map(|p| p.to_string())
                .unwrap_or_default(),
            metrics(&x[..FFT_SIZE]).rms
        )?;
    }
    Ok(())
}

// A steady target should contain only harmonics of that target. This is a
// diagnostic for synthetic periodic input, not a perceptual score for vocals.
fn outside_harmonics(samples: &[f32], targets: &[f32]) -> f64 {
    let energy = samples.iter().map(|&s| (s as f64).powi(2)).sum::<f64>();
    let mut frequencies = Vec::new();
    for &target in targets {
        for harmonic in 1..=24 {
            let f = target as f64 * harmonic as f64;
            if !frequencies.iter().any(|&old: &f64| (old - f).abs() < 0.1) {
                frequencies.push(f);
            }
        }
    }
    let mut captured = 0.0;
    for f in frequencies {
        let (mut re, mut im) = (0.0, 0.0);
        for (i, &s) in samples.iter().enumerate() {
            let phase = std::f64::consts::TAU * f * i as f64 / SAMPLE_RATE as f64;
            re += s as f64 * phase.cos();
            im += s as f64 * phase.sin();
        }
        captured += 2.0 * (re * re + im * im) / samples.len() as f64;
    }
    (1.0 - captured / energy).clamp(0.0, 1.0)
}

fn probes(out: &Path) -> Result<()> {
    let mut csv = fs::File::create(out.join("synthetic-probes.csv"))?;
    writeln!(
        csv,
        "source_hz,shape,target_hz,outside_target_harmonics_fraction,estimated_hz,rms"
    )?;
    for source in [90.0, 130.0, 200.0, 300.0] {
        for shape in ["sine", "harmonic", "vibrato", "changing-formants"] {
            let mut phase = 0.0;
            let input: Vec<_> = (0..(SAMPLE_RATE * 2.0) as usize)
                .map(|i| {
                    let t = i as f64 / SAMPLE_RATE as f64;
                    let frequency = source
                        * if matches!(shape, "vibrato" | "changing-formants") {
                            2_f64.powf(50.0 * (std::f64::consts::TAU * 5.0 * t).sin() / 1200.0)
                        } else {
                            1.0
                        };
                    phase += std::f64::consts::TAU * frequency / SAMPLE_RATE as f64;
                    let harmonics = if shape == "sine" { 1 } else { 10 };
                    (1..=harmonics)
                        .map(|h| {
                            let amplitude = if shape == "changing-formants" {
                                0.035 * (1.0 + 0.8 * (std::f64::consts::TAU * t + h as f64).sin())
                            } else {
                                0.15 / h as f64
                            };
                            amplitude * (phase * h as f64).sin()
                        })
                        .sum::<f64>() as f32
                })
                .collect();
            for target in [source as f32, 220.0, 329.62756] {
                let mut config = vocal_frame::default_config();
                config.sample_rate = SAMPLE_RATE;
                config.harmony_lead_gain = 0.0;
                let mut settings = MusicalSettings {
                    mode: ProcessingMode::Harmony,
                    ..Default::default()
                };
                settings.midi_frequencies[0] = target;
                let result = audio_lab::render(&input, &[], &config, &settings);
                let samples =
                    &result.audio[(SAMPLE_RATE * 0.4) as usize..(SAMPLE_RATE * 1.8) as usize];
                let residual = outside_harmonics(samples, &[target]);
                writeln!(
                    csv,
                    "{source},{shape},{target},{residual},{},{}",
                    estimate_pitch(samples)
                        .map(|p| p.to_string())
                        .unwrap_or_default(),
                    metrics(samples).rms
                )?;
                if source == 200.0 && shape == "changing-formants" && target == 220.0 {
                    let mut w = writer(&out.join("synthetic-moving-formants-input.wav"))?;
                    for &s in &input {
                        w.write_sample(s)?;
                    }
                    w.finalize()?;
                    let mut w = writer(&out.join("synthetic-moving-formants-output.wav"))?;
                    for &s in &result.audio {
                        w.write_sample(s)?;
                    }
                    w.finalize()?;
                }
            }
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let polyphony = args.get(3).is_some_and(|flag| flag == "--polyphony");
    if args.len() != 3 && !(args.len() == 4 && polyphony) {
        return Err(
            "usage: harmony_isolation VOWEL.wav NEW_OUTPUT_DIRECTORY SECONDS [--polyphony]".into(),
        );
    }
    let (source, directory, seconds) = (&args[0], &args[1], &args[2]);
    let seconds: f64 = seconds.parse()?;
    if !seconds.is_finite() || !(1.0..=600.0).contains(&seconds) {
        return Err("duration must be 1..600 seconds".into());
    }
    let reader = hound::WavReader::open(source)?;
    let spec = reader.spec();
    if spec.channels != 1
        || spec.sample_rate != SAMPLE_RATE as u32
        || spec.sample_format != hound::SampleFormat::Float
    {
        return Err("need mono 48014 Hz float WAV".into());
    }
    let source = reader
        .into_samples::<f32>()
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if source.len() < FFT_SIZE || source.iter().any(|s| !s.is_finite()) {
        return Err("invalid vowel".into());
    }
    let out = Path::new(directory);
    fs::create_dir(out)?; // Refuse to replace any prior evidence.
    if !polyphony {
        probes(out)?;
    }
    render(&source, out, seconds, polyphony)
}

#[cfg(test)]
mod tests {
    use super::*;
    use midly::{MidiMessage, Smf, TrackEventKind};

    #[test]
    fn diagnostic_midi_contains_only_requested_notes_held_for_full_duration() {
        for keys in [&[57][..], &[57, 64][..]] {
            let bytes = held_midi(288.0, keys).unwrap();
            let smf = Smf::parse(&bytes).unwrap();
            let mut tick = 0_u64;
            let (mut on, mut off) = (Vec::new(), Vec::new());
            for event in &smf.tracks[0] {
                tick += u64::from(event.delta.as_int());
                if let TrackEventKind::Midi { channel, message } = event.kind {
                    assert_eq!(channel.as_int(), 0);
                    match message {
                        MidiMessage::NoteOn { key, vel } => {
                            assert_eq!(tick, 0);
                            assert_eq!(vel.as_int(), 100);
                            on.push(key.as_int());
                        }
                        MidiMessage::NoteOff { key, .. } => {
                            assert_eq!(tick, 288 * 960);
                            off.push(key.as_int());
                        }
                        other => panic!("unexpected MIDI event: {other:?}"),
                    }
                }
            }
            assert_eq!(on, keys);
            assert_eq!(off, keys);
        }
    }
}
