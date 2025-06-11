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
mod types;

mod rtic_app {
    #[rtic::app(
    device = stm32h7xx_hal::stm32,
    peripherals = true,
    dispatchers = [DMA1_STR0]
    )]
    mod app {
        use crate::{
            autotune::{circular_buffer::CircularBuffer, oscillator::{Oscillator, Waveform}}, constants::{BLOCK_SIZE, BUFFER_SIZE, FFT_SIZE, HOP_SIZE, SAMPLE_RATE},
        };
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
        use rotary_encoder_embedded::standard::StandardMode;
        use rotary_encoder_embedded::RotaryEncoder;
        use ssd1306::{mode::BufferedGraphicsMode, prelude::*, I2CDisplayInterface, Ssd1306};
        use state_machines::AppStateMachine;
        use stm32h7xx_hal::{
            i2c::{I2c, I2cExt},
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

        pub struct Knob {
            pub rotary_encoder: RotaryEncoder<StandardMode, Daisy3<Input>, Daisy4<Input>>,
            value: u8,
        }

        impl Knob {
            pub fn new(
                rotary_encoder: RotaryEncoder<StandardMode, Daisy3<Input>, Daisy4<Input>>,
            ) -> Knob {
                Knob {
                    rotary_encoder: rotary_encoder,
                    value: 0_u8,
                }
            }
        }

        #[shared]
        struct Shared {
            in_buffer: [f32; BUFFER_SIZE],
            out_buffer: [f32; BUFFER_SIZE],
            last_input_phases: [f32; FFT_SIZE],
            last_output_phases: [f32; FFT_SIZE],
            synthesis_magnitudes: [f32; FFT_SIZE],
            synthesis_frequencies: [f32; FFT_SIZE],
            previous_pitch_shift_ratio: f32,
            hop_counter: u32,
            in_buffer_pointer: u32,
            in_buffer_pointer_cached: u32,
            out_buffer_write_pointer: u32,
            out_buffer_read_pointer: u32,
            app_state_machine: AppStateMachine,
            old_matrix_state: [[bool; 3]; 4],
            // For sample-rate reduction
            sr_hold_counter: i32,
            sr_held_value: f32,
            carrier_osc: Oscillator,
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

            let mut encoder_button = hid::Switch::new(encoder_sw_pin, hid::SwitchType::PullUp);
            encoder_button.set_double_thresh(Some(100));

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
                    in_buffer: [0.0; BUFFER_SIZE],
                    out_buffer: [0.0; BUFFER_SIZE],
                    last_input_phases: [0.0; FFT_SIZE],
                    last_output_phases: [0.0; FFT_SIZE],
                    synthesis_magnitudes: [0.0; FFT_SIZE],
                    synthesis_frequencies: [0.0; FFT_SIZE],
                    previous_pitch_shift_ratio: 1.0,
                    hop_counter: 0,
                    in_buffer_pointer: 0,
                    in_buffer_pointer_cached: 0,
                    out_buffer_write_pointer: FFT_SIZE as u32 + (2 * HOP_SIZE) as u32,
                    out_buffer_read_pointer: 0,
                    app_state_machine: AppStateMachine::new(),
                    old_matrix_state: [[false; 3]; 4],
                    sr_hold_counter: 0,
                    sr_held_value: 0.0,
                    carrier_osc: Oscillator::new(55.0, SAMPLE_RATE, Waveform::Saw),
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
        in_buffer_pointer,
        in_buffer_pointer_cached,
        out_buffer_write_pointer,
        out_buffer_read_pointer,
        app_state_machine,
        sr_hold_counter,
        sr_held_value,
        carrier_osc,
    ], priority = 8)]
        fn update_handler(mut ctx: update_handler::Context) {
            crate::handler::update_handler(ctx.local.audio, ctx.local.buffer, &mut ctx.shared);
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
            crate::handler::interface_handler(ctx.local, &mut ctx.shared);
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
        app_state_machine,
        in_buffer_pointer,
        in_buffer_pointer_cached,
        out_buffer_write_pointer,
        out_buffer_read_pointer,
        carrier_osc,
    ], local = [], priority = 7)]
        fn dma1_stream0_software_task(mut ctx: dma1_stream0_software_task::Context) {
            // Call audio processing from handler module
            crate::handler::dma1_stream0_software_task(&mut ctx.shared);
        }
    }
}

pub use rtic_app::app::*;
