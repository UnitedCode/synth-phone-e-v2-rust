use audio_lab::{
    constants::SAMPLE_RATE,
    midi_events::{get_midi_queue_status, try_enqueue_midi_event, MidiEvent},
    voice_generator::VoiceManager,
};
use heapless::spsc::Queue;

fn on(key: u8) -> MidiEvent {
    MidiEvent::NoteOn {
        channel: 2,
        key,
        velocity: 100,
    }
}

fn off(key: u8) -> MidiEvent {
    MidiEvent::NoteOff {
        channel: 2,
        key,
        velocity: 0,
    }
}

fn cc(controller: u8, value: u8) -> MidiEvent {
    MidiEvent::ControlChange {
        channel: 2,
        controller,
        value,
    }
}

fn full(events: impl IntoIterator<Item = MidiEvent>) -> Queue<MidiEvent, 128> {
    let mut queue = Queue::new();
    for event in events {
        queue.enqueue(event).unwrap();
    }
    assert_eq!(get_midi_queue_status(&queue), (127, 127));
    queue
}

fn drain(queue: &mut Queue<MidiEvent, 128>, voices: &mut VoiceManager<8>) {
    while let Some(event) = queue.dequeue() {
        match event {
            MidiEvent::NoteOn {
                channel,
                key,
                velocity,
            } => voices.note_on(key, velocity, channel),
            MidiEvent::NoteOff { channel, key, .. } => voices.note_off(key, channel),
            MidiEvent::ControlChange {
                channel,
                controller: 64,
                value,
            } => voices.set_sustain(channel, value >= 64),
            MidiEvent::QueueOverflow | MidiEvent::UartError | MidiEvent::SystemReset => {
                voices.reset_midi_performance()
            }
            _ => {}
        }
    }
}

#[test]
fn ordinary_enqueue_is_fifo_and_uses_all_127_slots() {
    let expected: Vec<_> = (0..127).map(on).collect();
    let mut queue = Queue::new();
    for &event in &expected {
        try_enqueue_midi_event(&mut queue, event).unwrap();
    }
    assert_eq!(get_midi_queue_status(&queue), (127, 127));
    assert_eq!(queue.iter().copied().collect::<Vec<_>>(), expected);
}

#[test]
fn full_queue_rejection_does_not_rotate_notes_or_drop_pedal_release() {
    // The old overflow path rotated NoteOn behind NoteOff, or discarded CC64 up.
    for middle in [cc(7, 20), cc(64, 0)] {
        let mut queue = full(
            [on(60), middle, off(60)]
                .into_iter()
                .chain((0..124).map(on)),
        );
        let before: Vec<_> = queue.iter().copied().collect();
        assert_eq!(
            try_enqueue_midi_event(&mut queue, MidiEvent::Other),
            Err(MidiEvent::Other)
        );
        assert_eq!(queue.iter().copied().collect::<Vec<_>>(), before);
    }
}

#[test]
fn removing_any_slot_preserves_fifo_even_when_the_ring_wraps() {
    // Includes candidates beyond the old sixteen-event search window.
    for offset in 0..128 {
        for discard in 0..127 {
            let mut queue = Queue::<MidiEvent, 128>::new();
            for _ in 0..offset {
                queue.enqueue(on(0)).unwrap();
                queue.dequeue().unwrap();
            }
            let mut expected: Vec<_> = (0..127).map(on).collect();
            expected[discard] = cc(7, 20);
            for &event in &expected {
                queue.enqueue(event).unwrap();
            }
            try_enqueue_midi_event(&mut queue, off(60)).unwrap();
            expected.remove(discard);
            expected.push(off(60));
            assert_eq!(queue.iter().copied().collect::<Vec<_>>(), expected);
        }
    }
}

#[test]
fn overflow_eviction_cannot_leave_note_off_ahead_of_note_on() {
    let mut voices = VoiceManager::<8>::new(SAMPLE_RATE);
    let mut queue = full(
        [on(60), cc(7, 20), off(60)]
            .into_iter()
            .chain(std::iter::repeat_n(cc(7, 30), 124)),
    );
    try_enqueue_midi_event(&mut queue, cc(7, 40)).unwrap();
    drain(&mut queue, &mut voices);
    assert_eq!(voices.get_cached_frequencies(), [0.0; 8]);
    assert_eq!(voices.get_mixed_sources(), (0.0, 0.0));
}

#[test]
fn queued_sustain_and_panic_controls_survive_control_floods() {
    for protected in [
        off(60),
        MidiEvent::NoteOn {
            channel: 2,
            key: 60,
            velocity: 0,
        },
        cc(64, 0),
        cc(64, 127),
        cc(120, 0),
        cc(121, 0),
        cc(123, 0),
        MidiEvent::QueueOverflow,
        MidiEvent::UartError,
        MidiEvent::SystemReset,
    ] {
        let mut queue = full(
            [protected]
                .into_iter()
                .chain(std::iter::repeat_n(cc(7, 20), 126)),
        );
        for _ in 0..256 {
            try_enqueue_midi_event(
                &mut queue,
                MidiEvent::PitchBend {
                    channel: 2,
                    value: 8192,
                },
            )
            .unwrap();
            assert_eq!(queue.peek(), Some(&protected));
        }
    }
}

#[test]
fn sustain_release_reaches_the_voice_manager_under_overload() {
    let mut voices = VoiceManager::<8>::new(SAMPLE_RATE);
    voices.set_sustain(2, true);
    voices.note_on(60, 100, 2);
    voices.note_off(60, 2);
    assert!(voices.get_cached_frequencies().iter().any(|&f| f > 0.0));
    let mut queue = full(
        [cc(64, 0)]
            .into_iter()
            .chain(std::iter::repeat_n(cc(7, 20), 126)),
    );
    try_enqueue_midi_event(&mut queue, cc(7, 40)).unwrap();
    drain(&mut queue, &mut voices);
    assert_eq!(voices.get_cached_frequencies(), [0.0; 8]);
}

#[test]
fn incoming_releases_can_evict_new_notes_programs_and_pedal_down() {
    for incoming in [
        off(60),
        MidiEvent::NoteOn {
            channel: 2,
            key: 60,
            velocity: 0,
        },
        cc(64, 63),
        cc(120, 0),
        cc(121, 0),
        cc(123, 0),
    ] {
        for evicted in [
            on(60),
            MidiEvent::ProgramChange {
                channel: 2,
                program: 1,
            },
            cc(64, 64),
        ] {
            let mut queue = full(
                [evicted]
                    .into_iter()
                    .chain(std::iter::repeat_n(off(61), 126)),
            );
            try_enqueue_midi_event(&mut queue, incoming).unwrap();
            assert_eq!(
                queue.iter().copied().collect::<Vec<_>>(),
                std::iter::repeat_n(off(61), 126)
                    .chain([incoming])
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn low_priority_events_cannot_displace_notes_or_protected_controls() {
    for incoming in [
        on(70),
        cc(7, 90),
        cc(64, 64),
        MidiEvent::PitchBend {
            channel: 2,
            value: 8000,
        },
        MidiEvent::ProgramChange {
            channel: 2,
            program: 1,
        },
    ] {
        let mut queue = full((0..127).map(on));
        let before: Vec<_> = queue.iter().copied().collect();
        assert_eq!(try_enqueue_midi_event(&mut queue, incoming), Err(incoming));
        assert_eq!(queue.iter().copied().collect::<Vec<_>>(), before);
        let mut queue = full(std::iter::repeat_n(cc(64, 0), 127));
        assert_eq!(try_enqueue_midi_event(&mut queue, incoming), Err(incoming));
        assert!(queue.iter().all(|&event| event == cc(64, 0)));
    }
}

#[test]
fn all_release_saturation_places_reset_before_subsequent_events() {
    let releases = [
        off(60),
        MidiEvent::NoteOn {
            channel: 2,
            key: 60,
            velocity: 0,
        },
        cc(64, 0),
        cc(120, 0),
        cc(121, 0),
        cc(123, 0),
        MidiEvent::QueueOverflow,
    ];
    let mut queue = full(releases.into_iter().cycle().take(127));
    try_enqueue_midi_event(&mut queue, off(61)).unwrap();
    try_enqueue_midi_event(&mut queue, on(62)).unwrap();
    assert_eq!(
        queue.iter().copied().collect::<Vec<_>>(),
        [MidiEvent::QueueOverflow, off(61), on(62)]
    );
    // Saturating again must retain a reset even before the consumer sees it.
    while queue.dequeue().is_some() {}
    queue.enqueue(MidiEvent::QueueOverflow).unwrap();
    for _ in 0..400 {
        try_enqueue_midi_event(&mut queue, off(63)).unwrap();
    }
    assert_eq!(queue.peek(), Some(&MidiEvent::QueueOverflow));
}

#[test]
fn emergency_recovery_stops_all_voices_and_clears_pedals_and_bends() {
    let mut voices = VoiceManager::<8>::new(SAMPLE_RATE);
    for channel in 0..16 {
        voices.set_sustain(channel, true);
        voices.apply_channel_pitch_bend(channel, 1.0);
        voices.note_on(69, 100, channel);
    }
    let mut queue = full(std::iter::repeat_n(off(60), 127));
    try_enqueue_midi_event(&mut queue, cc(64, 0)).unwrap();
    drain(&mut queue, &mut voices);
    assert_eq!(voices.get_cached_frequencies(), [0.0; 8]);
    assert_eq!(voices.get_mixed_sources(), (0.0, 0.0));
    for channel in 0..16 {
        if channel == 9 {
            continue;
        }
        voices.note_on(69, 100, channel);
        if channel != 1 {
            assert!(voices
                .get_cached_frequencies()
                .iter()
                .any(|&f| (f - 440.0).abs() < 0.01));
        }
        voices.note_off(69, channel);
        assert_eq!(voices.get_cached_frequencies(), [0.0; 8]);
        assert_eq!(voices.get_mixed_sources(), (0.0, 0.0));
    }
}
