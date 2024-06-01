//! examples/passthru.rs
#![no_main]
#![no_std]

const DESIRED_FREQ: f32 = 440.0;
const SAMPLE_RATE: f32 = 48_014.312;
const FFT_SIZE: usize = 256;
const HOP_SIZE: usize = FFT_SIZE / 2; // 50% overlap

#[rtic::app(
    device = stm32h7xx_hal::stm32,
    peripherals = true,
)]
mod app {
    use libdaisy::{audio, logger, system};
    use libm::{sinf, cosf};
    use log::{info, warn};
    use microfft::complex::cfft_256;
    use microfft::inverse::ifft_256;
    use microfft::Complex32;

    use crate::{DESIRED_FREQ, SAMPLE_RATE, FFT_SIZE, HOP_SIZE};

    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        audio: audio::Audio,
        buffer: audio::AudioBuffer,
        fft_input: [Complex32; FFT_SIZE],
        fft_output: [Complex32; FFT_SIZE],
        ifft_output: [Complex32; FFT_SIZE],
        envelope: [f32; FFT_SIZE],
        carrier: [Complex32; FFT_SIZE],
        window: [f32; FFT_SIZE],
        overlap_buffer: [f32; FFT_SIZE - HOP_SIZE],
    }

    #[init]
    fn init(ctx: init::Context) -> (Shared, Local, init::Monotonics) {
        logger::init();

        let mut core = ctx.core;
        let device = ctx.device;
        let ccdr = system::System::init_clocks(device.PWR, device.RCC, &device.SYSCFG);
        let system = libdaisy::system_init!(core, device, ccdr);

        let buffer = [(0.0, 0.0); audio::BLOCK_SIZE_MAX];
        let fft_input = [Complex32::new(0.0, 0.0); FFT_SIZE];
        let fft_output = [Complex32::new(0.0, 0.0); FFT_SIZE];
        let ifft_output = [Complex32::new(0.0, 0.0); FFT_SIZE];
        let envelope = [0.0; FFT_SIZE];
        let mut carrier = [Complex32::new(0.0, 0.0); FFT_SIZE];
        let window = generate_hanning_window();
        let overlap_buffer = [0.0; FFT_SIZE - HOP_SIZE];

        for (i, c) in carrier.iter_mut().enumerate() {
            let phase = 2.0 * core::f32::consts::PI * DESIRED_FREQ * i as f32 / SAMPLE_RATE;
            *c = Complex32::new(cosf(phase), sinf(phase)); // Correct phase calculation
        }

        info!("Startup done!!");

        (
            Shared {},
            Local {
                audio: system.audio,
                buffer,
                fft_input,
                fft_output,
                ifft_output,
                envelope,
                carrier,
                window,
                overlap_buffer,
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

    #[task(binds = DMA1_STR1, local = [audio, buffer, fft_input, fft_output, ifft_output, envelope, carrier, window, overlap_buffer], priority = 8)]
    fn audio_handler(ctx: audio_handler::Context) {
        let audio = ctx.local.audio;
        let buffer = ctx.local.buffer;
        let fft_input = ctx.local.fft_input;
        let fft_output = ctx.local.fft_output;
        let ifft_output = ctx.local.ifft_output;
        let envelope = ctx.local.envelope;
        let carrier = ctx.local.carrier;
        let window = ctx.local.window;
        let overlap_buffer = ctx.local.overlap_buffer;

        if audio.get_stereo(buffer) {
            // Process the buffer in chunks with overlap
            for chunk_start in (0..audio::BLOCK_SIZE_MAX).step_by(HOP_SIZE) {
                if chunk_start + FFT_SIZE > audio::BLOCK_SIZE_MAX {
                    info!("BREAK");
                    break; // Avoid out of bounds
                }

                // Fill FFT input with overlapping data
                for i in 0..FFT_SIZE {
                    let sample_index = chunk_start + i;
                    let sample = if sample_index < FFT_SIZE - HOP_SIZE {
                        overlap_buffer[sample_index] // Use overlap buffer for initial samples
                    } else {
                        buffer[sample_index - (FFT_SIZE - HOP_SIZE)].0 // Left channel
                    };
                    fft_input[i] = Complex32::new(sample * window[i], 0.0);
                }

                // Perform FFT
                let fft_result = cfft_256(fft_input);
                fft_output.copy_from_slice(fft_result);

                // Extract the envelope of the modulator signal
                for (i, &value) in fft_output.iter().enumerate() {
                    envelope[i] = value.norm_sqr();
                }

                // Apply filtering and modulation on fft_output
                for i in 0..FFT_SIZE {
                    fft_output[i] = Complex32::new(carrier[i].re * envelope[i], carrier[i].im * envelope[i]);
                }

                // Perform inverse FFT to get the time-domain signal
                let ifft_result = ifft_256(fft_output);
                ifft_output.copy_from_slice(ifft_result);

                // Update buffer with processed FFT data
                for i in 0..FFT_SIZE {
                    let output_index = chunk_start + i;
                    if output_index < FFT_SIZE - HOP_SIZE {
                        overlap_buffer[output_index] = ifft_output[i].re; // Update overlap buffer
                    } else if output_index - (FFT_SIZE - HOP_SIZE) < audio::BLOCK_SIZE_MAX {
                        buffer[output_index - (FFT_SIZE - HOP_SIZE)].0 = ifft_output[i].re;
                        buffer[output_index - (FFT_SIZE - HOP_SIZE)].1 = ifft_output[i].re;
                    }
                }
            }

            // Push the processed audio back
            for (left, right) in buffer.iter() {
                info!("{:?} {:?}", left, right );
                if audio.push_stereo((*left, *right)).is_err() {
                    warn!("Failed to write audio data");
                }
            }
        } else {
            warn!("Error reading data!");
        }
    }

    fn generate_hanning_window() -> [f32; FFT_SIZE] {
        let mut window = [0.0; FFT_SIZE];
        for i in 0..FFT_SIZE {
            window[i] = 0.5 * (1.0 - libm::cosf(2.0 * core::f32::consts::PI * i as f32 / (FFT_SIZE as f32)));
        }
        window
    }
}
