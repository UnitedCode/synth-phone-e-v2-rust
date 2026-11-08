use audio_lab::{constants::SAMPLE_RATE, voice_generator::VoiceManager};

#[test]
fn vocal_retrigger_after_a_free_slot_has_one_release() {
    let mut m = VoiceManager::<8>::new(SAMPLE_RATE);
    m.note_on(60, 100, 0);
    m.note_on(64, 100, 0);
    m.note_off(60, 0);
    m.note_on(64, 100, 0);
    m.note_off(64, 0);
    assert_eq!(m.get_cached_frequencies(), [0.0; 8]);
}

#[test]
fn bending_sound_only_notes_never_retunes_the_vocal() {
    let mut m = VoiceManager::<8>::new(SAMPLE_RATE);
    m.note_on(60, 100, 1);
    m.note_on(38, 100, 9);
    m.apply_pitch_bend(0.5);
    assert_eq!(m.get_cached_frequencies(), [0.0; 8]);
}

#[test]
fn stealing_an_auto_voice_for_sound_only_clears_vocal_pitch() {
    let mut m = VoiceManager::<1>::new(SAMPLE_RATE);
    m.note_on(60, 100, 2);
    m.note_on(64, 100, 1);
    assert_eq!(m.get_cached_frequencies(), [0.0]);
}

#[test]
fn sustain_retains_released_keys_and_pedal_up_preserves_held_keys() {
    for channel in [0, 2, 3] {
        let mut m = VoiceManager::<8>::new(SAMPLE_RATE);
        m.note_on(60, 100, channel);
        m.note_on(64, 100, channel);
        m.set_sustain(channel, true);
        m.note_off(60, channel);
        assert_eq!(
            m.get_cached_frequencies()
                .iter()
                .filter(|&&f| f > 0.0)
                .count(),
            2
        );
        m.set_sustain(channel, false);
        assert!((m.get_cached_frequencies()[0] - 329.62756).abs() < 0.01);
        assert_eq!(
            m.get_cached_frequencies()
                .iter()
                .filter(|&&f| f > 0.0)
                .count(),
            1
        );
        m.note_off(64, channel);
        assert_eq!(m.get_cached_frequencies(), [0.0; 8]);
    }
}

#[test]
fn pitch_bend_is_channel_scoped_and_applies_to_new_notes() {
    let mut m = VoiceManager::<8>::new(SAMPLE_RATE);
    m.note_on(69, 100, 2);
    m.apply_channel_pitch_bend(0, 1.0);
    m.note_on(69, 100, 0);
    let f = m.get_cached_frequencies();
    assert!((f[0] - 440.0).abs() < 0.01);
    assert!((f[1] - 493.8833).abs() < 0.01);
    m.apply_channel_pitch_bend(0, -1.0);
    assert!((m.get_cached_frequencies()[1] - 391.9954).abs() < 0.01);
}

#[test]
fn all_notes_off_respects_sustain_but_sound_off_is_immediate() {
    for channel in [0, 2, 3] {
        let mut m = VoiceManager::<8>::new(SAMPLE_RATE);
        m.set_sustain(channel, true);
        m.note_on(60, 100, channel);
        m.release_channel_notes(channel);
        assert!(m.get_cached_frequencies()[0] > 0.0);
        m.all_notes_off(Some(channel));
        assert_eq!(m.get_cached_frequencies(), [0.0; 8]);
        m.set_sustain(channel, false);
        assert_eq!(m.get_cached_frequencies(), [0.0; 8]);
    }
}

#[test]
fn explicit_lower_lead_has_priority_and_no_raw_synth_or_channel_leak() {
    let mut m = VoiceManager::<8>::new(SAMPLE_RATE);
    m.note_on(57, 100, 3); // A3 lead below the backing E4.
    m.note_on(64, 100, 0);
    let (freq, gains) = m.get_harmony_voices(64);
    assert!((freq[0] - 220.0).abs() < 0.01);
    assert!((freq[1] - 329.62756).abs() < 0.01);
    assert!((gains[0] / gains[1] - 127.0 / 64.0).abs() < 0.001);
    assert_eq!(m.get_mixed_sources(), (0.0, 0.0));
    m.apply_channel_pitch_bend(3, 1.0);
    assert!((m.get_cached_frequencies()[0] - 246.94165).abs() < 0.01);
    assert!((m.get_cached_frequencies()[1] - 329.62756).abs() < 0.01);
    m.set_sustain(3, true);
    m.note_off(57, 3);
    m.all_notes_off(Some(0));
    assert!(m.get_cached_frequencies()[0] > 0.0);
    m.set_sustain(3, false);
    assert_eq!(m.get_cached_frequencies(), [0.0; 8]);
}

#[test]
fn lead_unison_is_not_doubled_and_backing_does_not_jump_during_lead_rests() {
    let mut m = VoiceManager::<8>::new(SAMPLE_RATE);
    m.note_on(60, 100, 0);
    m.note_on(60, 100, 3);
    assert_eq!(
        m.get_cached_frequencies()
            .iter()
            .filter(|&&f| f > 0.0)
            .count(),
        1
    );
    m.note_off(60, 3);
    assert!(m.get_cached_frequencies()[0] > 0.0);
    m.all_notes_off(None);
    m.note_on(57, 100, 3);
    m.note_on(64, 100, 0);
    m.note_on(67, 100, 0);
    let (freq, gain) = m.get_harmony_voices(64);
    let index = freq
        .iter()
        .position(|&f| (f - 329.62756).abs() < 0.01)
        .unwrap();
    let before = gain[index] / 3.0;
    m.note_off(57, 3);
    let (freq, gain) = m.get_harmony_voices(64);
    let index = freq
        .iter()
        .position(|&f| (f - 329.62756).abs() < 0.01)
        .unwrap();
    assert!((gain[index] / 2.0 - before).abs() < 1e-6);
    assert!(m.get_harmony_voices(0).1.iter().all(|&g| g == 0.0));
}

#[test]
fn lead_survives_full_backing_capacity_and_legacy_files_keep_unity_gains() {
    let mut m = VoiceManager::<8>::new(SAMPLE_RATE);
    for key in 60..68 {
        m.note_on(key, 100, 0);
    }
    assert_eq!(m.get_harmony_voices(0).1, [1.0; 8]);
    m.note_on(55, 100, 3);
    for key in 70..75 {
        m.note_on(key, 100, 0);
    }
    assert!((m.get_cached_frequencies()[0] - 195.99771).abs() < 0.01);
    m.all_notes_off(None);
    m.note_on(60, 100, 0);
    assert_eq!(m.get_harmony_voices(0).1, [1.0; 8]);
}

#[test]
fn midi_backing_balance_is_transient_and_can_activate_during_a_lead_rest() {
    let mut state = audio_lab::state_machine::AppStateMachine::new();
    let saved = state.saved_settings();
    assert_eq!(state.snapshot().harmony_backing, 64);
    state.set_harmony_backing_from_midi(32);
    assert_eq!(state.snapshot().harmony_backing, 32);
    assert_eq!(state.saved_settings(), saved);
    let mut m = VoiceManager::<8>::new(SAMPLE_RATE);
    m.enable_midi_lead_routing();
    m.note_on(64, 100, 0);
    let resting = m.get_harmony_voices(32).1[0];
    m.note_on(57, 100, 3);
    let (freq, gain) = m.get_harmony_voices(32);
    let i = freq
        .iter()
        .position(|&f| (f - 329.62756).abs() < 0.01)
        .unwrap();
    assert!((gain[i] / 2.0 - resting).abs() < 1e-6);
}

#[test]
fn role_gains_stay_inside_the_existing_polyphonic_headroom_budget() {
    for leads in 0..=3 {
        for backs in 0..=8 - leads {
            let mut m = VoiceManager::<8>::new(SAMPLE_RATE);
            m.enable_midi_lead_routing();
            for key in 48..48 + leads {
                m.note_on(key, 100, 3);
            }
            for key in 60..60 + backs {
                m.note_on(key, 100, 0);
            }
            for level in [0, 32, 64, 127] {
                let (frequencies, gains) = m.get_harmony_voices(level);
                let count = frequencies.iter().filter(|&&f| f > 0.0).count();
                assert_eq!(count, (leads + backs) as usize);
                assert!(gains
                    .iter()
                    .all(|g| g.is_finite() && *g >= 0.0 && *g <= 8.0));
                assert!(gains.iter().sum::<f32>() <= count as f32 + 1e-5);
            }
        }
    }
}

#[test]
fn duplicate_pitches_across_vocal_channels_do_not_displace_distinct_notes() {
    let mut m = VoiceManager::<8>::new(SAMPLE_RATE);
    for note in 60..64 {
        m.note_on(note, 100, 2);
    }
    for note in 60..68 {
        m.note_on(note, 100, 0);
    }
    let f = m.get_cached_frequencies();
    assert!(f.iter().all(|&x| x > 0.0));
    for (i, x) in f.iter().enumerate() {
        assert!(!f[..i].contains(x));
    }
}
