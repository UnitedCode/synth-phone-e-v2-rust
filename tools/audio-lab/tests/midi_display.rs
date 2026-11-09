use audio_lab::state_machine::{AppEvent, AppStateMachine, MENU_ITEMS};

fn phone() -> AppStateMachine {
    let mut app = AppStateMachine::new();
    app.handle_event(AppEvent::SplashComplete);
    app
}

#[test]
fn midi_volume_requests_a_redraw_with_the_new_display_value() {
    let mut app = phone();
    assert!(app.apply_midi_control_change(7, 0));
    assert_eq!(app.snapshot().volume, 0);
    assert!(app.apply_midi_control_change(7, 127));
    assert_eq!(app.snapshot().volume, 10);
}

#[test]
fn midi_menu_controls_update_the_open_menu_and_request_a_redraw() {
    for (cc, name, low, high) in [
        (34, "Pitch Low", -12, 0),
        (35, "Pitch High", 0, 12),
        (36, "Bit Rate 1", 4, 32),
        (37, "Bit Rate 2", 4, 32),
        (38, "Sample Rate 1", 1, 32),
        (39, "Sample Rate 2", 1, 32),
        (40, "Formant Male", 1, 10),
        (41, "Formant Female", 1, 10),
        (42, "Magnitude", 1, 10),
        (44, "Voice Level", 0, 20),
        (45, "Synth Level", 0, 20),
        (46, "Autotune Speed", 1, 10),
    ] {
        let mut app = phone();
        app.handle_event(AppEvent::EncoderDoublePress);
        for _ in 0..MENU_ITEMS.len() {
            if app.current().current_item.0 == name {
                break;
            }
            app.handle_event(AppEvent::EncoderRotate(1));
        }
        assert_eq!(app.current().current_item.0, name);
        // MIDI must refresh even while the encoder has this value open for editing.
        app.handle_event(AppEvent::EncoderPress);
        for (value, expected) in [(0, low), (127, high)] {
            assert!(app.apply_midi_control_change(cc, value), "CC{cc}");
            assert_eq!(app.current().current_item, (name, expected));
        }
    }
}

#[test]
fn existing_key_octave_formant_and_crush_controls_still_request_a_redraw() {
    let mut app = phone();
    assert!(app.apply_midi_control_change(43, 127));
    assert_eq!(app.snapshot().key, 1);
    app.apply_midi_control_change(43, 0);
    assert_eq!(app.snapshot().key, 1, "button release must not step twice");
    assert!(app.apply_midi_control_change(80, 127));
    assert_eq!(app.snapshot().octave_preset, 2);
    assert!(app.apply_midi_control_change(81, 127));
    assert_eq!(app.snapshot().formant, 2);
    assert!(app.apply_midi_control_change(82, 127));
    assert_eq!(app.snapshot().crush, 2);
}

#[test]
fn melodic_program_changes_redraw_and_drum_program_changes_remain_ignored() {
    for channel in 0..16 {
        let mut app = phone();
        for (program, expected) in [(1, 1), (2, 2), (3, 0), (127, 1)] {
            assert_eq!(app.set_waveform_from_midi(channel, program), channel != 9);
            assert_eq!(
                app.snapshot().waveform,
                if channel == 9 { 0 } else { expected }
            );
        }
    }
}

#[test]
fn unrelated_midi_controls_do_not_request_a_redraw_or_change_the_ui() {
    let mut app = phone();
    let before = app.snapshot();
    for cc in [0, 1, 33, 47, 48, 64, 79, 83, 120, 121, 123, 127] {
        assert!(!app.apply_midi_control_change(cc, 127), "CC{cc}");
        assert_eq!(app.snapshot(), before);
    }
}
