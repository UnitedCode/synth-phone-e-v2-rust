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
use autotune;
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
            },
            process_frequencies::find_fundamental_frequency,
        };
        use heapless::String;
        use core::fmt::Write;
        use core::f32::consts::PI;
        use embedded_graphics::{
            mono_font::{ascii::FONT_6X10, MonoTextStyle},
            image::Image,
            pixelcolor::BinaryColor,
            prelude::*,
            text::{Baseline, Text},
        };
        use libdaisy::{audio, hid, logger, prelude::{Output, PushPull, Input}, system, gpio::*};
        use libm::{atan2f, cosf, floorf, fmodf, sinf, sqrtf};
        use log::{info, warn};
        use state_machines::MenuStateMachine;
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
            menu_state_machine: MenuStateMachine,
        }
        
        #[local]
        struct Local {
            audio: audio::Audio,
            buffer: audio::AudioBuffer,
            button: hid::Switch<Daisy28<Input>>,
            timer2: Timer<stm32::TIM2>,
            knob_1: Knob,
            display: Ssd1306<ssd1306::prelude::I2CInterface<I2c<stm32h7xx_hal::stm32::I2C1>>, ssd1306::prelude::DisplaySize128x32, BufferedGraphicsMode<ssd1306::prelude::DisplaySize128x32>>,
            col_1_pin: Daisy21<Output<PushPull>>,
            col_2_pin: Daisy20<Output<PushPull>>,
            col_3_pin: Daisy19<Output<PushPull>>,
            row_1_pin: Daisy15<Input>,
            row_2_pin: Daisy16<Input>,
            row_3_pin: Daisy17<Input>,
            row_4_pin: Daisy18<Input>,
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
            let text_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);

            Text::with_baseline("Nathan is wrong! its a fallacy", Point::zero(), text_style, Baseline::Top)
                .draw(&mut display)
                .unwrap();

            // Send the buffer to the display
            display.flush().expect("Could not write to display");

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
                    menu_state_machine: MenuStateMachine::new(),
                },
                Local {
                    audio: system.audio,
                    buffer,
                    button: switch1,
                    timer2,
                    knob_1,
                    display,
                    col_1_pin, col_2_pin, col_3_pin, row_1_pin, row_2_pin, row_3_pin, row_4_pin
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
        menu_state_machine,
    ], priority = 8)]
        fn update_handler(mut ctx: update_handler::Context) {
            let audio = ctx.local.audio;
            let buffer = ctx.local.buffer;
            let switch1 = ctx.local.button;
            let button_pressed = switch1.is_held() || switch1.is_pressed();

            if audio.get_stereo(buffer) {
                for (left, _right) in &buffer.as_slice()[..BLOCK_SIZE] {
                    let mut out_sample = *left;

                    // Lock to write to in_buffer
                    ctx.shared.in_buffer.lock(|in_buffer| {
                        in_buffer.write(*left);
                    });

                    // Lock to read from out_buffer and reset the value
                    ctx.shared.out_buffer.lock(|out_buffer| {
                        if button_pressed {
                            out_sample = out_buffer.read_and_reset();
                        } else {
                            out_sample = *left
                        }
                    });

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

        #[task(binds = TIM2, local = [knob_1, timer2, display, col_1_pin, col_2_pin, col_3_pin, row_1_pin, row_2_pin, row_3_pin, row_4_pin], shared = [])]
        fn interface_handler(ctx: interface_handler::Context) {
            ctx.local.timer2.clear_irq();

            let matrix_state = scan_button_matrix(
                ctx.local.col_1_pin,
                ctx.local.col_2_pin,
                ctx.local.col_3_pin,
                ctx.local.row_1_pin,
                ctx.local.row_2_pin,
                ctx.local.row_3_pin,
                ctx.local.row_4_pin,
            );

            info!("{:?}", matrix_state);

            let text_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);

            match ctx.local.knob_1.rotary_encoder.update() {
                Direction::Clockwise => {

                    if ctx.local.knob_1.value < 255 { 

                        ctx.local.knob_1.value += 1;

                        let mut buffer: String<3> = String::new();
                        write!(&mut buffer, "{}", ctx.local.knob_1.value).unwrap();

                        ctx.local.display.clear();
                        Text::with_baseline(&buffer, Point::new(110, 20), text_style, Baseline::Top)
                            .draw(ctx.local.display)
                            .unwrap();
                        ctx.local.display.flush().expect("could not draw to screen");
                        
                        info!("Value increased")
                    }
                }
                Direction::Anticlockwise => {
                    if ctx.local.knob_1.value > 0 {

                        ctx.local.knob_1.value -= 1;

                        let mut buffer: String<3> = String::new();
                        write!(&mut buffer, "{}", ctx.local.knob_1.value).unwrap();

                        ctx.local.display.clear();
                        Text::with_baseline(&buffer, Point::new(110, 20), text_style, Baseline::Top)
                            .draw(ctx.local.display)
                            .unwrap();
                        ctx.local.display.flush().expect("could not draw to screen");

                        info!("Value decreased")
                    }
                }
                Direction::None => {
                    
                }
            }

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
                let magnitude_threshold = 0.05; // Adjust this threshold as needed
                if amplitude < magnitude_threshold {
                    continue; // Skip this bin if the magnitude is too low
                }

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

            // Get the fundamental frequency (Loudest)
            let fundamental_index = find_fundamental_frequency(&analysis_magnitudes);
            // let harmonics = collect_harmonics(fundamental_index);

            // Exact frequency is tied to the bin.
            let exact_frequency = analysis_frequencies[fundamental_index] * BIN_WIDTH;

            // We cannot divide by 0
            if exact_frequency > 0.1 {
                let target_frequency =
                    find_nearest_note_in_key(exact_frequency, &C_MAJOR_SCALE_FREQUENCIES);
                let current_pitch_shift_ratio = target_frequency / exact_frequency;

                let previous_pitch_shift_ratio = ctx
                    .shared
                    .previous_pitch_shift_ratio
                    .lock(|previous_pitch_shift_ratio| *previous_pitch_shift_ratio);

                let pitch_shift_ratio =
                    0.99 * current_pitch_shift_ratio + 0.01 * previous_pitch_shift_ratio;

                // shift all bins by the ratio
                for i in 0..FFT_SIZE / 2 {
                    let new_bin = floorf(i as f32 * pitch_shift_ratio + 0.5) as usize;
                    if new_bin < FFT_SIZE / 2 {
                        ctx.shared
                            .synthesis_magnitudes
                            .lock(|synthesis_magnitudes| {
                                synthesis_magnitudes[new_bin] = analysis_magnitudes[i];
                            });
                        ctx.shared
                            .synthesis_frequencies
                            .lock(|synthesis_frequencies| {
                                synthesis_frequencies[new_bin] =
                                    analysis_frequencies[i] * pitch_shift_ratio;
                            });
                    }
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
    }


}
