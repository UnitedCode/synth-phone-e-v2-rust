use audio_lab::{cents, midi_notes};
use synthphone_e_vocal_dsp::{
    audio::{get_frequency, get_scale_by_key},
    dsp::calculate_pitch_shift_with_state,
    MusicalSettings,
};

// Musical expectations use equal temperament and scale intervals, independently
// of the DSP's rounded frequency tables and note-selection implementation.
const ROOTS: [i32; 24] = [
    0, 7, 2, 9, 4, 11, 6, 1, 5, 10, 3, 8, 9, 4, 11, 6, 1, 8, 2, 7, 0, 5, 10, 3,
];

fn hz(midi: i32) -> f64 {
    440.0 * 2_f64.powf((midi as f64 - 69.0) / 12.0)
}

#[test]
fn board_sample_rate_matches_production_clock_registers() {
    // Captured from the complete Rev5 ELF. Decode register fields independently
    // of the firmware's rate constant; PLL/SAI changes must refresh this evidence.
    let select = 0x0030_8012_u32;
    let dividers = 0x4c4c_98b1_u32;
    let fraction = 0x0000_19d8_u32;
    let sai = 0x0013_02c1_u32;
    let m = (select >> 20) & 63;
    let n = (dividers & 511) + 1;
    let p = ((dividers >> 9) & 127) + 1;
    let fracn = (fraction >> 3) & 8191;
    let mckdiv = ((sai >> 20) & 63).max(1);
    let oversampling = 1 + ((sai >> 26) & 1);
    let rate = 16_000_000.0 * (n as f64 + fracn as f64 / 8192.0)
        / (m * p * mckdiv * 256 * oversampling) as f64;
    assert!((audio_lab::constants::SAMPLE_RATE as f64 - rate).abs() < 0.002);
    assert!(cents(audio_lab::constants::SAMPLE_RATE as f64, rate).abs() < 0.001);
}

fn intervals(key: usize) -> [i32; 7] {
    if key < 12 {
        [0, 2, 4, 5, 7, 9, 11]
    } else {
        [0, 2, 3, 5, 7, 8, 10]
    }
}

#[test]
fn all_midi_notes_match_a440_equal_temperament() {
    assert_eq!(audio_lab::tuning::KEY_TONICS.map(|root| root as i32), ROOTS);
    for note in 0..=127 {
        assert!(cents(midi_notes::frequency(note) as f64, hz(note as i32)).abs() < 0.001);
    }
}

#[test]
fn fixed_note_correction_keeps_the_target_octave_for_high_input_notes() {
    use audio_lab::{constants::SAMPLE_RATE, estimate_pitch, render, steady, tone, vocal_frame};
    for source in [110.0, 220.0, 659.255114, 880.0] {
        let input = tone(source, 0.8, &[(1, 0.2)]);
        let settings = MusicalSettings {
            note: 6,
            ..Default::default()
        }; // A4, 440Hz.
        let result = render(&input, &[], &vocal_frame::default_config(), &settings);
        let measured = estimate_pitch(steady(&result.audio)).unwrap();
        assert!(
            cents(measured, 440.0).abs() < 20.0,
            "{source}Hz at {SAMPLE_RATE}Hz sample rate became {measured}Hz instead of A4"
        );
    }
}

#[test]
fn formant_processing_retains_the_corrected_pitch_for_bright_vocals() {
    use audio_lab::{estimate_pitch, metrics, render, steady, tone, vocal_frame};
    for (source, target, key) in [(112.246479, 110.0, 8), (224.492958, 233.081881, 5)] {
        let input = tone(source, 0.8, &[(1, 0.06), (2, 0.18), (3, 0.08), (4, 0.04)]);
        for formant in 0..=2 {
            let settings = MusicalSettings {
                key,
                formant,
                ..Default::default()
            };
            let result = render(&input, &[], &vocal_frame::default_config(), &settings);
            let measured = estimate_pitch(steady(&result.audio)).unwrap();
            assert!(
                cents(measured, target).abs() < 20.0,
                "key={key}, formant={formant}: {measured}Hz vs {target}Hz"
            );
            assert!(metrics(steady(&result.audio)).rms > 0.01);
            assert_eq!(metrics(&result.raw).non_finite, 0);
        }
    }
}

#[test]
fn keypad_scale_degrees_and_transposition_match_the_selected_key() {
    for (key, root) in ROOTS.into_iter().enumerate() {
        for note in 1..=12 {
            for (carrier, octave) in [(true, 2), (false, 4)] {
                for semitones in [-12, -7, 0, 7, 12] {
                    let degree = note as usize - 1;
                    let expected = hz(12
                        + root
                        + octave * 12
                        + intervals(key)[degree % 7]
                        + (degree / 7) as i32 * 12
                        + semitones);
                    let actual = get_frequency(
                        key as i8,
                        note,
                        2_f32.powf(semitones as f32 / 12.0),
                        carrier,
                    );
                    assert!(cents(actual as f64, expected).abs() < 0.5,
                        "key={key} note={note} carrier={carrier} shift={semitones}: {actual} vs {expected}");
                }
            }
        }
    }
}

#[test]
fn correction_selects_the_nearest_scale_note_in_cents_at_boundaries() {
    let mut config = audio_lab::vocal_frame::default_config();
    config.pitch_correction_strength = 1.0;
    let mut errors = Vec::new();
    for (key, root) in ROOTS.iter().enumerate() {
        let notes: Vec<_> = (36..=84)
            .filter(|midi| {
                intervals(key)
                    .iter()
                    .any(|step| (root + step) % 12 == midi % 12)
            })
            .collect();
        // Verify the complete scale independently before testing its selector.
        for &note in &notes {
            assert!(get_scale_by_key(key as i8)
                .iter()
                .any(|&f| cents(f as f64, hz(note)).abs() < 0.5));
        }
        for pair in notes.windows(2) {
            let middle = (hz(pair[0]) * hz(pair[1])).sqrt();
            for offset in [-1.0, 1.0] {
                let input = middle * 2_f64.powf(offset / 1200.0);
                let expected = hz(if offset < 0.0 { pair[0] } else { pair[1] });
                let settings = MusicalSettings {
                    key: key as i8,
                    ..Default::default()
                };
                let mut ratio = 1.0;
                let actual = input
                    * calculate_pitch_shift_with_state(&mut ratio, &settings, &config, input as f32)
                        as f64;
                if cents(actual, expected).abs() > 0.5 {
                    errors.push(format!("key={key} input={input:.4}Hz: selected={actual:.4}, expected={expected:.4}"));
                }
            }
        }
    }
    assert!(
        errors.is_empty(),
        "{} boundary errors; examples: {:?}",
        errors.len(),
        &errors[..errors.len().min(6)]
    );
}
