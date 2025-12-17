// keymap_sampler.rs - Direct keymap sample playback (no pitch shifting)
// Each key plays its own specific sample at original speed

/// Simple sample player - plays a sample from start to finish
pub struct SamplePlayer {
    sample_data: Option<&'static [f32]>,
    playback_position: usize,
    is_playing: bool,
    volume: f32,
    loop_enabled: bool,
}

impl SamplePlayer {
    pub fn new() -> Self {
        Self {
            sample_data: None,
            playback_position: 0,
            is_playing: false,
            volume: 1.0,
            loop_enabled: false,
        }
    }

    /// Assign a sample to this player
    pub fn set_sample(&mut self, data: &'static [f32]) {
        self.sample_data = Some(data);
    }

    /// Enable/disable looping
    pub fn set_loop(&mut self, enabled: bool) {
        self.loop_enabled = enabled;
    }

    /// Set volume (0.0 to 1.0)
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
    }

    /// Trigger playback from the beginning
    pub fn trigger(&mut self) {
        if self.sample_data.is_some() {
            self.playback_position = 0;
            self.is_playing = true;
        }
    }

    /// Stop playback
    pub fn stop(&mut self) {
        self.is_playing = false;
        self.playback_position = 0;
    }

    /// Check if currently playing
    pub fn is_playing(&self) -> bool {
        self.is_playing
    }

    /// Get next sample value
    #[inline(always)]
    pub fn next_value(&mut self) -> f32 {
        if !self.is_playing {
            return 0.0;
        }

        let Some(data) = self.sample_data else {
            return 0.0;
        };

        if self.playback_position >= data.len() {
            if self.loop_enabled {
                self.playback_position = 0;
            } else {
                self.is_playing = false;
                return 0.0;
            }
        }

        let sample = data[self.playback_position];
        self.playback_position += 1;

        sample * self.volume
    }
}

impl Default for SamplePlayer {
    fn default() -> Self {
        Self::new()
    }
}

/// Keymap for assigning samples to specific MIDI keys
pub struct Keymap {
    entries: [(Option<&'static [f32]>, bool); 128], // (sample data, loop enabled)
}

impl Keymap {
    pub fn new() -> Self {
        Self {
            entries: [(None, false); 128],
        }
    }

    /// Assign a sample to a specific MIDI key
    pub fn set_key(&mut self, midi_note: u8, sample: &'static [f32], loop_enabled: bool) {
        if (midi_note as usize) < 128 {
            self.entries[midi_note as usize] = (Some(sample), loop_enabled);
        }
    }

    /// Clear a key assignment
    pub fn clear_key(&mut self, midi_note: u8) {
        if (midi_note as usize) < 128 {
            self.entries[midi_note as usize] = (None, false);
        }
    }

    /// Get sample data for a key
    pub fn get_key(&self, midi_note: u8) -> Option<(&'static [f32], bool)> {
        if (midi_note as usize) < 128 {
            let (sample, loop_enabled) = self.entries[midi_note as usize];
            sample.map(|s| (s, loop_enabled))
        } else {
            None
        }
    }

    /// Check if a key has a sample assigned
    pub fn has_key(&self, midi_note: u8) -> bool {
        if (midi_note as usize) < 128 {
            self.entries[midi_note as usize].0.is_some()
        } else {
            false
        }
    }
}

impl Default for Keymap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_player_basic() {
        static TEST_SAMPLE: [f32; 4] = [0.1, 0.5, 0.9, 0.5];
        
        let mut player = SamplePlayer::new();
        player.set_sample(&TEST_SAMPLE);
        player.trigger();
        
        assert!(player.is_playing());
        assert_eq!(player.next_value(), 0.1);
        assert_eq!(player.next_value(), 0.5);
        assert_eq!(player.next_value(), 0.9);
        assert_eq!(player.next_value(), 0.5);
        assert_eq!(player.next_value(), 0.0); // Should stop after sample ends
        assert!(!player.is_playing());
    }

    #[test]
    fn test_sample_player_looping() {
        static TEST_SAMPLE: [f32; 2] = [0.5, 1.0];
        
        let mut player = SamplePlayer::new();
        player.set_sample(&TEST_SAMPLE);
        player.set_loop(true);
        player.trigger();
        
        assert_eq!(player.next_value(), 0.5);
        assert_eq!(player.next_value(), 1.0);
        assert_eq!(player.next_value(), 0.5); // Should loop back
        assert_eq!(player.next_value(), 1.0);
        assert!(player.is_playing()); // Should still be playing
    }

    #[test]
    fn test_keymap_basic() {
        static SAMPLE1: [f32; 2] = [0.1, 0.2];
        static SAMPLE2: [f32; 2] = [0.3, 0.4];
        
        let mut keymap = Keymap::new();
        keymap.set_key(60, &SAMPLE1, false);
        keymap.set_key(61, &SAMPLE2, true);
        
        assert!(keymap.has_key(60));
        assert!(keymap.has_key(61));
        assert!(!keymap.has_key(62));
        
        let (sample, looping) = keymap.get_key(60).unwrap();
        assert_eq!(sample.len(), 2);
        assert!(!looping);
        
        let (sample, looping) = keymap.get_key(61).unwrap();
        assert_eq!(sample.len(), 2);
        assert!(looping);
    }

    #[test]
    fn test_keymap_clear() {
        static SAMPLE: [f32; 2] = [0.1, 0.2];
        
        let mut keymap = Keymap::new();
        keymap.set_key(60, &SAMPLE, false);
        assert!(keymap.has_key(60));
        
        keymap.clear_key(60);
        assert!(!keymap.has_key(60));
    }
}