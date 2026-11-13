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

#[test]
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
fn vocoder_backing_mute_changes_the_audio() {
    let pair = backing_pair(ProcessingMode::Vocode);
    let difference = relative_difference(middle(&pair.a), middle(&pair.b));
    assert!(
        difference > 0.05,
        "backing mute produced {:.6}% relative RMS difference",
        difference * 100.0
    );
}

#[test]
fn vocoder_carrier_fader_is_linear_and_all_muted_notes_are_silent() {
    let input = vowel(200.0, SECONDS, 1.0);
    let mut settings = vocoder_settings();
    settings.midi_frequencies[0] = 330.0;
    let full = vocode(&input, &midi_carrier(&settings, SECONDS), settings);
    settings.midi_gains[0] = 0.125;
    let quiet = vocode(&input, &midi_carrier(&settings, SECONDS), settings);
    let ratio =
        audio_lab::metrics(middle(&quiet.raw)).rms / audio_lab::metrics(middle(&full.raw)).rms;
    assert!(
        (ratio - 0.125).abs() < 0.001,
        "carrier gain was renormalized: {ratio}"
    );
    settings.midi_gains[0] = 0.0;
    let mute = vocode(&input, &midi_carrier(&settings, SECONDS), settings);
    assert!(mute.raw.iter().all(|&x| x == 0.0));
}

#[test]
fn muted_carrier_notes_keep_phase_and_do_not_trigger_keypad_fallback() {
    use synthphone_e_vocal_dsp::audio::{Oscillator, Waveform};
    let mut muted = core::array::from_fn(|_| Oscillator::new(330.0, RATE as f32, Waveform::Saw));
    let mut full = core::array::from_fn(|_| Oscillator::new(330.0, RATE as f32, Waveform::Saw));
    let frequencies = [330.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    let mut a = [0.0; audio_lab::constants::HOP_SIZE];
    let mut b = a;
    assert!(audio_lab::vocal_carrier::render_midi_hop(
        &mut muted,
        &frequencies,
        &[0.0; 8],
        Waveform::Saw,
        &mut a
    ));
    audio_lab::vocal_carrier::render_midi_hop(
        &mut full,
        &frequencies,
        &[1.0; 8],
        Waveform::Saw,
        &mut b,
    );
    assert!(a.iter().all(|&s| s == 0.0));
    audio_lab::vocal_carrier::render_midi_hop(
        &mut muted,
        &frequencies,
        &[1.0; 8],
        Waveform::Saw,
        &mut a,
    );
    audio_lab::vocal_carrier::render_midi_hop(
        &mut full,
        &frequencies,
        &[1.0; 8],
        Waveform::Saw,
        &mut b,
    );
    assert_eq!(a, b);
}

#[test]
fn both_vocoder_formants_change_timbre_without_moving_carrier_pitch() {
    let input = vowel(200.0, SECONDS, 1.0);
    let carrier = carrier(&[330.0], SECONDS);
    let settings = vocoder_settings();
    let plain = vocode(&input, &carrier, settings);
    for formant in [1, 2] {
        let shifted = vocode(
            &input,
            &carrier,
            synthphone_e_vocal_dsp::MusicalSettings {
                formant,
                ..settings
            },
        );
        assert!(relative_difference(middle(&plain.raw), middle(&shifted.raw)) > 0.05);
        assert!(harmonic_fraction(middle(&shifted.raw), 330.0) > 0.9);
    }
}

#[test]
fn a_noise_gap_releases_previous_phrase_correction() {
    let first = audio_lab::tone(180.0, 0.7, &[(1, 0.15)]);
    let noise = noise(0.3);
    let next = audio_lab::tone(224.492958, 0.8, &[(1, 0.15)]);
    let mut input = first.clone();
    let mut reference = vec![0.0; first.len()];
    for audio in [&mut input, &mut reference] {
        audio.extend_from_slice(&noise);
        audio.extend_from_slice(&next);
    }
    let mut settings = synthphone_e_vocal_dsp::MusicalSettings::default();
    settings.midi_frequencies[0] = 220.0;
    let config = audio_lab::vocal_frame::config_for_speed(1);
    let actual = audio_lab::render(&input, &[], &config, &settings);
    let fresh = audio_lab::render(&reference, &[], &config, &settings);
    let start = first.len() + noise.len();
    assert!(relative_difference(&actual.raw[start..], &fresh.raw[start..]) < 0.00001);
}
