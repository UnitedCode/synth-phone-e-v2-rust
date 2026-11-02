use audio_lab::{cents, constants::SAMPLE_RATE, render, steady, tone, vocal_frame};
use synthphone_e_vocal_dsp::{MusicalSettings, ProcessingMode};

fn check(settings: MusicalSettings, input: f64, expected: f64) {
    let audio = render(
        &tone(input, 1.2, &[(1, 0.2)]),
        &[],
        &vocal_frame::default_config(),
        &settings,
    );
    // Autocorrelation can choose a subharmonic after large upward shifts.
    // Measure the strongest spectral component independently instead.
    let samples = &steady(&audio.audio)[..8192];
    let actual = (70..=2000)
        .step_by(5)
        .map(|hz| {
            let angle = 2.0 * std::f64::consts::PI * hz as f64 / f64::from(SAMPLE_RATE);
            let (sin_step, cos_step) = angle.sin_cos();
            let (mut sin, mut cos, mut re, mut im) = (0.0, 1.0, 0.0, 0.0);
            for &x in samples {
                re += f64::from(x) * cos;
                im += f64::from(x) * sin;
                (sin, cos) = (
                    sin * cos_step + cos * sin_step,
                    cos * cos_step - sin * sin_step,
                );
            }
            (hz as f64, re * re + im * im)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap()
        .0;
    assert!(
        cents(actual, expected).abs() < 25.0,
        "expected {expected}Hz, got {actual}Hz"
    );
}

#[test]
fn dry_octave_is_applied_once() {
    for ratio in [0.5, 2.0] {
        check(
            MusicalSettings {
                mode: ProcessingMode::Dry,
                octave_ratio: ratio,
                ..Default::default()
            },
            220.0,
            220.0 * f64::from(ratio),
        );
    }
}

#[test]
fn fixed_note_octave_is_applied_once() {
    // C major degree 6 at the vocal reference octave is A4 (440 Hz).
    for ratio in [0.5, 1.0, 2.0] {
        check(
            MusicalSettings {
                mode: ProcessingMode::PitchControl,
                key: 0,
                note: 6,
                octave_ratio: ratio,
                ..Default::default()
            },
            448.99,
            440.0 * f64::from(ratio),
        );
    }
}
