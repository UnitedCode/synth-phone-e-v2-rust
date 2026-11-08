//! Render a WAV at its declared clock and compare steady input/output windows.
use audio_lab::{
    cents, constants::FFT_SIZE, estimate_pitch_at_rate, metrics, render, tuning, vocal_frame, wav,
};
use std::{error::Error, fs, io::Write, path::Path};
use synthphone_e_vocal_dsp::{MusicalSettings, ProcessingMode};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        return Err("Usage: audit_file INPUT.wav OUTPUT_DIRECTORY KEY_0_TO_23".into());
    }
    let key: usize = args[2].parse()?;
    if key >= 24 {
        return Err("Key must be 0..23".into());
    }
    let input = Path::new(&args[0]);
    let out = Path::new(&args[1]);
    let source = wav::read(input)?;
    let rate = source.sample_rate as f64;
    let config = synthphone_e_vocal_dsp::VocalEffectsConfig {
        sample_rate: source.sample_rate as f32,
        ..vocal_frame::default_config()
    };
    let mut impulse = vec![0.0; FFT_SIZE * 4];
    impulse[FFT_SIZE] = 0.1;
    let dry = MusicalSettings {
        mode: ProcessingMode::Dry,
        ..Default::default()
    };
    let calibration = render(&impulse, &[], &config, &dry);
    let peak = calibration
        .audio
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
        .unwrap()
        .0;
    let delay = peak
        .checked_sub(FFT_SIZE)
        .ok_or("Unexpected negative render delay")?;
    fs::create_dir_all(out)?;
    let mut csv = fs::File::create(out.join("pitch.csv"))?;
    writeln!(csv, "mode,input_start_seconds,input_end_seconds,input_hz,inferred_target_hz,output_hz,error_cents,rms,peak,target_stable")?;
    for (name, formant, mode) in [
        ("dry", 0, ProcessingMode::Dry),
        ("tuned", 0, ProcessingMode::PitchControl),
        ("male", 1, ProcessingMode::PitchControl),
        ("female", 2, ProcessingMode::PitchControl),
    ] {
        let settings = MusicalSettings {
            key: key as i8,
            formant,
            mode,
            ..Default::default()
        };
        let result = wav::render(input, &out.join(format!("{name}.wav")), &config, &settings)?;
        let hop = source.sample_rate as usize / 2;
        let window = (source.sample_rate as usize / 3).max(4096);
        for start in (source.sample_rate as usize / 10..source.samples.len()).step_by(hop) {
            let end = start + window;
            if end > source.samples.len() || end + delay > result.audio.len() {
                break;
            }
            let input_hz = estimate_pitch_at_rate(&source.samples[start..end], rate);
            let target = if mode == ProcessingMode::Dry {
                input_hz
            } else {
                input_hz.and_then(|hz| tuning::nearest_scale_hz(hz, key))
            };
            let output = &result.audio[start + delay..end + delay];
            let output_hz = estimate_pitch_at_rate(output, rate);
            let error = output_hz.zip(target).map(|(a, b)| cents(a, b));
            // Keep boundary-crossing errors visible, but distinguish them from
            // windows whose early and late input select the same target.
            let half = window / 2;
            let target_stable = estimate_pitch_at_rate(&source.samples[start..start + half], rate)
                .zip(estimate_pitch_at_rate(
                    &source.samples[end - half..end],
                    rate,
                ))
                .and_then(|(early, late)| {
                    tuning::nearest_scale_hz(early, key).zip(tuning::nearest_scale_hz(late, key))
                })
                .map(|(early, late)| early == late);
            let m = metrics(output);
            let value = |x: Option<f64>| x.map(|n| format!("{n:.6}")).unwrap_or_default();
            writeln!(
                csv,
                "{name},{:.6},{:.6},{},{},{},{},{:.6},{:.6},{}",
                start as f64 / rate,
                end as f64 / rate,
                value(input_hz),
                value(target),
                value(output_hz),
                value(error),
                m.rms,
                m.peak,
                target_stable
                    .map(|stable| stable.to_string())
                    .unwrap_or_default()
            )?;
        }
    }
    fs::write(out.join("audit.txt"), format!(
        "source={}\nsample_rate={}\ninput_frames={}\nkey={}\nkey_name={}\nrender_delay_frames={delay}\n\
        Expected targets are inferred from measured input periods and the selected key.\n\
        This file has no independent note annotations; windows containing changes or unvoiced audio need review.\n\
        target_stable compares inferred scale targets in the first and second half of each input window; blank means unavailable.\n\
        Pitch measurement covers steady monophonic passages from 65 to 1000 Hz.\n",
        input.display(), source.sample_rate, source.samples.len(), key,
        synthphone_e_vocal_dsp::audio::get_key_name(key as i8)))?;
    println!(
        "Wrote dry/tuned/male/female WAVs and pitch.csv to {}",
        out.display()
    );
    Ok(())
}
