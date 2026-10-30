//! Identical deterministic inputs/settings for host and emulated ARM execution.
use crate::constants::SAMPLE_RATE;
use synthphone_e_vocal_dsp::{MusicalSettings, ProcessingMode};

pub const COUNT: usize = 9;
pub const INPUT_SAMPLES: usize = 24_000;
pub const MAGIC: &[u8; 8] = b"SAEARM01";
pub const NAMES: [&str; COUNT] = [
    "silence",
    "pitch-110",
    "pitch-220",
    "pitch-440",
    "dry-237",
    "harmony",
    "vocode",
    "female-bright-220",
    "male-sine-440",
];

pub fn settings(case: usize) -> MusicalSettings {
    let mode = match case {
        4 => ProcessingMode::Dry,
        5 => ProcessingMode::Harmony,
        6 => ProcessingMode::Vocode,
        _ => ProcessingMode::PitchControl,
    };
    MusicalSettings {
        mode,
        formant: match case {
            7 => 2,
            8 => 1,
            _ => 0,
        },
        midi_frequencies: if case == 5 {
            [220.0, 330.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
        } else {
            [0.0; 8]
        },
        ..MusicalSettings::default()
    }
}

pub fn input(case: usize, index: usize) -> f32 {
    if case == 0 || index >= INPUT_SAMPLES {
        return 0.0;
    }
    let hz = match case {
        1 => 110.0,
        3 | 8 => 440.0,
        4 => 237.0,
        _ => 220.0,
    };
    let detune = if case == 4 { 1.0 } else { 1.0204225 }; // 35 cents
    let phase = core::f32::consts::TAU * hz * detune * index as f32 / SAMPLE_RATE;
    if case == 7 {
        0.06 * libm::sinf(phase)
            + 0.18 * libm::sinf(2.0 * phase)
            + 0.08 * libm::sinf(3.0 * phase)
            + 0.04 * libm::sinf(4.0 * phase)
    } else {
        0.25 * libm::sinf(phase)
    }
}

pub fn carrier(case: usize, index: usize) -> f32 {
    if case != 6 || index >= INPUT_SAMPLES {
        return 0.0;
    }
    let phase = core::f32::consts::TAU * 220.0 * index as f32 / SAMPLE_RATE;
    0.2 * libm::sinf(phase) + 0.1 * libm::sinf(phase * 2.0)
}
