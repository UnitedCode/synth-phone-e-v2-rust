use audio_lab::{
    cents, constants::SAMPLE_RATE, estimate_pitch, metrics, render, tone, vocal_frame,
};
use synthphone_e_vocal_dsp::MusicalSettings;

fn pitch_in(audio: &[f32], start: f64, end: f64) -> f64 {
    estimate_pitch(
        &audio[(start * SAMPLE_RATE as f64) as usize..(end * SAMPLE_RATE as f64) as usize],
    )
    .expect("voiced output must retain a measurable pitch")
}

#[test]
fn a_silent_gap_releases_the_previous_phrases_correction() {
    let mut input = tone(180.0, 1.5, &[(1, 0.2)]);
    input.resize((1.8 * SAMPLE_RATE as f64) as usize, 0.0);
    let mut reference = vec![0.0; input.len()];
    let phrase = tone(224.492958, 0.6, &[(1, 0.2)]);
    input.extend_from_slice(&phrase);
    reference.extend_from_slice(&phrase);
    let mut settings = MusicalSettings::default();
    settings.midi_frequencies[0] = 220.0;
    let config = vocal_frame::config_for_speed(1);
    let actual = render(&input, &[], &config, &settings);
    let fresh = render(&reference, &[], &config, &settings);
    let difference = cents(
        pitch_in(&actual.audio, 1.9, 2.08),
        pitch_in(&fresh.audio, 1.9, 2.08),
    )
    .abs();
    assert!(
        difference < 10.0,
        "stale correction changed the new phrase by {difference} cents"
    );
    assert!(
        metrics(
            &actual.raw[(1.65 * SAMPLE_RATE as f64) as usize..(1.75 * SAMPLE_RATE as f64) as usize]
        )
        .peak
            < 1e-6
    );
}

#[test]
fn speed_menu_cc_and_saved_settings_reach_the_shared_vocal_configuration() {
    use audio_lab::state_machine::{AppEvent, AppStateMachine, MENU_ITEMS};
    let mut app = AppStateMachine::new();
    app.handle_event(AppEvent::SplashComplete);
    app.handle_event(AppEvent::EncoderDoublePress);
    for _ in 0..MENU_ITEMS.len() {
        if app.current().current_item.0 == "Autotune Speed" {
            break;
        }
        app.handle_event(AppEvent::EncoderRotate(1));
    }
    assert_eq!(app.current().current_item, ("Autotune Speed", 10));
    app.handle_event(AppEvent::EncoderPress);
    for _ in 0..9 {
        app.handle_event(AppEvent::EncoderRotate(-1));
    }
    let menu = vocal_frame::config_for_speed(app.snapshot().autotune_speed);
    app.set_menu_value_from_cc(46, 0);
    let slow = vocal_frame::config_for_speed(app.snapshot().autotune_speed);
    assert_eq!(menu, slow);
    app.set_menu_value_from_cc(46, 127);
    let mut restored = AppStateMachine::new();
    restored.apply_saved_settings(app.saved_settings());
    let fast = vocal_frame::config_for_speed(restored.snapshot().autotune_speed);
    assert!(slow.transition_speed < 0.02);
    assert_eq!(fast.transition_speed, 1.0);
}

#[test]
fn slow_correction_glides_then_reaches_the_same_target_as_fast_correction() {
    // Warm up on-target before a 35-cent detuning. This measures a note
    // transition rather than the phase vocoder's cold-start transient.
    let mut input = tone(220.0, 0.4, &[(1, 0.2)]);
    input.extend(tone(224.492958, 1.8, &[(1, 0.2)]));
    let mut settings = MusicalSettings::default();
    settings.midi_frequencies[0] = 220.0;
    let mut fast = vocal_frame::default_config();
    fast.transition_speed = 1.0;
    let mut slow = fast;
    slow.transition_speed = 0.01;
    let fast = render(&input, &[], &fast, &settings);
    let slow = render(&input, &[], &slow, &settings);
    let early_fast = cents(pitch_in(&fast.audio, 0.48, 0.62), 220.0).abs();
    let early_slow = cents(pitch_in(&slow.audio, 0.48, 0.62), 220.0).abs();
    let settled_slow = cents(pitch_in(&slow.audio, 1.8, 2.1), 220.0).abs();
    assert!(early_fast < 20.0, "fast correction: {early_fast} cents");
    assert!(
        early_slow > 20.0,
        "slow correction must glide: {early_slow} cents"
    );
    assert!(
        settled_slow < 20.0,
        "slow correction must retain state and settle: {settled_slow} cents"
    );
}
