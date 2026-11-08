// Pinned libdaisy f08c01f requests 48_000 * 257 Hz for PLL3P, then SAI
// divides MCLK by 256. These programmed PLL3 values were verified in the
// production ELF: HSE=16 MHz, M=3, N=178, FRACN=827, P=77, MCKDIV=1.
// Keep synthesis and pitch analysis on that clock, not the nominal 48 kHz.
pub const SAMPLE_RATE: f32 =
    (16_000_000.0_f64 * (178.0 + 827.0 / 8192.0) / (3.0 * 77.0 * 256.0)) as f32;
pub const FFT_SIZE: usize = 1024;
pub const BUFFER_SIZE: usize = FFT_SIZE * 4;
pub const HOP_SIZE: usize = 256;
pub const BLOCK_SIZE: usize = 2;
