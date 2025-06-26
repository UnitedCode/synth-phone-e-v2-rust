use crate::constants::*;
use autotune::circular_buffer::CircularBuffer;
use libdaisy::{
    audio,
    gpio::*,
    hid,
    prelude::{Input, Output, PushPull},
};
use rotary_encoder_embedded::{standard::StandardMode, RotaryEncoder};
use ssd1306::{mode::BufferedGraphicsMode, Ssd1306};
use state_machines::AppStateMachine;
use stm32h7xx_hal::{i2c::I2c, stm32, timer::Timer};

pub type LcdDisplay = Ssd1306<
    ssd1306::prelude::I2CInterface<I2c<stm32h7xx_hal::stm32::I2C1>>,
    ssd1306::prelude::DisplaySize128x32,
    BufferedGraphicsMode<ssd1306::prelude::DisplaySize128x32>,
>;

pub struct Knob {
    pub rotary_encoder: RotaryEncoder<StandardMode, Daisy3<Input>, Daisy4<Input>>,
    pub value: u8,
}

impl Knob {
    pub fn new(rotary_encoder: RotaryEncoder<StandardMode, Daisy3<Input>, Daisy4<Input>>) -> Knob {
        Knob {
            rotary_encoder,
            value: 0_u8,
        }
    }
}

pub struct Shared {
    pub in_buffer: [f32; BUFFER_SIZE],
    pub out_buffer: [f32; BUFFER_SIZE],
    pub carrier_buffer: [f32; BUFFER_SIZE],
    pub last_input_phases: [f32; FFT_SIZE],
    pub last_output_phases: [f32; FFT_SIZE],
    pub synthesis_magnitudes: [f32; FFT_SIZE],
    pub synthesis_frequencies: [f32; FFT_SIZE],
    pub previous_pitch_shift_ratio: f32,
    pub hop_counter: u32,
    pub app_state_machine: AppStateMachine,
    pub old_matrix_state: [[bool; 3]; 4],
    // For sample-rate reduction
    pub sr_hold_counter: i32,
    pub sr_held_value: f32,
}

pub struct Local {
    pub audio: audio::Audio,
    pub buffer: audio::AudioBuffer,
    pub button: hid::Switch<Daisy28<Input>>,
    pub timer2: Timer<stm32::TIM2>,
    pub knob_1: Knob,
    pub display: LcdDisplay,
    pub col_1_pin: Daisy21<Output<PushPull>>,
    pub col_2_pin: Daisy20<Output<PushPull>>,
    pub col_3_pin: Daisy19<Output<PushPull>>,
    pub row_1_pin: Daisy15<Input>,
    pub row_2_pin: Daisy16<Input>,
    pub row_3_pin: Daisy17<Input>,
    pub row_4_pin: Daisy18<Input>,
    pub encoder_button: hid::Switch<Daisy2<Input>>,
}
