use audio_lab::display_updates::DisplayUpdates;

#[test]
fn failure_retries_without_new_input_after_delay_from_completion() {
    let mut updates = DisplayUpdates::new(1000);
    assert!(updates.begin(0));
    updates.complete(40, false);
    for now in 41..140 {
        assert!(!updates.ready(now));
        assert!(!updates.begin(now));
    }
    assert!(updates.ready(140));
    assert!(updates.begin(140));
    updates.complete(141, true);
    assert!(!updates.ready(1000));
}

#[test]
fn new_requests_do_not_bypass_failure_backoff() {
    let mut updates = DisplayUpdates::new(1000);
    let mut attempts = Vec::new();
    for now in 0..=1000 {
        updates.request();
        if updates.begin(now) {
            attempts.push(now);
            updates.complete(now, false);
        }
    }
    assert_eq!(
        attempts,
        (0..=10).map(|period| period * 100).collect::<Vec<_>>()
    );
}

#[test]
fn requests_during_a_successful_write_remain_pending() {
    let mut updates = DisplayUpdates::new(1000);
    assert!(updates.begin(0));
    assert!(!updates.ready(1));
    updates.request();
    updates.complete(20, true);
    assert!(updates.ready(20));
    assert!(updates.begin(20));
    updates.complete(40, true);
    assert!(!updates.ready(41));
}

#[test]
fn retry_deadline_survives_counter_wrap() {
    let mut updates = DisplayUpdates::new(1000);
    let failure = u32::MAX - 50;
    assert!(updates.begin(failure));
    updates.complete(failure, false);
    assert!(!updates.ready(failure.wrapping_add(99)));
    assert!(updates.begin(failure.wrapping_add(100)));
    updates.complete(failure.wrapping_add(100), true);
    assert!(!updates.ready(failure.wrapping_add(101)));
}

#[test]
fn fractional_clock_rates_round_the_retry_delay_up() {
    let mut updates = DisplayUpdates::new(1001);
    assert!(updates.begin(0));
    updates.complete(0, false);
    assert!(!updates.begin(100));
    assert!(updates.begin(101));
}

mod driver {
    use super::*;
    use display_interface::{DataFormat, DisplayError, WriteOnlyDataCommand};
    use ssd1306::{mode::BufferedGraphicsMode, prelude::*, Ssd1306};
    use std::{cell::RefCell, rc::Rc};

    #[derive(Default)]
    struct BusState {
        fail_commands: bool,
        fail_data_call: Option<usize>,
        commands: usize,
        data_calls: usize,
        data: Vec<u8>,
    }
    struct Bus(Rc<RefCell<BusState>>);
    impl WriteOnlyDataCommand for Bus {
        fn send_commands(&mut self, _: DataFormat<'_>) -> Result<(), DisplayError> {
            let mut state = self.0.borrow_mut();
            state.commands += 1;
            if state.fail_commands {
                Err(DisplayError::BusWriteError)
            } else {
                Ok(())
            }
        }
        fn send_data(&mut self, data: DataFormat<'_>) -> Result<(), DisplayError> {
            let DataFormat::U8(bytes) = data else {
                return Err(DisplayError::DataFormatNotImplemented);
            };
            let mut state = self.0.borrow_mut();
            state.data_calls += 1;
            if state.fail_data_call == Some(state.data_calls) {
                state.data.extend_from_slice(&bytes[..bytes.len() / 2]);
                state.fail_data_call = None;
                return Err(DisplayError::BusWriteError);
            }
            state.data.extend_from_slice(bytes);
            Ok(())
        }
    }

    type Display = Ssd1306<Bus, DisplaySize128x32, BufferedGraphicsMode<DisplaySize128x32>>;

    fn display() -> (Display, Rc<RefCell<BusState>>) {
        let state = Rc::new(RefCell::new(BusState::default()));
        let mut display = Ssd1306::new(
            Bus(state.clone()),
            DisplaySize128x32,
            DisplayRotation::Rotate0,
        )
        .into_buffered_graphics_mode();
        display.init().unwrap();
        (display, state)
    }

    fn attempt(
        updates: &mut DisplayUpdates,
        display: &mut Display,
        now: u32,
        frame: u8,
    ) -> Option<bool> {
        if !updates.begin(now) {
            return None;
        }
        // The same full-clear/redraw/flush sequence as the background task.
        display.clear();
        for y in 0..32 {
            for x in 0..128 {
                display.set_pixel(x, y, frame & (1 << (y % 8)) != 0);
            }
        }
        let successful = display.flush().is_ok();
        updates.complete(now, successful);
        Some(successful)
    }

    #[test]
    fn a_partial_failed_transfer_is_replaced_by_the_latest_complete_frame() {
        let (mut display, state) = display();
        let mut updates = DisplayUpdates::new(1000);
        state.borrow_mut().fail_data_call = Some(2);
        assert_eq!(attempt(&mut updates, &mut display, 0, 0xff), Some(false));
        assert!(!state.borrow().data.is_empty());
        assert!(state.borrow().data.len() < 512);
        let commands = state.borrow().commands;
        assert_eq!(attempt(&mut updates, &mut display, 99, 0x55), None);
        assert_eq!(state.borrow().commands, commands);
        state.borrow_mut().data.clear();
        assert_eq!(attempt(&mut updates, &mut display, 100, 0x55), Some(true));
        assert_eq!(state.borrow().data, vec![0x55; 512]);
        assert!(!updates.ready(200));
    }

    #[test]
    fn command_failures_retry_without_input_and_stop_after_recovery() {
        let (mut display, state) = display();
        let mut updates = DisplayUpdates::new(1000);
        state.borrow_mut().fail_commands = true;
        assert_eq!(attempt(&mut updates, &mut display, 0, 0xff), Some(false));
        let commands = state.borrow().commands;
        for now in 1..100 {
            assert_eq!(attempt(&mut updates, &mut display, now, 0xff), None);
        }
        assert_eq!(state.borrow().commands, commands);
        assert_eq!(attempt(&mut updates, &mut display, 100, 0xff), Some(false));
        state.borrow_mut().fail_commands = false;
        assert_eq!(attempt(&mut updates, &mut display, 200, 0xaa), Some(true));
        assert_eq!(state.borrow().data, vec![0xaa; 512]);
        assert_eq!(attempt(&mut updates, &mut display, 300, 0xaa), None);
    }
}
