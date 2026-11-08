use audio_lab::{constants::SAMPLE_RATE, metrics, voice_generator::VoiceManager};

#[test]
fn settings_quiet_interval_survives_skipped_ui_ticks_and_counter_wrap() {
    use audio_lab::settings_storage::SaveDebounce;
    let mut save = SaveDebounce::new(100);
    assert!(!save.ready(1000));
    save.changed(7);
    assert!(!save.ready(106));
    assert!(save.ready(107)); // One delayed UI tick is enough; no 100-tick loop.
    save.changed(90);
    assert!(!save.ready(189));
    assert!(save.ready(190));
    save.submitted();
    assert!(!save.ready(200));
    let mut wrap = SaveDebounce::new(10);
    wrap.changed(u32::MAX - 5);
    assert!(!wrap.ready(3));
    assert!(wrap.ready(4));
}

#[test]
fn midi_channels_keep_their_sound_and_vocal_control_roles() {
    let mut manager = VoiceManager::<8>::new(SAMPLE_RATE);
    manager.note_on(57, 127, 0);
    assert!(manager.get_cached_frequencies()[0] > 219.0);
    assert!((0..1024).all(|_| manager.get_mixed_sample(false, 1.0, 1.0) == 0.0));
    manager.all_notes_off(None);
    manager.note_on(57, 127, 1);
    assert_eq!(manager.get_cached_frequencies(), [0.0; 8]);
    let sound: Vec<_> = (0..1024)
        .map(|_| manager.get_mixed_sample(true, 1.0, 1.0))
        .collect();
    assert!(
        metrics(&sound).rms > 0.1,
        "channel 2 remains audible in vocal modes"
    );
    manager.all_notes_off(None);
    manager.note_on(57, 127, 2);
    assert!(manager.get_cached_frequencies()[0] > 219.0);
    assert!((0..1024).all(|_| manager.get_mixed_sample(true, 1.0, 1.0) == 0.0));
    assert!((0..1024).any(|_| manager.get_mixed_sample(false, 1.0, 1.0) != 0.0));
}

#[test]
fn all_held_phone_sounds_stop_after_note_off_and_do_not_retune_vocals() {
    for note in 82..=105 {
        let mut manager = VoiceManager::<8>::new(SAMPLE_RATE);
        manager.note_on(note, 127, 9);
        assert_eq!(manager.get_cached_frequencies(), [0.0; 8]);
        let start: Vec<_> = (0..4800)
            .map(|_| manager.get_mixed_sample(false, 1.0, 1.0))
            .collect();
        assert!(metrics(&start).rms > 0.0001, "held sound {note} must start");
        manager.note_off(note, 9);
        for _ in 0..48000 {
            assert!(manager.get_mixed_sample(false, 1.0, 1.0).is_finite());
        }
        assert!(
            (0..1024).all(|_| manager.get_mixed_sample(false, 1.0, 1.0) == 0.0),
            "held sound {note} is stuck"
        );
    }
}

#[test]
fn all_standard_drums_produce_finite_audio() {
    for note in 35..=81 {
        let mut manager = VoiceManager::<8>::new(SAMPLE_RATE);
        manager.note_on(note, 100, 9);
        let samples: Vec<_> = (0..24000)
            .map(|_| manager.get_mixed_sample(false, 1.0, 1.0))
            .collect();
        let m = metrics(&samples);
        assert_eq!(m.non_finite, 0, "drum {note}");
        assert!(m.rms > 0.0001, "drum {note} is silent");
    }
}
