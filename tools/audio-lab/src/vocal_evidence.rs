//! Reproducible listening fixtures and independent measurements for vocal audits.
//! Uses the selected firmware DSP pin via `render`, including experiments.
use crate::{metrics, render, vocal_frame, Render};
use std::f64::consts::TAU;
use synthphone_e_vocal_dsp::{MusicalSettings, ProcessingMode, VocalEffectsConfig};

pub const RATE: u32 = 48_000;
pub const SECONDS: f64 = 1.2;
pub const SOURCES: [f64; 3] = [110.0, 200.0, 300.0];
pub const TARGETS: [f64; 4] = [110.0, 220.0, 330.0, 440.0];

pub fn config() -> VocalEffectsConfig {
    VocalEffectsConfig {
        sample_rate: RATE as f32,
        ..vocal_frame::default_config()
    }
}

/// A source/filter vowel, with a known excitation and three broad resonances.
/// This is synthetic audio, not a recording or a model of vocal intelligibility.
pub fn vowel(hz: f64, seconds: f64, formant_scale: f64) -> Vec<f32> {
    let partials: Vec<_> = (1..=(8_000.0 / hz) as usize)
        .map(|h| {
            let frequency = h as f64 * hz;
            let envelope = [
                (730.0, 100.0, 1.0),
                (1090.0, 140.0, 0.7),
                (2440.0, 220.0, 0.35),
            ]
            .iter()
            .map(|&(center, width, gain)| {
                gain * (-0.5 * ((frequency - center * formant_scale) / width).powi(2)).exp()
            })
            .sum::<f64>();
            (h, (0.12 + envelope) / (h as f64).sqrt())
        })
        .collect();
    let mut samples: Vec<_> = (0..(seconds * RATE as f64) as usize)
        .map(|i| {
            let t = i as f64 / RATE as f64;
            partials
                .iter()
                .map(|&(h, a)| a * (TAU * hz * h as f64 * t).sin())
                .sum::<f64>() as f32
        })
        .collect();
    let peak = metrics(&samples).peak.max(1e-9);
    for sample in &mut samples {
        *sample *= 0.2 / peak;
    }
    samples
}

pub fn carrier(frequencies: &[f64], seconds: f64) -> Vec<f32> {
    (0..(seconds * RATE as f64) as usize)
        .map(|i| {
            frequencies
                .iter()
                .map(|&hz| {
                    let phase = TAU * hz * i as f64 / RATE as f64;
                    (1..=(10_000.0 / hz) as usize)
                        .map(|h| (phase * h as f64).sin() / h as f64)
                        .sum::<f64>()
                })
                .sum::<f64>() as f32
                * (0.15 / frequencies.len().max(1) as f32)
        })
        .collect()
}

/// Fixed-seed broadband, aperiodic input; independently verify its voicing below.
pub fn noise(seconds: f64) -> Vec<f32> {
    let mut seed = 1_234_567_u32;
    (0..(seconds * RATE as f64) as usize)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed as f64 / u32::MAX as f64 * 0.2 - 0.1) as f32
        })
        .collect()
}

pub fn middle(samples: &[f32]) -> &[f32] {
    let start = RATE as usize / 4;
    &samples[start..start + RATE as usize * 4 / 5]
}

pub fn relative_difference(a: &[f32], b: &[f32]) -> f64 {
    assert_eq!(a.len(), b.len());
    let error = a
        .iter()
        .zip(b)
        .map(|(&a, &b)| (a as f64 - b as f64).powi(2))
        .sum::<f64>();
    let energy = a.iter().map(|&x| (x as f64).powi(2)).sum::<f64>();
    (error / energy.max(1e-30)).sqrt()
}

/// Projection onto a requested harmonic grid, independent of the pitch detector.
/// Rectangular integer-duration windows make different grid harmonics orthogonal.
pub fn harmonic_fraction(samples: &[f32], hz: f64) -> f64 {
    let energy = samples.iter().map(|&x| (x as f64).powi(2)).sum::<f64>();
    let mut projected = 0.0;
    for h in 1..=(10_000.0 / hz) as usize {
        let (mut re, mut im) = (0.0, 0.0);
        for (i, &sample) in samples.iter().enumerate() {
            let phase = TAU * hz * h as f64 * i as f64 / RATE as f64;
            re += sample as f64 * phase.cos();
            im += sample as f64 * phase.sin();
        }
        projected += 2.0 * (re * re + im * im) / samples.len() as f64;
    }
    projected / energy.max(1e-30)
}

pub fn vocode(input: &[f32], carrier: &[f32], settings: MusicalSettings) -> Render {
    assert_eq!(settings.mode, ProcessingMode::Vocode);
    render(input, carrier, &config(), &settings)
}

pub fn vocoder_settings() -> MusicalSettings {
    MusicalSettings {
        mode: ProcessingMode::Vocode,
        ..Default::default()
    }
}

pub struct Pair {
    pub input: Vec<f32>,
    pub a: Vec<f32>,
    pub b: Vec<f32>,
}

pub fn unvoiced_pair() -> Pair {
    let input = noise(SECONDS);
    let mut bypass = config();
    bypass.pitch_correction_strength = 0.0;
    Pair {
        a: render(&input, &[], &bypass, &MusicalSettings::default()).raw,
        b: render(&input, &[], &config(), &MusicalSettings::default()).raw,
        input,
    }
}

pub fn formant_pair(mode: ProcessingMode) -> Pair {
    let input = vowel(200.0, SECONDS, 1.0);
    let carrier = carrier(&[330.0], SECONDS);
    let settings = MusicalSettings {
        mode,
        ..Default::default()
    };
    let shifted = MusicalSettings {
        formant: 1,
        formant_male_ratio: 0.5,
        ..settings
    };
    Pair {
        a: render(&input, &carrier, &config(), &settings).raw,
        b: render(&input, &carrier, &config(), &shifted).raw,
        input,
    }
}

/// The device's carrier builder, with one continuous oscillator bank per render.
pub fn midi_carrier(settings: &MusicalSettings, seconds: f64) -> Vec<f32> {
    use synthphone_e_vocal_dsp::audio::{Oscillator, Waveform};
    let mut oscillators =
        core::array::from_fn(|_| Oscillator::new(440.0, RATE as f32, Waveform::Saw));
    let length = (seconds * RATE as f64) as usize;
    let mut output = Vec::with_capacity(length);
    while output.len() < length {
        let mut hop = [0.0; crate::constants::HOP_SIZE];
        crate::vocal_carrier::render_midi_hop(
            &mut oscillators,
            &settings.midi_frequencies,
            &settings.midi_gains,
            Waveform::Saw,
            &mut hop,
        );
        output.extend_from_slice(&hop[..hop.len().min(length - output.len())]);
    }
    output
}

/// Exercises real VoiceManager gains and the shared device carrier builder,
/// then the DSP boundary. This does not emulate MIDI interrupts.
pub fn backing_pair(mode: ProcessingMode) -> Pair {
    let input = vowel(200.0, SECONDS, 1.0);
    let mut manager = crate::voice_generator::VoiceManager::<8>::new(RATE as f32);
    manager.note_on(57, 100, 3);
    manager.note_on(64, 100, 0);
    let (frequencies, gains) = manager.get_harmony_voices(127);
    let settings = MusicalSettings {
        mode,
        midi_frequencies: frequencies,
        midi_gains: gains,
        ..Default::default()
    };
    let muted = MusicalSettings {
        midi_gains: manager.get_harmony_voices(0).1,
        ..settings
    };
    let mut config = config();
    config.harmony_lead_gain = 0.0;
    Pair {
        a: render(
            &input,
            &midi_carrier(&settings, SECONDS),
            &config,
            &settings,
        )
        .raw,
        b: render(&input, &midi_carrier(&muted, SECONDS), &config, &muted).raw,
        input,
    }
}
