use audio_lab::{constants::SAMPLE_RATE, render, steady, tone, vocal_frame};
use synthphone_e_vocal_dsp::{MusicalSettings, ProcessingMode};

// Independent sinusoidal projection with a Hann taper. A monophonic pitch
// estimator cannot establish that every note of a chord is present.
fn component(samples: &[f32], hz: f64) -> f64 {
    (-3..=3)
        .map(|step| {
            let hz = hz * 2_f64.powf(step as f64 * 5.0 / 1200.0);
            let (mut re, mut im, mut weight) = (0.0, 0.0, 0.0);
            for (i, &sample) in samples.iter().enumerate() {
                let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / samples.len() as f64).cos();
                let phase = std::f64::consts::TAU * hz * i as f64 / SAMPLE_RATE as f64;
                re += sample as f64 * w * phase.cos();
                im += sample as f64 * w * phase.sin();
                weight += w;
            }
            2.0 * (re * re + im * im).sqrt() / weight
        })
        .fold(0.0, f64::max)
}

#[test]
fn a_four_note_vocal_chord_contains_every_requested_note() {
    let input = tone(200.0, 0.85, &[(1, 0.2)]);
    let notes = [220.0_f32, 246.94165, 261.62558, 293.66476];
    let mut settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    settings.midi_frequencies[..4].copy_from_slice(&notes);
    let result = render(&input, &[], &vocal_frame::default_config(), &settings);
    for note in notes {
        let amplitude = component(steady(&result.audio), note as f64);
        assert!(
            amplitude > 0.015,
            "missing chord note {note} Hz: amplitude {amplitude}"
        );
    }
}

#[test]
fn a_unison_held_through_silence_does_not_cancel_the_vocal() {
    let mut input = vec![0.0; (SAMPLE_RATE * 0.25) as usize];
    input.extend(tone(220.0, 0.7, &[(1, 0.2)]));
    let mut settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    let config = vocal_frame::default_config();
    let dry = render(&input, &[], &config, &settings);
    settings.midi_frequencies[0] = 220.0;
    let unison = render(&input, &[], &config, &settings);
    let range = (SAMPLE_RATE * 0.5) as usize..(SAMPLE_RATE * 0.85) as usize;
    let ratio =
        component(&unison.audio[range.clone()], 220.0) / component(&dry.audio[range], 220.0);
    assert!(
        ratio > 0.85,
        "unison cancels the lead: amplitude ratio {ratio}"
    );
}

#[test]
fn midi_only_harmony_is_silent_without_notes() {
    let input = tone(200.0, 0.5, &[(1, 0.2)]);
    let mut config = vocal_frame::default_config();
    config.harmony_lead_gain = 0.0;
    let settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    let output = render(&input, &[], &config, &settings);
    assert!(output.audio.iter().all(|x| x.abs() < 1e-7));
}

#[test]
fn chord_note_order_does_not_change_the_audio() {
    let input = tone(200.0, 0.6, &[(1, 0.2), (2, 0.05)]);
    let mut settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    settings.midi_frequencies = [
        220.0, 246.94165, 261.62558, 293.66476, 329.62756, 349.22824, 391.99542, 440.0,
    ];
    let config = vocal_frame::default_config();
    let forward = render(&input, &[], &config, &settings);
    settings.midi_frequencies.reverse();
    let reverse = render(&input, &[], &config, &settings);
    assert_eq!(forward.audio, reverse.audio);
    assert_eq!(audio_lab::metrics(&forward.audio).non_finite, 0);
    for note in settings.midi_frequencies {
        assert!(
            component(steady(&forward.audio), note as f64) > 0.007,
            "missing {note}"
        );
    }
}
