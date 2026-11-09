use audio_lab::{
    constants::SAMPLE_RATE,
    state_machine::{AppEvent, AppStateMachine},
    voice_generator::VoiceManager,
};

fn phone() -> AppStateMachine {
    let mut app = AppStateMachine::new();
    app.handle_event(AppEvent::SplashComplete);
    app.handle_event(AppEvent::EncoderPress);
    app
}

fn sync<const N: usize>(app: &mut AppStateMachine, voices: &VoiceManager<N>) -> bool {
    app.set_midi_note_for_display(voices.display_note())
}

#[test]
fn every_midi_note_displays_its_absolute_pitch_in_every_key() {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let mut app = phone();
    let mut voices = VoiceManager::<8>::new(SAMPLE_RATE);
    app.handle_event(AppEvent::KeypadPress(6));
    for key in 0..24 {
        assert_eq!(app.snapshot().key, key);
        let saved = app.saved_settings();
        for note in 0..128 {
            voices.note_on(note, 100, 2);
            assert_eq!(voices.display_note(), Some(note));
            assert!(sync(&mut app, &voices));
            assert_eq!(
                app.snapshot().display_note_name(),
                NAMES[note as usize % 12]
            );
            assert_eq!(
                app.snapshot().note,
                6,
                "MIDI must not change the keypad DSP input"
            );
            voices.note_off(note, 2);
            assert!(sync(&mut app, &voices));
            assert_eq!(app.snapshot().midi_pitch_class, None);
        }
        assert_eq!(
            app.saved_settings(),
            saved,
            "performance notes must not persist"
        );
        app.cycle_key_from_midi(127);
    }
}

#[test]
fn newest_pitched_note_wins_across_audio_and_vocal_control_channels() {
    let mut voices = VoiceManager::<8>::new(SAMPLE_RATE);
    let notes = [(1, 60), (0, 64), (2, 67), (3, 69), (15, 71)];
    for (channel, note) in notes {
        voices.note_on(note, 100, channel);
        assert_eq!(voices.display_note(), Some(note));
    }
    // An unmatched release, including the same key on another channel, is inert.
    voices.note_off(71, 2);
    voices.note_off(50, 15);
    assert_eq!(voices.display_note(), Some(71));
    for index in (0..notes.len()).rev() {
        let (channel, note) = notes[index];
        voices.note_off(note, channel);
        assert_eq!(
            voices.display_note(),
            index.checked_sub(1).map(|i| notes[i].1)
        );
    }
}

#[test]
fn retrigger_updates_recency_and_velocity_zero_restores_the_previous_note() {
    for channel in [0, 1, 2, 3] {
        let mut voices = VoiceManager::<8>::new(SAMPLE_RATE);
        for note in [60, 64, 67, 60] {
            voices.note_on(note, 100, channel);
            assert_eq!(voices.display_note(), Some(note));
        }
        voices.note_on(60, 0, channel);
        assert_eq!(voices.display_note(), Some(67));
        voices.note_off(64, channel);
        assert_eq!(voices.display_note(), Some(67));
        voices.note_off(67, channel);
        assert_eq!(voices.display_note(), None);
    }
}

#[test]
fn sustain_keeps_the_note_visible_until_the_pedal_releases() {
    for channel in [0, 1, 2, 3, 15] {
        let mut voices = VoiceManager::<8>::new(SAMPLE_RATE);
        let mut app = phone();
        voices.note_on(60, 100, channel);
        voices.note_on(64, 100, channel);
        assert!(sync(&mut app, &voices));
        voices.set_sustain(channel, true);
        voices.note_off(64, channel);
        assert!(!sync(&mut app, &voices));
        assert_eq!(app.snapshot().display_note_name(), "E");
        voices.note_off(60, channel);
        assert_eq!(voices.display_note(), Some(64));
        voices.set_sustain(channel, false);
        assert!(sync(&mut app, &voices));
        assert_eq!(app.snapshot().display_note_name(), "");
    }
}

#[test]
fn channel_panic_all_notes_off_and_controller_reset_follow_actual_voices() {
    for channel in [0, 1, 2, 3] {
        let mut voices = VoiceManager::<8>::new(SAMPLE_RATE);
        voices.note_on(60, 100, 15);
        voices.set_sustain(channel, true);
        voices.note_on(64, 100, channel);
        voices.release_channel_notes(channel); // CC123 respects sustain.
        assert_eq!(voices.display_note(), Some(64));
        voices.set_sustain(channel, false); // CC121 releases the pedal.
        voices.apply_channel_pitch_bend(channel, 0.0);
        assert_eq!(voices.display_note(), Some(60));
        voices.set_sustain(channel, true);
        voices.note_on(67, 100, channel);
        voices.all_notes_off(Some(channel)); // CC120 is immediate.
        assert_eq!(voices.display_note(), Some(60));
        voices.all_notes_off(None);
        assert_eq!(voices.display_note(), None);
    }
}

#[test]
fn emergency_queue_reset_clears_the_display_and_restores_the_keypad() {
    let mut voices = VoiceManager::<8>::new(SAMPLE_RATE);
    let mut app = phone();
    app.handle_event(AppEvent::KeypadPress(6));
    voices.set_sustain(3, true);
    voices.note_on(61, 100, 3);
    voices.note_off(61, 3);
    sync(&mut app, &voices);
    assert_eq!(app.snapshot().display_note_name(), "C#");
    voices.reset_midi_performance();
    assert!(sync(&mut app, &voices));
    assert_eq!(app.snapshot().display_note_name(), "A");
    assert_eq!(app.snapshot().note, 6);
}

#[test]
fn stolen_and_unaccepted_notes_never_reappear_on_the_display() {
    let mut voices = VoiceManager::<2>::new(SAMPLE_RATE);
    for note in [60, 64, 67] {
        voices.note_on(note, 100, 2);
    }
    assert_eq!(voices.display_note(), Some(67));
    voices.note_off(67, 2);
    assert_eq!(voices.display_note(), Some(64));
    voices.note_off(64, 2);
    assert_eq!(voices.display_note(), None, "stolen C must not reappear");
    // A full designated-lead bank rejects backing notes rather than stealing it.
    voices.note_on(55, 100, 3);
    voices.note_on(57, 100, 3);
    voices.note_on(60, 100, 0);
    assert_eq!(voices.display_note(), Some(57));
    voices.note_on(59, 100, 3);
    voices.note_off(59, 3);
    assert_eq!(voices.display_note(), Some(57));
    voices.note_off(57, 3);
    assert_eq!(voices.display_note(), None);
}

#[test]
fn drums_and_phone_sounds_never_replace_a_pitched_label() {
    let mut voices = VoiceManager::<8>::new(SAMPLE_RATE);
    voices.note_on(60, 100, 1);
    for note in [36, 42, 82, 84, 94, 98, 127] {
        voices.note_on(note, 100, 9);
        assert_eq!(voices.display_note(), Some(60));
    }
    voices.note_off(60, 1);
    assert_eq!(voices.display_note(), None);
    // If a drum steals the final synth voice, its old label must disappear too.
    let mut voices = VoiceManager::<1>::new(SAMPLE_RATE);
    voices.note_on(60, 100, 2);
    voices.note_on(36, 100, 9);
    assert_eq!(voices.display_note(), None);
}

#[test]
fn midi_and_keypad_holds_remain_independent_across_screen_changes() {
    let mut voices = VoiceManager::<8>::new(SAMPLE_RATE);
    let mut app = phone();
    app.handle_event(AppEvent::KeypadPress(1));
    app.handle_event(AppEvent::KeypadPress(6));
    voices.note_on(61, 100, 2);
    sync(&mut app, &voices);
    app.handle_event(AppEvent::EncoderPress);
    app.handle_event(AppEvent::KeypadRelease(6));
    assert_eq!(app.snapshot().note, 1);
    assert_eq!(app.snapshot().display_note_name(), "C#");
    app.handle_event(AppEvent::EncoderDoublePress);
    voices.note_off(61, 2);
    assert!(sync(&mut app, &voices));
    assert_eq!(app.snapshot().display_note_name(), "C");
    app.handle_event(AppEvent::KeypadRelease(1));
    assert_eq!(app.snapshot().display_note_name(), "");
}

#[test]
fn octave_retrigger_and_pitch_bend_do_not_cause_redundant_label_updates() {
    let mut voices = VoiceManager::<8>::new(SAMPLE_RATE);
    let mut app = phone();
    assert!(!sync(&mut app, &voices));
    voices.note_on(60, 100, 2);
    assert!(sync(&mut app, &voices));
    voices.note_on(72, 100, 2);
    assert!(!sync(&mut app, &voices));
    voices.apply_channel_pitch_bend(2, 1.0);
    assert!(!sync(&mut app, &voices));
    assert_eq!(app.snapshot().display_note_name(), "C");
    voices.note_off(72, 2);
    assert!(!sync(&mut app, &voices));
    voices.note_off(60, 2);
    assert!(sync(&mut app, &voices));
}

#[test]
fn long_retrigger_sessions_preserve_older_held_notes_in_both_banks() {
    let mut voices = VoiceManager::<8>::new(SAMPLE_RATE);
    voices.note_on(60, 100, 2);
    voices.note_on(64, 100, 0);
    voices.note_on(67, 100, 2);
    voices.note_on(69, 100, 3);
    for _ in 0..70_000 {
        voices.note_on(67, 100, 2);
        voices.note_on(69, 100, 3);
    }
    for (note, channel, expected) in [
        (69, 3, Some(67)),
        (67, 2, Some(64)),
        (64, 0, Some(60)),
        (60, 2, None),
    ] {
        voices.note_off(note, channel);
        assert_eq!(voices.display_note(), expected);
    }
}
