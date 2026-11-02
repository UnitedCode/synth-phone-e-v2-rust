//! Keypad → drum-channel note mapping for the Percussion profile.
//!
//! All three sound sets play on MIDI channel 10 (index 9), continuing past the
//! end of the GM1 drum map (35–81):
//!   82–93  ringers (one per keypad key)
//!   94–105 DTMF    (keypad order 1–9, *, 0, #)
//!
//! Kept free of hardware types so it builds into the host test library.

pub const DRUM_CHANNEL: u8 = 9;
pub const RINGER_FIRST_NOTE: u8 = 82;
pub const DTMF_FIRST_NOTE: u8 = 94;
pub const KEYPAD_KEYS: usize = 12;

/// Sound sets selected with keys 7/8/9 on the Effects screen (`current_percussion`).
pub const SET_DIAL_TONES: i8 = 0;
pub const SET_RINGER: i8 = 1;
pub const SET_DRUMS: i8 = 2;

/// Maps a 4×3 keypad key number (1–12) to a GM drum note on channel 10.
/// Layout (column order is reversed by the scan hardware):
///   Key 3 | Key 2 | Key 1   row 0
///   Key 6 | Key 5 | Key 4   row 1
///   Key 9 | Key 8 | Key 7   row 2
///   Key 12| Key 11| Key 10  row 3
pub fn keypad_to_drum_note(key: usize) -> Option<u8> {
    match key {
        1 => Some(36),  // Bass Drum 1
        2 => Some(38),  // Acoustic Snare
        3 => Some(42),  // Closed Hi-Hat
        4 => Some(35),  // Acoustic Bass Drum
        5 => Some(39),  // Hand Clap
        6 => Some(46),  // Open Hi-Hat
        7 => Some(41),  // Low Floor Tom
        8 => Some(45),  // Low Tom
        9 => Some(49),  // Crash Cymbal 1
        10 => Some(47), // Low-Mid Tom
        11 => Some(50), // High Tom
        12 => Some(51), // Ride Cymbal 1
        _ => None,
    }
}

/// Note a keypad key (1–12) plays in the given sound set.
pub fn keypad_to_percussion_note(set: i8, key: usize) -> Option<u8> {
    if !(1..=KEYPAD_KEYS).contains(&key) {
        return None;
    }
    let offset = (key - 1) as u8;
    match set {
        SET_DIAL_TONES => Some(DTMF_FIRST_NOTE + offset),
        SET_RINGER => Some(RINGER_FIRST_NOTE + offset),
        SET_DRUMS => keypad_to_drum_note(key),
        _ => None,
    }
}

/// Tracks the note each keypad key started, so a release always stops the
/// note its press started — even if the screen or sound set changed while the
/// key was held.
///
/// Dial tones are "first key wins", like a real phone keypad: while one DTMF
/// key is held, other DTMF presses are ignored.
pub struct HeldKeys {
    notes: [Option<u8>; KEYPAD_KEYS],
    dtmf_key: Option<usize>,
}

impl HeldKeys {
    pub const fn new() -> Self {
        Self {
            notes: [None; KEYPAD_KEYS],
            dtmf_key: None,
        }
    }

    /// Returns the note to start, or `None` if the key is unmapped or another
    /// DTMF key is already sounding.
    pub fn press(&mut self, set: i8, key: usize) -> Option<u8> {
        let note = keypad_to_percussion_note(set, key)?;
        if set == SET_DIAL_TONES {
            if self.dtmf_key.is_some() {
                return None;
            }
            self.dtmf_key = Some(key);
        }
        self.notes[key - 1] = Some(note);
        Some(note)
    }

    /// Returns the note to stop, if this key started one.
    pub fn release(&mut self, key: usize) -> Option<u8> {
        if self.dtmf_key == Some(key) {
            self.dtmf_key = None;
        }
        self.notes.get_mut(key.checked_sub(1)?)?.take()
    }
}

impl Default for HeldKeys {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_map_to_their_note_ranges() {
        assert_eq!(keypad_to_percussion_note(SET_RINGER, 1), Some(82));
        assert_eq!(keypad_to_percussion_note(SET_RINGER, 12), Some(93));
        assert_eq!(keypad_to_percussion_note(SET_DIAL_TONES, 1), Some(94));
        assert_eq!(keypad_to_percussion_note(SET_DIAL_TONES, 12), Some(105));
        assert_eq!(keypad_to_percussion_note(SET_DRUMS, 2), Some(38));
    }

    #[test]
    fn out_of_range_keys_are_unmapped() {
        for set in [SET_DIAL_TONES, SET_RINGER, SET_DRUMS] {
            assert_eq!(keypad_to_percussion_note(set, 0), None);
            assert_eq!(keypad_to_percussion_note(set, 13), None);
        }
    }

    #[test]
    fn unknown_set_plays_nothing() {
        assert_eq!(keypad_to_percussion_note(3, 1), None);
    }

    #[test]
    fn release_stops_the_note_the_press_started() {
        let mut held = HeldKeys::new();
        assert_eq!(held.press(SET_RINGER, 5), Some(86));
        // Release sends the ringer note even though the set is irrelevant now.
        assert_eq!(held.release(5), Some(86));
        assert_eq!(held.release(5), None);
    }

    #[test]
    fn dtmf_first_key_wins() {
        let mut held = HeldKeys::new();
        assert_eq!(held.press(SET_DIAL_TONES, 1), Some(94));
        assert_eq!(held.press(SET_DIAL_TONES, 2), None);
        // The ignored key has nothing to stop; the first key is still sounding.
        assert_eq!(held.release(2), None);
        assert_eq!(held.release(1), Some(94));
        // Once released, a new DTMF key can sound.
        assert_eq!(held.press(SET_DIAL_TONES, 2), Some(95));
    }

    #[test]
    fn first_key_wins_applies_only_to_dial_tones() {
        let mut held = HeldKeys::new();
        assert_eq!(held.press(SET_RINGER, 1), Some(82));
        assert_eq!(held.press(SET_RINGER, 2), Some(83));
        assert_eq!(held.press(SET_DRUMS, 3), Some(42));
    }

    #[test]
    fn release_of_unknown_key_is_ignored() {
        let mut held = HeldKeys::new();
        assert_eq!(held.release(0), None);
        assert_eq!(held.release(13), None);
    }
}
