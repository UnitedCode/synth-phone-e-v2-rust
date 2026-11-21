use audio_lab::{
    estimate_pitch, metrics, render,
    state_machine::{AppEvent, AppStateMachine, MENU_ITEMS},
    steady, tone, vocal_frame,
};
use synthphone_e_vocal_dsp::{MusicalSettings, ProcessingMode};

fn config(app: &AppStateMachine) -> synthphone_e_vocal_dsp::VocalEffectsConfig {
    let snapshot = app.snapshot();
    vocal_frame::config_for_menu(snapshot.autotune_speed, snapshot.magnitude)
}

#[test]
fn default_magnitude_preserves_existing_processing_and_speed() {
    for speed in 1..=10 {
        assert_eq!(
            vocal_frame::config_for_menu(speed, 5),
            vocal_frame::config_for_speed(speed)
        );
    }
}

#[test]
fn magnitude_range_is_monotonic_bounded_and_independent_of_speed() {
    let mut previous = 0.0;
    for magnitude in 1..=10 {
        let config = vocal_frame::config_for_menu(10, magnitude);
        assert!(config.magnitude_threshold > previous);
        assert_eq!(config.transition_speed, 1.0);
        assert_eq!(config.sample_rate, audio_lab::constants::SAMPLE_RATE);
        previous = config.magnitude_threshold;
    }
    assert_eq!(
        vocal_frame::config_for_menu(10, i8::MIN),
        vocal_frame::config_for_menu(10, 1)
    );
    assert_eq!(
        vocal_frame::config_for_menu(10, i8::MAX),
        vocal_frame::config_for_menu(10, 10)
    );
}

#[test]
fn menu_midi_and_saved_magnitude_values_reach_the_live_configuration() {
    let mut app = AppStateMachine::new();
    app.handle_event(AppEvent::SplashComplete);
    app.handle_event(AppEvent::EncoderDoublePress);
    for _ in 0..MENU_ITEMS.len() {
        if app.current().current_item.0 == "Magnitude" {
            break;
        }
        app.handle_event(AppEvent::EncoderRotate(1));
    }
    assert_eq!(app.current().current_item, ("Magnitude", 5));
    app.handle_event(AppEvent::EncoderPress);
    app.handle_event(AppEvent::EncoderRotate(1));
    assert_eq!(app.snapshot().magnitude, 6);
    assert!(config(&app).magnitude_threshold > vocal_frame::default_config().magnitude_threshold);
    assert!(app.apply_midi_control_change(42, 127));
    assert_eq!(app.current().current_item, ("Magnitude", 10));
    let mut restored = AppStateMachine::new();
    restored.apply_saved_settings(app.saved_settings());
    assert_eq!(config(&restored), config(&app));
    assert!(app.apply_midi_control_change(42, 0));
    assert_eq!(app.snapshot().magnitude, 1);
    assert!(config(&app).magnitude_threshold < config(&restored).magnitude_threshold);
}

#[test]
fn magnitude_changes_real_quiet_audio_while_preserving_audible_voiced_input() {
    let quiet = tone(224.5, 0.7, &[(1, 0.0001)]);
    let normal = tone(224.5, 0.7, &[(1, 0.25)]);
    for settings in [
        MusicalSettings::default(),
        MusicalSettings {
            mode: ProcessingMode::Harmony,
            midi_frequencies: [220.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ..Default::default()
        },
    ] {
        // Isolate generated harmony so the unchanged original voice cannot
        // mask whether spectral rejection has an effect.
        let threshold = |magnitude| {
            let mut config = vocal_frame::config_for_menu(10, magnitude);
            config.harmony_lead_gain = 0.0;
            config
        };
        let sensitive = render(&quiet, &[], &threshold(1), &settings);
        let rejecting = render(&quiet, &[], &threshold(10), &settings);
        let low = metrics(steady(&sensitive.audio));
        let high = metrics(steady(&rejecting.audio));
        assert!(low.rms > 1e-5);
        assert!(
            high.rms < low.rms * 0.01,
            "quiet input should be rejected at the high threshold"
        );
        assert_eq!(low.non_finite + high.non_finite, 0);
        for magnitude in [1, 5, 10] {
            let output = render(&normal, &[], &threshold(magnitude), &settings);
            let samples = steady(&output.audio);
            assert!(audio_lab::cents(estimate_pitch(samples).unwrap(), 220.0).abs() < 20.0);
            assert_eq!(metrics(&output.raw).non_finite, 0);
        }
    }
}
