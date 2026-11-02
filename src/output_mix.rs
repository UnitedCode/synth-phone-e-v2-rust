//! Shared source trims and output stage, with a common master volume.
use synthphone_e_vocal_dsp::{audio::Waveform, dsp::normalize_sample};

/// 0=mute, 10=unity, 20=2x. Voice trim is applied before the vocal DSP.
pub fn level_gain(level: i8) -> f32 {
    level.clamp(0, 20) as f32 * 0.1
}

/// Keep unity/max at 10/20, with finer attenuation below unity for quiet mics.
/// The nonzero range is 0.01x..2x (46.02 dB); zero remains a true mute.
pub fn synth_gain(level: i8) -> f32 {
    let linear = level_gain(level);
    if level <= 10 {
        linear * linear
    } else {
        linear
    }
}

pub fn waveform_gain(waveform: Waveform) -> f32 {
    // Equal RMS for unit-peak ideal waveforms, referenced to a sine. These are
    // power calibrations, not a claim of equal perceived loudness at every pitch.
    match waveform {
        Waveform::Sine => 1.0,
        Waveform::Triangle | Waveform::Saw => 1.224_744_9,
        Waveform::Square => core::f32::consts::FRAC_1_SQRT_2,
    }
}

pub fn keypad_gain(waveform: Waveform) -> f32 {
    // The pinned Dry path mixes carrier at 0.04 and overlap-adds four Hann
    // windows (sum=2). With the final vocal gain 0.75, 3.75 gives a 0.225
    // sine peak, matching the nominal 0.2-peak live input after vocal DSP.
    3.75 * waveform_gain(waveform)
}

#[inline]
pub fn mix(vocal: f32, melody: f32, drums: f32, waveform: Waveform, master: f32) -> f32 {
    mix_with_synth_gain(vocal, melody, drums, waveform, master, 1.0)
}

pub fn mix_with_synth_gain(
    vocal: f32,
    melody: f32,
    drums: f32,
    waveform: Waveform,
    master: f32,
    synth_gain: f32,
) -> f32 {
    finish(
        sum_sources(vocal, melody * synth_gain, drums, waveform),
        master,
    )
}

/// Unclipped source sum so Percussion can crush the whole mix before limiting.
pub fn sum_sources(vocal: f32, melody: f32, drums: f32, waveform: Waveform) -> f32 {
    // Preserve the existing percussion gain; only melodic tones are calibrated.
    let drum_gain = match waveform {
        Waveform::Sine => 0.14,
        Waveform::Triangle => 0.12,
        Waveform::Square | Waveform::Saw => 0.1,
    };
    vocal * 0.75 + melody * waveform_gain(waveform) * 0.225 + drums * drum_gain
}

pub fn finish(sample: f32, master: f32) -> f32 {
    // Master gain must also pass through the ceiling, including loud chords.
    normalize_sample(sample * master, 0.8)
}
