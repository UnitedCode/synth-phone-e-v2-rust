#![no_std]
#![no_main]
use synth_phone_e_v2_rust::utils::logger;
use cortex_m::asm::nop;
use cortex_m_rt::entry;
use panic_halt as _;
use stm32h7xx_hal as hal;
use rtt_target::{rtt_init_print, rprintln};

#[entry]
fn main() -> !{
    logger::init();
    rtt_init_print!();
    rprintln!("Hello World");
    let mut x: usize = 0;
    loop {
        x += 1;
        for _ in 0..x {
            nop();
        }
    }
}
