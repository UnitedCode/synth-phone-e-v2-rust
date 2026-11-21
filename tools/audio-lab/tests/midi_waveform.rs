use audio_lab::{
    constants::SAMPLE_RATE,
    midi_events::MidiEvent,
    state_machine::{AppEvent, AppStateMachine, ProcessingProfile},
    voice_generator::VoiceManager,
};
use heapless::spsc::Queue;
use synthphone_e_vocal_dsp::audio::Waveform;

fn phone() -> AppStateMachine {
    let mut app = AppStateMachine::new();
    app.handle_event(AppEvent::SplashComplete);
    app.handle_event(AppEvent::SetProcessingProfile(ProcessingProfile::Dry));
    app.handle_event(AppEvent::EncoderPress);
    app
}

fn selected(app: &AppStateMachine) -> Waveform {
    app.snapshot().oscillator_waveform()
}

fn matching_audio<const N: usize>(
    actual: &mut VoiceManager<N>,
    reference: &mut VoiceManager<N>,
    waveform: Waveform,
) {
    reference.set_waveform(waveform);
    for sample in 0..1024 {
        assert_eq!(
            actual.render_sources(waveform, false, 1.0, 1.0),
            reference.get_mixed_sources(),
            "waveform or oscillator phase differs at sample {sample}"
        );
    }
}

#[test]
fn keypad_changes_reach_a_held_midi_note_without_another_event() {
    let mut app = phone();
    let mut actual = VoiceManager::<8>::new(SAMPLE_RATE);
    let mut reference = VoiceManager::<8>::new(SAMPLE_RATE);
    actual.note_on(69, 100, 1);
    reference.note_on(69, 100, 1);
    matching_audio(&mut actual, &mut reference, selected(&app));
    for key in [8, 9, 7, 8] {
        app.handle_event(AppEvent::KeypadPress(key));
        matching_audio(&mut actual, &mut reference, selected(&app));
        assert_eq!(actual.display_note(), Some(69));
    }
    actual.note_off(69, 1);
    assert_eq!(
        actual.render_sources(selected(&app), false, 1.0, 1.0),
        (0.0, 0.0)
    );
}

#[test]
fn a_final_program_change_updates_every_held_voice_after_the_queue_empties() {
    let mut app = phone();
    let mut queue = Queue::<MidiEvent, 128>::new();
    let mut actual = VoiceManager::<8>::new(SAMPLE_RATE);
    let mut reference = VoiceManager::<8>::new(SAMPLE_RATE);
    for note in 60..68 {
        actual.note_on(note, 100, 2);
        reference.note_on(note, 100, 2);
    }
    matching_audio(&mut actual, &mut reference, selected(&app));
    let pitches = actual.get_cached_frequencies();
    for program in [1, 2, 0, 127] {
        queue
            .enqueue(MidiEvent::ProgramChange {
                channel: 2,
                program,
            })
            .unwrap();
        if let Some(MidiEvent::ProgramChange { channel, program }) = queue.dequeue() {
            assert!(app.set_waveform_from_midi(channel, program));
        }
        assert!(queue.is_empty());
        matching_audio(&mut actual, &mut reference, selected(&app));
        assert_eq!(actual.get_cached_frequencies(), pitches);
        assert_eq!(actual.display_note(), Some(67));
    }
}

#[test]
fn sustained_notes_change_waveform_without_retriggering_or_losing_the_pedal() {
    let mut app = phone();
    let mut actual = VoiceManager::<8>::new(SAMPLE_RATE);
    let mut reference = VoiceManager::<8>::new(SAMPLE_RATE);
    for voices in [&mut actual, &mut reference] {
        voices.set_sustain(2, true);
        voices.apply_channel_pitch_bend(2, 0.5);
        voices.note_on(69, 100, 2);
        voices.note_off(69, 2);
    }
    matching_audio(&mut actual, &mut reference, selected(&app));
    app.handle_event(AppEvent::KeypadPress(8));
    matching_audio(&mut actual, &mut reference, selected(&app));
    assert_eq!(
        actual.get_cached_frequencies(),
        reference.get_cached_frequencies()
    );
    actual.set_sustain(2, false);
    assert_eq!(
        actual.render_sources(selected(&app), false, 1.0, 1.0),
        (0.0, 0.0)
    );
}

#[test]
fn waveform_updates_leave_phone_sounds_and_drum_program_changes_alone() {
    let mut app = phone();
    let mut actual = VoiceManager::<8>::new(SAMPLE_RATE);
    let mut reference = VoiceManager::<8>::new(SAMPLE_RATE);
    for note in [36, 82, 94] {
        actual.note_on(note, 100, 9);
        reference.note_on(note, 100, 9);
    }
    for key in [8, 9, 7] {
        app.handle_event(AppEvent::KeypadPress(key));
        assert!(!app.set_waveform_from_midi(9, 2));
        for _ in 0..1024 {
            assert_eq!(
                actual.render_sources(selected(&app), false, 1.0, 1.0),
                reference.get_mixed_sources()
            );
        }
    }
}

#[test]
fn a_waveform_selected_while_silent_also_applies_to_new_notes() {
    let mut actual = VoiceManager::<8>::new(SAMPLE_RATE);
    let mut reference = VoiceManager::<8>::new(SAMPLE_RATE);
    matching_audio(&mut actual, &mut reference, Waveform::Square);
    actual.note_on(69, 100, 1);
    reference.note_on(69, 100, 1);
    matching_audio(&mut actual, &mut reference, Waveform::Square);
}

#[test]
fn repeated_waveform_sync_preserves_phase_routing_and_source_gains() {
    let mut actual = VoiceManager::<8>::new(SAMPLE_RATE);
    let mut reference = VoiceManager::<8>::new(SAMPLE_RATE);
    for voices in [&mut actual, &mut reference] {
        voices.note_on(60, 100, 1);
        voices.note_on(64, 80, 2);
        voices.note_on(36, 100, 9);
    }
    for waveform in [Waveform::Square, Waveform::Saw, Waveform::Triangle] {
        reference.set_waveform(waveform);
        for mute in [false, true] {
            for _ in 0..2048 {
                assert_eq!(
                    actual.render_sources(waveform, mute, 0.5, 1.5),
                    reference.get_mixed_sources_with_gains(mute, 0.5, 1.5)
                );
            }
        }
    }
}
