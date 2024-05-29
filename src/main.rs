//! examples/passthru.rs
#![no_main]
#![no_std]
#[rtic::app(
    device = stm32h7xx_hal::stm32,
    peripherals = true,
)]
mod app {
    use core::cmp::Ordering;
    use libdaisy::{audio, hal::adc, logger, system};
    use log::{info, warn};
    use microfft::real::rfft_1024;

    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        audio: audio::Audio,
        buffer: audio::AudioBuffer,
        adc_buffer: [f32; 1024],
        last_pitch: f32,
    }

    #[init]
    fn init(ctx: init::Context) -> (Shared, Local, init::Monotonics) {
        logger::init();

        // Latest changes here. This approach allows you to
        // access peripherals and resources that were simply
        // moved out of the function in the previous implementation.
        let mut core = ctx.core;
        let device = ctx.device;
        let ccdr = system::System::init_clocks(device.PWR, device.RCC, &device.SYSCFG);
        let system = libdaisy::system_init!(core, device, ccdr);
        let buffer = [(0.0, 0.0); audio::BLOCK_SIZE_MAX];
        let adc_buffer = [0.0; 1024];
        let last_pitch = 0.0;

        info!("Startup done!!");

        (
            Shared {},
            Local {
                audio: system.audio,
                buffer,
                adc_buffer,
                last_pitch,
            },
            init::Monotonics(),
        )
    }

    // Non-default idle ensures chip doesn't go to sleep which causes issues for
    // probe.rs currently
    #[idle]
    fn idle(_ctx: idle::Context) -> ! {
        loop {
            // info!("Idling");
            cortex_m::asm::nop();
        }
    }

    // Interrupt handler for audio
    #[task(binds = DMA1_STR1, local = [audio, buffer, adc_buffer, last_pitch], priority = 8)]
    fn audio_handler(ctx: audio_handler::Context) {
        let audio = ctx.local.audio;
        let buffer = ctx.local.buffer;
        let adc_buffer = ctx.local.adc_buffer;
        let last_pitch = ctx.local.last_pitch;

        if audio.get_stereo(buffer) {
            let buffer_size = buffer.len();
            info!("Audio buffer size: {}", buffer_size);

            let mut index = 0;
            for (left, _) in buffer.iter().take(1024) {
                adc_buffer[index] = *left;
                index += 1;
            }

            // Log the collected audio samples
            for (i, sample) in adc_buffer.iter().enumerate().take(10) {
                info!("Sample {}: {}", i, sample);
            }

            // Normalize the audio samples to remove any DC offset
            let mean = adc_buffer.iter().sum::<f32>() / adc_buffer.len() as f32;
            for sample in adc_buffer.iter_mut() {
                *sample -= mean;
            }

            // Perform FFT
            let spectrum = rfft_1024(adc_buffer);

            // Find the peak frequency by magnitude
            if let Some((peak_index, peak_value)) =
                spectrum.iter().enumerate().max_by(|(_, a), (_, b)| {
                    a.norm_sqr()
                        .partial_cmp(&b.norm_sqr())
                        .unwrap_or(Ordering::Equal)
                })
            {
                // Convert the peak frequency index to a real frequency
                // Example: Assuming a sampling rate of 48 kHz
                let freq = (peak_index as f32) * 48014.324 / 1024.0;

                // Debug information
                info!("Peak index: {}", peak_index);
                info!("Peak value: {:?}", peak_value);
                info!("Detected frequency: {} Hz", freq);
            } else {
                warn!("Failed to find peak frequency.");
            }

            // Passthrough audio
            for (left, right) in buffer.iter() {
                if audio.push_stereo((*left, *right)).is_err() {
                    warn!("Failed to write audio data");
                }
            }
        } else {
            warn!("Error reading data!");
        }
    }
}
