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

const SAMPLE_RATE: f32 = 48_014.312;
const FFT_SIZE: usize = 1024;
const BUFFER_SIZE: usize = FFT_SIZE * 2;
const HOP_SIZE: usize = 256;
const BLOCK_SIZE: usize = 2;
const BIN_WIDTH: f32 = SAMPLE_RATE as f32 / FFT_SIZE as f32 * 2.0;
use autotune::{self, keys};
use autotune::hann_window;

mod rtic_app {
    #[rtic::app(
    device = stm32h7xx_hal::stm32,
    peripherals = true,
    dispatchers = [DMA1_STR0]
    )]
    mod app {
        use autotune::{
            frequencies::{
                find_nearest_note_in_key, C_MAJOR_SCALE_FREQUENCIES,
            }, keys::{get_key, get_key_name, get_mode_name, get_note_name, get_scale_by_key, C_MAJOR_SCALE, E_MAJOR_SCALE}, process_frequencies::{cepstral_smoothing, bitcrush, sample_rate_reduce, find_fundamental_frequency, normalize_sample}
        };
        use heapless::String;
        use core::fmt::Write;
        use core::f32::consts::PI;
        use embedded_graphics::{
            image::Image,
            mono_font::{ascii::{FONT_10X20, FONT_6X10, FONT_6X9, FONT_6X13}, MonoTextStyle, MonoTextStyleBuilder},
            pixelcolor::BinaryColor,
            prelude::*,
            text::{Baseline, Text},
            primitives::{Line, PrimitiveStyle},
        };
        use libdaisy::{audio, hid, logger, prelude::{Output, PushPull, Input}, system, gpio::*};
        use libm::{atan2f, cosf, floorf, fmodf, sinf, sqrtf, roundf};
        use log::{info, warn};
        use state_machines::{AppState, MenuState, ProcessingProfile, AppStateMachine};
        use stm32h7xx_hal::{ i2c::{I2c, I2cExt}, stm32, time::MilliSeconds, timer::Timer
        };
        use tinybmp::Bmp;
        use crate::{
            autotune::circular_buffer::CircularBuffer, hann_window, BIN_WIDTH, BLOCK_SIZE,
            BUFFER_SIZE, FFT_SIZE, HOP_SIZE,
        };
        use fugit::RateExtU32;
        use ssd1306::{mode::BufferedGraphicsMode, prelude::*, I2CDisplayInterface, Ssd1306};
        use rotary_encoder_embedded::standard::StandardMode;
        use rotary_encoder_embedded::{Direction, RotaryEncoder};

        type LcdDisplay = Ssd1306<ssd1306::prelude::I2CInterface<I2c<stm32h7xx_hal::stm32::I2C1>>, ssd1306::prelude::DisplaySize128x32, BufferedGraphicsMode<ssd1306::prelude::DisplaySize128x32>>;

        pub struct Knob {
            rotary_encoder: RotaryEncoder<StandardMode, Daisy3<Input>, Daisy4<Input>>,
            value: u8,
        }

        impl Knob {
            pub fn new(
                rotary_encoder: RotaryEncoder<
                    StandardMode,
                    Daisy3<Input>,
                    Daisy4<Input>,
                >,
            ) -> Knob {
                Knob {
                    rotary_encoder: rotary_encoder,
                    value: 0_u8,
                }
            }
        }

        #[shared]
        struct Shared {
            in_buffer: CircularBuffer<f32, BUFFER_SIZE>,
            out_buffer: CircularBuffer<f32, BUFFER_SIZE>,
            last_input_phases: [f32; FFT_SIZE],
            last_output_phases: [f32; FFT_SIZE],
            synthesis_magnitudes: [f32; FFT_SIZE],
            synthesis_frequencies: [f32; FFT_SIZE],
            previous_pitch_shift_ratio: f32,
            hop_counter: u32,
            app_state_machine: AppStateMachine,
            old_matrix_state: [[bool; 3]; 4],
            // For sample-rate reduction
            sr_hold_counter: i32,
            sr_held_value: f32,
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

            let encoder_button = hid::Switch::new(encoder_sw_pin, hid::SwitchType::PullUp);

            let encoder_1 = RotaryEncoder::new(encoder_dt, encoder_clk).into_standard_mode();

            let knob_1 = Knob::new(encoder_1);

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
            let col_1_pin: Daisy21<Output<PushPull>> = system.gpio.daisy21.take().expect("Failed to get D21").into_push_pull_output();
            let col_2_pin: Daisy20<Output<PushPull>> = system.gpio.daisy20.take().expect("Failed to get D20").into_push_pull_output();
            let col_3_pin: Daisy19<Output<PushPull>> = system.gpio.daisy19.take().expect("Failed to get D19").into_push_pull_output();

            let row_1_pin: Daisy15<Input> = system.gpio.daisy15.take().expect("Failed to get D15").into_pull_up_input();
            let row_2_pin: Daisy16<Input> = system.gpio.daisy16.take().expect("Failed to get D16").into_pull_up_input();
            let row_3_pin: Daisy17<Input> = system.gpio.daisy17.take().expect("Failed to get D17").into_pull_up_input();
            let row_4_pin: Daisy18<Input> = system.gpio.daisy18.take().expect("Failed to get D18").into_pull_up_input();

            let i2c_interface = I2CDisplayInterface::new_custom_address(i2c, 0x3C);

            let mut display =
                Ssd1306::new(i2c_interface, DisplaySize128x32, DisplayRotation::Rotate0)
                    .into_buffered_graphics_mode();

            display.init().expect("Failed to initialize display");

            display.clear();

            let bmp: Bmp<BinaryColor> =
                Bmp::from_slice(include_bytes!("../assets/synthophoneV2.bmp")).unwrap();

            let image = Image::new(&bmp, Point::new(0, 0));

            // Display the image
            image.draw(&mut display).expect("Failed to display image");

            display.flush().expect("Could not write to display");
            display.clear();

            // Create a text style
            //let text_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);

            let mut switch1 = hid::Switch::new(daisy28_btn, hid::SwitchType::PullUp);
            switch1.set_double_thresh(Some(500));
            switch1.set_held_thresh(Some(150));

            let mut timer2 = stm32h7xx_hal::timer::TimerExt::timer(
                device.TIM2,
                MilliSeconds::from_ticks(1).into_rate(),
                ccdr.peripheral.TIM2,
                &ccdr.clocks,
            );
            timer2.listen(stm32h7xx_hal::timer::Event::TimeOut);

            info!("Startup done!! yo!");

            (
                Shared {
                    in_buffer: CircularBuffer::new(0.0, None),
                    out_buffer: CircularBuffer::new(0.0, Some(HOP_SIZE)),
                    last_input_phases: [0.0; FFT_SIZE],
                    last_output_phases: [0.0; FFT_SIZE],
                    synthesis_magnitudes: [0.0; FFT_SIZE],
                    synthesis_frequencies: [0.0; FFT_SIZE],
                    previous_pitch_shift_ratio: 1.0,
                    hop_counter: 0,
                    app_state_machine: AppStateMachine::new(),
                    old_matrix_state: [[false; 3]; 4],
                    // For sample-rate reduction
                    sr_hold_counter: 0,
                    sr_held_value: 0.0,
                },
                Local {
                    audio: system.audio,
                    buffer,
                    button: switch1,
                    timer2,
                    knob_1,
                    display,
                    col_1_pin, col_2_pin, col_3_pin, row_1_pin, row_2_pin, row_3_pin, row_4_pin,
                    encoder_button
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

        #[task(binds = DMA1_STR1, local = [audio, buffer, button], shared = [
        in_buffer,
        out_buffer,
        last_input_phases,
        last_output_phases,
        hop_counter,
        app_state_machine,
        sr_hold_counter,
        sr_held_value,
    ], priority = 8)]
        fn update_handler(mut ctx: update_handler::Context) {
            let audio = ctx.local.audio;
            let buffer = ctx.local.buffer;
            let switch1 = ctx.local.button;
            let button_pressed = switch1.is_held() || switch1.is_pressed();

            if audio.get_stereo(buffer) {
                for (left, _right) in &buffer.as_slice()[..BLOCK_SIZE] {
                    let mut out_sample = *left;


                    // Get current processing mode
                    let mut current_mode = ProcessingProfile::Autotune;
                    ctx.shared.app_state_machine.lock(|msm| {
                        let snapshot = msm.snapshot();
                        match snapshot.current_state {
                            AppState::Processing(mode) | AppState::EffectsProfile(mode) | AppState::Menu(_, mode) => {
                                current_mode = mode;
                            },
                            AppState::Splash => {}
                        }
                    });

                    // Lock to write to in_buffer
                    ctx.shared.in_buffer.lock(|in_buffer| {
                        in_buffer.write(*left);
                    });

                    // Process based on current mode
                    let apply_effects = match current_mode {
                        ProcessingProfile::Autotune => true,  // Apply autotune
                        ProcessingProfile::Vocode => true,    // Apply vocoder
                        ProcessingProfile::Dry => false,      // Passthrough (no processing)
                    };

                    // Get the processed audio if effects are enabled
                    if apply_effects {
                        ctx.shared.out_buffer.lock(|out_buffer| {
                            out_sample = out_buffer.read_and_reset();
                        });
                    }

                    // ************** SAMPLE-RATE REDUCE **************
                    // Apply sample rate reduction effect
                    let mut sr_factor = 1;
                    ctx.shared.app_state_machine.lock(|msm| {
                        sr_factor = msm.snapshot().crush1; 
                    });

                    // Apply the effect
                    ctx.shared.sr_hold_counter.lock(|hold_ctr| {
                        ctx.shared.sr_held_value.lock(|held_val| {
                            out_sample = sample_rate_reduce(out_sample, sr_factor, hold_ctr, held_val);
                        });
                    });

                    // ************** BIT DEPTH REDUCE **************
                    // 3) Get bit depth from your menu
                    let mut bit_depth = 32;
                    ctx.shared.app_state_machine.lock(|msm| { 
                        bit_depth = msm.snapshot().crush2;
                    });
                    out_sample = bitcrush(out_sample, bit_depth as u8);

                    // Normalize final output
                    out_sample = normalize_sample(out_sample, 0.8);

                    // Check and handle hop counter
                    let mut local_hop_counter: u32 = 0;
                    ctx.shared.hop_counter.lock(|count| {
                        local_hop_counter = *count;
                    });
                    if local_hop_counter >= HOP_SIZE as u32 {
                        ctx.shared.hop_counter.lock(|count| {
                            *count = 0;
                        });

                        // Run FFT Process in new software task
                        if dma1_stream0_software_task::spawn().is_err() {
                            warn!("Could not unwrap software task - underrun error");
                        }

                        // Lock to advance the output buffer's hop
                        ctx.shared.out_buffer.lock(|out_buffer| {
                            out_buffer.next_hop();
                        });
                    }
                    ctx.shared.hop_counter.lock(|count| {
                        *count += 1;
                    });

                    // Output the processed audio or further processing
                    if audio.push_stereo((out_sample, out_sample)).is_err() {
                        warn!("Failed to write audio data");
                    }
                }
            } else {
                warn!("Error reading data!");
            }
        }

        #[task(binds = TIM2, local = [
            knob_1, 
            timer2, 
            display, 
            col_1_pin, 
            col_2_pin, 
            col_3_pin, 
            row_1_pin, 
            row_2_pin, 
            row_3_pin, 
            row_4_pin,
            encoder_button,
            ], shared = [app_state_machine, old_matrix_state])]
        fn interface_handler(mut ctx: interface_handler::Context) {
            ctx.local.timer2.clear_irq();

            let mut update_state: bool = false;

            let new_matrix_state = scan_button_matrix(
                ctx.local.col_1_pin,
                ctx.local.col_2_pin,
                ctx.local.col_3_pin,
                ctx.local.row_1_pin,
                ctx.local.row_2_pin,
                ctx.local.row_3_pin,
                ctx.local.row_4_pin,
            );


            // info!("{:?}", new_matrix_state);
            // Process button matrix changes
            for row in 0..4 {
                for col in 0..3 {
                    let was_pressed = ctx.shared.old_matrix_state.lock(|oms| {
                        oms[row][col]
                    });
                    let is_pressed = new_matrix_state[row][col];

                    // If there is a change, decide how to handle it
                    if is_pressed != was_pressed {
                        ctx.shared.old_matrix_state.lock(|oms| {
                            oms[row][col] = is_pressed;
                        });
                        if is_pressed {
                            update_state = true;
                            // Button has just been pressed
                            ctx.shared.app_state_machine.lock(|msm| {
                                msm.handle_event(handle_button_press(row, col, msm.snapshot().current_state));
                            });
                        } else {
                            update_state = true;
                            // Button has just been released
                            ctx.shared.app_state_machine.lock(|msm| {
                                msm.handle_event(handle_button_release(row, col, msm.snapshot().current_state));
                            });
                        }
                    }
                }
            }

            // Handle encoder button - single press vs double press
            ctx.local.encoder_button.update();

            // Check for double press first
            if ctx.local.encoder_button.is_double() {
                info!("Encoder double press detected!");
                update_state = true;
                
                ctx.shared.app_state_machine.lock(|msm| {
                    msm.handle_event(state_machines::AppEvent::EncoderDoublePress);
                });
            } 
            // Check for single press if not a double press
            else if ctx.local.encoder_button.is_falling() {
                info!("Encoder single press detected!");
                update_state = true;
                
                ctx.shared.app_state_machine.lock(|msm| {
                    msm.handle_event(state_machines::AppEvent::EncoderPress);
                });
            }

            // Handle encoder rotation
            match ctx.local.knob_1.rotary_encoder.update() {
                Direction::Clockwise => {
                    update_state = true;
                    ctx.shared.app_state_machine.lock(|msm| {
                        msm.handle_event(state_machines::AppEvent::EncoderRotate(1));
                    });
                }
                Direction::Anticlockwise => {
                    update_state = true;
                    ctx.shared.app_state_machine.lock(|msm| {
                        msm.handle_event(state_machines::AppEvent::EncoderRotate(-1));
                    });
                }
                Direction::None => { }
            }


             // Update display if state changed
            ctx.shared.app_state_machine.lock(|msm| {
                if update_state {
                    let snapshot = msm.snapshot();
                    info!("state - {:?} -", snapshot.current_state);
                    
                    match snapshot.current_state {
                        // For splash screen
                        AppState::Splash => {
                            draw_splash_screen(ctx.local.display);
                        },
                        
                        // For processing modes
                        AppState::Processing(mode) => {
                            draw_processing_screen(
                                mode, 
                                snapshot.key, 
                                snapshot.octave, 
                                snapshot.note, 
                                snapshot.volume, 
                                ctx.local.display
                            );
                        },
                        
                        // For effects screen
                        AppState::EffectsProfile(mode) => {
                            draw_effects_screen(
                                mode,
                                snapshot.key,
                                ctx.local.display
                            );
                        },
                        
                        // For menu screens
                        AppState::Menu(nav_state, _) => {
                            match nav_state {
                                MenuState::Selecting(idx) => {
                                    let menu_context = msm.current();
                                    draw_menu_screen(
                                        menu_context.previous_item,
                                        menu_context.current_item,
                                        menu_context.next_item,
                                        false,
                                        ctx.local.display
                                    );
                                },
                                MenuState::Editing(idx) => {
                                    let menu_context = msm.current();
                                    draw_menu_screen(
                                        menu_context.previous_item,
                                        menu_context.current_item,
                                        menu_context.next_item,
                                        true,
                                        ctx.local.display
                                    );
                                }
                            }
                        }
                    }
                    
                    ctx.local.display.flush().expect("could not draw to screen");
                    update_state = false;
                }
            });

        }

        /// FFT TASK
        #[task(shared = [
        in_buffer,
        out_buffer,
        last_input_phases,
        last_output_phases,
        synthesis_magnitudes,
        synthesis_frequencies,
        previous_pitch_shift_ratio,
        app_state_machine
    ], local = [], priority = 7)]
        fn dma1_stream0_software_task(mut ctx: dma1_stream0_software_task::Context) {
            // START ACTUAL FFT PROCESSING
            let analysis_window_buffer: [f32; FFT_SIZE] = hann_window::HANN_WINDOW;

            let mut unwrapped_buffer: [f32; FFT_SIZE] = hann_window::HANN_WINDOW;
            let mut full_spectrum: [microfft::Complex32; FFT_SIZE] =
                [microfft::Complex32 { re: 0.0, im: 0.0 }; FFT_SIZE];
            let mut analysis_magnitudes = [0.0; FFT_SIZE / 2];
            let mut analysis_frequencies = [0.0; FFT_SIZE / 2];
            let mut _synthesis_count = [0; FFT_SIZE / 2];

            // Copy buffer into FFT input, starting one window ago
            ctx.shared.in_buffer.lock(|in_buffer| {
                in_buffer.push_read_back(FFT_SIZE - HOP_SIZE);
            });

            for n in 0..FFT_SIZE {
                ctx.shared.in_buffer.lock(|in_buffer| {
                    unwrapped_buffer[n] *= in_buffer.read();
                });
            }

            // Process the FFT based on the time domain input
            let fft = microfft::real::rfft_1024(&mut unwrapped_buffer);

            // ANALYSIS
            for i in 0..fft.len() {
                // Turn real and imaginary components into amplitude and phase
                let amplitude = sqrtf(fft[i].re * fft[i].re + fft[i].im * fft[i].im);
                let phase = atan2f(fft[i].im, fft[i].re);

                //cut out noise
                // let mut magnitude_threshold = 0.05; // Adjust this threshold as needed
                // ctx.shared.app_state_machine.lock(|msm| {
                //     magnitude_threshold = msm.magnitude as f32 / 100.0;
                // });
                // if amplitude < magnitude_threshold {
                //     continue; // Skip this bin if the magnitude is too low
                // }

                // Calculate the phase difference in this bin between the last
                // hop and this one, which will indirectly give us the exact frequency
                let mut phase_diff = 0.0;
                ctx.shared.last_input_phases.lock(|last_input_phases| {
                    phase_diff = phase - last_input_phases[i];
                });

                // Subtract the amount of phase increment we'd expect to see based
                // on the centre frequency of this bin (2*pi*n/gFftSize) for this
                // hop size, then wrap to the range -pi to pi
                let bin_centre_frequency = 2.0 * PI * i as f32 / FFT_SIZE as f32;
                phase_diff = wrap_phase(phase_diff - bin_centre_frequency * HOP_SIZE as f32);

                // Find deviation from the centre frequency
                let bin_deviation = phase_diff * FFT_SIZE as f32 / HOP_SIZE as f32 / (2.0 * PI);

                // Add the original bin number to get the fractional bin where this partial belongs
                analysis_frequencies[i] = i as f32 + bin_deviation;
                // Save the magnitude for later
                analysis_magnitudes[i] = amplitude;

                // Save the phase for next hop
                ctx.shared.last_input_phases.lock(|last_input_phases| {
                    last_input_phases[i] = phase;
                });
            }

            // Zero out the synthesis bins, ready for new data (NOT done since it should already be zero)
            ctx.shared.synthesis_magnitudes.lock(|syn_mag| {
                for bin in syn_mag.iter_mut() {
                    *bin = 0.0;
                }
            });
            ctx.shared.synthesis_frequencies.lock(|syn_freq| {
                for bin in syn_freq.iter_mut() {
                    *bin = 0.0;
                }
            });

            let mut analysis_magnitudes_full = [0.0f32; FFT_SIZE];
            // Set the DC component.
            analysis_magnitudes_full[0] = analysis_magnitudes[0];
            // For bins 1 to FFT_SIZE/2 - 1, mirror the half-spectrum.
            for i in 1..(FFT_SIZE / 2) {
                analysis_magnitudes_full[i] = analysis_magnitudes[i];
                analysis_magnitudes_full[FFT_SIZE - i] = analysis_magnitudes[i];
            }

            // Now compute the envelope using cepstral smoothing.
            let envelope = cepstral_smoothing(&analysis_magnitudes_full);

            // 1) Create our carrier in freq domain: a harmonic stack at 440 Hz
            // let mut freq_domain_carrier: [microfft::Complex32; FFT_SIZE] = [microfft::Complex32 { re: 0.0, im: 0.0 }; FFT_SIZE];

            // let bin_for_440 = roundf(440.0 / BIN_WIDTH) as usize;

            // let max_harmonic = (FFT_SIZE / 2) / bin_for_440;

            // // Example amplitude roll-off for each harmonic. Tweak as you wish.
            // fn amplitude_function(h: usize) -> f32 {
            //     // A simple 1/h rolloff, or do something fancier
            //     100.0 / (h as f32)
            // }

            // // Populate the carrier bins for each harmonic
            // for h in 1..=max_harmonic {
            //     let bin = bin_for_440 * h;
            //     freq_domain_carrier[bin].re = amplitude_function(h);
            // }

            // // Mirror them for a real IFFT
            // for i in 1..(FFT_SIZE / 2) {
            //     freq_domain_carrier[FFT_SIZE - i] = freq_domain_carrier[i].conj();
            // }

            // // 2) Multiply the carrier bins by the voice envelope
            // for i in 0..(FFT_SIZE / 2) {
            //     let amp_scale = envelope[i];
            //     freq_domain_carrier[i].re *= amp_scale;
            //     freq_domain_carrier[i].im *= amp_scale;

            //     // Mirror side
            //     if i != 0 {
            //         freq_domain_carrier[FFT_SIZE - i].re *= amp_scale;
            //         freq_domain_carrier[FFT_SIZE - i].im *= amp_scale;
            //     }
            // }

            // // 3) Inverse FFT to get time-domain data
            // microfft::inverse::ifft_1024(&mut freq_domain_carrier);

            // // 4) Overlap-add or window it into out_buffer
            // for (n, val) in freq_domain_carrier.iter().enumerate() {
            //     let windowed_val = val.re * analysis_window_buffer[n]; // or skip the window if you prefer
            //     ctx.shared.out_buffer.lock(|out_buffer| {
            //         out_buffer.add_value(windowed_val);
            //     });
            // }


            // TODO: the fundimental can now be found from the spectral analysis
            // Get the fundamental frequency (Loudest)
            let fundamental_index = find_fundamental_frequency(&analysis_magnitudes);
            let harmonics = collect_harmonics(fundamental_index);

            // Exact frequency is tied to the bin.
            let exact_frequency = analysis_frequencies[fundamental_index] * BIN_WIDTH;

            // We cannot divide by 0
            if exact_frequency > 0.001 {
                let mut scale_frequencies = &C_MAJOR_SCALE_FREQUENCIES;

                let mut octave_factor = 1.0; // Adjust this threshold as needed
                ctx.shared.app_state_machine.lock(|msm| {
                    octave_factor = msm.snapshot().octave as f32 * 0.5;//todo snapshot?!?! nate halp
                });
                
                ctx.shared.app_state_machine.lock(|msm| {
                    scale_frequencies = get_scale_by_key(msm.snapshot().key);
                });
                let target_frequency =
                    find_nearest_note_in_key(exact_frequency, scale_frequencies);
                let current_pitch_shift_ratio = target_frequency / exact_frequency;

                let previous_pitch_shift_ratio = ctx
                    .shared
                    .previous_pitch_shift_ratio
                    .lock(|previous_pitch_shift_ratio| *previous_pitch_shift_ratio);

                let pitch_shift_ratio =
                    0.999 * current_pitch_shift_ratio + 0.001 * previous_pitch_shift_ratio;

                
                let mut formant_ratio = 1.0;
                // ctx.shared.app_state_machine.lock(|msm| {
                //     formant_ratio = 20.0;//msm.speed as f32 / 10.0;//TODO: change to real var
                // });

                // shift all bins by the ratio
                for i in 0..FFT_SIZE / 2 {
                    let amplitude_in = analysis_magnitudes[i];
                    let old_envelope = envelope[i].max(1e-9);
                    let new_bin = (floorf(i as f32 * pitch_shift_ratio + 0.5) * octave_factor) as usize;//*2 to test octave
                    if new_bin < FFT_SIZE / 2 {
                        // find new envelope at new_bin

                        // clamp so we don't go out of bounds
                        let mut shifted_env_bin_f32 = (i as f32 * formant_ratio)
                            .clamp(0.0, FFT_SIZE as f32 / 2.0 - 1.0); 
                        
                        shifted_env_bin_f32 = floorf(shifted_env_bin_f32 + 0.5);

                        let shifted_env_bin = shifted_env_bin_f32 as usize;

                        let shifted_env_bin = shifted_env_bin.min(FFT_SIZE/2 - 1);

                        let new_envelope = envelope[shifted_env_bin];
                        let adjusted_mag = amplitude_in * (new_envelope / old_envelope);
                        
                        ctx.shared
                            .synthesis_magnitudes
                            .lock(|synthesis_magnitudes| {
                                synthesis_magnitudes[new_bin] = adjusted_mag;
                                //synthesis_magnitudes[i] = analysis_magnitudes[i];
                            });
                        ctx.shared
                            .synthesis_frequencies
                            .lock(|synthesis_frequencies| {
                                synthesis_frequencies[new_bin] = analysis_frequencies[i] * pitch_shift_ratio * octave_factor;
                                //synthesis_frequencies[i] = analysis_frequencies[i];
                            });
                    }
                    // ctx.shared
                    //     .synthesis_magnitudes
                    //     .lock(|synthesis_magnitudes| {
                    //         //synthesis_magnitudes[new_bin] = adjusted_mag;
                    //         synthesis_magnitudes[i] = analysis_magnitudes[i];
                    //     });
                    // ctx.shared
                    //     .synthesis_frequencies
                    //     .lock(|synthesis_frequencies| {
                    //         //synthesis_frequencies[new_bin] = analysis_frequencies[i] * pitch_shift_ratio * octave_factor;
                    //         synthesis_frequencies[i] = analysis_frequencies[i];
                    //     });
                }
            }

            // SYNTHESIS
            for i in 0..FFT_SIZE / 2 {
                let amplitude = ctx
                    .shared
                    .synthesis_magnitudes
                    .lock(|synthesis_magnitudes| synthesis_magnitudes[i]);
                let bin_deviation = ctx
                    .shared
                    .synthesis_frequencies
                    .lock(|synthesis_frequencies| synthesis_frequencies[i] - i as f32);
                let mut phase_diff = bin_deviation * 2.0 * PI * HOP_SIZE as f32 / FFT_SIZE as f32;
                let bin_centre_frequency = 2.0 * PI * i as f32 / FFT_SIZE as f32;
                phase_diff += bin_centre_frequency * HOP_SIZE as f32;

                let mut out_phase = 0.0;
                ctx.shared.last_output_phases.lock(|last_output_phases| {
                    out_phase = wrap_phase(last_output_phases[i] + phase_diff);
                });

                fft[i].re = amplitude * cosf(out_phase);
                fft[i].im = amplitude * sinf(out_phase);

                // Also store the complex conjugate in the upper half of the spectrum
                full_spectrum[i] = fft[i]; // First half directly
                if i > 0 && i < (FFT_SIZE / 2) {
                    // Conjugate symmetry for the second half
                    full_spectrum[FFT_SIZE - i] = fft[i].conj();
                }

                // Save the phase for the next hop
                ctx.shared.last_output_phases.lock(|last_output_phases| {
                    last_output_phases[i] = out_phase;
                });
            }

            // Run the inverse FFT
            let res = microfft::inverse::ifft_1024(&mut full_spectrum);

            // Add time domain into the output buffer
            for (n, val) in res.iter().enumerate() {
                let windowed_val = val.re * analysis_window_buffer[n]; // Window again and scale
                ctx.shared.out_buffer.lock(|out_buffer| {
                    out_buffer.add_value(windowed_val);
                });
            }
        }

        #[inline(always)]
        fn wrap_phase(phase_in: f32) -> f32 {
            if phase_in >= 0.0 {
                return fmodf(phase_in + PI, 2.0 * PI) - PI;
            }
            fmodf(phase_in - PI, -2.0 * PI) + PI
        }

        //find a n number of harmonics by the fundamental index
        #[inline(always)]
        fn collect_harmonics(fundamental_index: usize) -> [usize; 4] {
            let mut harmonics = [0; 4];
            for n in 1..=4 {
                let harmonic_index = fundamental_index * n;
                harmonics[n - 1] = harmonic_index;
            }
            harmonics
        }

        fn initialize_display(
            i2c: I2c<stm32h7xx_hal::stm32::I2C1>,
        ) -> Ssd1306<
            I2CInterface<I2c<stm32h7xx_hal::stm32::I2C1>>,
            DisplaySize128x64,
            BufferedGraphicsMode<DisplaySize128x64>,
        > {
            // Create the I2C interface for the display
            let interface = I2CDisplayInterface::new(i2c);

            // Initialize the display with the size 128x64 and default rotation
            let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
                .into_buffered_graphics_mode();

            // Initialize the display
            display.init().expect("Failed to init display");

            display
        }

        fn draw_splash_screen(display: &mut LcdDisplay) {
            display.clear();
            
            // Display the splash image
            let bmp: Bmp<BinaryColor> = Bmp::from_slice(include_bytes!("../assets/synthophoneV2.bmp"))
                .expect("Could not load splash BMP");
                
            let image = Image::new(&bmp, Point::new(0, 0));
            image.draw(display).expect("Failed to display splash image");
        }

        fn draw_processing_screen(
            mode: ProcessingProfile, 
            key: i32, 
            octave: i32, 
            note: i32, 
            volume: i32, 
            display: &mut LcdDisplay
        ) {
            display.clear();
            
            // Load the background image
            let bmp: Bmp<BinaryColor> = Bmp::from_slice(include_bytes!("../assets/SynthphoneE_MenuBlank.bmp"))
                .expect("Could not load BMP");
            
            let image = Image::new(&bmp, Point::new(0, 0));
            image.draw(display).expect("Draw background");
            
            // Styles for text
            let text_style = MonoTextStyleBuilder::new()
                .font(&FONT_6X9)
                .text_color(BinaryColor::Off) 
                .background_color(BinaryColor::On) 
                .build();
        
            let h1_style = MonoTextStyleBuilder::new()
                .font(&FONT_10X20)
                .text_color(BinaryColor::Off)   
                .background_color(BinaryColor::On) 
                .build();
            
            // Format mode name
            let mode_name = match mode {
                ProcessingProfile::Autotune => "AUTO",
                ProcessingProfile::Vocode => "VOCODE",
                ProcessingProfile::Dry => "DRY",
            };
            
            // Create text buffers
            let mut key_buffer: String<2> = String::new();
            write!(&mut key_buffer, "{}", get_key_name(key))
                .expect("Failed converting key to string");
            
            let mut mode_buffer: String<6> = String::new();
            write!(&mut mode_buffer, "{}", mode_name)
                .expect("Failed converting mode to string");
            
            let mut note_buffer: String<2> = String::new();
            info!("note - {:?} -", note);
            write!(&mut note_buffer, "{}", get_note_name(note, get_key(key)))
                .expect("Failed converting note to string");
            
            let mut oct_buffer: String<1> = String::new();
            write!(&mut oct_buffer, "{octave}")
                .expect("Failed converting octave to string");
            
            let mut vol_buffer: String<3> = String::new();
            write!(&mut vol_buffer, "{volume}")
                .expect("Failed converting volume to string");
            
            // Draw text
            draw_text(display, &key_buffer, Point::new(26, 3), &text_style);
            draw_text(display, &mode_buffer, Point::new(80, 3), &text_style);
            draw_centered_text(display, &note_buffer, Point::new(62, 15), h1_style);
            draw_text(display, &oct_buffer, Point::new(14, 28), &text_style);
            draw_text(display, &vol_buffer, Point::new(112, 28), &text_style);
        }

        fn draw_effects_screen(
            mode: ProcessingProfile,
            key: i32,
            display: &mut LcdDisplay
        ) {
            display.clear();
            
            // Load the background image
            let bmp: Bmp<BinaryColor> = Bmp::from_slice(include_bytes!("../assets/SynthphoneE_MenuBlank.bmp"))
                .expect("Could not load BMP");
            
            let image = Image::new(&bmp, Point::new(0, 0));
            image.draw(display).expect("Draw background");
            
            // Styles for text
            let text_style = MonoTextStyleBuilder::new()
                .font(&FONT_6X9)
                .text_color(BinaryColor::Off) 
                .background_color(BinaryColor::On) 
                .build();
        
            let h1_style = MonoTextStyleBuilder::new()
                .font(&FONT_10X20)
                .text_color(BinaryColor::Off)   
                .background_color(BinaryColor::On) 
                .build();
            
            // Format mode name
            let mode_name = match mode {
                ProcessingProfile::Autotune => "EFFECTS (Auto)",
                ProcessingProfile::Vocode => "EFFECTS (Vocode)",
                ProcessingProfile::Dry => "EFFECTS (Dry)",
            };

        }

        fn draw_menu_screen(
            prev: (&str, i32),
            current: (&str, i32),
            next: (&str, i32),
            is_editing: bool,
            display: &mut LcdDisplay
        ) {
            display.clear();
            
            // Styles for text
            let text_style = MonoTextStyleBuilder::new()
                .font(&FONT_6X9)
                .text_color(BinaryColor::On) 
                .background_color(BinaryColor::Off) 
                .build();
        
            let h1_style = MonoTextStyleBuilder::new()
                .font(&FONT_6X13)
                .text_color(BinaryColor::On)   
                .background_color(BinaryColor::Off) 
                .build();
            
            
            // Draw items
            let options = [prev, current, next];
            
            if is_editing {
                Line::new(Point::new(2, 22), Point::new(103, 22))
                    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 3))
                    .draw(display)
                    .expect("Failed to draw name underline");
            } else {
                Line::new(Point::new(108, 22), Point::new(123, 22))
                    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 3))
                    .draw(display)
                    .expect("Failed to draw value underline");
            }
            
            // Draw all menu items (previous, current, next)
            for (i, &option) in options.iter().enumerate() {
                let style = if i == 1 { h1_style } else { text_style };
                let y = 10 + (i as i32 * 12);
                
                // Draw option name
                Text::with_baseline(option.0, Point::new(5, y), style, Baseline::Middle)
                    .draw(display)
                    .expect("Failed to draw option name");
                
                // Draw option value
                let mut option_value_buffer: String<3> = String::new();
                write!(&mut option_value_buffer, "{}", option.1)
                    .expect("Failed converting option value to string");
                
                Text::with_baseline(&option_value_buffer, Point::new(110, y), style, Baseline::Middle)
                    .draw(display)
                    .expect("Failed to draw option value");
            }
        }

        fn draw_text<D>(
            display: &mut D,
            text: &str,
            position: Point,
            style: &MonoTextStyle<BinaryColor>,
        )
        where
            D: DrawTarget<Color = BinaryColor>,
        {
            Text::with_baseline(text, position, *style, Baseline::Middle).draw(display);
        }
        
        fn draw_centered_text<D>(
            display: &mut D,
            text: &str,
            center: Point,
            style: MonoTextStyle<BinaryColor>,
        )
        where
            D: DrawTarget<Color = BinaryColor>,
        {
            // 1) Create a Text at (0,0) just to measure it
            let text_obj = Text::with_baseline(text, Point::zero(), style, Baseline::Top);
        
            // 2) bounding_box() gives us the width/height of this text
            let bbox = text_obj.bounding_box();
            let text_width = bbox.size.width as i32;
            let text_height = bbox.size.height as i32;
        
            // 3) Compute a new top-left so that the text is centered on `center`
            let draw_x = center.x - text_width / 2;
            let draw_y = center.y - text_height / 2;
        
            // 4) Draw the text at the adjusted position
            Text::with_baseline(text, Point::new(draw_x, draw_y), style, Baseline::Top)
                .draw(display);
        }

        /// Scans a 4x3 button matrix without diodes.
        /// The returned structure could be a 2D array or vector of booleans.
        /// For simplicity, this returns a nested array: [4 rows][3 cols].
        fn scan_button_matrix(
            col_1: &mut Daisy21<Output<PushPull>>,
            col_2: &mut Daisy20<Output<PushPull>>,
            col_3: &mut Daisy19<Output<PushPull>>,
            row_1: &Daisy15<Input>,
            row_2: &Daisy16<Input>,
            row_3: &Daisy17<Input>,
            row_4: &Daisy18<Input>,
        ) -> [[bool; 3]; 4] {
            let mut state = [[false; 3]; 4];
    
            // Helper closure to read rows
            let read_rows = |
                            r1: &Daisy15<Input>,
                            r2: &Daisy16<Input>,
                            r3: &Daisy17<Input>,
                            r4: &Daisy18<Input>| -> [bool; 4] {
                [
                    r1.is_low(), // true if button pressed
                    r2.is_low(),
                    r3.is_low(),
                    r4.is_low(),
                ]
            };
    

    
            // // Drive COL_2 low, others high
            col_1.set_high();
            col_2.set_low();
            col_3.set_high();
            let col2_rows = read_rows(row_1, row_2, row_3, row_4);
            for (r, pressed) in col2_rows.iter().enumerate() {
                state[r][1] = *pressed;
            }

            // Drive COL_1 low, others high
            col_2.set_high();
            col_1.set_low();
            col_3.set_high();
            let col1_rows = read_rows(row_1, row_2, row_3, row_4);
            for (r, pressed) in col1_rows.iter().enumerate() {
                state[r][0] = *pressed;
            }
    
            // Drive COL_3 low, others high
            col_1.set_high();
            col_2.set_high();
            col_3.set_low();
            let col3_rows = read_rows(row_1, row_2, row_3, row_4);
            for (r, pressed) in col3_rows.iter().enumerate() {
                state[r][2] = *pressed;
            }
    
            // Finally, return all keys states
            // state[row][col] = true means that button is pressed
            state
        }

        // Button press handling based on current state
        fn handle_button_press(row: usize, col: usize, current_state: AppState) -> state_machines::AppEvent {
            match current_state {
                // In Processing state - buttons are notes or key changes
                AppState::Processing(_) => {
                    match (row, col) {
                        // First 9 buttons (3x3 grid) are notes
                        (0, 0) => state_machines::AppEvent::KeypadPress(1),
                        (0, 1) => state_machines::AppEvent::KeypadPress(2),
                        (0, 2) => state_machines::AppEvent::KeypadPress(3),
                        (1, 0) => state_machines::AppEvent::KeypadPress(4),
                        (1, 1) => state_machines::AppEvent::KeypadPress(5),
                        (1, 2) => state_machines::AppEvent::KeypadPress(6),
                        (2, 0) => state_machines::AppEvent::KeypadPress(7),
                        (2, 1) => state_machines::AppEvent::KeypadPress(8),
                        (2, 2) => state_machines::AppEvent::KeypadPress(9),
                        (3, 0) => state_machines::AppEvent::KeypadPress(10),
                        (3, 1) => state_machines::AppEvent::KeypadPress(11),
                        (3, 2) => state_machines::AppEvent::KeypadPress(12),
                        
                        _ => state_machines::AppEvent::NoOp
                    }
                },
                
                // In Effects state - buttons control effects
                AppState::EffectsProfile(_) => {
                    match (row, col) {
                        // Row 1: Octave controls
                        (0, 0) => {
                            info!("Octave low enabled");
                            state_machines::AppEvent::NoOp
                        }
                        (0, 1) => {
                            info!("Octave normal enabled");
                            state_machines::AppEvent::NoOp    
                        }
                        (0, 2) => {
                            info!("Octave High enabled");
                            state_machines::AppEvent::NoOp    
                        }
                        // Row 2: Crush controls
                        (1, 0) => {
                            info!("Crush 1 enabled");
                            state_machines::AppEvent::NoOp
                        },
                        (1, 1) => {
                            info!("No crush");
                            state_machines::AppEvent::NoOp
                        },
                        (1, 2) => {
                            info!("Crush 2 enabled");
                            state_machines::AppEvent::NoOp
                        },
                        
                        // Row 3: Formant controls
                        (2, 0) => {
                            info!("Formant male");
                            state_machines::AppEvent::NoOp
                        },
                        (2, 1) => {
                            info!("No formant");
                            state_machines::AppEvent::NoOp
                        },
                        (2, 2) => {
                            info!("Formant female");
                            state_machines::AppEvent::NoOp
                        },
                        
                        // Row 4: Key and mode controls
                        (3, 0) => state_machines::AppEvent::KeyChange(-1), // Key down
                        (3, 1) => state_machines::AppEvent::CycleProcessingProfile, // Cycle mode
                        (3, 2) => state_machines::AppEvent::KeyChange(1), // Key up
                        
                        _ => state_machines::AppEvent::NoOp
                    }
                },
                
                // In Menu state - buttons go back to processing
                AppState::Menu(_, _) => {
                    state_machines::AppEvent::EncoderDoublePress // Exit menu on any button press
                },
                
                // In Splash state - any button exits splash
                AppState::Splash => {
                    state_machines::AppEvent::SplashComplete
                }
            }
        }
        
        fn handle_button_release(row: usize, col: usize, current_state: AppState) -> state_machines::AppEvent {
            match current_state {
                // In Processing state - release notes
                AppState::Processing(_) => {
                    info!("note off");
                    match (row, col) {
                        (0, _) | (1, _) | (2, _) => state_machines::AppEvent::KeypadPress(0), // Stop the note
                        _ => state_machines::AppEvent::NoOp
                    }
                },
                
                // Other states - button releases don't matter
                _ => state_machines::AppEvent::NoOp
            }
        }
        
    }




}
