use audio_lab::{
    settings_storage::SavedSettings,
    state_machine::{AppEvent, AppStateMachine, MENU_ITEMS},
    vocal_frame::config_for_controls,
};

#[test]
fn amount_menu_midi_and_saved_settings_reach_the_shared_dsp_configuration() {
    let mut app = AppStateMachine::new();
    assert_eq!(app.snapshot().vocoder_amount, 5);
    app.handle_event(AppEvent::SplashComplete);
    app.handle_event(AppEvent::EncoderDoublePress);
    for _ in 0..MENU_ITEMS.len() {
        if app.current().current_item.0 == "Vocode Amount" {
            break;
        }
        app.handle_event(AppEvent::EncoderRotate(1));
    }
    assert_eq!(app.current().current_item, ("Vocode Amount", 5));
    app.handle_event(AppEvent::EncoderPress);
    app.handle_event(AppEvent::EncoderRotate(-1));
    assert_eq!(app.snapshot().vocoder_amount, 4);
    for (cc, expected) in [(0, 0), (64, 5), (127, 10)] {
        app.set_menu_value_from_cc(49, cc);
        let mut restored = AppStateMachine::new();
        restored.apply_saved_settings(app.saved_settings());
        let snapshot = restored.snapshot();
        assert_eq!(snapshot.vocoder_amount, expected);
        let config = config_for_controls(snapshot.autotune_speed, snapshot.vocoder_amount);
        assert!((config.vocoder_mix - expected as f32 / 10.0).abs() < 1e-6);
    }
}

#[test]
fn records_from_before_the_new_control_keep_all_existing_settings() {
    // Frozen record produced by the original 21-item v2 encoder, including CRC.
    let legacy = [
        83, 80, 72, 86, 2, 21, 255, 255, 9, 0, 0, 0, 14, 249, 9, 24, 12, 3, 8, 4, 6, 7, 3, 0, 255,
        8, 12, 6, 1, 1, 0, 1, 1, 1, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 151, 236, 201, 202,
    ];
    let (generation, saved) =
        SavedSettings::decode_record(&legacy).expect("existing settings must survive");
    assert_eq!(generation, 9);
    assert_eq!(saved.master, 14);
    assert_eq!(
        &saved.menu[..21],
        &[-7, 9, 24, 12, 3, 8, 4, 6, 7, 3, 0, -1, 8, 12, 6, 1, 1, 0, 1, 1, 1]
    );
    let mut app = AppStateMachine::new();
    app.apply_saved_settings(saved);
    assert_eq!(app.snapshot().vocoder_amount, 5);
    assert_eq!(app.snapshot().voice_volume, 8);
    assert_eq!(app.snapshot().synth_octave, -1);
}

#[test]
fn new_records_retain_the_amount_and_reject_corruption() {
    let mut app = AppStateMachine::new();
    app.set_menu_value_from_cc(49, 102);
    let saved = app.saved_settings();
    let mut bytes = saved.encode_record(18);
    assert_eq!(SavedSettings::decode_record(&bytes), Some((18, saved)));
    bytes[13] ^= 1;
    assert!(SavedSettings::decode_record(&bytes).is_none());
}
