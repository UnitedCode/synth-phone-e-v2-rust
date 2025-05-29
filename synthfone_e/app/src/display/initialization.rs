use crate::types::LcdDisplay;
use ssd1306::{
    mode::DisplayConfig,
    prelude::{DisplayRotation, DisplaySize128x32},
    I2CDisplayInterface, Ssd1306,
};
use stm32h7xx_hal::i2c::I2c;

pub fn initialize_display(i2c: I2c<stm32h7xx_hal::stm32::I2C1>) -> LcdDisplay {
    // Create the I2C interface for the display
    let interface = I2CDisplayInterface::new(i2c);

    // Initialize the display with the size 128x32 and default rotation
    let mut display = Ssd1306::new(interface, DisplaySize128x32, DisplayRotation::Rotate0)
        .into_buffered_graphics_mode();

    // Initialize the display
    display.init().expect("Failed to init display");

    display
}
