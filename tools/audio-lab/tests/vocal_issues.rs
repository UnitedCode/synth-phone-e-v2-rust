use audio_lab::vocal_evidence::*;
use synthphone_e_vocal_dsp::{dsp::frequency_analysis::find_pitch_harmony, ProcessingMode};

#[test]
fn harmonic_measurement_accepts_the_carrier_and_rejects_a_different_excitation() {
    for target in TARGETS {
        let correct = vowel(target, SECONDS, 1.0);
        assert!(harmonic_fraction(middle(&correct), target) > 0.98);
    }
    let wrong = vowel(200.0, SECONDS, 1.0);
    assert!(harmonic_fraction(middle(&wrong), 330.0) < 0.1);
}

#[test]
fn the_unvoiced_fixture_is_aperiodic_and_the_detector_accepts_a_vowel() {
    let input = noise(SECONDS);
    let rejected = input
        .windows(2048)
        .step_by(256)
        .filter(|frame| find_pitch_harmony(frame, &config()) == 0.0)
        .count();
    let total = input.windows(2048).step_by(256).count();
    assert!(rejected as f64 / total as f64 > 0.99);
    let voiced = vowel(200.0, SECONDS, 1.0);
    assert!((find_pitch_harmony(&voiced[3000..5048], &config()) - 200.0).abs() < 2.0);
}

#[test]
fn formant_comparison_detects_a_working_control_in_pitch_mode() {
    let pair = formant_pair(ProcessingMode::PitchControl);
    assert!(relative_difference(middle(&pair.a), middle(&pair.b)) > 0.05);
}

#[test]
fn backing_comparison_detects_a_working_fader_in_harmony_mode() {
    let pair = backing_pair(ProcessingMode::Harmony);
    assert!(relative_difference(middle(&pair.a), middle(&pair.b)) > 0.05);
}

// Red acceptance tests are explicit and opt-in until the requested DSP fixes
// are authorized. Never invert these assertions to make a defect pass.
#[test]
#[ignore = "known defect: vocoder transfers individual microphone bins"]
fn vocoder_preserves_the_carriers_harmonic_grid() {
    let mut failures = Vec::new();
    for source in SOURCES {
        let input = vowel(source, SECONDS, 1.0);
        for target in TARGETS {
            let carrier = carrier(&[target], SECONDS);
            let output = vocode(&input, &carrier, vocoder_settings());
            let fraction = harmonic_fraction(middle(&output.raw), target);
            if fraction <= 0.9 {
                failures.push(format!(
                    "{source} -> {target} Hz: {:.2}% on carrier grid",
                    fraction * 100.0
                ));
            }
        }
    }
    // Allow 10% residual energy; the independent reference retains >98%.
    assert!(
        failures.is_empty(),
        "carrier grid failures: {}",
        failures.join("; ")
    );
}

#[test]
#[ignore = "known defect: pitch correction falls back after rejecting unvoiced input"]
fn rejected_unvoiced_audio_is_not_pitch_corrected() {
    let pair = unvoiced_pair();
    let difference = relative_difference(middle(&pair.a), middle(&pair.b));
    assert!(
        difference < 0.01,
        "unvoiced correction changed audio by {:.2}% relative RMS",
        difference * 100.0
    );
}

#[test]
#[ignore = "known defect: vocoder ignores formant settings"]
fn vocoder_formant_control_changes_the_audio() {
    let pair = formant_pair(ProcessingMode::Vocode);
    let difference = relative_difference(middle(&pair.a), middle(&pair.b));
    assert!(
        difference > 0.05,
        "formant change produced {:.6}% relative RMS difference",
        difference * 100.0
    );
}

#[test]
#[ignore = "known defect: vocoder ignores supplied MIDI group gains"]
fn vocoder_backing_mute_changes_the_audio() {
    let pair = backing_pair(ProcessingMode::Vocode);
    let difference = relative_difference(middle(&pair.a), middle(&pair.b));
    assert!(
        difference > 0.05,
        "backing mute produced {:.6}% relative RMS difference",
        difference * 100.0
    );
}
