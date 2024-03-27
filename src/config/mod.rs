// config.rs
use stm32h7xx_hal::{gpio, rcc, dma, sai, pac, time::Hertz};

pub struct Peripherals {
    // Add fields for peripherals you initialize
}


// Continue with other setups like clock configuration

// Audio processing configurations
pub const BLOCK_SIZE_MAX: usize = 1024;
pub const DMA_BUFFER_SIZE: usize = BLOCK_SIZE_MAX * 2 * 2;

// DMA related configurations
pub const START_OF_DRAM2: u32 = 0x30000000;
pub const DMA_MEM_SIZE: usize = 32 * 1024;

// Audio stream and sample rate configurations
pub const MAX_TRANSFER_SIZE: usize = BLOCK_SIZE_MAX * 2;
pub const MILLI: u32 = 1_000;
pub const AUDIO_FRAME_RATE_HZ: u32 = 1_000;
pub const AUDIO_BLOCK_SIZE: u16 = 48;
pub const AUDIO_SAMPLE_RATE: u32 = 48_000;

// System clock configurations
pub const CLOCK_RATE_HZ: u32 = 480_000_000_u32;
pub const HSE_CLOCK_MHZ: u32 = 16_000_000; // Hertz::MHz(16) equivalent

// Heap configurations for the allocator
pub const HEAP_START: usize = 0x24020000;
pub const HEAP_SIZE: usize = (512 - 128) * 1024; // 384KB

