//! Identical deterministic inputs/settings for host and emulated ARM execution.
use crate::constants::SAMPLE_RATE;
use synthphone_e_vocal_dsp::{MusicalSettings, ProcessingMode};

pub const COUNT: usize = 20;
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
    "harmony-vibrato",
    "harmony-rich-eight",
    "harmony-noise-eight",
    "harmony-weighted-lead",
    "dry-female-237",
    "vocode-dry",
    "vocode-half",
    "vocode-full",
    "harmony-low-90",
    "harmony-noise-with-lead",
    "harmony-vowel-register",
];

pub fn config(case: usize) -> synthphone_e_vocal_dsp::VocalEffectsConfig {
    let amount = match case {
        14 => 0,
        15 => 5,
        _ => 10,
    };
    let mut config = crate::vocal_frame::config_for_controls(10, amount);
    if case >= 9 {
        config.harmony_lead_gain = 0.0;
    }
    if case == 18 {
        config.harmony_lead_gain = 1.0;
    }
    config
}

pub fn settings(case: usize) -> MusicalSettings {
    let mode = match case {
        4 | 13 => ProcessingMode::Dry,
        5 | 9..=12 | 17..=19 => ProcessingMode::Harmony,
        6 | 14..=16 => ProcessingMode::Vocode,
        _ => ProcessingMode::PitchControl,
    };
    MusicalSettings {
        mode,
        formant: match case {
            7 | 13 => 2,
            8 => 1,
            _ => 0,
        },
        midi_frequencies: if case == 5 {
            [220.0, 330.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
        } else if case == 12 {
            [220.0, 329.62756, 391.99542, 440.0, 0.0, 0.0, 0.0, 0.0]
        } else if case == 10 || case == 11 {
            [
                130.81278, 164.81378, 195.99771, 220.0, 261.62555, 329.62756, 391.99542, 440.0,
            ]
        } else if matches!(case, 9 | 17..=19) {
            [220.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
        } else {
            [0.0; 8]
        },
        midi_gains: if case == 12 {
            [2.0, 2.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0, 1.0, 1.0, 1.0, 1.0]
        } else {
            [1.0; 8]
        },
        ..MusicalSettings::default()
    }
}

pub fn input(case: usize, index: usize) -> f32 {
    if case == 0 || index >= INPUT_SAMPLES {
        return 0.0;
    }
    if case == 11 || case == 18 {
        let mut random = (index as u32)
            .wrapping_mul(0x9e3779b9)
            .wrapping_add(0x12345678);
        random ^= random << 13;
        random ^= random >> 17;
        random ^= random << 5;
        return random as f32 / u32::MAX as f32 * 0.2 - 0.1;
    }
    if case == 17 {
        return 0.15 * libm::sinf(core::f32::consts::TAU * 90.0 * index as f32 / SAMPLE_RATE);
    }
    if case == 19 {
        let time = index as f32 / SAMPLE_RATE;
        let phase = core::f32::consts::TAU * 150.0 * time;
        let balance = 0.5 + 0.5 * libm::sinf(core::f32::consts::TAU * 2.0 * time);
        let odd = balance * balance * 0.15;
        return (0.005 + odd * 0.2) * libm::sinf(phase)
            + 0.12 * libm::sinf(2.0 * phase)
            + (0.01 + odd * 0.3) * libm::sinf(3.0 * phase)
            + 0.08 * libm::sinf(4.0 * phase)
            + (0.005 + odd * 0.1) * libm::sinf(5.0 * phase)
            + 0.04 * libm::sinf(6.0 * phase);
    }
    if case == 9 || case == 10 || case == 12 {
        let time = index as f32 / SAMPLE_RATE;
        let phase = core::f32::consts::TAU * 200.0 * time
            - 1.2 * libm::cosf(core::f32::consts::TAU * 5.0 * time);
        return if case == 10 || case == 12 {
            (1..=10)
                .map(|h| 0.15 / h as f32 * libm::sinf(phase * h as f32))
                .sum()
        } else {
            0.2 * libm::sinf(phase)
        };
    }
    let hz = match case {
        1 => 110.0,
        3 | 8 => 440.0,
        4 | 13 => 237.0,
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
    if !matches!(case, 6 | 14..=16) || index >= INPUT_SAMPLES {
        return 0.0;
    }
    let phase = core::f32::consts::TAU * 220.0 * index as f32 / SAMPLE_RATE;
    0.2 * libm::sinf(phase) + 0.1 * libm::sinf(phase * 2.0)
}
