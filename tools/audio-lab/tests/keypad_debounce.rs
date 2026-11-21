use audio_lab::{
    keypad::KeypadDebouncer,
    percussion::{HeldKeys, SET_RINGER},
    state_machine::{AppEvent, AppStateMachine, ProcessingProfile},
};

type Matrix = [[bool; 3]; 4];

fn scan(debounce: &mut KeypadDebouncer, raw: Matrix, time: u32) -> Vec<(usize, usize, bool)> {
    let mut events = Vec::new();
    for (row, columns) in debounce.update(raw, time).into_iter().enumerate() {
        for (col, change) in columns.into_iter().enumerate() {
            if let Some(pressed) = change {
                events.push((row, col, pressed));
            }
        }
    }
    events
}

#[test]
fn contact_chatter_produces_one_press_and_one_release() {
    let mut debounce = KeypadDebouncer::new(1000);
    let mut events = Vec::new();
    for time in 0..36 {
        let mut raw = [[false; 3]; 4];
        raw[0][2] = matches!(time, 1 | 3 | 5..=19 | 21 | 23);
        for edge in scan(&mut debounce, raw, time) {
            events.push((time, edge));
        }
    }
    assert_eq!(events, [(10, (0, 2, true)), (29, (0, 2, false))]);
}

#[test]
fn twenty_ms_taps_are_preserved_but_a_four_ms_glitch_is_ignored() {
    let mut debounce = KeypadDebouncer::new(1000);
    let mut events = Vec::new();
    for time in 0..120 {
        let mut raw = [[false; 3]; 4];
        raw[3][0] = time % 40 < 20;
        for (_, _, pressed) in scan(&mut debounce, raw, time) {
            events.push((time, pressed));
        }
    }
    assert_eq!(
        events,
        [
            (5, true),
            (25, false),
            (45, true),
            (65, false),
            (85, true),
            (105, false)
        ]
    );
    for time in 120..130 {
        let mut raw = [[false; 3]; 4];
        raw[3][0] = time < 124;
        assert!(scan(&mut debounce, raw, time).is_empty());
    }
}

#[test]
fn one_bouncing_key_does_not_delay_another_key() {
    let mut debounce = KeypadDebouncer::new(1000);
    let mut raw = [[false; 3]; 4];
    raw[0][2] = true;
    assert!(scan(&mut debounce, raw, 0).is_empty());
    raw[1][0] = true;
    assert!(scan(&mut debounce, raw, 2).is_empty());
    raw[1][0] = false;
    assert!(scan(&mut debounce, raw, 4).is_empty());
    assert_eq!(scan(&mut debounce, raw, 5), [(0, 2, true)]);
    raw[1][0] = true;
    assert!(scan(&mut debounce, raw, 6).is_empty());
    assert!(scan(&mut debounce, raw, 10).is_empty());
    assert_eq!(scan(&mut debounce, raw, 11), [(1, 0, true)]);
}

#[test]
fn every_contact_can_press_and_release_together_without_repeats() {
    let mut debounce = KeypadDebouncer::new(1000);
    assert!(scan(&mut debounce, [[true; 3]; 4], 100).is_empty());
    let presses = scan(&mut debounce, [[true; 3]; 4], 105);
    assert_eq!(presses.len(), 12);
    assert!(presses.iter().all(|&(_, _, pressed)| pressed));
    assert!(scan(&mut debounce, [[true; 3]; 4], 5000).is_empty());
    assert!(scan(&mut debounce, [[false; 3]; 4], 5001).is_empty());
    let releases = scan(&mut debounce, [[false; 3]; 4], 5006);
    assert_eq!(releases.len(), 12);
    assert!(releases.iter().all(|&(_, _, pressed)| !pressed));
}

#[test]
fn elapsed_time_handles_scan_jitter_and_counter_wrap() {
    let mut debounce = KeypadDebouncer::new(1000);
    let start = u32::MAX - 2;
    let raw = [[true; 3]; 4];
    assert!(scan(&mut debounce, raw, start).is_empty());
    assert!(scan(&mut debounce, raw, start.wrapping_add(4)).is_empty());
    assert_eq!(scan(&mut debounce, raw, start.wrapping_add(9)).len(), 12);
    assert!(scan(&mut debounce, [[false; 3]; 4], 20).is_empty());
    assert!(scan(&mut debounce, [[false; 3]; 4], 24).is_empty());
    assert_eq!(scan(&mut debounce, [[false; 3]; 4], 33).len(), 12);
}

#[test]
fn device_core_clock_requires_five_actual_ms_of_stability() {
    for clock_hz in [480_000_000, 1001] {
        let mut debounce = KeypadDebouncer::new(clock_hz);
        let ticks = (u64::from(clock_hz) * 5).div_ceil(1000) as u32;
        let raw = [[true; 3]; 4];
        assert!(scan(&mut debounce, raw, 100).is_empty());
        assert!(scan(&mut debounce, raw, 100 + ticks - 1).is_empty());
        assert_eq!(scan(&mut debounce, raw, 100 + ticks).len(), 12);
    }
}

#[test]
fn debounced_edges_preserve_overlapping_notes_and_release_after_screen_changes() {
    let mut app = AppStateMachine::new();
    app.handle_event(AppEvent::SplashComplete);
    app.handle_event(AppEvent::SetProcessingProfile(ProcessingProfile::Dry));
    let mut debounce = KeypadDebouncer::new(1000);
    let mut raw = [[false; 3]; 4];
    for (time, first, sixth, expected) in [
        (0, true, false, 0),
        (5, true, false, 1),
        (10, true, true, 1),
        (15, true, true, 6),
        (20, false, true, 6),
        (21, true, true, 6),
        (22, false, true, 6),
        (27, false, true, 6),
        (30, false, false, 6),
        (31, false, true, 6),
        (32, false, false, 6),
        (37, false, false, 0),
    ] {
        if time == 30 {
            app.handle_event(AppEvent::EncoderDoublePress);
        }
        raw[0][2] = first;
        raw[1][0] = sixth;
        for (row, col, pressed) in scan(&mut debounce, raw, time) {
            let key = match (row, col) {
                (0, 2) => 1,
                (1, 0) => 6,
                _ => panic!("unexpected contact"),
            };
            app.handle_event(if pressed {
                AppEvent::KeypadPress(key)
            } else {
                AppEvent::KeypadRelease(key)
            });
        }
        assert_eq!(app.snapshot().note, expected, "at {time} ms");
    }
}

#[test]
fn ringer_contacts_do_not_retrigger_or_release_early_during_chatter() {
    let mut debounce = KeypadDebouncer::new(1000);
    let mut held = HeldKeys::new();
    let mut notes = Vec::new();
    for time in 0..36 {
        let mut raw = [[false; 3]; 4];
        raw[0][2] = matches!(time, 1 | 3 | 5..=19 | 21 | 23);
        for (_, _, pressed) in scan(&mut debounce, raw, time) {
            let note = if pressed {
                held.press(SET_RINGER, 1)
            } else {
                held.release(1)
            };
            notes.push((time, pressed, note));
        }
    }
    assert_eq!(notes, [(10, true, Some(82)), (29, false, Some(82))]);
    assert_eq!(held.release(1), None);
}
