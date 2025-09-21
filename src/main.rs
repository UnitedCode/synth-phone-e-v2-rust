#![no_std]
#![no_main]
#![deny(unsafe_code)]

// Synthphone-E v2 by Enoch and Nathan Bradshaw
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

// Module declarations
mod audio;
mod constants;
mod display;
mod handler;
mod input;
mod midi;
mod state_machine;
mod types;

mod rtic_app {
    #[rtic::app(
    device = stm32h7xx_hal::stm32,
    peripherals = true,
    dispatchers = [DMA1_STR0, DMA1_STR2, USART1]
    )]
    mod app {
        use crate::{
            constants::{BLOCK_SIZE, BUFFER_SIZE, FFT_SIZE, HOP_SIZE, SAMPLE_RATE},
            midi::{MidiEvent, MidiReceiver},
            state_machine::{AppState, AppStateMachine, MenuState},
        };
        use embedded_graphics::{image::Image, pixelcolor::BinaryColor, prelude::*};
        use fugit::RateExtU32;
        use heapless;
        use libdaisy::{
            audio,
            gpio::*,
            hid, logger,
            prelude::{Input, Output, PushPull},
            system,
        };
        use log::info;
        use rotary_encoder_embedded::standard::StandardMode;
        use rotary_encoder_embedded::RotaryEncoder;
        use ssd1306::{mode::BufferedGraphicsMode, prelude::*, I2CDisplayInterface, Ssd1306};
        use stm32h7xx_hal::{
            i2c::{I2c, I2cExt},
            serial::{config::Config as SerialConfig, SerialExt},
            stm32,
            time::MilliSeconds,
            time::U32Ext,
            timer::Timer,
        };
        use synthphone_e_vocal_dsp::{
            audio::{Oscillator, Waveform},
            ring_buffer::RingBuffer,
        };
        use tinybmp::Bmp;

        type LcdDisplay = Ssd1306<
            ssd1306::prelude::I2CInterface<I2c<stm32h7xx_hal::stm32::I2C1>>,
            ssd1306::prelude::DisplaySize128x32,
            BufferedGraphicsMode<ssd1306::prelude::DisplaySize128x32>,
        >;

        pub struct Knob {
            pub rotary_encoder: RotaryEncoder<StandardMode, Daisy3<Input>, Daisy4<Input>>,
            pub value: u8,
        }

        impl Knob {
            pub fn new(
                rotary_encoder: RotaryEncoder<StandardMode, Daisy3<Input>, Daisy4<Input>>,
            ) -> Knob {
                Knob {
                    rotary_encoder,
                    value: 0_u8,
                }
            }
        }

        #[shared]
        struct Shared {
            in_ring: RingBuffer<BUFFER_SIZE>,
            out_ring: RingBuffer<BUFFER_SIZE>,
            in_pointer_cached: u32,
            previous_pitch_shift_ratio: f32,
            app_state_machine: AppStateMachine,
            old_matrix_state: [[bool; 3]; 4],
            // For sample-rate reduction
            sr_hold_counter: i32,
            sr_held_value: f32,
            display_needs_update: bool,
            midi_events: heapless::spsc::Queue<MidiEvent, 32>,
            midi_osc: Oscillator,
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
            hop_counter: u32,
            last_input_phases: [f32; FFT_SIZE],
            last_output_phases: [f32; FFT_SIZE],
            carrier_ring: RingBuffer<FFT_SIZE>,
            osc: Oscillator,
            previous_pitch_shift_ratio: f32,
            midi_receiver: MidiReceiver,
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

            let daisy14_sda = system
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
                (daisy13_scl, daisy14_sda),
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

            // Initialize MIDI USART1 on pin 14 (RX only)
            let midi_rx_pin = system
                .gpio
                .daisy14
                .take()
                .expect("Failed to get daisy14 for MIDI RX")
                .into_alternate::<7>(); // USART1_RX alternate function

            let midi_tx_pin = system
                .gpio
                .daisy13
                .take()
                .expect("Failed to get daisy13 for MIDI TX")
                .into_alternate::<7>(); // USART1_TX alternate function (not used but needed for Serial)

            let mut midi_config = SerialConfig::default();
            midi_config.baudrate = 31_250_u32.bps(); // MIDI baud rate

            let midi_serial = device
                .USART1
                .serial(
                    (midi_tx_pin, midi_rx_pin),
                    midi_config,
                    ccdr.peripheral.USART1,
                    &ccdr.clocks,
                )
                .unwrap();

            let (_midi_tx, midi_rx) = midi_serial.split();
            let midi_receiver = MidiReceiver::new(midi_rx);

            // Spawn MIDI task
            midi_handler::spawn().ok();

            info!("Startup done!! yo!");

            (
                Shared {
                    in_ring: RingBuffer::new(),
                    out_ring: RingBuffer::with_offset((FFT_SIZE + (2 * HOP_SIZE)) as u32),
                    previous_pitch_shift_ratio: 1.0,
                    in_pointer_cached: 0,
                    app_state_machine: AppStateMachine::new(),
                    old_matrix_state: [[false; 3]; 4],
                    sr_hold_counter: 0,
                    sr_held_value: 0.0,
                    display_needs_update: false,
                    midi_events: heapless::spsc::Queue::new(),
                    midi_osc: Oscillator::new(440.0, SAMPLE_RATE, Waveform::Saw),
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
                    hop_counter: 0,
                    previous_pitch_shift_ratio: 1.0,
                    last_input_phases: [0.0; FFT_SIZE],
                    last_output_phases: [0.0; FFT_SIZE],
                    carrier_ring: RingBuffer::new(),
                    osc: Oscillator::new(440.0, SAMPLE_RATE, Waveform::Saw),
                    midi_receiver,
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

        #[task(binds = DMA1_STR1,
            local = [
                audio,
                buffer,
                hangup_button,
                hop_counter,

            ],
            shared = [
                in_ring,
                out_ring,
                in_pointer_cached,
                previous_pitch_shift_ratio,
                app_state_machine,
                sr_hold_counter,
                sr_held_value,
                midi_events,
                midi_osc
            ],
            priority = 8)
        ]
        fn update_handler(mut ctx: update_handler::Context) {
            // Process MIDI events first
            ctx.shared.midi_events.lock(|events| {
                ctx.shared.midi_osc.lock(|osc| {
                    process_midi_events(events, osc);
                });
            });

            crate::handler::audio_handler(
                ctx.local.audio,
                ctx.local.buffer,
                ctx.local.hangup_button,
                ctx.local.hop_counter,
                &mut ctx.shared,
            );
        }

        #[task(
            local = [display],
            shared = [app_state_machine, display_needs_update],
            priority = 1
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

        #[task(
            binds = TIM2,
            local = [
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
            ],
            shared = [
                app_state_machine,
                old_matrix_state,
                display_needs_update,
            ],
            priority = 3
        )]
        fn interface_handler(mut ctx: interface_handler::Context) {
            crate::handler::interface_handler(ctx.local, &mut ctx.shared);
        }

        /// FFT TASK
        #[task(
            shared = [
                in_ring,
                out_ring,
                previous_pitch_shift_ratio,
                app_state_machine,
                in_pointer_cached,
            ],
            local = [
                last_input_phases,
                last_output_phases,
                previous_pitch_shift_ratio,
                carrier_ring,
                osc,
            ],
            priority = 7,
        )]
        fn dma1_stream0_fft_task(mut ctx: dma1_stream0_fft_task::Context) {
            crate::handler::handle_vocal_effects(
                &mut ctx.shared,
                ctx.local.last_input_phases,
                ctx.local.last_output_phases,
                ctx.local.previous_pitch_shift_ratio,
                ctx.local.carrier_ring,
                ctx.local.osc,
            );
        }

        #[task(local = [midi_receiver], shared = [midi_events], priority = 2)]
        fn midi_handler(ctx: midi_handler::Context) {
            let midi_handler::LocalResources { midi_receiver, .. } = ctx.local;
            let mut midi_events = ctx.shared.midi_events;

            // Try to read MIDI data
            match midi_receiver.try_read() {
                Ok(Some(event)) => {
                    // Handle MIDI event
                    match event {
                        MidiEvent::NoteOn {
                            channel,
                            key,
                            velocity,
                        } => {
                            info!(
                                "MIDI Note On: ch={}, key={}, vel={}",
                                channel, key, velocity
                            );
                            // Add to event queue for processing by other tasks
                            midi_events.lock(|queue| {
                                queue.enqueue(event).ok();
                            });
                        }
                        MidiEvent::NoteOff {
                            channel,
                            key,
                            velocity,
                        } => {
                            info!(
                                "MIDI Note Off: ch={}, key={}, vel={}",
                                channel, key, velocity
                            );
                            midi_events.lock(|queue| {
                                queue.enqueue(event).ok();
                            });
                        }
                        MidiEvent::ControlChange {
                            channel,
                            controller,
                            value,
                        } => {
                            info!("MIDI CC: ch={}, cc={}, val={}", channel, controller, value);
                            midi_events.lock(|queue| {
                                queue.enqueue(event).ok();
                            });
                        }
                        MidiEvent::PitchBend { channel, value } => {
                            info!("MIDI Pitch Bend: ch={}, val={}", channel, value);
                            midi_events.lock(|queue| {
                                queue.enqueue(event).ok();
                            });
                        }
                        MidiEvent::Other => {
                            // Ignore other MIDI events
                        }
                    }

                    // Schedule next read
                    midi_handler::spawn().ok();
                }
                Ok(None) => {
                    // No data available, schedule next read with delay
                    midi_handler::spawn().ok();
                }
                Err(_e) => {
                    // Error reading, schedule retry
                    midi_handler::spawn().ok();
                }
            }
        }

        // Helper function to process MIDI events from the queue
        fn process_midi_events(
            midi_events: &mut heapless::spsc::Queue<MidiEvent, 32>,
            osc: &mut Oscillator,
        ) {
            while let Some(event) = midi_events.dequeue() {
                match event {
                    MidiEvent::NoteOn { key, .. } => {
                        // Convert MIDI note to frequency and update oscillator
                        let frequency = MidiEvent::note_to_frequency(key);
                        osc.set_freq(frequency);
                        info!(
                            "Setting oscillator frequency to {} Hz (note {})",
                            frequency, key
                        );
                    }
                    MidiEvent::NoteOff { key, .. } => {
                        // For now, just log the note off
                        info!("Note off: {}", key);
                        // You might want to implement envelope or voice management here
                    }
                    MidiEvent::ControlChange {
                        controller, value, ..
                    } => {
                        // Example: Use CC 1 (mod wheel) to control some parameter
                        match controller {
                            1 => {
                                // Modulation wheel - could control vibrato, filter, etc.
                                info!("Modulation wheel: {}", value);
                            }
                            7 => {
                                // Volume - could control amplitude
                                info!("Volume: {}", value);
                            }
                            _ => {
                                info!("Unhandled CC: {} = {}", controller, value);
                            }
                        }
                    }
                    MidiEvent::PitchBend { value, .. } => {
                        // Apply pitch bend to oscillator
                        let bend_ratio = (value as f32 - 8192.0) / 8192.0; // Normalize to -1.0 to 1.0
                        let bent_freq = osc.freq * (1.0 + bend_ratio * 0.1); // +/- 10% bend range
                        osc.set_freq(bent_freq);
                        info!("Pitch bend: {} (ratio: {})", value, bend_ratio);
                    }
                    MidiEvent::Other => {
                        // Ignore other events
                    }
                }
            }
        }
    }
}

pub use rtic_app::app::*;
