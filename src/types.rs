use ssd1306::{mode::BufferedGraphicsMode, Ssd1306};

use stm32h7xx_hal::i2c::I2c;

pub type LcdDisplay = Ssd1306<
    ssd1306::prelude::I2CInterface<I2c<stm32h7xx_hal::stm32::I2C1>>,
    ssd1306::prelude::DisplaySize128x32,
    BufferedGraphicsMode<ssd1306::prelude::DisplaySize128x32>,
>;
