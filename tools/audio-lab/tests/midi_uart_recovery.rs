// Compile the firmware receiver against a deterministic UART, including its
// real parser and queue policy, without linking the board's peripheral crate.
extern crate self as stm32h7xx_hal;
pub use audio_lab::{audio, constants, percussion};

pub mod nb {
    #[derive(Debug, Clone, Copy)]
    pub enum Error<E> {
        WouldBlock,
        Other(E),
    }
}
pub mod prelude {}
pub mod stm32 {
    pub struct USART1;
}
pub mod serial {
    use std::{
        cell::{Cell, RefCell},
        collections::VecDeque,
        marker::PhantomData,
        rc::Rc,
    };
    #[derive(Debug, Clone, Copy)]
    pub enum Error {
        Noise,
        Framing,
        Parity,
        Overrun,
    }
    pub type Step = Result<u8, crate::nb::Error<Error>>;
    #[derive(Clone, Default)]
    pub struct Input {
        pub data: Rc<RefCell<VecDeque<Step>>>,
        pub reads: Rc<Cell<usize>>,
    }
    pub struct Rx<T> {
        input: Input,
        _peripheral: PhantomData<T>,
    }
    impl<T> Rx<T> {
        pub fn new(input: Input) -> Self {
            Self {
                input,
                _peripheral: PhantomData,
            }
        }
        pub fn read(&mut self) -> Step {
            self.input.reads.set(self.input.reads.get() + 1);
            self.input
                .data
                .borrow_mut()
                .pop_front()
                .unwrap_or(Err(crate::nb::Error::WouldBlock))
        }
    }
}

#[allow(dead_code, unused_imports, unused_parens)]
#[path = "../../../src/midi/mod.rs"]
mod midi;

use audio_lab::state_machine::AppStateMachine;
use heapless::spsc::Queue;
use midi::{try_enqueue_midi_event, voice_generator::VoiceManager, MidiEvent, MidiReceiver};

fn receiver() -> (MidiReceiver, serial::Input) {
    let input = serial::Input::default();
    (MidiReceiver::new(serial::Rx::new(input.clone())), input)
}

fn send(
    receiver: &mut MidiReceiver,
    input: &serial::Input,
    steps: impl IntoIterator<Item = serial::Step>,
) -> Vec<MidiEvent> {
    input.data.borrow_mut().extend(steps);
    let polls = input.data.borrow().len() + 2;
    (0..polls)
        .filter_map(|_| receiver.try_read().unwrap())
        .collect()
}

fn apply(
    voices: &mut VoiceManager<8>,
    app: &mut AppStateMachine,
    events: impl IntoIterator<Item = MidiEvent>,
) {
    for event in events {
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
            MidiEvent::PitchBend { channel, value } => {
                voices.apply_channel_pitch_bend(channel, (value as f32 - 8192.0) / 8192.0)
            }
            MidiEvent::UartError | MidiEvent::QueueOverflow => voices.reset_midi_performance(),
            _ => {}
        }
    }
    app.set_midi_note_for_display(voices.display_note());
}

#[test]
fn hard_uart_faults_clear_notes_pedals_bends_and_the_display_then_resynchronize() {
    for error in [
        serial::Error::Framing,
        serial::Error::Parity,
        serial::Error::Overrun,
    ] {
        let (mut receiver, input) = receiver();
        let mut voices = VoiceManager::<8>::new(constants::SAMPLE_RATE);
        let mut app = AppStateMachine::new();
        let setup = [0xb2, 64, 127, 0xe2, 0, 96, 0x92, 69, 100, 69, 0];
        apply(
            &mut voices,
            &mut app,
            send(&mut receiver, &input, setup.map(Ok)),
        );
        assert_eq!(voices.display_note(), Some(69));
        assert_eq!(app.snapshot().midi_pitch_class, Some(9));
        // The HAL leaves pending bytes in its FIFO when it reports an error.
        // Even a stale status byte must not restart a voice after recovery.
        let fault = [
            Err(nb::Error::Other(error)),
            Ok(0x92),
            Ok(60),
            Ok(100),
            Ok(69),
            Ok(0),
        ];
        let events = send(&mut receiver, &input, fault);
        assert_eq!(events, [MidiEvent::UartError]);
        apply(&mut voices, &mut app, events);
        assert_eq!(voices.get_cached_frequencies(), [0.0; 8]);
        assert_eq!(voices.get_mixed_sources(), (0.0, 0.0));
        assert_eq!(app.snapshot().midi_pitch_class, None);
        apply(
            &mut voices,
            &mut app,
            send(&mut receiver, &input, [0x92, 69, 100].map(Ok)),
        );
        assert_eq!(voices.get_cached_frequencies()[0], 440.0, "bend must reset");
        apply(
            &mut voices,
            &mut app,
            send(&mut receiver, &input, [69, 0].map(Ok)),
        );
        assert_eq!(
            voices.display_note(),
            None,
            "old sustain must not carry over"
        );
    }
}

#[test]
fn noise_flags_preserve_partial_messages_and_running_status_releases() {
    let (mut receiver, input) = receiver();
    let on = send(
        &mut receiver,
        &input,
        [
            Ok(0x92),
            Ok(60),
            Err(nb::Error::Other(serial::Error::Noise)),
            Ok(100),
        ],
    );
    assert_eq!(
        on,
        [MidiEvent::NoteOn {
            channel: 2,
            key: 60,
            velocity: 100
        }]
    );
    let off = send(
        &mut receiver,
        &input,
        [Err(nb::Error::Other(serial::Error::Noise)), Ok(60), Ok(0)],
    );
    assert_eq!(
        off,
        [MidiEvent::NoteOff {
            channel: 2,
            key: 60,
            velocity: 0
        }]
    );
}

#[test]
fn hard_fault_discards_a_partial_message_and_requires_a_fresh_status() {
    let (mut receiver, input) = receiver();
    send(&mut receiver, &input, [0x92, 60, 100, 61].map(Ok));
    assert_eq!(
        send(
            &mut receiver,
            &input,
            [Err(nb::Error::Other(serial::Error::Framing)), Ok(100)]
        ),
        [MidiEvent::UartError]
    );
    assert!(send(&mut receiver, &input, [62, 100].map(Ok)).is_empty());
    assert_eq!(
        send(&mut receiver, &input, [0x92, 64, 100].map(Ok)),
        [MidiEvent::NoteOn {
            channel: 2,
            key: 64,
            velocity: 100
        }]
    );
}

#[test]
fn fault_reset_survives_a_full_queue_and_precedes_new_notes() {
    for filler in [
        MidiEvent::ControlChange {
            channel: 2,
            controller: 7,
            value: 20,
        },
        MidiEvent::NoteOff {
            channel: 2,
            key: 60,
            velocity: 0,
        },
    ] {
        let (mut receiver, input) = receiver();
        let mut queue = Queue::<MidiEvent, 128>::new();
        for _ in 0..127 {
            queue.enqueue(filler).unwrap();
        }
        let mut voices = VoiceManager::<8>::new(constants::SAMPLE_RATE);
        let mut app = AppStateMachine::new();
        voices.note_on(55, 100, 0);
        voices.note_on(57, 100, 3);
        voices.note_on(94, 100, 9);
        for event in send(
            &mut receiver,
            &input,
            [Err(nb::Error::Other(serial::Error::Overrun))],
        ) {
            try_enqueue_midi_event(&mut queue, event).unwrap();
        }
        for event in send(&mut receiver, &input, [0x92, 69, 100].map(Ok)) {
            try_enqueue_midi_event(&mut queue, event).unwrap();
        }
        let events: Vec<_> = queue.iter().copied().collect();
        let reset = events
            .iter()
            .position(|&e| e == MidiEvent::UartError)
            .unwrap();
        assert!(reset < events.len() - 1);
        apply(&mut voices, &mut app, events);
        assert_eq!(voices.display_note(), Some(69));
        assert_eq!(
            voices.get_cached_frequencies(),
            [440.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
        );
        assert_eq!(
            voices.get_mixed_sources().1,
            0.0,
            "old phone tone must stop"
        );
    }
}

#[test]
fn recovery_drain_is_bounded_and_never_parses_the_damaged_backlog() {
    let (mut receiver, input) = receiver();
    input
        .data
        .borrow_mut()
        .push_back(Err(nb::Error::Other(serial::Error::Overrun)));
    input
        .data
        .borrow_mut()
        .extend([0x92, 60, 100].into_iter().cycle().take(300).map(Ok));
    assert_eq!(receiver.try_read().unwrap(), Some(MidiEvent::UartError));
    assert_eq!(input.reads.get(), 33);
    for _ in 0..10 {
        let before = input.reads.get();
        assert_eq!(receiver.try_read().unwrap(), None);
        assert!(input.reads.get() - before <= 32);
    }
    assert!(input.data.borrow().is_empty());
    assert_eq!(
        send(&mut receiver, &input, [0x92, 69, 100].map(Ok)),
        [MidiEvent::NoteOn {
            channel: 2,
            key: 69,
            velocity: 100
        }]
    );
}
