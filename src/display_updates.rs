//! Coalesced OLED requests and delayed recovery after a failed bus write.

pub const RETRY_MS: u32 = 100;

pub struct DisplayUpdates {
    pending: bool,
    failed_at: Option<u32>,
    retry_ticks: u32,
}

impl DisplayUpdates {
    /// `clock_hz` must match the wrapping counter supplied to these methods.
    pub fn new(clock_hz: u32) -> Self {
        Self {
            pending: true,
            failed_at: None,
            retry_ticks: clock_hz.div_ceil(1000 / RETRY_MS).max(1),
        }
    }

    pub fn request(&mut self) {
        self.pending = true;
    }

    /// Also used by the periodic input task to wake the background renderer.
    pub fn ready(&self, now: u32) -> bool {
        self.pending
            && self
                .failed_at
                .is_none_or(|failed_at| now.wrapping_sub(failed_at) >= self.retry_ticks)
    }

    /// Consume only the request being rendered. A request made during the bus
    /// write remains pending after a successful completion.
    pub fn begin(&mut self, now: u32) -> bool {
        if !self.ready(now) {
            return false;
        }
        self.pending = false;
        true
    }

    /// Called after the bus write, with no display-state lock held during I/O.
    pub fn complete(&mut self, now: u32, successful: bool) {
        if successful {
            self.failed_at = None;
        } else {
            self.pending = true;
            self.failed_at = Some(now);
        }
    }
}
