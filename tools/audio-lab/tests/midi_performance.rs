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
    for channel in [0, 2] {
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
    for channel in [0, 2] {
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
