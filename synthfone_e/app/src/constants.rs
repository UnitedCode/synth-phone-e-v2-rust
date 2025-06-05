pub const SAMPLE_RATE: f32 = 48_014.312;
pub const FFT_SIZE: usize = 1024;
pub const BUFFER_SIZE: usize = FFT_SIZE * 2;
pub const HOP_SIZE: usize = 256;
pub const BLOCK_SIZE: usize = 2;
pub const BIN_WIDTH: f32 = SAMPLE_RATE as f32 / FFT_SIZE as f32 * 2.0;
