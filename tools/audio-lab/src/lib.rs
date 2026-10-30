//! Deterministic audio replay, not CPU/peripheral emulation.
use std::time::Instant;
use synthphone_e_vocal_dsp::{dsp::normalize_sample, MusicalSettings, VocalEffectsConfig};
pub mod emulator;
pub mod replay;

// Compile the same entry point and constants as the device, without its HAL.
#[path = "../../../src/constants.rs"]
pub mod constants;
#[path = "../../../src/vocal_frame.rs"]
pub mod vocal_frame;
use constants::{BLOCK_SIZE, FFT_SIZE, HOP_SIZE, SAMPLE_RATE};

pub struct Render {
    /// Before the firmware's final peak limiter, useful for detecting overloads.
    pub raw: Vec<f32>,
    pub audio: Vec<f32>,
    /// Host timings only: these do not predict Cortex-M7 deadline compliance.
    pub hop_micros: Vec<f64>,
    pub hops: usize,
}

/// Replay the firmware's input history, per-sample output reads, hop counter and
/// overlap-add. The lower priority FFT task runs after each DMA block. There is
/// deliberately no simulated CPU duration, IRQ contention, DMA or codec model.
pub fn render(
    input: &[f32],
    carrier: &[f32],
    config: &VocalEffectsConfig,
    settings: &MusicalSettings,
) -> Render {
    render_with_fault(input, carrier, config, settings, None)
}

/// Fault injection lets the tests prove they detect a lost synthesis frame.
pub fn render_with_fault(
    input: &[f32],
    carrier: &[f32],
    config: &VocalEffectsConfig,
    settings: &MusicalSettings,
    omit_hop: Option<usize>,
) -> Render {
    assert_eq!(config.fft_size, FFT_SIZE);
    assert_eq!(config.hop_size, HOP_SIZE);
    assert_eq!((FFT_SIZE as f32 * config.hop_ratio) as usize, HOP_SIZE);
    let mut replay = replay::Replay::new();
    let mut hop_micros = Vec::new();
    let mut raw = Vec::with_capacity(input.len() + FFT_SIZE * 2);
    let total = (input.len() + FFT_SIZE * 2).div_ceil(BLOCK_SIZE) * BLOCK_SIZE;
    for block in (0..total).step_by(BLOCK_SIZE) {
        for i in block..block + BLOCK_SIZE {
            raw.push(replay.sample(input.get(i).copied().unwrap_or(0.0)));
        }
        if replay.pending() {
            let mut carrier_block = [0.0; HOP_SIZE];
            for (i, sample) in carrier_block.iter_mut().enumerate() {
                *sample = carrier
                    .get(replay.hops * HOP_SIZE + i)
                    .copied()
                    .unwrap_or(0.0);
            }
            let start = Instant::now();
            replay.finish_block(&carrier_block, config, settings, omit_hop);
            hop_micros.push(start.elapsed().as_secs_f64() * 1e6);
        }
    }
    let audio = raw.iter().map(|&x| normalize_sample(x, 0.8)).collect();
    Render {
        raw,
        audio,
        hop_micros,
        hops: replay.hops,
    }
}

/// Deterministic voiced fixture; harmonics are (harmonic number, amplitude).
pub fn tone(hz: f64, seconds: f64, harmonics: &[(usize, f64)]) -> Vec<f32> {
    (0..(seconds * SAMPLE_RATE as f64) as usize)
        .map(|i| {
            let phase = std::f64::consts::TAU * hz * i as f64 / SAMPLE_RATE as f64;
            harmonics
                .iter()
                .map(|&(n, a)| a * (phase * n as f64).sin())
                .sum::<f64>() as f32
        })
        .collect()
}

#[derive(Debug)]
pub struct Metrics {
    pub rms: f64,
    pub peak: f32,
    pub dc: f64,
    pub non_finite: usize,
    pub over_full_scale: usize,
    pub max_step: f32,
    /// Silence is measured over whole hops to avoid counting normal zero crossings.
    pub silent_hops: usize,
    /// Smallest / largest hop RMS; useful for sustained-tone dropout detection.
    pub hop_rms_ratio: f64,
}

pub fn metrics(samples: &[f32]) -> Metrics {
    let finite: Vec<f32> = samples.iter().copied().filter(|x| x.is_finite()).collect();
    let n = finite.len().max(1) as f64;
    let hop_rms: Vec<f64> = samples
        .as_chunks::<HOP_SIZE>()
        .0
        .iter()
        .map(|hop| (hop.iter().map(|&x| (x as f64).powi(2)).sum::<f64>() / HOP_SIZE as f64).sqrt())
        .collect();
    let min_rms = hop_rms.iter().copied().fold(f64::INFINITY, f64::min);
    let max_rms = hop_rms.iter().copied().fold(0.0, f64::max);
    Metrics {
        rms: (finite.iter().map(|&x| (x as f64).powi(2)).sum::<f64>() / n).sqrt(),
        peak: finite.iter().map(|x| x.abs()).fold(0.0, f32::max),
        dc: finite.iter().map(|&x| x as f64).sum::<f64>() / n,
        non_finite: samples.len() - finite.len(),
        over_full_scale: finite.iter().filter(|x| x.abs() > 1.0).count(),
        max_step: samples
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max),
        silent_hops: samples
            .as_chunks::<HOP_SIZE>()
            .0
            .iter()
            .filter(|hop| {
                hop.iter().all(|x| x.is_finite())
                    && hop.iter().map(|&x| x as f64 * x as f64).sum::<f64>() / (HOP_SIZE as f64)
                        < 1e-10
            })
            .count(),
        hop_rms_ratio: if max_rms > 0.0 {
            min_rms / max_rms
        } else {
            0.0
        },
    }
}

/// Independent normalized autocorrelation oracle (not the firmware's HPS/YIN).
/// For steady monophonic passages only; a missing estimate fails pitch criteria.
pub fn estimate_pitch(samples: &[f32]) -> Option<f64> {
    if samples.len() < 4096 || metrics(samples).rms < 1e-5 {
        return None;
    }
    let x: Vec<f64> = samples.iter().step_by(4).map(|&x| x as f64).collect();
    let sr = SAMPLE_RATE as f64 / 4.0;
    let min_lag = (sr / 1000.0) as usize;
    let max_lag = (sr / 65.0) as usize;
    let window = x.len() - max_lag - 1;
    let mean = x.iter().sum::<f64>() / x.len() as f64;
    let mut corr = vec![0.0; max_lag + 1];
    for (lag, value) in corr.iter_mut().enumerate().skip(min_lag - 1) {
        let (mut xy, mut xx, mut yy) = (0.0, 0.0, 0.0);
        for i in 0..window {
            let a = x[i] - mean;
            let b = x[i + lag] - mean;
            xy += a * b;
            xx += a * a;
            yy += b * b;
        }
        *value = xy / (xx * yy).sqrt().max(1e-30);
    }
    for lag in min_lag..max_lag {
        if corr[lag] > 0.8 && corr[lag] > corr[lag - 1] && corr[lag] >= corr[lag + 1] {
            let denominator = corr[lag - 1] - 2.0 * corr[lag] + corr[lag + 1];
            let offset = if denominator.abs() > 1e-12 {
                0.5 * (corr[lag - 1] - corr[lag + 1]) / denominator
            } else {
                0.0
            };
            return Some(sr / (lag as f64 + offset));
        }
    }
    None
}

pub fn cents(measured: f64, target: f64) -> f64 {
    1200.0 * (measured / target).log2()
}

/// Ignore startup and tail; assess the middle of a sustained fixture.
pub fn steady(samples: &[f32]) -> &[f32] {
    let start = (SAMPLE_RATE * 0.2) as usize;
    let end = samples.len().saturating_sub(FFT_SIZE * 3);
    &samples[start.min(end)..end]
}
