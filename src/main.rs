#![no_main]
#![no_std]

const DESIRED_FREQ: f32 = 880.0;
const SAMPLE_RATE: f32 = 48_014.312;
const FFT_SIZE: usize = 256;
const HOP_SIZE: usize = FFT_SIZE / 2; // 50% overlap

#[rtic::app(
    device = stm32h7xx_hal::stm32,
    peripherals = true,
)]
mod app {
    use libdaisy::{audio, gpio, hid, logger, system};
    use libm::{atan2f, cosf, sinf};
    use log::{info, warn};
    use microfft::complex::cfft_256;
    use microfft::inverse::ifft_256;
    use microfft::Complex32;
    use stm32h7xx_hal::{gpio::Input, time::MilliSeconds};

    use crate::{DESIRED_FREQ, FFT_SIZE, HOP_SIZE, SAMPLE_RATE};

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
        last_phase: [f32; FFT_SIZE],
        button: hid::Switch<gpio::Daisy28<Input>>,
    }

    #[init]
    fn init(ctx: init::Context) -> (Shared, Local, init::Monotonics) {
        logger::init();

        let mut core = ctx.core;
        let device = ctx.device;
        let ccdr = system::System::init_clocks(device.PWR, device.RCC, &device.SYSCFG);
        let mut system = libdaisy::system_init!(core, device, ccdr);

        let buffer = [(0.0, 0.0); audio::BLOCK_SIZE_MAX];
        let fft_input = [Complex32::new(0.0, 0.0); FFT_SIZE];
        let fft_output = [Complex32::new(0.0, 0.0); FFT_SIZE];
        let ifft_output = [Complex32::new(0.0, 0.0); FFT_SIZE];
        let envelope = [0.0; FFT_SIZE];
        let mut carrier = [Complex32::new(0.0, 0.0); FFT_SIZE];
        let window = generate_hanning_window();
        let overlap_buffer = [0.0; FFT_SIZE - HOP_SIZE];
        let last_phase = [0.0; FFT_SIZE];

        let daisy28 = system
            .gpio
            .daisy28
            .take()
            .expect("Failed to get pin daisy28!")
            .into_pull_up_input();

        let mut switch1 = hid::Switch::new(daisy28, hid::SwitchType::PullUp);
        switch1.set_double_thresh(Some(500));
        switch1.set_held_thresh(Some(150));

        for (i, c) in carrier.iter_mut().enumerate() {
            let phase = 2.0 * core::f32::consts::PI * DESIRED_FREQ * i as f32 / SAMPLE_RATE;
            *c = Complex32::new(cosf(phase), sinf(phase)); // Correct phase calculation
        }

        info!("Startup done!!");
        let mut timer2 = stm32h7xx_hal::timer::TimerExt::timer(
            device.TIM2,
            MilliSeconds::from_ticks(100).into_rate(),
            ccdr.peripheral.TIM2,
            &ccdr.clocks,
        );
        timer2.listen(stm32h7xx_hal::timer::Event::TimeOut);

        timer2.set_freq(MilliSeconds::from_ticks(500).into_rate());
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
                last_phase,
                button: switch1,
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

    #[task(binds = DMA1_STR1, local = [audio, buffer, fft_input, fft_output, ifft_output, envelope, carrier, window, overlap_buffer, last_phase, button], priority = 8)]
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
        let last_phase = ctx.local.last_phase;

        let switch1 = ctx.local.button;
        let button_pressed = switch1.is_held() || switch1.is_pressed();

        if audio.get_stereo(buffer) {
            if button_pressed {
                for chunk_start in (0..audio::BLOCK_SIZE_MAX).step_by(HOP_SIZE) {
                    if chunk_start + FFT_SIZE > audio::BLOCK_SIZE_MAX {
                        break; // Avoid out of bounds
                    }

                    for i in 0..FFT_SIZE {
                        let sample_index = chunk_start + i;
                        let sample = if sample_index < FFT_SIZE - HOP_SIZE {
                            overlap_buffer[sample_index] // Use overlap buffer for initial samples
                        } else {
                            buffer[sample_index - (FFT_SIZE - HOP_SIZE)].0 // Left channel
                        };
                        fft_input[i] = Complex32::new(sample * window[i], 0.0);
                    }

                    let fft_result = cfft_256(fft_input);
                    fft_output.copy_from_slice(fft_result);

                    for (i, &value) in fft_output.iter().enumerate() {
                        let current_phase = phase(&value);
                        let phase_difference = current_phase - last_phase[i];
                        let true_freq = DESIRED_FREQ
                            + phase_difference * SAMPLE_RATE
                                / (2.0 * core::f32::consts::PI * HOP_SIZE as f32);

                        last_phase[i] = current_phase; // Update last phase

                        envelope[i] = value.norm_sqr(); // Magnitude squared for envelope
                        carrier[i] = Complex32::new(cosf(true_freq), sinf(true_freq));
                        // Adjust carrier frequency based on true frequency
                    }

                    for i in 0..FFT_SIZE {
                        fft_output[i] = Complex32::new(
                            carrier[i].re * envelope[i],
                            carrier[i].im * envelope[i],
                        );
                    }

                    let ifft_result = ifft_256(fft_output);
                    ifft_output.copy_from_slice(ifft_result);

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
            }

            for (left, right) in buffer.iter() {
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
            window[i] = 0.5
                * (1.0 - libm::cosf(2.0 * core::f32::consts::PI * i as f32 / (FFT_SIZE as f32)));
        }
        window
    }

    fn phase(c: &Complex32) -> f32 {
        atan2f(c.im, c.re)
    }
}
