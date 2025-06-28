use libdaisy::{gpio::*, prelude::Input};
use rotary_encoder_embedded::{standard::StandardMode, RotaryEncoder};
use ssd1306::{mode::BufferedGraphicsMode, Ssd1306};
use stm32h7xx_hal::i2c::I2c;

pub(crate) type LcdDisplay = Ssd1306<
    ssd1306::prelude::I2CInterface<I2c<stm32h7xx_hal::stm32::I2C1>>,
    ssd1306::prelude::DisplaySize128x32,
    BufferedGraphicsMode<ssd1306::prelude::DisplaySize128x32>,
>;

pub(crate) struct Knob {
    pub(crate) rotary_encoder: RotaryEncoder<StandardMode, Daisy3<Input>, Daisy4<Input>>,
    // pub(crate) value: u8,
}

impl Knob {
    pub(crate) fn new(
        rotary_encoder: RotaryEncoder<StandardMode, Daisy3<Input>, Daisy4<Input>>,
    ) -> Knob {
        Knob {
            rotary_encoder,
            // value: 0_u8,
        }
    }
}
