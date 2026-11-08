//! Offline listening mix. These gains are not a firmware policy or DSP benchmark.
use crate::metrics;

pub struct StemMix {
    pub audio: Vec<f32>,
    pub upper_gain: f64,
    pub backing_gain: f64,
}

/// Set an explicit whole-recording upper/backing RMS ratio using constant
/// faders, then apply a shared playback gain. No compression or note removal.
pub fn balance(upper: &[f32], backing: &[f32], upper_db: f64) -> Result<StemMix, &'static str> {
    if upper.is_empty() || upper.len() != backing.len() {
        return Err("need equal nonempty stem lengths");
    }
    if !upper_db.is_finite() || !(-18.0..=18.0).contains(&upper_db) {
        return Err("balance must be -18..18 dB");
    }
    let (lead, back) = (metrics(upper), metrics(backing));
    if lead.non_finite != 0 || back.non_finite != 0 || lead.rms < 1e-7 {
        return Err("need finite stems and an audible upper part");
    }
    let upper_gain = 1.0 / lead.rms;
    let backing_gain = if back.rms > 1e-7 {
        10_f64.powf(-upper_db / 20.0) / back.rms
    } else {
        0.0
    };
    let mut audio: Vec<_> = upper
        .iter()
        .zip(backing)
        .map(|(&a, &b)| (a as f64 * upper_gain + b as f64 * backing_gain) as f32)
        .collect();
    let mixed = metrics(&audio);
    if mixed.rms < 1e-7 {
        return Err("stems cancel; choose a different balance");
    }
    let gain = (0.05 / mixed.rms).min(0.9 / mixed.peak as f64);
    for sample in &mut audio {
        *sample = (*sample as f64 * gain) as f32;
    }
    Ok(StemMix {
        audio,
        upper_gain: upper_gain * gain,
        backing_gain: backing_gain * gain,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quiet_upper_line_can_lead_a_much_louder_backing_without_clipping() {
        let n = 8192;
        let upper: Vec<_> = (0..n)
            .map(|i| (0.01 * (std::f64::consts::TAU * 37.0 * i as f64 / n as f64).sin()) as f32)
            .collect();
        let backing: Vec<_> = (0..n)
            .map(|i| (0.3 * (std::f64::consts::TAU * 93.0 * i as f64 / n as f64).sin()) as f32)
            .collect();
        let mixed = balance(&upper, &backing, 6.0).unwrap();
        // Independent Fourier projections recover each line's amplitude.
        let amplitude = |frequency: f64| {
            mixed
                .audio
                .iter()
                .enumerate()
                .map(|(i, &s)| {
                    s as f64 * (std::f64::consts::TAU * frequency * i as f64 / n as f64).sin()
                })
                .sum::<f64>()
                * 2.0
                / n as f64
        };
        let ratio = amplitude(37.0) / amplitude(93.0);
        assert!((20.0 * ratio.log10() - 6.0).abs() < 0.001);
        let m = metrics(&mixed.audio);
        assert!((m.rms - 0.05).abs() < 1e-6);
        assert!(m.peak <= 0.9 && m.non_finite == 0);
        assert!(balance(&upper, &backing, f64::NAN).is_err());
    }
}
