#![no_main]
#![no_std]
// #![deny(warnings)]
#![deny(unsafe_code)]
// #![deny(missing_docs)]

const SAMPLE_RATE: f32 = 48_014.312;
const FFT_SIZE: usize = 1024;
const BUFFER_SIZE: usize = FFT_SIZE + 1024;
const HOP_SIZE: usize = 64; // 50% overlap
const PITCH_SHIFT: f32 = 0.5;
mod circular_buffer;
mod hann_window;

#[rtic::app(
    device = stm32h7xx_hal::stm32,
    peripherals = true,
    dispatchers = [DMA1_STR0]
)]
mod app {

    use core::f32::consts::PI;

    use libdaisy::{audio, gpio, hid, logger, system};
    use libm::{atan2f, cosf, floorf, fmodf, sinf, sqrtf};
    use log::{info, warn};
    use stm32h7xx_hal::{gpio::Input, time::MilliSeconds};

    use crate::{
        circular_buffer::{self, CircularBuffer},
        hann_window, BUFFER_SIZE, FFT_SIZE, HOP_SIZE, PITCH_SHIFT, SAMPLE_RATE,
    };

    pub struct Resources {
        in_buffer: CircularBuffer<f32, BUFFER_SIZE>,
        out_buffer: CircularBuffer<f32, BUFFER_SIZE>,
        last_input_phases: [f32; FFT_SIZE],
        last_output_phases: [f32; FFT_SIZE],
        bin_frequencies: [f32; FFT_SIZE / 2],
        process_fft: bool,
    }

    #[shared]
    struct Shared {
        audio_resources: Resources,
    }

    #[local]
    struct Local {
        audio: audio::Audio,
        buffer: audio::AudioBuffer,
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

        let daisy28 = system
            .gpio
            .daisy28
            .take()
            .expect("Failed to get pin daisy28!")
            .into_pull_up_input();

        let mut switch1 = hid::Switch::new(daisy28, hid::SwitchType::PullUp);
        switch1.set_double_thresh(Some(500));
        switch1.set_held_thresh(Some(150));

        let resources = Resources {
            in_buffer: CircularBuffer::new(0.0, None),
            out_buffer: CircularBuffer::new(0.0, Some(HOP_SIZE)),
            last_input_phases: [0.0; FFT_SIZE],
            last_output_phases: [0.0; FFT_SIZE],
            bin_frequencies: [0.0; FFT_SIZE / 2],
            process_fft: false,
        };

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
            Shared {
                audio_resources: resources,
            },
            Local {
                audio: system.audio,
                buffer,
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

    #[task( shared = [audio_resources])]
    fn dma1_stream0_software_task(ctx:  dma1_stream0_software_task::Context) {
        info!("running task");
        let mut audio_resources = ctx.shared.audio_resources;
        audio_resources.lock(|res| {
            if res.process_fft {
                process_fft(
                    &mut res.in_buffer,
                    &mut res.out_buffer,
                    &mut res.last_input_phases,
                    &mut res.last_output_phases,
                    &mut res.bin_frequencies,
                );
                res.process_fft = false; // Reset the flag
            }
        });
    }

    #[task(binds = DMA1_STR1, local = [audio, buffer, button], shared = [audio_resources], priority = 8)]
    fn audio_handler(mut ctx: audio_handler::Context) {
        let audio = ctx.local.audio;

        
        let buffer = ctx.local.buffer;
        let switch1 = ctx.local.button;

        let mut hop_counter = 0;
        let button_pressed = switch1.is_held() || switch1.is_pressed();

        if audio.get_stereo(buffer) {
            for (left, _right) in buffer.iter() {
                let mut out_sample = *left;
                // Lock the shared resources to safely access them
                ctx.shared.audio_resources.lock(|audio_res| {
                    if button_pressed {
                        // Store the sample in the input buffer
                        audio_res.in_buffer.write(*left);

                        // Read from the output buffer and reset the value
                        out_sample = audio_res.out_buffer.read_and_reset();

                        // Scale the output dow by the overlap factor
                        // out_sample = out_sample * HOP_SIZE as f32 / FFT_SIZE as f32;
                        if hop_counter >= HOP_SIZE {
                            hop_counter = 0;

                            if dma1_stream0_software_task::spawn().is_err()
                            {
                                info!("Could not unwrap software task");
                            }
                            // Run the FFT processing (THIS TAKES TOO LONG)
                            // process_fft(
                            //     &mut audio_res.in_buffer,
                            //     &mut audio_res.out_buffer,
                            //     &mut audio_res.last_input_phases,
                            //     &mut audio_res.last_output_phases,
                            //     &mut audio_res.bin_frequencies,
                            // );
                            audio_res.process_fft = true;
                        }
                        audio_res.out_buffer.next_hop();
                    }
                    hop_counter += 1;

                    // Output the processed audio or further processing
                });

                if audio.push_stereo((out_sample, out_sample)).is_err() {
                    warn!("Failed to write audio data");
                }
            }
        } else {
            warn!("Error reading data!");
        }
    }

    fn process_fft(
        in_buffer: &mut CircularBuffer<f32, BUFFER_SIZE>,
        out_buffer: &mut CircularBuffer<f32, BUFFER_SIZE>,
        last_input_phases: &mut [f32; FFT_SIZE],
        last_output_phases: &mut [f32; FFT_SIZE],
        _bin_frequencies: &mut [f32; FFT_SIZE / 2],
    ) {
        let analysis_window_buffer: [f32; FFT_SIZE] = hann_window::HANN_WINDOW;

        let mut unwrapped_buffer: [f32; FFT_SIZE] = [0.0; FFT_SIZE];
        let mut full_spectrum: [microfft::Complex32; FFT_SIZE] =
            [microfft::Complex32 { re: 0.0, im: 0.0 }; FFT_SIZE];
        let mut analysis_magnitudes = [0.0; FFT_SIZE / 2];
        let mut analysis_frequencies = [0.0; FFT_SIZE / 2];
        let mut synthesis_magnitudes = [0.0; FFT_SIZE / 2];
        let mut synthesis_frequencies = [0.0; FFT_SIZE / 2];
        let mut _synthesis_count = [0; FFT_SIZE / 2];

        // copy buffer into FFT input, starting one window ago
        in_buffer.push_read_back(FFT_SIZE - HOP_SIZE);
        for n in 0..FFT_SIZE {
            unwrapped_buffer[n] = in_buffer.read() * analysis_window_buffer[n]
        }

        // Process the FFT based on the time domain input
        let fft = microfft::real::rfft_1024(&mut unwrapped_buffer);

        // ANALYSIS
        for i in 0..fft.len() {
            // Turn real and imaginary components into amplitude and phase
            let amplitude = sqrtf(fft[i].re * fft[i].re + fft[i].im * fft[i].im);
            let phase = atan2f(fft[i].im, fft[i].re);

            // Calculate the phase difference in this bin between the last
            // hop and this one, which will indirectly give us the exact frequency
            let mut phase_diff = phase - last_input_phases[i];

            // Subtract the amount of phase increment we'd expect to see based
            // on the centre frequency of this bin (2*pi*n/gFftSize) for this
            // hop size, then wrap to the range -pi to pi
            let bin_centre_frequency = 2.0 * PI * i as f32 / FFT_SIZE as f32;
            phase_diff = wrap_phase(phase_diff - bin_centre_frequency * HOP_SIZE as f32);

            // Find deviation from the centre frequency
            let bin_deviation = phase_diff * FFT_SIZE as f32 / HOP_SIZE as f32 / (2.0 * PI);

            // Add the original bin number to get the fractional bin where this partial belongs
            analysis_frequencies[i] = i as f32 + bin_deviation;
            // Save the magnitude for later
            analysis_magnitudes[i] = amplitude;
            // Save the phase for next hop
            last_input_phases[i] = phase;
        }

        // Zero out the synthesis bins, ready for new data (NOT done since it should already be zero)

        // Handle the pitch shift, storing frequencies into new bins
        for i in 0..FFT_SIZE / 2 {
            // find the nearest bin to the shifted frequency
            let new_bin = floorf(i as f32 * PITCH_SHIFT + 0.5) as usize;

            // Ignore any bins that have shifted above Nyquist
            if new_bin < FFT_SIZE / 2 {
                synthesis_magnitudes[new_bin] += analysis_magnitudes[i];
                synthesis_frequencies[new_bin] = analysis_frequencies[i] * PITCH_SHIFT;
            }
        }

        // SYNTHESIS
        for i in 0..FFT_SIZE / 2 {
            let amplitude = synthesis_magnitudes[i];
            // Get the fractional offset from the bin centre frequency

            let bin_deviation = synthesis_frequencies[i] - i as f32;
            // Multiply to get back to a phase value
            let mut phase_diff = bin_deviation * 2.0 * PI * HOP_SIZE as f32 / FFT_SIZE as f32;
            // Add the expected phase increment based on the bin centre frequency
            let bin_centre_frequency = 2.0 * PI * i as f32 / FFT_SIZE as f32;
            phase_diff += bin_centre_frequency * HOP_SIZE as f32;
            // Advance the phase from the previous hop
            let out_phase = wrap_phase(last_output_phases[i] + phase_diff);

            // Now convert magnitude and phase back to real and imaginary components
            fft[i].re = amplitude * cosf(out_phase);
            fft[i].im = amplitude * sinf(out_phase);
            // Also store the complex conjugate in the upper half of the spectrum

            // Save the phase for the next hop
            last_output_phases[i] = out_phase;
        }

        // Reconstruct the full spectrum for the IFFT
        for i in 0..(FFT_SIZE / 2) {
            full_spectrum[i] = fft[i]; // First half directly
            if i > 0 && i < (FFT_SIZE / 2) {
                full_spectrum[FFT_SIZE - i] = fft[i].conj(); // Conjugate symmetry for the second half
            }
        }

        // Run the inverse FFT
        let res = microfft::inverse::ifft_1024(&mut full_spectrum);

        // Add time domain into the output buffer
        for (n, val) in res.iter().enumerate() {
            let windowed_val = val.re * analysis_window_buffer[n]; // Window again and scale
            out_buffer.add_value(windowed_val);
        }
    }

    fn wrap_phase(phase_in: f32) -> f32 {
        if phase_in >= 0.0 {
            return fmodf(phase_in + PI, 2.0 * PI) - PI;
        }
        fmodf(phase_in - PI, -2.0 * PI) + PI
    }
}
