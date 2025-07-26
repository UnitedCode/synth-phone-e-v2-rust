#![no_std]
#![no_main]
#![deny(unsafe_code)]
// #![deny(warnings)]
// #![deny(missing_docs)]

/// Synthphone-E v2 by Enoch and Nathan Bradshaw
//- if you are going to make spaghetti, at least leave a recipe
//   ______________________________________________________________________________________________________
//  [                                                                                                      ]\
//  [   SSSS                 TT   HH            HH                                     EEEE   TTT  M M     ] }
//  [   SS    YY  YY NNNNN  TTTTT  HHHHH  PPPPP  HHHHH    OOOOO  NNNNN   EEEE        EE        T  M M M    ] }
//  [    SSSS YYYYY  NN  NN   TT   HH  HH PP  PP HH  HH  OO   OO NN  NN EEEEEE  --- EEEEEEE                ] }
//  [       SS YYYY  NN  NN   TT   HH  HH PP  PP HH  HH  OO   OO NN  NN EE           EE                    ] }
//  [    SSS    YY   NN  NN    TT  HH  HH PPPPP  HH  HH    OOO   NN  NN  EEEE          EEEE                ] }
//  [          YY                         PP                                                               ] }
//   \-----------------------------------------------------------------------------------------------------\ }
//    \______________________________________________________________________________________________________\
use autotune;

// Module declarations
mod audio;
mod constants;
mod display;
mod handler;
mod input;
mod midi;
mod types;

mod rtic_app {
    #[rtic::app(
    device = stm32h7xx_hal::stm32,
    peripherals = true,
    dispatchers = [DMA1_STR0, DMA1_STR2]
    )]
    mod app {
        use crate::{
            autotune::{
                oscillator::{Oscillator, Waveform},
                ring_buffer::RingBuffer,
            },
            constants::{BLOCK_SIZE, BUFFER_SIZE, FFT_SIZE, HOP_SIZE, SAMPLE_RATE},
            midi::MidiParser,
            types::Knob,
        };
        use core::sync::atomic::AtomicU32;
        use embedded_graphics::{image::Image, pixelcolor::BinaryColor, prelude::*};
        use fugit::RateExtU32;
        use libdaisy::{
            audio,
            gpio::*,
            hid, logger,
            prelude::{Input, Output, PushPull},
            system,
        };
        use log::info;

        use cortex_m::prelude::_embedded_hal_serial_Read;
        use fugit::HertzU32;
        use nb;
        use rotary_encoder_embedded::RotaryEncoder;
        use ssd1306::{mode::BufferedGraphicsMode, prelude::*, I2CDisplayInterface, Ssd1306};
        use state_machines::AppStateMachine;
        use state_machines::{AppState, MenuState};
        use stm32h7xx_hal::{
            i2c::{I2c, I2cExt},
            serial::{Serial, SerialExt},
            stm32,
            time::MilliSeconds,
            timer::Timer,
        };
        use tinybmp::Bmp;

        type LcdDisplay = Ssd1306<
            ssd1306::prelude::I2CInterface<I2c<stm32h7xx_hal::stm32::I2C1>>,
            ssd1306::prelude::DisplaySize128x32,
            BufferedGraphicsMode<ssd1306::prelude::DisplaySize128x32>,
        >;

        type UartMidi = Serial<stm32h7xx_hal::stm32::USART1>;

        #[shared]
        struct Shared {
            in_ring: RingBuffer<BUFFER_SIZE>,
            out_ring: RingBuffer<BUFFER_SIZE>,
            carrier_ring: RingBuffer<BUFFER_SIZE>,
            last_input_phases: [f32; FFT_SIZE],
            last_output_phases: [f32; FFT_SIZE],
            synthesis_magnitudes: [f32; FFT_SIZE],
            synthesis_frequencies: [f32; FFT_SIZE],
            previous_pitch_shift_ratio: f32,
            hop_counter: u32,
            in_pointer_cached: AtomicU32,
            app_state_machine: AppStateMachine,
            old_matrix_state: [[bool; 3]; 4],
            // For sample-rate reduction
            sr_hold_counter: i32,
            sr_held_value: f32,
            carrier_osc: Oscillator,
            display_needs_update: bool,
            display_buffer: [u8; 512], // 128x32 / 8 = 512 bytes for the display buffer
            midi_parser: MidiParser,
        }

        #[local]
        struct Local {
            audio: audio::Audio,
            buffer: audio::AudioBuffer,
            button: hid::Switch<Daisy28<Input>>,
            timer2: Timer<stm32::TIM2>,
            knob_1: Knob,
            display: LcdDisplay,
            col_1_pin: Daisy21<Output<PushPull>>,
            col_2_pin: Daisy20<Output<PushPull>>,
            col_3_pin: Daisy19<Output<PushPull>>,
            row_1_pin: Daisy15<Input>,
            row_2_pin: Daisy16<Input>,
            row_3_pin: Daisy17<Input>,
            row_4_pin: Daisy18<Input>,
            encoder_button: hid::Switch<Daisy2<Input>>,
            hangup_button: hid::Switch<Daisy1<Input>>,
            uart_midi: UartMidi,
        }

        #[init]
        fn init(ctx: init::Context) -> (Shared, Local, init::Monotonics) {
            logger::init();

            let mut core = ctx.core;
            let device = ctx.device;
            let ccdr = system::System::init_clocks(device.PWR, device.RCC, &device.SYSCFG);
            let mut system = libdaisy::system_init!(core, device, ccdr, BLOCK_SIZE);

            let buffer = [(0.0, 0.0); audio::BLOCK_SIZE_MAX];

            let encoder_dt = system
                .gpio
                .daisy3
                .take()
                .expect("Failed to get daisy3")
                .into_floating_input();
            let encoder_clk = system
                .gpio
                .daisy4
                .take()
                .expect("failed to get pin daisy4")
                .into_floating_input();

            let encoder_sw_pin = system
                .gpio
                .daisy2
                .take()
                .expect("Failed to get pin daisy2")
                .into_pull_up_input();

            let mut encoder_button = hid::Switch::new(encoder_sw_pin, hid::SwitchType::PullUp);
            encoder_button.set_double_thresh(Some(100));

            let encoder_1 = RotaryEncoder::new(encoder_dt, encoder_clk).into_standard_mode();
            let knob_1 = Knob::new(encoder_1);

            let daisy1 = system
                .gpio
                .daisy1
                .take()
                .expect("Failed to get pin daisy1")
                .into_pull_up_input();

            let daisy28_btn = system
                .gpio
                .daisy28
                .take()
                .expect("Failed to get pin daisy28!")
                .into_pull_up_input();

            let daisy12_sda = system
                .gpio
                .daisy12
                .take()
                .expect("Failed to get pin daisy9")
                .into_alternate::<4>()
                .internal_pull_up(true)
                .set_open_drain();
            let daisy13_scl = system
                .gpio
                .daisy11
                .take()
                .expect("Failed to get daisy 8 pin")
                .into_alternate::<4>()
                .internal_pull_up(true)
                .set_open_drain();

            let i2c = device.I2C1.i2c(
                (daisy13_scl, daisy12_sda),
                100_u32.kHz(),
                ccdr.peripheral.I2C1,
                &ccdr.clocks,
            );

            let col_1_pin: Daisy21<Output<PushPull>> = system
                .gpio
                .daisy21
                .take()
                .expect("Failed to get D21")
                .into_push_pull_output();
            let col_2_pin: Daisy20<Output<PushPull>> = system
                .gpio
                .daisy20
                .take()
                .expect("Failed to get D20")
                .into_push_pull_output();
            let col_3_pin: Daisy19<Output<PushPull>> = system
                .gpio
                .daisy19
                .take()
                .expect("Failed to get D19")
                .into_push_pull_output();

            let row_1_pin: Daisy15<Input> = system
                .gpio
                .daisy15
                .take()
                .expect("Failed to get D15")
                .into_pull_up_input();
            let row_2_pin: Daisy16<Input> = system
                .gpio
                .daisy16
                .take()
                .expect("Failed to get D16")
                .into_pull_up_input();
            let row_3_pin: Daisy17<Input> = system
                .gpio
                .daisy17
                .take()
                .expect("Failed to get D17")
                .into_pull_up_input();
            let row_4_pin: Daisy18<Input> = system
                .gpio
                .daisy18
                .take()
                .expect("Failed to get D18")
                .into_pull_up_input();

            // Configure UART1 for MIDI - Use pins A9 (TX) and A10 (RX) for UART1
            // For MIDI input, we only need RX (A10), but HAL requires both TX and RX
            let uart_tx = system
                .gpio
                .daisy14
                .take()
                .expect("Failed to get daisy14 for UART TX")
                .into_alternate::<7>(); // UART1 TX alternate function 7

            let uart_rx = system
                .gpio
                .daisy13
                .take()
                .expect("Failed to get daisy15 for UART RX")
                .into_alternate::<7>(); // UART1 RX alternate function 7

            let mut uart_midi = device
                .USART1
                .serial(
                    (uart_rx, uart_tx),   // TX and RX pins
                    HertzU32::Hz(31_250), // MIDI baud rate
                    ccdr.peripheral.USART1,
                    &ccdr.clocks,
                )
                .unwrap();

            // Enable UART1 receive interrupt
            uart_midi.listen(stm32h7xx_hal::serial::Event::Rxne);

            let i2c_interface = I2CDisplayInterface::new_custom_address(i2c, 0x3C);

            let mut display =
                Ssd1306::new(i2c_interface, DisplaySize128x32, DisplayRotation::Rotate0)
                    .into_buffered_graphics_mode();

            display.init().expect("Failed to initialize display");
            display.clear();

            let bmp: Bmp<BinaryColor> =
                Bmp::from_slice(include_bytes!("../assets/synthophoneV2.bmp")).unwrap();

            let image = Image::new(&bmp, Point::new(0, 0));
            image.draw(&mut display).expect("Failed to display image");
            display.flush().expect("Could not write to display");
            display.clear();

            let mut switch1 = hid::Switch::new(daisy28_btn, hid::SwitchType::PullUp);
            switch1.set_double_thresh(Some(500));
            switch1.set_held_thresh(Some(150));

            let mut hangup_button = hid::Switch::new(daisy1, hid::SwitchType::PullUp);
            hangup_button.set_double_thresh(Some(500));
            hangup_button.set_held_thresh(Some(150));

            let mut timer2 = stm32h7xx_hal::timer::TimerExt::timer(
                device.TIM2,
                MilliSeconds::from_ticks(1).into_rate(),
                ccdr.peripheral.TIM2,
                &ccdr.clocks,
            );
            timer2.listen(stm32h7xx_hal::timer::Event::TimeOut);
            display_update_task::spawn().ok();

            info!("Startup done!! yo!");

            (
                Shared {
                    in_ring: RingBuffer::new(),
                    out_ring: RingBuffer::with_offset((FFT_SIZE + (2 * HOP_SIZE)) as u32),
                    carrier_ring: RingBuffer::new(),
                    last_input_phases: [0.0; FFT_SIZE],
                    last_output_phases: [0.0; FFT_SIZE],
                    synthesis_magnitudes: [0.0; FFT_SIZE],
                    synthesis_frequencies: [0.0; FFT_SIZE],
                    previous_pitch_shift_ratio: 1.0,
                    hop_counter: 0,
                    in_pointer_cached: AtomicU32::new(0),
                    app_state_machine: AppStateMachine::new(),
                    old_matrix_state: [[false; 3]; 4],
                    sr_hold_counter: 0,
                    sr_held_value: 0.0,
                    carrier_osc: Oscillator::new(55.0, SAMPLE_RATE, Waveform::Square),
                    display_needs_update: false,
                    display_buffer: [0; 512],
                    midi_parser: MidiParser::new(),
                },
                Local {
                    audio: system.audio,
                    buffer,
                    button: switch1,
                    timer2,
                    knob_1,
                    display,
                    col_1_pin,
                    col_2_pin,
                    col_3_pin,
                    row_1_pin,
                    row_2_pin,
                    row_3_pin,
                    row_4_pin,
                    encoder_button,
                    hangup_button,
                    uart_midi,
                },
                init::Monotonics(),
            )
        }

        #[idle]
        fn idle(_ctx: idle::Context) -> ! {
            loop {
                cortex_m::asm::nop();
            }
        }

        #[task(binds = DMA1_STR1, local = [audio, buffer, button, hangup_button], shared = [
        in_ring,
        out_ring,
        carrier_ring,
        last_input_phases,
        last_output_phases,
        hop_counter,
        in_pointer_cached,
        app_state_machine,
        sr_hold_counter,
        sr_held_value,
        carrier_osc,
    ], priority = 8)]
        fn update_handler(mut ctx: update_handler::Context) {
            crate::handler::update_handler(
                ctx.local.audio,
                ctx.local.buffer,
                ctx.local.hangup_button,
                &mut ctx.shared,
            );
        }

        #[task(
            local = [display],  // Display is now local to this task
            shared = [app_state_machine, display_needs_update],
            priority = 1  // Low priority so it doesn't block audio
        )]
        fn display_update_task(mut ctx: display_update_task::Context) {
            // Check if update is needed
            let needs_update = ctx.shared.display_needs_update.lock(|flag| {
                let update = *flag;
                if update {
                    *flag = false; // Clear the flag
                }
                update
            });

            if needs_update {
                // Get the current state snapshot
                let snapshot = ctx.shared.app_state_machine.lock(|msm| msm.snapshot());

                // Draw based on current state
                match snapshot.current_state {
                    AppState::Splash => {
                        crate::display::screens::draw_splash_screen(ctx.local.display);
                    }
                    AppState::Processing(process) => {
                        crate::display::screens::draw_processing_screen(
                            process,
                            snapshot.key,
                            snapshot.octave,
                            snapshot.note,
                            snapshot.volume,
                            ctx.local.display,
                        );
                    }
                    AppState::EffectsProfile(process) => {
                        crate::display::screens::draw_effects_screen(
                            process,
                            snapshot.key,
                            snapshot.octave,
                            snapshot.formant,
                            snapshot.crush,
                            snapshot.key_down_pressed,
                            snapshot.process_cycle_pressed,
                            snapshot.key_up_pressed,
                            ctx.local.display,
                        );
                    }
                    AppState::Menu(nav_state, _) => {
                        let menu_context = ctx.shared.app_state_machine.lock(|msm| msm.current());
                        let is_editing = matches!(nav_state, MenuState::Editing(_));

                        crate::display::screens::draw_menu_screen(
                            menu_context.previous_item,
                            menu_context.current_item,
                            menu_context.next_item,
                            is_editing,
                            ctx.local.display,
                        );
                    }
                }

                // NOW we can do the blocking flush in low priority
                ctx.local.display.flush().ok();
            }
        }

        #[task(binds = TIM2, local = [
            knob_1,
            timer2,
            col_1_pin,
            col_2_pin,
            col_3_pin,
            row_1_pin,
            row_2_pin,
            row_3_pin,
            row_4_pin,
            encoder_button,
            ], shared = [app_state_machine, old_matrix_state, display_needs_update])]
        fn interface_handler(mut ctx: interface_handler::Context) {
            crate::handler::interface_handler(ctx.local, &mut ctx.shared);
        }

        /// FFT TASK
        #[task(shared = [
        in_ring,
        out_ring,
        carrier_ring,
        last_input_phases,
        last_output_phases,
        synthesis_magnitudes,
        synthesis_frequencies,
        previous_pitch_shift_ratio,
        app_state_machine,
        in_pointer_cached,
        carrier_osc,
    ], local = [], priority = 7)]
        fn dma1_stream0_software_task(mut ctx: dma1_stream0_software_task::Context) {
            // Call audio processing from handler module
            crate::handler::dma1_stream0_software_task(&mut ctx.shared);
        }

        /// UART1 MIDI receive interrupt handler
        #[task(binds = USART1, local = [uart_midi], shared = [midi_parser], priority = 6)]
        fn uart1_midi_handler(mut ctx: uart1_midi_handler::Context) {
            let uart = ctx.local.uart_midi;

            // Check if we have received data
            match uart.read() {
                Ok(byte) => {
                    // Process the MIDI byte
                    ctx.shared.midi_parser.lock(|parser| {
                        if let Some(message) = parser.process_byte(byte) {
                            crate::midi::log_midi_message(message);
                        }
                    });
                }
                Err(nb::Error::WouldBlock) => {
                    // No data available, this is normal
                }
                Err(_) => {
                    // Handle other errors if needed
                }
            }
        }
    }
}

pub use rtic_app::app::*;
