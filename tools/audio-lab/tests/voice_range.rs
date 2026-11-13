use audio_lab::{
    settings_storage::SavedSettings,
    state_machine::{AppEvent, AppStateMachine, MENU_ITEMS},
    vocal_frame::config_for_voice_controls,
};

#[test]
fn voice_range_can_be_selected_with_the_encoder() {
    let mut app = AppStateMachine::new();
    app.handle_event(AppEvent::SplashComplete);
    app.handle_event(AppEvent::EncoderDoublePress);
    for _ in 0..MENU_ITEMS.len() {
        if app.current().current_item.0 == "Voice Range" {
            break;
        }
        app.handle_event(AppEvent::EncoderRotate(1));
    }
    assert_eq!(app.current().current_item, ("Voice Range", 0));
    app.handle_event(AppEvent::EncoderPress);
    app.handle_event(AppEvent::EncoderRotate(1));
    app.handle_event(AppEvent::EncoderRotate(1));
    assert_eq!(app.current().current_item, ("Voice Range", 2));
    let saved = app.saved_settings();
    let (_, decoded) = SavedSettings::decode_record(&saved.encode_record(27)).unwrap();
    let mut restored = AppStateMachine::new();
    restored.apply_saved_settings(decoded);
    assert_eq!(restored.snapshot().voice_range, 2);
    let snapshot = restored.snapshot();
    assert_eq!(
        config_for_voice_controls(
            snapshot.autotune_speed,
            snapshot.vocoder_amount,
            snapshot.voice_range
        )
        .min_frequency,
        150.0
    );
    for (cc, range, floor) in [(0, 0, 50.0), (64, 1, 100.0), (127, 2, 150.0)] {
        restored.set_menu_value_from_cc(50, cc);
        let snapshot = restored.snapshot();
        assert_eq!(snapshot.voice_range, range);
        assert_eq!(
            config_for_voice_controls(
                snapshot.autotune_speed,
                snapshot.vocoder_amount,
                snapshot.voice_range
            )
            .min_frequency,
            floor
        );
    }
}

#[test]
fn existing_22_value_records_keep_their_values_and_use_the_wide_range() {
    // Frozen output from the previous encoder, before Voice Range existed.
    let bytes = [
        83, 80, 72, 86, 2, 22, 255, 255, 18, 0, 0, 0, 10, 244, 12, 32, 10, 5, 16, 5, 5, 10, 5, 0,
        0, 10, 10, 10, 1, 1, 1, 1, 1, 1, 8, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 231, 72, 173, 106,
    ];
    let (generation, saved) =
        SavedSettings::decode_record(&bytes).expect("old presets must survive");
    assert_eq!(generation, 18);
    let mut app = AppStateMachine::new();
    app.apply_saved_settings(saved);
    assert_eq!(app.snapshot().vocoder_amount, 8);
    assert_eq!(app.snapshot().voice_range, 0);
    assert_eq!(app.snapshot().pitch_semitones, 0);
}

#[test]
fn changing_range_does_not_keep_a_cached_pitch_below_the_new_floor() {
    use audio_lab::{
        constants::{BLOCK_SIZE, FFT_SIZE, HOP_SIZE, SAMPLE_RATE},
        replay::Replay,
        tone,
    };
    use synthphone_e_vocal_dsp::{MusicalSettings, ProcessingMode};
    let input = tone(100.0, 1.0, &[(1, 0.15)]);
    let mut replay = Replay::new();
    let mut output = Vec::new();
    let mut settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    settings.midi_frequencies[0] = 350.0;
    let total = (input.len() + FFT_SIZE * 2).div_ceil(BLOCK_SIZE) * BLOCK_SIZE;
    for block in (0..total).step_by(BLOCK_SIZE) {
        for i in block..block + BLOCK_SIZE {
            output.push(replay.sample(input.get(i).copied().unwrap_or(0.0)));
        }
        let range = if block < input.len() / 2 { 0 } else { 2 };
        let mut config = config_for_voice_controls(10, 10, range);
        config.harmony_lead_gain = 0.0;
        replay.finish_block(&[0.0; HOP_SIZE], &config, &settings, None);
    }
    let before = audio_lab::estimate_pitch(
        &output[(SAMPLE_RATE * 0.2) as usize..(SAMPLE_RATE * 0.4) as usize],
    )
    .unwrap();
    let after = audio_lab::estimate_pitch(
        &output[(SAMPLE_RATE * 0.65) as usize..(SAMPLE_RATE * 0.85) as usize],
    )
    .unwrap();
    assert!(audio_lab::cents(before, 350.0).abs() < 5.0);
    assert!(
        audio_lab::cents(after, 100.0).abs() < 5.0,
        "High range retained the excluded source estimate and retuned it: {after} Hz"
    );
}
