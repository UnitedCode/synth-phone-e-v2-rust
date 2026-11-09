//! Per-contact debounce for the existing 4×3 keypad wiring.

/// Both presses and releases must remain stable for this interval.
pub const DEBOUNCE_MS: u32 = 5;

pub struct KeypadDebouncer {
    stable: [[bool; 3]; 4],
    candidate: [[bool; 3]; 4],
    changed_at: [[u32; 3]; 4],
    delay_ticks: u32,
}

impl KeypadDebouncer {
    /// `clock_hz` must describe the wrapping counter supplied to `update`.
    pub fn new(clock_hz: u32) -> Self {
        Self {
            stable: [[false; 3]; 4],
            candidate: [[false; 3]; 4],
            changed_at: [[0; 3]; 4],
            delay_ticks: clock_hz.div_ceil(1000 / DEBOUNCE_MS).max(1),
        }
    }

    /// Return only accepted changes: `Some(true)` presses, `Some(false)` releases.
    /// Regular sampling is required; elapsed ticks keep scan jitter from changing
    /// the interval. Contact activity between samples cannot be reconstructed.
    pub fn update(&mut self, raw: [[bool; 3]; 4], now: u32) -> [[Option<bool>; 3]; 4] {
        let mut changes = [[None; 3]; 4];
        for (row, columns) in raw.iter().enumerate() {
            for (col, &pressed) in columns.iter().enumerate() {
                if pressed != self.candidate[row][col] {
                    self.candidate[row][col] = pressed;
                    self.changed_at[row][col] = now;
                }
                if pressed != self.stable[row][col]
                    && now.wrapping_sub(self.changed_at[row][col]) >= self.delay_ticks
                {
                    self.stable[row][col] = pressed;
                    changes[row][col] = Some(pressed);
                }
            }
        }
        changes
    }
}
