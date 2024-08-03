#![no_main]
#![no_std]
// #![deny(warnings)]
#![deny(unsafe_code)]
// #![deny(missing_docs)]

const _SAMPLE_RATE: f32 = 48_014.312;
const FFT_SIZE: usize = 1024;
const BUFFER_SIZE: usize = FFT_SIZE * 2;
const HOP_SIZE: usize = 256;
const BLOCK_SIZE: usize = HOP_SIZE / 128;
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
    use stm32h7xx_hal::{
        adc,
        gpio::{Analog, Input},
        stm32,
        time::MilliSeconds,
    };

    use crate::{
        circular_buffer::CircularBuffer, hann_window, BLOCK_SIZE, BUFFER_SIZE, FFT_SIZE, HOP_SIZE,
    };

    #[shared]
    struct Shared {
        in_buffer: CircularBuffer<f32, BUFFER_SIZE>,
        out_buffer: CircularBuffer<f32, BUFFER_SIZE>,
        last_input_phases: [f32; FFT_SIZE],
        last_output_phases: [f32; FFT_SIZE],
        bin_frequencies: [f32; FFT_SIZE / 2],
        process_fft: bool,
        hop_counter: u32,
    }

    #[local]
    struct Local {
        audio: audio::Audio,
        buffer: audio::AudioBuffer,
        button: hid::Switch<gpio::Daisy28<Input>>,
        pot_input: hid::AnalogControl<gpio::Daisy15<Analog>>,
        adc1: adc::Adc<stm32::ADC1, adc::Enabled>,
    }

    #[init]
    fn init(ctx: init::Context) -> (Shared, Local, init::Monotonics) {
        logger::init();

        let mut core = ctx.core;
        let device = ctx.device;
        let ccdr = system::System::init_clocks(device.PWR, device.RCC, &device.SYSCFG);
        let mut system = libdaisy::system_init!(core, device, ccdr, BLOCK_SIZE);

        let buffer = [(0.0, 0.0); audio::BLOCK_SIZE_MAX];

        let daisy28 = system
            .gpio
            .daisy28
            .take()
            .expect("Failed to get pin daisy28!")
            .into_pull_up_input();

        let daisy15 = system
            .gpio
            .daisy15
            .take()
            .expect("Failed to get pin daisy29!")
            .into_analog();

        let mut switch1 = hid::Switch::new(daisy28, hid::SwitchType::PullUp);
        switch1.set_double_thresh(Some(500));
        switch1.set_held_thresh(Some(150));

        let mut adc1 = system.adc1.enable();
        adc1.set_resolution(adc::Resolution::EightBit);
        let adc1_max = adc1.slope() as f32;

        let pot_input = hid::AnalogControl::new(daisy15, adc1_max);

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
                in_buffer: CircularBuffer::new(0.0, None),
                out_buffer: CircularBuffer::new(0.0, Some(HOP_SIZE)),
                last_input_phases: [0.0; FFT_SIZE],
                last_output_phases: [0.0; FFT_SIZE],
                bin_frequencies: [0.0; FFT_SIZE / 2],
                process_fft: false,
                hop_counter: 0,
            },
            Local {
                pot_input,
                adc1,
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

    #[task(binds = DMA1_STR1, local = [audio, buffer, button], shared = [

        in_buffer,
        out_buffer,
        last_input_phases,
        last_output_phases,
        bin_frequencies,
        process_fft,
        hop_counter,

    ], priority = 8)]
    fn audio_handler(mut ctx: audio_handler::Context) {
        let audio = ctx.local.audio;
        let buffer = ctx.local.buffer;
        let switch1 = ctx.local.button;
        let button_pressed = switch1.is_held() || switch1.is_pressed();

        if audio.get_stereo(buffer) {
            for (left, right) in &buffer.as_slice()[..BLOCK_SIZE] {
                let mut out_sample = *left;
                // info!("{out_sample}");

                // Lock to write to in_buffer
                ctx.shared.in_buffer.lock(|in_buffer| {
                    in_buffer.write(*left);
                });

                // Lock to read from out_buffer and reset the value
                ctx.shared.out_buffer.lock(|out_buffer| {
                    if button_pressed {
                        out_sample = out_buffer.read_and_reset();
                    } else {
                        _ = out_buffer.read_and_reset();
                    }
                });

                // Check and handle hop counter

                let mut local_hop_counter: u32 = 0;

                ctx.shared.hop_counter.lock(|count| {
                    local_hop_counter = *count;
                });
                if local_hop_counter >= HOP_SIZE as u32 {
                    ctx.shared.hop_counter.lock(|count| {
                        *count = 0;
                    });

                    // Run FFT Process in new software task
                    if dma1_stream0_software_task::spawn().is_err() {
                        warn!("Could not unwrap software task - underrun error");
                    }

                    // Lock to set process_fft flag
                    ctx.shared.process_fft.lock(|process_fft| {
                        *process_fft = true;
                    });

                    // Lock to advance the output buffer's hop
                    ctx.shared.out_buffer.lock(|out_buffer| {
                        out_buffer.next_hop();
                    });
                }
                ctx.shared.hop_counter.lock(|count| {
                    *count += 1;
                });

                // Output the processed audio or further processing
                if audio.push_stereo((out_sample, *right)).is_err() {
                    warn!("Failed to write audio data");
                }
            }
        } else {
            warn!("Error reading data!");
        }
    }

    /// FFT TASK
    #[task( shared = [
        in_buffer,
        out_buffer,
        last_input_phases,
        last_output_phases,
        bin_frequencies,
        process_fft,
    ], local = [adc1, pot_input],priority = 7)]
    fn dma1_stream0_software_task(mut ctx: dma1_stream0_software_task::Context) {
        // info!("running task");
        let adc1 = ctx.local.adc1;
        let pot = ctx.local.pot_input;

        adc1.start_conversion(pot.get_pin());

        let adc_result = adc1.read_sample().unwrap_or(1);
        let pitch_shift = 0.15 * adc_result as f32 - 1.15;
        info!("ADC result: {}, pitch shift: {}", adc_result, pitch_shift);

        // START ACTUAL FFT PROCESSING
        // let start_cycles = cortex_m::peripheral::DWT::cycle_count();
        let analysis_window_buffer: [f32; FFT_SIZE] = hann_window::HANN_WINDOW;

        let mut unwrapped_buffer: [f32; FFT_SIZE] = hann_window::HANN_WINDOW;
        let mut full_spectrum: [microfft::Complex32; FFT_SIZE] =
            [microfft::Complex32 { re: 0.0, im: 0.0 }; FFT_SIZE];
        let mut analysis_magnitudes = [0.0; FFT_SIZE / 2];
        let mut analysis_frequencies = [0.0; FFT_SIZE / 2];
        let mut synthesis_magnitudes = [0.0; FFT_SIZE / 2];
        let mut synthesis_frequencies = [0.0; FFT_SIZE / 2];
        let mut _synthesis_count = [0; FFT_SIZE / 2];

        // copy buffer into FFT input, starting one window ago
        ctx.shared.in_buffer.lock(|in_buffer| {
            in_buffer.push_read_back(FFT_SIZE - HOP_SIZE);
        });

        for n in 0..FFT_SIZE {
            ctx.shared.in_buffer.lock(|in_buffer| {
                unwrapped_buffer[n] *= in_buffer.read();
            });
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
            let mut phase_diff = 0.0;
            ctx.shared.last_input_phases.lock(|last_input_phases| {
                phase_diff = phase - last_input_phases[i];
            });

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
            ctx.shared.last_input_phases.lock(|last_input_phases| {
                last_input_phases[i] = phase;
            });
        }

        // Zero out the synthesis bins, ready for new data (NOT done since it should already be zero)

        // Handle the pitch shift, storing frequencies into new bins
        for i in 0..FFT_SIZE / 2 {
            // find the nearest bin to the shifted frequency
            let new_bin = floorf(i as f32 * pitch_shift + 0.5) as usize;

            // Ignore any bins that have shifted above Nyquist
            if new_bin < FFT_SIZE / 2 {
                synthesis_magnitudes[new_bin] += analysis_magnitudes[i];
                synthesis_frequencies[new_bin] = analysis_frequencies[i] * pitch_shift;
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
            let mut out_phase = 0.0;
            ctx.shared.last_output_phases.lock(|last_output_phases| {
                out_phase = wrap_phase(last_output_phases[i] + phase_diff);
            });

            // Now convert magnitude and phase back to real and imaginary components
            fft[i].re = amplitude * cosf(out_phase);
            fft[i].im = amplitude * sinf(out_phase);
            // Also store the complex conjugate in the upper half of the spectrum

            // Save the phase for the next hop
            ctx.shared.last_output_phases.lock(|last_output_phases| {
                last_output_phases[i] = out_phase;
            });
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
            ctx.shared.out_buffer.lock(|out_buffer| {
                out_buffer.add_value(windowed_val);
            });
        }

        let end_cycle = cortex_m::peripheral::DWT::cycle_count();

        // let elapsed = start_cycles.wrapping_sub(end_cycle);
        // info!("FFT Process Time{elapsed}");
    }

    fn wrap_phase(phase_in: f32) -> f32 {
        if phase_in >= 0.0 {
            return fmodf(phase_in + PI, 2.0 * PI) - PI;
        }
        fmodf(phase_in - PI, -2.0 * PI) + PI
    }
}
