//! examples/passthru.rs
#![no_main]
#![no_std]

const DESIRED_FREQ: f32 = 440.0;
const SAMPLE_RATE: f32 = 48_014.312;

#[rtic::app(
    device = stm32h7xx_hal::stm32,
    peripherals = true,
)]
mod app {
    use libdaisy::{audio, logger, system};
    use libm::{sinf, cosf};
    use log::{info, warn};
    use microfft::complex::{cfft_256};
    use microfft::inverse::ifft_256;
    use microfft::Complex32;

    use crate::{DESIRED_FREQ, SAMPLE_RATE};

    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        audio: audio::Audio,
        buffer: audio::AudioBuffer,
        fft_input: [Complex32; 256],
        fft_output: [Complex32; 256],
        ifft_output: [Complex32; 256],
        envelope: [f32; 256],
        carrier: [Complex32; 256],
    }

    #[init]
    fn init(ctx: init::Context) -> (Shared, Local, init::Monotonics) {
        logger::init();

        let mut core = ctx.core;
        let device = ctx.device;
        let ccdr = system::System::init_clocks(device.PWR, device.RCC, &device.SYSCFG);
        let system = libdaisy::system_init!(core, device, ccdr);

        let buffer = [(0.0, 0.0); audio::BLOCK_SIZE_MAX];
        let fft_input = [Complex32::new(0.0, 0.0); 256];
        let fft_output = [Complex32::new(0.0, 0.0); 256];
        let ifft_output = [Complex32::new(0.0, 0.0); 256];
        let envelope = [0.0; 256];
        let mut carrier = [Complex32::new(0.0, 0.0); 256];

        for (i, c) in carrier.iter_mut().enumerate() {
            let phase = 2.0 * core::f32::consts::PI * DESIRED_FREQ * i as f32 / SAMPLE_RATE;
            *c = Complex32::new(sinf(phase), cosf(phase));
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

    #[task(binds = DMA1_STR1, local = [audio, buffer, fft_input, fft_output, ifft_output, envelope, carrier], priority = 8)]
    fn audio_handler(ctx: audio_handler::Context) {
        let audio = ctx.local.audio;
        let buffer = ctx.local.buffer;
        let fft_input = ctx.local.fft_input;
        let fft_output = ctx.local.fft_output;
        let ifft_output = ctx.local.ifft_output;
        let envelope = ctx.local.envelope;
        let carrier = ctx.local.carrier;

        if audio.get_stereo(buffer) {
            // Convert time-domain samples to complex numbers for FFT
            for (i, (left, _)) in buffer.iter().enumerate().take(256) {
                fft_input[i] = Complex32::new(*left, 0.0);
            }

            // Perform FFT
            let fft_result = cfft_256(fft_input);
            fft_output.copy_from_slice(fft_result);

            // Extract the envelope of the modulator signal
            for (i, &value) in fft_output.iter().enumerate() {
                envelope[i] = value.norm_sqr();
            }

            // Apply filtering and modulation on fft_output
            for i in 0..256 {
                fft_output[i] = Complex32::new(carrier[i].re * envelope[i], carrier[i].im * envelope[i]);
            }

            // Perform inverse FFT to get the time-domain signal
            let ifft_result = ifft_256(fft_output);
            ifft_output.copy_from_slice(ifft_result);

            // Update buffer with processed FFT data
            for (i, (left, right)) in buffer.iter_mut().enumerate().take(256) {
                *left = ifft_output[i].re;
                *right = ifft_output[i].re;
            }

            // Push the processed audio back
            for (left, right) in buffer.iter() {
                info!("{} {}", *left, *right);

                if audio.push_stereo((*left, *right)).is_err() {
                    warn!("Failed to write audio data");
                }
            }
        } else {
            warn!("Error reading data!");
        }
    }
}
