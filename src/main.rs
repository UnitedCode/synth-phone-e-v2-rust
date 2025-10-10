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
    dispatchers = [DMA1_STR0, DMA1_STR2, DMA1_STR3, DMA1_STR4, DMA1_STR5, DMA1_STR6]
    )]
    mod app {
        use crate::{
            constants::{BLOCK_SIZE, BUFFER_SIZE, FFT_SIZE, HOP_SIZE, SAMPLE_RATE},
            midi::{midi_voice::VoiceManager, try_enqueue_midi_event, MidiEvent, MidiReceiver},
            state_machine::{AppState, AppStateMachine, MenuState},
        };
        use embedded_graphics::{image::ImageRawBE, pixelcolor::BinaryColor};
        use fugit::RateExtU32;
        use heapless;
        use libdaisy::{
            audio,
            gpio::*,
            hid, logger,
            prelude::{Input, Output, PushPull},
            system,
        };
        use log::{info, warn};
        use rotary_encoder_embedded::standard::StandardMode;
        use rotary_encoder_embedded::RotaryEncoder;
        use ssd1306::{mode::BufferedGraphicsMode, prelude::*, I2CDisplayInterface, Ssd1306};
        use stm32h7xx_hal::nb;
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
            midi_events: heapless::spsc::Queue<MidiEvent, 128>,
            voice_manager: VoiceManager<8>,
        }

        #[local]
        struct Local {
            audio: audio::Audio,
            buffer: audio::AudioBuffer,
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
            sprite_atlas: ImageRawBE<'static, BinaryColor>,
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

            let sprite_atlas = ImageRawBE::<BinaryColor>::new(
                include_bytes!("../assets/SynthphoneE-Full-Spritesheet.raw"),
                128,
            );

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

            let (_midi_tx, mut midi_rx) = midi_serial.split();

            // Enable RX interrupt
            midi_rx.listen();

            let midi_receiver = MidiReceiver::new(midi_rx);

            info!("Startup done!! yo!");
            startup_complete_task::spawn().ok();

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
                    display_needs_update: true,
                    midi_events: heapless::spsc::Queue::new(),
                    voice_manager: VoiceManager::new(SAMPLE_RATE),
                },
                Local {
                    audio: system.audio,
                    buffer,
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
                    osc: Oscillator::new(440.0, SAMPLE_RATE, Waveform::Triangle),
                    midi_receiver,
                    sprite_atlas,
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

        #[task(
            shared = [app_state_machine, display_needs_update],
            priority = 1
        )]
        fn startup_complete_task(mut ctx: startup_complete_task::Context) {
            use crate::state_machine::AppEvent;

            log::info!("Startup complete, transitioning away from splash screen");

            ctx.shared.app_state_machine.lock(|state_machine| {
                state_machine.handle_event(AppEvent::SplashComplete);
            });

            ctx.shared.display_needs_update.lock(|flag| {
                *flag = true;
            });

            display_update_task::spawn().ok();
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
                voice_manager
            ],
            priority = 8)
        ]
        fn update_handler(mut ctx: update_handler::Context) {
            // Audio processing only - MIDI is handled separately
            crate::handler::audio_handler(
                ctx.local.audio,
                ctx.local.buffer,
                ctx.local.hangup_button,
                ctx.local.hop_counter,
                &mut ctx.shared,
            );
        }

        #[task(
            shared = [midi_events, voice_manager],
            priority = 3
        )]
        fn midi_batch_processing_task(mut ctx: midi_batch_processing_task::Context) {
            // Periodic MIDI processing task for non-critical events
            // Runs at lower priority to handle CC, pitch bend, etc.
            ctx.shared.midi_events.lock(|events| {
                ctx.shared.voice_manager.lock(|voice_manager| {
                    // Process larger batches of non-critical events
                    let mut processed_count = 0;
                    const MAX_BATCH_EVENTS: usize = 16;

                    while let Some(event) = events.dequeue() {
                        processed_count += 1;
                        if processed_count >= MAX_BATCH_EVENTS {
                            // Re-queue this event for next batch
                            let _ = events.enqueue(event);
                            break;
                        }

                        match event {
                            crate::midi::MidiEvent::NoteOn {
                                channel,
                                key,
                                velocity,
                            } => {
                                voice_manager.note_on(key, velocity, channel);
                                let frequency = crate::midi::MidiEvent::note_to_frequency(key);
                                info!(
                                    "Note on: ch={}, key={}, vel={} (freq: {:.2} Hz)",
                                    channel, key, velocity, frequency
                                );
                            }
                            crate::midi::MidiEvent::NoteOff { channel, key, .. } => {
                                voice_manager.note_off(key, channel);
                                info!("Note off: ch={}, key={}", channel, key);
                            }
                            crate::midi::MidiEvent::ControlChange {
                                channel,
                                controller,
                                value,
                            } => match controller {
                                1 => info!("Modulation wheel: ch={}, val={}", channel, value),
                                7 => info!("Volume: ch={}, val={}", channel, value),
                                64 => info!("Sustain pedal: ch={}, val={}", channel, value),
                                123 => {
                                    voice_manager.all_notes_off(Some(channel));
                                    info!("All notes off: ch={}", channel);
                                }
                                _ => info!(
                                    "Unhandled CC: ch={}, cc={} = {}",
                                    channel, controller, value
                                ),
                            },
                            crate::midi::MidiEvent::PitchBend { channel, value } => {
                                let bend_ratio = (value as f32 - 8192.0) / 8192.0;
                                voice_manager.apply_pitch_bend(bend_ratio);
                                info!(
                                    "Pitch bend: ch={}, val={} (ratio: {:.3})",
                                    channel, value, bend_ratio
                                );
                            }
                            crate::midi::MidiEvent::Other => {
                                warn!("Unknown MIDI event");
                            }
                        }
                    }
                });
            });
        }

        #[task(
            local = [display, sprite_atlas],
            shared = [display_needs_update, app_state_machine],
            priority = 2
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
                        crate::display::screens::draw_splash_screen(
                            ctx.local.display,
                            ctx.local.sprite_atlas,
                        );
                    }
                    AppState::Processing(process) => {
                        crate::display::screens::draw_processing_screen(
                            process,
                            snapshot.key,
                            snapshot.octave,
                            snapshot.note,
                            snapshot.volume,
                            ctx.local.display,
                            ctx.local.sprite_atlas,
                        );
                    }
                    AppState::EffectsProfile(process) => {
                        crate::display::screens::draw_effects_screen(
                            process,
                            snapshot.key,
                            snapshot.octave,
                            snapshot.formant,
                            snapshot.crush,
                            snapshot.volume,
                            snapshot.key_down_pressed,
                            snapshot.process_cycle_pressed,
                            snapshot.key_up_pressed,
                            snapshot.waveform,
                            ctx.local.display,
                            ctx.local.sprite_atlas,
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
                voice_manager,
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

        #[task(binds = USART1, local = [midi_receiver], shared = [midi_events], priority = 1)]
        fn usart1_interrupt(ctx: usart1_interrupt::Context) {
            let usart1_interrupt::LocalResources { midi_receiver, .. } = ctx.local;
            let mut midi_events = ctx.shared.midi_events;

            // Limit processing to prevent audio underruns - max 8 events per interrupt
            for _ in 0..8 {
                match midi_receiver.try_read() {
                    Ok(Some(event)) => {
                        // Handle MIDI event - use smart enqueue with overflow protection
                        midi_events.lock(|queue| {
                            if let Err(dropped_event) = try_enqueue_midi_event(queue, event) {
                                // Log dropped event for debugging - this should rarely happen now
                                log::warn!("Dropped MIDI event: {:?}", dropped_event);
                            }
                        });

                        // Spawn batch processing for all events
                        if crate::rtic_app::app::midi_batch_processing_task::spawn().is_err() {
                            // Task already spawned - that's ok
                        }
                    }
                    Ok(None) | Err(nb::Error::WouldBlock) => {
                        // No more data available, exit early
                        break;
                    }
                    Err(_e) => {
                        // Handle other errors if needed
                        break;
                    }
                }
            }
        }
    }
}

pub use rtic_app::app::*;
