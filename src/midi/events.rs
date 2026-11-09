//! Hardware-independent events and the bounded queue policy shared by all inputs.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiEvent {
    NoteOn {
        channel: u8,
        key: u8,
        velocity: u8,
    },
    NoteOff {
        channel: u8,
        key: u8,
        velocity: u8,
    },
    ControlChange {
        channel: u8,
        controller: u8,
        value: u8,
    },
    PitchBend {
        channel: u8,
        value: u16,
    },
    ProgramChange {
        channel: u8,
        program: u8,
    },
    Other,
    /// Internal recovery marker, never produced by the MIDI parser.
    QueueOverflow,
    /// UART framing/parity/overrun recovery, ordered before later input.
    UartError,
    /// MIDI 1.0 System Reset (0xFF).
    SystemReset,
}

impl MidiEvent {
    pub fn is_note_event(&self) -> bool {
        matches!(self, Self::NoteOn { .. } | Self::NoteOff { .. })
    }

    pub fn get_channel(&self) -> Option<u8> {
        match *self {
            Self::NoteOn { channel, .. }
            | Self::NoteOff { channel, .. }
            | Self::ControlChange { channel, .. }
            | Self::PitchBend { channel, .. }
            | Self::ProgramChange { channel, .. } => Some(channel),
            Self::Other | Self::QueueOverflow | Self::UartError | Self::SystemReset => None,
        }
    }

    fn is_release(self) -> bool {
        matches!(
            self,
            Self::NoteOff { .. }
                | Self::NoteOn { velocity: 0, .. }
                | Self::ControlChange {
                    controller: 64,
                    value: 0..=63,
                    ..
                }
                | Self::ControlChange {
                    controller: 120 | 121 | 123,
                    ..
                }
                | Self::QueueOverflow
                | Self::UartError
                | Self::SystemReset
        )
    }

    fn is_disposable(self) -> bool {
        match self {
            Self::Other | Self::PitchBend { .. } => true,
            Self::ControlChange { controller, .. } => !matches!(controller, 64 | 120 | 121 | 123),
            _ => false,
        }
    }
}

pub fn get_midi_queue_status(queue: &heapless::spsc::Queue<MidiEvent, 128>) -> (usize, usize) {
    (queue.len(), queue.capacity())
}

/// Sample-clocked queue diagnostics, independent of the FFT hop counter.
#[derive(Default)]
pub struct MidiQueueMonitor {
    elapsed_samples: usize,
}

impl MidiQueueMonitor {
    pub const fn new() -> Self {
        Self { elapsed_samples: 0 }
    }

    /// Call once per completed audio block. Read occupancy only at the roughly
    /// 100 ms deadline and report queues above 75%, at most once per interval.
    pub fn on_audio_block(
        &mut self,
        queue_status: impl FnOnce() -> (usize, usize),
    ) -> Option<(usize, usize)> {
        const INTERVAL_SAMPLES: usize = 4_800;
        self.elapsed_samples += crate::constants::BLOCK_SIZE;
        if self.elapsed_samples < INTERVAL_SAMPLES {
            return None;
        }
        self.elapsed_samples %= INTERVAL_SAMPLES;
        let (len, capacity) = queue_status();
        (len > capacity * 3 / 4).then_some((len, capacity))
    }
}

/// Call with exclusive access, as both UART and keypad producers do under RTIC.
/// On overflow, retain FIFO order and prefer releases over controls and new notes.
/// If every queued event is a release, recover with an emergency reset, retaining
/// any pending System Reset, then append the incoming event. A bounded queue
/// cannot retain unlimited releases.
pub fn try_enqueue_midi_event(
    queue: &mut heapless::spsc::Queue<MidiEvent, 128>,
    event: MidiEvent,
) -> Result<(), MidiEvent> {
    if queue.enqueue(event).is_ok() {
        return Ok(());
    }
    // Unhandled messages must not displace anything the audio task can use.
    if event == MidiEvent::Other {
        return Err(event);
    }

    let discard = queue.iter().position(|queued| queued.is_disposable());
    let discard = discard.or_else(|| {
        if event.is_release() {
            queue.iter().position(|queued| !queued.is_release())
        } else {
            None
        }
    });
    if let Some(discard) = discard {
        // Rotate the complete original queue, omitting one event. Moving only a
        // prefix to the tail would put later NoteOffs before their NoteOns.
        for index in 0..queue.len() {
            if let Some(queued) = queue.dequeue() {
                if index != discard {
                    let _ = queue.enqueue(queued);
                }
            }
        }
    } else if event.is_release() {
        // A stream reset also restores UI performance controls. QueueOverflow
        // only stops voices, so it must not replace a pending System Reset.
        let system_reset_pending = queue.iter().any(|event| *event == MidiEvent::SystemReset);
        while queue.dequeue().is_some() {}
        let _ = queue.enqueue(MidiEvent::QueueOverflow);
        if system_reset_pending && event != MidiEvent::SystemReset {
            let _ = queue.enqueue(MidiEvent::SystemReset);
        }
    } else {
        return Err(event);
    }
    queue.enqueue(event)
}
