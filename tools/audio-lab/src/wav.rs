//! WAV rendering uses the file's clock for both DSP and generated carriers.
use crate::{constants::FFT_SIZE, midi_notes, Render};
use std::{error::Error, path::Path};
use synthphone_e_vocal_dsp::{MusicalSettings, ProcessingMode, VocalEffectsConfig};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub struct Audio {
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}

pub fn read(path: &Path) -> Result<Audio> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    if spec.channels != 1 {
        return Err("Use a mono WAV; channels are not mixed implicitly".into());
    }
    if !(8000..=192000).contains(&spec.sample_rate) {
        return Err("WAV sample rate must be between 8000 and 192000 Hz".into());
    }
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<std::result::Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 2_f32.powi(spec.bits_per_sample as i32 - 1);
            reader
                .samples::<i32>()
                .map(|value| value.map(|s| s as f32 / scale))
                .collect::<std::result::Result<_, _>>()?
        }
    };
    if samples.is_empty() || samples.iter().any(|s| !s.is_finite()) {
        return Err("Input WAV must contain finite audio samples".into());
    }
    Ok(Audio {
        sample_rate: spec.sample_rate,
        samples,
    })
}

pub fn write(path: &Path, samples: &[f32], sample_rate: u32) -> Result<()> {
    if sample_rate == 0 || samples.iter().any(|s| !s.is_finite()) {
        return Err("Output WAV requires a positive sample rate and finite samples".into());
    }
    let mut writer = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 1,
            sample_rate,
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

pub fn render(
    input: &Path,
    output: &Path,
    config: &VocalEffectsConfig,
    settings: &MusicalSettings,
) -> Result<Render> {
    let source = read(input)?;
    let mut config = *config;
    config.sample_rate = source.sample_rate as f32;
    config.max_frequency = config.max_frequency.min(config.sample_rate * 0.5);
    let carrier = if settings.mode == ProcessingMode::Vocode {
        let hz = f64::from(midi_notes::frequency(57));
        (0..source.samples.len() + FFT_SIZE * 2)
            .map(|i| {
                let phase = std::f64::consts::TAU * hz * i as f64 / f64::from(source.sample_rate);
                (0.2 * phase.sin() + 0.1 * (2.0 * phase).sin() + 0.06 * (3.0 * phase).sin()) as f32
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let result = crate::render(&source.samples, &carrier, &config, settings);
    write(output, &result.audio, source.sample_rate)?;
    Ok(result)
}
