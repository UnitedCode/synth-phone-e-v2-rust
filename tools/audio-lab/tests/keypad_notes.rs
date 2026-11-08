use audio_lab::state_machine::{AppEvent, AppState, AppStateMachine, ProcessingProfile};

fn phone(profile: ProcessingProfile) -> AppStateMachine {
    let mut app = AppStateMachine::new();
    app.handle_event(AppEvent::SplashComplete);
    app.handle_event(AppEvent::SetProcessingProfile(profile));
    app
}

#[test]
fn held_note_releases_on_the_effects_screen_or_in_the_menu() {
    for profile in [
        ProcessingProfile::PitchControl,
        ProcessingProfile::Vocode,
        ProcessingProfile::Dry,
        ProcessingProfile::Tuned,
        ProcessingProfile::Harmony,
    ] {
        for navigation in [AppEvent::EncoderPress, AppEvent::EncoderDoublePress] {
            let mut app = phone(profile);
            app.handle_event(AppEvent::KeypadPress(6));
            app.handle_event(navigation);
            assert_eq!(app.snapshot().note, 6);
            app.handle_event(AppEvent::KeypadRelease(6));
            assert_eq!(app.snapshot().note, 0, "{profile:?}, {navigation:?}");
        }
    }
}

#[test]
fn releasing_an_older_key_keeps_the_newer_held_note_sounding() {
    let mut app = phone(ProcessingProfile::Dry);
    app.handle_event(AppEvent::KeypadPress(1));
    app.handle_event(AppEvent::KeypadPress(6));
    app.handle_event(AppEvent::KeypadRelease(1));
    assert_eq!(app.snapshot().note, 6);
    app.handle_event(AppEvent::KeypadRelease(6));
    assert_eq!(app.snapshot().note, 0);
}

#[test]
fn releasing_the_newest_key_restores_the_previous_held_note() {
    let mut app = phone(ProcessingProfile::Dry);
    app.handle_event(AppEvent::KeypadPress(1));
    app.handle_event(AppEvent::KeypadPress(6));
    app.handle_event(AppEvent::KeypadPress(9));
    for (release, expected) in [(9, 6), (6, 1), (1, 0)] {
        app.handle_event(AppEvent::KeypadRelease(release));
        assert_eq!(app.snapshot().note, expected);
    }
}

#[test]
fn all_twelve_keys_fit_and_releasing_in_reverse_order_restores_each_note() {
    let mut app = phone(ProcessingProfile::Dry);
    for key in 1..=12 {
        app.handle_event(AppEvent::KeypadPress(key));
    }
    for key in (1..=12).rev() {
        assert_eq!(app.snapshot().note, key as i8);
        app.handle_event(AppEvent::KeypadRelease(key));
    }
    assert_eq!(app.snapshot().note, 0);
}

#[test]
fn effect_controls_and_profile_changes_do_not_take_ownership_of_held_notes() {
    let mut app = phone(ProcessingProfile::Dry);
    app.handle_event(AppEvent::KeypadPress(6));
    app.handle_event(AppEvent::EncoderPress);
    app.handle_event(AppEvent::KeypadPress(9));
    app.handle_event(AppEvent::KeypadRelease(9));
    assert_eq!(app.snapshot().waveform, 2);
    assert_eq!(app.snapshot().note, 6);
    app.handle_event(AppEvent::KeypadPress(11));
    assert!(matches!(
        app.state(),
        AppState::EffectsProfile(ProcessingProfile::Tuned)
    ));
    app.handle_event(AppEvent::KeypadRelease(6));
    assert_eq!(app.snapshot().note, 0);
    app.handle_event(AppEvent::KeypadRelease(11));
    app.handle_event(AppEvent::EncoderPress);
    app.handle_event(AppEvent::KeypadPress(3));
    app.handle_event(AppEvent::KeypadRelease(3));
    assert_eq!(app.snapshot().note, 0, "old notes must not reappear");
}

#[test]
fn duplicate_and_unmatched_events_do_not_drop_or_latch_a_note() {
    let mut app = phone(ProcessingProfile::Dry);
    app.handle_event(AppEvent::KeypadPress(6));
    app.handle_event(AppEvent::KeypadPress(6));
    for key in [0, 1, 13, 262, usize::MAX] {
        app.handle_event(AppEvent::KeypadRelease(key));
        assert_eq!(app.snapshot().note, 6);
    }
    for key in [0, 13, 262, usize::MAX] {
        app.handle_event(AppEvent::KeypadPress(key));
        assert_eq!(app.snapshot().note, 6);
    }
    app.handle_event(AppEvent::KeypadRelease(6));
    assert_eq!(app.snapshot().note, 0);
    app.handle_event(AppEvent::KeypadRelease(6));
    assert_eq!(app.snapshot().note, 0);
}

#[test]
fn effect_button_indicators_release_even_after_entering_the_menu() {
    let mut app = phone(ProcessingProfile::Dry);
    app.handle_event(AppEvent::EncoderPress);
    app.handle_event(AppEvent::KeypadPress(10));
    assert!(app.snapshot().key_down_pressed);
    app.handle_event(AppEvent::EncoderDoublePress);
    app.handle_event(AppEvent::KeypadRelease(10));
    assert!(!app.snapshot().key_down_pressed);
}
