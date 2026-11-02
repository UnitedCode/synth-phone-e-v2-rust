use audio_lab::{
    levels::{compare, db_ratio, WAVES},
    metrics, output_mix, steady,
};
use synthphone_e_vocal_dsp::audio::Waveform;

#[test]
fn hot_input_can_be_trimmed_without_turning_down_keypad_or_midi() {
    let normal = compare(Waveform::Triangle, 127, 0.2, 1.0);
    let hot = audio_lab::levels::compare_with_levels(Waveform::Triangle, 127, 0.4, 1.0, 5, 10);
    let reference = metrics(steady(&normal.vocal)).rms;
    assert!(db_ratio(metrics(steady(&hot.vocal)).rms, reference).abs() < 0.1);
    for audio in [&hot.keypad, &hot.midi] {
        assert!(db_ratio(metrics(steady(audio)).rms, reference).abs() < 1.0);
    }
    let voice_muted =
        audio_lab::levels::compare_with_levels(Waveform::Triangle, 127, 0.4, 1.0, 0, 10);
    assert_eq!(metrics(steady(&voice_muted.vocal)).rms, 0.0);
    assert!(metrics(steady(&voice_muted.keypad)).rms > 0.15);
    assert!(metrics(steady(&voice_muted.midi)).rms > 0.15);
    let synth_muted =
        audio_lab::levels::compare_with_levels(Waveform::Triangle, 127, 0.4, 1.0, 5, 0);
    assert!(db_ratio(metrics(steady(&synth_muted.vocal)).rms, reference).abs() < 0.1);
    assert_eq!(metrics(steady(&synth_muted.keypad)).rms, 0.0);
    assert_eq!(metrics(steady(&synth_muted.midi)).rms, 0.0);
}

#[test]
fn voice_control_only_midi_channel_never_becomes_audible() {
    let mut manager =
        audio_lab::voice_generator::VoiceManager::<8>::new(audio_lab::constants::SAMPLE_RATE);
    manager.note_on(57, 127, 0);
    assert!(manager.get_cached_frequencies()[0] > 219.0);
    for _ in 0..1024 {
        assert_eq!(manager.get_mixed_sources(), (0.0, 0.0));
    }
    manager.note_off(57, 0);
    assert_eq!(manager.get_cached_frequencies(), [0.0; 8]);
}

#[test]
fn generated_sources_match_nominal_autotune_within_one_db_for_each_waveform() {
    // Independent acceptance target: compare PCM power against the actual
    // pitch-corrected 0.2-peak input, not the implementation's gain constants.
    for (name, wave) in WAVES {
        let c = compare(wave, 127, 0.2, 1.0);
        let reference = metrics(steady(&c.vocal)).rms;
        for (source, audio) in [("keypad", c.keypad), ("MIDI", c.midi)] {
            let m = metrics(steady(&audio));
            let error = db_ratio(m.rms, reference);
            assert!(
                error.abs() < 1.0,
                "{name} {source}: {error:.3}dB from autotune"
            );
            assert_eq!(m.non_finite, 0);
            assert_eq!(m.over_full_scale, 0);
            assert!(m.peak < 0.4, "nominal source must retain mixing headroom");
        }
    }
}

#[test]
fn maximum_master_and_combined_sources_cannot_exceed_the_output_ceiling() {
    for (_, wave) in WAVES {
        for master in [0.0, 0.5, 1.0, 2.0] {
            for sign in [-1.0, 1.0] {
                let sample = output_mix::mix(sign * 2.0, sign * 2.8, sign, wave, master);
                assert!(
                    sample.is_finite() && sample.abs() <= 0.800_001,
                    "master={master}: {sample}"
                );
                if master == 0.0 {
                    assert_eq!(sample, 0.0);
                }
            }
        }
    }
}

#[test]
fn midi_velocity_and_master_remain_dynamic_and_percussion_keeps_its_gain() {
    let loud = compare(Waveform::Triangle, 127, 0.2, 1.0);
    let quiet = compare(Waveform::Triangle, 64, 0.2, 0.5);
    let ratio = metrics(steady(&quiet.midi)).rms / metrics(steady(&loud.midi)).rms;
    assert!((ratio - 64.0 / 254.0).abs() < 0.00001);
    for (wave, expected) in [
        (Waveform::Triangle, 0.06),
        (Waveform::Sine, 0.07),
        (Waveform::Square, 0.05),
        (Waveform::Saw, 0.05),
    ] {
        assert!((output_mix::mix(0.0, 0.0, 0.5, wave, 1.0) - expected).abs() < 0.000001);
    }
}
