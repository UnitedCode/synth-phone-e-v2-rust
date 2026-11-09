use audio_lab::{constants::SAMPLE_RATE, voice_generator::VoiceManager};

fn unmapped_notes() -> impl Iterator<Item = u8> {
    (0..35).chain(106..=127)
}

#[test]
fn unmapped_drum_notes_cannot_orphan_a_held_phone_sound_with_eight_busy_voices() {
    for phone_note in [82, 94] {
        for unmapped in unmapped_notes() {
            let mut voices = VoiceManager::<8>::new(SAMPLE_RATE);
            voices.note_on(phone_note, 100, 9);
            for note in 60..67 {
                voices.note_on(note, 100, 2);
            }
            for _ in 0..1024 {
                voices.get_mixed_sources();
            }
            voices.note_on(unmapped, 100, 9);
            voices.note_off(phone_note, 9);
            for note in 60..67 {
                voices.note_off(note, 2);
            }
            voices.note_off(unmapped, 9);
            for _ in 0..48_000 {
                voices.get_mixed_sources();
            }
            assert!(
                (0..1024).all(|_| voices.get_mixed_sources() == (0.0, 0.0)),
                "phone note {phone_note} stuck after unmapped drum note {unmapped}"
            );
        }
    }
}

#[test]
fn unmapped_drum_notes_do_not_steal_or_change_an_active_melodic_voice() {
    let mut actual = VoiceManager::<1>::new(SAMPLE_RATE);
    let mut reference = VoiceManager::<1>::new(SAMPLE_RATE);
    actual.note_on(69, 100, 2);
    reference.note_on(69, 100, 2);
    for note in unmapped_notes() {
        actual.note_on(note, 100, 9);
        assert_eq!(actual.display_note(), Some(69), "unmapped drum {note}");
        assert_eq!(
            actual.get_cached_frequencies(),
            reference.get_cached_frequencies()
        );
        for _ in 0..256 {
            assert_eq!(actual.get_mixed_sources(), reference.get_mixed_sources());
        }
    }
}

#[test]
fn unmapped_drum_notes_do_not_occupy_idle_voices() {
    let mut voices = VoiceManager::<8>::new(SAMPLE_RATE);
    for note in unmapped_notes() {
        voices.note_on(note, 100, 9);
    }
    for note in 60..68 {
        voices.note_on(note, 100, 2);
    }
    assert_eq!(
        voices
            .get_cached_frequencies()
            .iter()
            .filter(|&&frequency| frequency > 0.0)
            .count(),
        8,
        "unmapped drums must leave capacity for all eight pitched voices"
    );
}

#[test]
fn unmapped_drum_notes_cannot_restart_a_phone_sound_after_panic() {
    for phone_note in 82..=105 {
        for unmapped in [0, 34, 106, 127] {
            let mut voices = VoiceManager::<8>::new(SAMPLE_RATE);
            voices.note_on(phone_note, 100, 9);
            for _ in 0..1024 {
                voices.get_mixed_sources();
            }
            voices.all_notes_off(Some(9));
            voices.note_on(unmapped, 100, 9);
            assert!(
                (0..1024).all(|_| voices.get_mixed_sources() == (0.0, 0.0)),
                "unmapped drum {unmapped} restarted phone sound {phone_note}"
            );
        }
    }
}

#[test]
fn notes_outside_the_drum_map_still_play_on_pitched_channels() {
    for channel in [0, 1, 2, 3, 15] {
        let mut voices = VoiceManager::<8>::new(SAMPLE_RATE);
        for note in [0, 34, 106, 127] {
            voices.note_on(note, 100, channel);
            assert_eq!(voices.display_note(), Some(note));
            voices.note_on(note, 0, channel);
            assert_eq!(voices.display_note(), None);
        }
    }
}
