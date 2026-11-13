//! Render one real mono recording with identical settings on any selected DSP pin.
use audio_lab::{metrics, output_mix, render, vocal_frame, wav};
use std::{error::Error, fs, path::Path};
use synthphone_e_vocal_dsp::{
    audio::{Oscillator, Waveform},
    MusicalSettings, ProcessingMode,
};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: vocal_recording INPUT_MONO.wav NEW_OUTPUT_DIRECTORY".into());
    }
    let input = wav::read(Path::new(&args[0]))?;
    let directory = Path::new(&args[1]);
    fs::create_dir(directory)?;
    let mut config = vocal_frame::config_for_speed(10);
    config.sample_rate = input.sample_rate as f32;
    config.max_frequency = config.max_frequency.min(config.sample_rate * 0.5);
    // A rich, unit-peak saw matches the device's carrier oscillator. A sparse
    // sine carrier would obscure the consonants and higher vowel resonances.
    let mut oscillator = Oscillator::new(220.0, config.sample_rate, Waveform::Saw);
    let carrier: Vec<_> = input
        .samples
        .iter()
        .map(|_| oscillator.next_value())
        .collect();
    let mut report = format!(
        "input={}\nsample_rate={}\nframes={}\ninput_metrics={:?}\nconfig={config:?}\n",
        Path::new(&args[0]).display(),
        input.sample_rate,
        input.samples.len(),
        metrics(&input.samples)
    );
    for (name, mode, formant) in [
        ("pitch", ProcessingMode::PitchControl, 0),
        ("vocoder", ProcessingMode::Vocode, 0),
        ("vocoder-male", ProcessingMode::Vocode, 1),
    ] {
        let settings = MusicalSettings {
            mode,
            formant,
            key: 0,
            ..Default::default()
        };
        let output = render(&input.samples, &carrier, &config, &settings);
        let raw = metrics(&output.raw);
        if raw.non_finite != 0 || raw.rms < 1e-8 {
            return Err(format!("{name}: non-finite or inaudible render").into());
        }
        // Same final vocal trim and ceiling as the device, without other synth
        // or drum sources. Preserve these unmatched outputs for level analysis.
        let playback: Vec<_> = output
            .raw
            .iter()
            .map(|&s| output_mix::finish(s * 0.75, 1.0))
            .collect();
        report.push_str(&format!(
            "{name}: settings={settings:?}; raw={raw:?}; final={:?}\n",
            metrics(&playback)
        ));
        wav::write(
            &directory.join(format!("{name}.wav")),
            &playback,
            input.sample_rate,
        )?;
    }
    fs::write(directory.join("render.txt"), &report)?;
    print!("{report}");
    Ok(())
}
