use crate::{constants::*, input::buttons::*};
use autotune::frequencies::{find_nearest_note_in_key, C_MAJOR_SCALE_FREQUENCIES};
use autotune::hann_window::{self, PI};
use autotune::keys::{get_frequency, get_scale_by_key};
use autotune::process_frequencies::{
    bitcrush, find_fundamental_frequency, normalize_sample, sample_rate_reduce,
};
use core::sync::atomic::Ordering;
use libdaisy::gpio::Daisy1;
use libdaisy::prelude::Input;
use libdaisy::{audio, hid};
use libm::{atan2f, cosf, expf, floorf, fmodf, logf, sinf, sqrtf};
use log::{info, warn};
use rotary_encoder_embedded::Direction;
use rtic::Mutex;
use state_machines::{AppState, ProcessingProfile};

// Common constants
const LIFTER_CUTOFF: usize = 64;
const ZERO_COMPLEX: microfft::Complex32 = microfft::Complex32 { re: 0.0, im: 0.0 };

// Common processing buffers structure
struct ProcessingBuffers {
    unwrapped_buffer: [f32; FFT_SIZE],
    unwrapped_synth_buffer: Option<[f32; FFT_SIZE]>,
    full_spectrum: [microfft::Complex32; FFT_SIZE],
    analysis_magnitudes: [f32; FFT_SIZE / 2],
    analysis_frequencies: [f32; FFT_SIZE / 2],
}

impl ProcessingBuffers {
    fn new(need_synth_buffer: bool) -> Self {
        Self {
            unwrapped_buffer: hann_window::HANN_WINDOW,
            unwrapped_synth_buffer: if need_synth_buffer {
                Some(hann_window::HANN_WINDOW)
            } else {
                None
            },
            full_spectrum: [ZERO_COMPLEX; FFT_SIZE],
            analysis_magnitudes: [0.0; FFT_SIZE / 2],
            analysis_frequencies: [0.0; FFT_SIZE / 2],
        }
    }
}

// Processing parameters structure
struct ProcessingParams {
    formant: i32,
    pitch_shift_ratio: f32,
    note: i32,
    key: i32,
    octave: i32,
}

impl ProcessingParams {
    fn from_state_machine(
        ctx: &mut crate::rtic_app::app::dma1_stream0_software_task::SharedResources,
    ) -> Self {
        let mut formant = 0;
        let mut pitch_shift_ratio = 1.0;
        let mut note = 0;
        let mut key = 0;
        let mut octave = 2;

        ctx.app_state_machine.lock(|asm| {
            let snapshot = asm.snapshot();
            formant = snapshot.formant;
            // Use octave as pitch control (0.5 = down octave, 2.0 = up octave)
            let octave_factor = snapshot.octave as f32 * 0.5;
            pitch_shift_ratio = if octave_factor <= 0.4 {
                1.0
            } else {
                octave_factor
            };
            note = snapshot.note;
            key = snapshot.key;
            octave = snapshot.octave;
        });

        Self {
            formant,
            pitch_shift_ratio,
            note,
            key,
            octave,
        }
    }
}

// Common data loading and windowing function
fn load_and_window_data(
    ctx: &mut crate::rtic_app::app::dma1_stream0_software_task::SharedResources,
    buffers: &mut ProcessingBuffers,
) {
    let write_idx = ctx
        .in_pointer_cached
        .lock(|in_pointer| in_pointer.load(Ordering::Relaxed));

    ctx.in_ring
        .lock(|rb| rb.block_from::<FFT_SIZE>(write_idx, &mut buffers.unwrapped_buffer));

    if let Some(ref mut synth_buffer) = buffers.unwrapped_synth_buffer {
        ctx.carrier_ring
            .lock(|rb| rb.block_from::<FFT_SIZE>(write_idx, synth_buffer));
    }

    // Apply window function
    for i in 0..FFT_SIZE {
        buffers.unwrapped_buffer[i] *= hann_window::HANN_WINDOW[i];
        if let Some(ref mut synth_buffer) = buffers.unwrapped_synth_buffer {
            synth_buffer[i] *= hann_window::HANN_WINDOW[i];
        }
    }
}

// Phase vocoder analysis function
fn perform_phase_vocoder_analysis(
    fft: &[microfft::Complex32],
    ctx: &mut crate::rtic_app::app::dma1_stream0_software_task::SharedResources,
    analysis_magnitudes: &mut [f32; FFT_SIZE / 2],
    analysis_frequencies: &mut [f32; FFT_SIZE / 2],
) {
    for i in 0..fft.len() {
        let amplitude = sqrtf(fft[i].re * fft[i].re + fft[i].im * fft[i].im);
        let phase = atan2f(fft[i].im, fft[i].re);

        // Phase difference for exact frequency
        let mut phase_diff = 0.0;
        ctx.last_input_phases.lock(|last_input_phases| {
            phase_diff = phase - last_input_phases[i];
        });

        let bin_centre_frequency = 2.0 * PI * i as f32 / FFT_SIZE as f32;
        phase_diff = wrap_phase(phase_diff - bin_centre_frequency * HOP_SIZE as f32);
        let bin_deviation = phase_diff * FFT_SIZE as f32 / HOP_SIZE as f32 / (2.0 * PI);

        analysis_frequencies[i] = i as f32 + bin_deviation;
        analysis_magnitudes[i] = amplitude;

        ctx.last_input_phases.lock(|last_input_phases| {
            last_input_phases[i] = phase;
        });
    }
}

// Phase vocoder synthesis function
fn perform_phase_vocoder_synthesis(
    ctx: &mut crate::rtic_app::app::dma1_stream0_software_task::SharedResources,
    full_spectrum: &mut [microfft::Complex32; FFT_SIZE],
) {
    for i in 0..FFT_SIZE / 2 {
        let amplitude = ctx
            .synthesis_magnitudes
            .lock(|synthesis_magnitudes| synthesis_magnitudes[i]);
        let bin_deviation = ctx
            .synthesis_frequencies
            .lock(|synthesis_frequencies| synthesis_frequencies[i] - i as f32);

        let mut phase_diff = bin_deviation * 2.0 * PI * HOP_SIZE as f32 / FFT_SIZE as f32;
        let bin_centre_frequency = 2.0 * PI * i as f32 / FFT_SIZE as f32;
        phase_diff += bin_centre_frequency * HOP_SIZE as f32;

        let mut out_phase = 0.0;
        ctx.last_output_phases.lock(|last_output_phases| {
            out_phase = wrap_phase(last_output_phases[i] + phase_diff);
            last_output_phases[i] = out_phase;
        });

        full_spectrum[i] = microfft::Complex32 {
            re: amplitude * cosf(out_phase),
            im: amplitude * sinf(out_phase),
        };

        if i > 0 && i < (FFT_SIZE / 2) {
            full_spectrum[FFT_SIZE - i] = full_spectrum[i].conj();
        }
    }
}

// Clear synthesis arrays
fn clear_synthesis_arrays(
    ctx: &mut crate::rtic_app::app::dma1_stream0_software_task::SharedResources,
) {
    ctx.synthesis_magnitudes.lock(|syn_mag| {
        for bin in syn_mag.iter_mut() {
            *bin = 0.0;
        }
    });
    ctx.synthesis_frequencies.lock(|syn_freq| {
        for bin in syn_freq.iter_mut() {
            *bin = 0.0;
        }
    });
}

pub(crate) fn update_handler(
    audio: &mut audio::Audio,
    buffer: &mut audio::AudioBuffer,
    hangup_button: &mut hid::Switch<Daisy1<Input>>,
    shared: &mut crate::rtic_app::app::update_handler::SharedResources,
) {
    hangup_button.update();
    let is_hangup_button_pressed = hangup_button.is_high();

    let mut sr_factor = 1;
    shared.app_state_machine.lock(|msm| {
        sr_factor = msm.snapshot().sample_reduction;
    });

    let mut bit_depth = 32;
    shared.app_state_machine.lock(|msm| {
        bit_depth = msm.snapshot().bit_rate;
    });

    if audio.get_stereo(buffer) {
        let snap = shared.app_state_machine.lock(|msm| msm.snapshot());
        let sr_factor = snap.sample_reduction;
        let bit_depth = snap.bit_rate;
        let note = snap.note;
        let key = snap.key;
        let octave = snap.octave;

        let current_process = match snap.current_state {
            AppState::Processing(p) | AppState::EffectsProfile(p) | AppState::Menu(_, p) => p,
            AppState::Splash => ProcessingProfile::Autotune,
        };

        for (left, right) in &buffer.as_slice()[..BLOCK_SIZE] {
            let sample = match is_hangup_button_pressed {
                false => *right,
                true => *left,
            };

            // Lock to write to in_buffer
            shared.in_ring.lock(|in_ring| in_ring.push(sample));

            if current_process == ProcessingProfile::Vocode
                || current_process == ProcessingProfile::Dry
            {
                let carrier_hz = get_frequency(key, note, octave, true);

                let sample = shared.carrier_osc.lock(|osc| {
                    osc.set_freq(carrier_hz);
                    osc.next()
                });

                shared
                    .carrier_ring
                    .lock(|carrier_ring| carrier_ring.push(sample));
            }

            let mut out_sample = shared.out_ring.lock(|out_ring| out_ring.pop());

            // ************** SAMPLE-RATE REDUCE **************
            shared.sr_hold_counter.lock(|hold_ctr| {
                shared.sr_held_value.lock(|held_val| {
                    out_sample = sample_rate_reduce(out_sample, sr_factor, hold_ctr, held_val);
                });
            });

            // ************** BIT DEPTH REDUCE **************
            out_sample = bitcrush(out_sample, bit_depth as u8);

            // Normalize final output
            out_sample = normalize_sample(out_sample, 0.8);

            // Check and handle hop counter
            let mut local_hop_counter: u32 = 0;
            shared.hop_counter.lock(|count| {
                local_hop_counter = *count;
            });

            if local_hop_counter >= HOP_SIZE as u32 {
                shared.hop_counter.lock(|count| {
                    *count = 0;
                });

                let pointer = shared.in_ring.lock(|in_ring| in_ring.write_index());

                shared.in_pointer_cached.lock(|cache| {
                    cache.store(pointer, Ordering::Relaxed);
                });

                // Run FFT Process in new software task
                if crate::rtic_app::app::dma1_stream0_software_task::spawn().is_err() {
                    warn!("Could not unwrap software task - underrun error");
                }
            }

            shared.hop_counter.lock(|count| {
                *count += 1;
            });

            // Output the processed audio
            if audio.push_stereo((out_sample, out_sample)).is_err() {
                warn!("Failed to write audio data");
            }
        }
    } else {
        warn!("Error reading data!");
    }
}

pub fn interface_handler(
    mut local: crate::rtic_app::app::interface_handler::LocalResources,
    shared: &mut crate::rtic_app::app::interface_handler::SharedResources,
) {
    local.timer2.clear_irq();

    let mut update_state: bool = false;

    let new_matrix_state = scan_button_matrix(
        &mut local.col_1_pin,
        &mut local.col_2_pin,
        &mut local.col_3_pin,
        &local.row_1_pin,
        &local.row_2_pin,
        &local.row_3_pin,
        &local.row_4_pin,
    );

    // Process button matrix changes
    for row in 0..4 {
        for col in 0..3 {
            let was_pressed = shared.old_matrix_state.lock(|oms| oms[row][col]);
            let is_pressed = new_matrix_state[row][col];

            // If there is a change, decide how to handle it
            if is_pressed != was_pressed {
                shared.old_matrix_state.lock(|oms| {
                    oms[row][col] = is_pressed;
                });

                if is_pressed {
                    update_state = true;
                    // Button has just been pressed
                    shared.app_state_machine.lock(|msm| {
                        msm.handle_event(handle_button_press(
                            row,
                            col,
                            msm.snapshot().current_state,
                        ));
                    });
                } else {
                    update_state = true;
                    // Button has just been released
                    shared.app_state_machine.lock(|msm| {
                        msm.handle_event(handle_button_release(
                            row,
                            col,
                            msm.snapshot().current_state,
                        ));
                    });
                }
            }
        }
    }

    // Handle encoder button - single press vs double press
    local.encoder_button.update();

    // Check for double press first
    if local.encoder_button.is_double() {
        info!("Encoder double press detected!");
        update_state = true;

        shared.app_state_machine.lock(|msm| {
            msm.handle_event(state_machines::AppEvent::EncoderDoublePress);
        });
    }
    //Check for single press if not a double press
    else if local.encoder_button.is_falling() {
        info!("Encoder single press detected!");
        update_state = true;

        shared.app_state_machine.lock(|msm| {
            msm.handle_event(state_machines::AppEvent::EncoderPress);
        });
    }

    // Handle encoder rotation
    match local.knob_1.rotary_encoder.update() {
        Direction::Clockwise => {
            update_state = true;
            shared.app_state_machine.lock(|msm| {
                msm.handle_event(state_machines::AppEvent::EncoderRotate(1));
            });
        }
        Direction::Anticlockwise => {
            update_state = true;
            shared.app_state_machine.lock(|msm| {
                msm.handle_event(state_machines::AppEvent::EncoderRotate(-1));
            });
        }
        Direction::None => {}
    }

    if update_state {
        // Set the flag
        shared.display_needs_update.lock(|flag| *flag = true);

        // Spawn the display task to run
        crate::rtic_app::app::display_update_task::spawn().ok();
    }
}

pub fn dma1_stream0_software_task(
    ctx: &mut crate::rtic_app::app::dma1_stream0_software_task::SharedResources,
) {
    let mut current_process = ProcessingProfile::Autotune;
    ctx.app_state_machine.lock(|msm| {
        let snapshot = msm.snapshot();
        match snapshot.current_state {
            AppState::Processing(process)
            | AppState::EffectsProfile(process)
            | AppState::Menu(_, process) => {
                current_process = process;
            }
            AppState::Splash => {}
        }
    });

    match current_process {
        ProcessingProfile::Autotune => process_autotune(ctx),
        ProcessingProfile::Vocode => process_vocode(ctx),
        ProcessingProfile::Dry => process_dry(ctx),
    }

    ctx.out_ring.lock(|rb| rb.advance_write(HOP_SIZE as u32));
}

pub fn process_dry(ctx: &mut crate::rtic_app::app::dma1_stream0_software_task::SharedResources) {
    let mut buffers = ProcessingBuffers::new(true);
    load_and_window_data(ctx, &mut buffers);

    // Forward FFT
    let fft = microfft::real::rfft_1024(&mut buffers.unwrapped_buffer);

    // Get processing parameters
    let params = ProcessingParams::from_state_machine(ctx);

    // If no effects, just pass through
    if params.formant == 0 && (params.pitch_shift_ratio > 0.99 && params.pitch_shift_ratio < 1.01) {
        // Direct pass-through - just copy spectrum
        for i in 0..512 {
            buffers.full_spectrum[i] = fft[i];
        }
        for i in 1..512 {
            buffers.full_spectrum[FFT_SIZE - i] = fft[i].conj();
        }
    } else {
        // ANALYSIS - Phase vocoder
        perform_phase_vocoder_analysis(
            fft,
            ctx,
            &mut buffers.analysis_magnitudes,
            &mut buffers.analysis_frequencies,
        );

        // ENVELOPE EXTRACTION (only if formant shifting)
        let envelope = if params.formant != 0 {
            let mut temp_spectrum = [ZERO_COMPLEX; FFT_SIZE];
            let mut cepstrum_buffer = [0.0f32; FFT_SIZE];

            // Log magnitude spectrum
            for i in 0..(FFT_SIZE / 2) {
                let mag = buffers.analysis_magnitudes[i].max(1e-6);
                let log_mag = logf(mag);
                temp_spectrum[i] = microfft::Complex32 {
                    re: log_mag,
                    im: 0.0,
                };
                if i != 0 {
                    temp_spectrum[FFT_SIZE - i] = microfft::Complex32 {
                        re: log_mag,
                        im: 0.0,
                    };
                }
            }

            // Get cepstrum
            let cepstrum = microfft::inverse::ifft_1024(&mut temp_spectrum);

            // Lifter - only copy low quefrency
            for i in 0..LIFTER_CUTOFF {
                cepstrum_buffer[i] = cepstrum[i].re;
            }
            for i in (FFT_SIZE - LIFTER_CUTOFF)..FFT_SIZE {
                cepstrum_buffer[i] = cepstrum[i].re;
            }

            // Get envelope
            let envelope_fft = microfft::real::rfft_1024(&mut cepstrum_buffer);
            let mut envelope = [1.0f32; FFT_SIZE / 2];
            for i in 0..(FFT_SIZE / 2) {
                envelope[i] = expf(envelope_fft[i].re);
            }
            envelope
        } else {
            [1.0f32; FFT_SIZE / 2]
        };

        // Zero synthesis arrays
        clear_synthesis_arrays(ctx);

        // Formant shift ratio
        let formant_ratio = match params.formant {
            1 => 0.8, // Lower formants
            2 => 1.3, // Raise formants
            _ => 1.0, // No formant shift
        };

        // PITCH AND FORMANT SHIFTING
        for i in 0..FFT_SIZE / 2 {
            // Get residual
            let residual = if params.formant != 0 {
                buffers.analysis_magnitudes[i] / envelope[i].max(1e-6)
            } else {
                buffers.analysis_magnitudes[i]
            };

            // New bin for pitch shift
            let new_bin = (floorf(i as f32 * params.pitch_shift_ratio + 0.5)) as usize;

            if new_bin < FFT_SIZE / 2 {
                // Get shifted envelope value
                let shifted_envelope = if params.formant != 0 {
                    let env_pos = (i as f32 / formant_ratio).clamp(0.0, (FFT_SIZE / 2 - 1) as f32);
                    let env_idx = env_pos as usize;
                    let frac = env_pos - env_idx as f32;

                    if env_idx < (FFT_SIZE / 2) - 1 {
                        envelope[env_idx] * (1.0 - frac) + envelope[env_idx + 1] * frac
                    } else {
                        envelope[env_idx]
                    }
                } else {
                    1.0
                };

                let final_magnitude = residual * shifted_envelope;

                ctx.synthesis_magnitudes.lock(|synthesis_magnitudes| {
                    synthesis_magnitudes[new_bin] += final_magnitude; // Note: += for overlapping bins
                });
                ctx.synthesis_frequencies.lock(|synthesis_frequencies| {
                    synthesis_frequencies[new_bin] =
                        buffers.analysis_frequencies[i] * params.pitch_shift_ratio;
                });
            }
        }

        // SYNTHESIS - Phase vocoder
        perform_phase_vocoder_synthesis(ctx, &mut buffers.full_spectrum);
    }

    // Inverse FFT
    let res = microfft::inverse::ifft_1024(&mut buffers.full_spectrum);

    let playing_note = params.note != 0;
    // Overlap-add to output
    let mut synth_frame = [0.0f32; FFT_SIZE];
    for i in 0..FFT_SIZE {
        let vocals = res[i].re;
        let synth = buffers.unwrapped_synth_buffer.as_ref().unwrap()[i];
        let mixed = if playing_note {
            vocals * 0.96 + synth * 0.04
        } else {
            vocals
        };
        synth_frame[i] = mixed * hann_window::HANN_WINDOW[i];
    }

    ctx.out_ring.lock(|rb| {
        for (i, &sample) in synth_frame.iter().enumerate() {
            rb.add_at_offset(i as u32, sample);
        }
    });
}

pub fn process_vocode(ctx: &mut crate::rtic_app::app::dma1_stream0_software_task::SharedResources) {
    let mut input_unwrapped_buffer: [f32; FFT_SIZE] = hann_window::HANN_WINDOW;
    let mut carrier_unwrapped_buffer: [f32; FFT_SIZE] = hann_window::HANN_WINDOW;
    let mut full_spectrum: [microfft::Complex32; FFT_SIZE] = [ZERO_COMPLEX; FFT_SIZE];

    let write_idx = ctx
        .in_pointer_cached
        .lock(|in_pointer| in_pointer.load(Ordering::Relaxed));

    ctx.in_ring
        .lock(|rb| rb.block_from::<FFT_SIZE>(write_idx, &mut input_unwrapped_buffer));
    ctx.carrier_ring
        .lock(|rb| rb.block_from::<FFT_SIZE>(write_idx, &mut carrier_unwrapped_buffer));

    // Apply window function
    for i in 0..FFT_SIZE {
        input_unwrapped_buffer[i] *= hann_window::HANN_WINDOW[i];
        carrier_unwrapped_buffer[i] *= hann_window::HANN_WINDOW[i];
    }

    let modulator_fft = microfft::real::rfft_1024(&mut input_unwrapped_buffer);
    let carrier_fft = microfft::real::rfft_1024(&mut carrier_unwrapped_buffer);

    // Copy the first half (including DC and Nyquist)
    for i in 0..(FFT_SIZE / 2) {
        // Get modulator magnitude
        let mod_mag = libm::sqrtf(
            modulator_fft[i].re * modulator_fft[i].re + modulator_fft[i].im * modulator_fft[i].im,
        );

        // Get carrier magnitude
        let car_mag = libm::sqrtf(
            carrier_fft[i].re * carrier_fft[i].re + carrier_fft[i].im * carrier_fft[i].im,
        );

        // Scale carrier by modulator envelope
        let scale_factor = if car_mag > 0.0001 {
            mod_mag / car_mag
        } else {
            0.0
        };

        // Apply scaling
        full_spectrum[i].re = carrier_fft[i].re * scale_factor;
        full_spectrum[i].im = carrier_fft[i].im * scale_factor;

        // Conjugate symmetry
        if i > 0 && i < (FFT_SIZE / 2) {
            full_spectrum[FFT_SIZE - i].re = full_spectrum[i].re;
            full_spectrum[FFT_SIZE - i].im = -full_spectrum[i].im;
        }
    }

    let res = microfft::inverse::ifft_1024(&mut full_spectrum);

    ctx.out_ring.lock(|rb| {
        for i in 0..FFT_SIZE {
            let windowed_sample = res[i].re * hann_window::HANN_WINDOW[i];
            rb.add_at_offset(i as u32, windowed_sample);
        }
    });
}

pub fn process_autotune(
    ctx: &mut crate::rtic_app::app::dma1_stream0_software_task::SharedResources,
) {
    let mut buffers = ProcessingBuffers::new(false);
    load_and_window_data(ctx, &mut buffers);

    // Process the FFT based on the time domain input
    let fft = microfft::real::rfft_1024(&mut buffers.unwrapped_buffer);

    let mut params = ProcessingParams::from_state_machine(ctx);
    let is_auto = params.note == 0;

    // ANALYSIS
    perform_phase_vocoder_analysis(
        fft,
        ctx,
        &mut buffers.analysis_magnitudes,
        &mut buffers.analysis_frequencies,
    );

    // Clear synthesis arrays
    clear_synthesis_arrays(ctx);

    // Compute envelope
    let envelope = if params.formant != 0 {
        let mut temp_spectrum = [ZERO_COMPLEX; FFT_SIZE];
        let mut cepstrum_buffer = [0.0f32; FFT_SIZE];

        // Log magnitude spectrum
        for i in 0..(FFT_SIZE / 2) {
            let mag = buffers.analysis_magnitudes[i].max(1e-6);
            let log_mag = logf(mag);
            temp_spectrum[i] = microfft::Complex32 {
                re: log_mag,
                im: 0.0,
            };
            if i != 0 {
                temp_spectrum[FFT_SIZE - i] = microfft::Complex32 {
                    re: log_mag,
                    im: 0.0,
                };
            }
        }

        // Get cepstrum
        let cepstrum = microfft::inverse::ifft_1024(&mut temp_spectrum);

        // Lifter - only copy low quefrency
        for i in 0..LIFTER_CUTOFF {
            cepstrum_buffer[i] = cepstrum[i].re;
        }
        for i in (FFT_SIZE - LIFTER_CUTOFF)..FFT_SIZE {
            cepstrum_buffer[i] = cepstrum[i].re;
        }

        // Get envelope
        let envelope_fft = microfft::real::rfft_1024(&mut cepstrum_buffer);
        let mut envelope = [1.0f32; FFT_SIZE / 2];
        for i in 0..(FFT_SIZE / 2) {
            envelope[i] = expf(envelope_fft[i].re);
        }
        envelope
    } else {
        [1.0f32; FFT_SIZE / 2]
    };

    // Get the fundamental frequency (Loudest)
    let fundamental_index = find_fundamental_frequency(&buffers.analysis_magnitudes);
    let _harmonics = collect_harmonics(fundamental_index);

    // Exact frequency is tied to the bin.
    let exact_frequency =
        buffers.analysis_frequencies[fundamental_index] * crate::constants::BIN_WIDTH;

    // We cannot divide by 0
    if exact_frequency > 0.001 {
        let mut scale_frequencies = &C_MAJOR_SCALE_FREQUENCIES;

        let mut octave_factor = 1.0;
        ctx.app_state_machine.lock(|msm| {
            let snapshot = msm.snapshot();
            params.octave = snapshot.octave;
            octave_factor = params.octave as f32 * 0.5;
            if octave_factor <= 0.4 {
                octave_factor = 1.0;
            }

            params.key = snapshot.key;
            scale_frequencies = get_scale_by_key(params.key);
        });

        let target_frequency = if is_auto {
            find_nearest_note_in_key(exact_frequency, scale_frequencies)
        } else {
            get_frequency(params.key, params.note, params.octave, false)
        };
        let current_pitch_shift_ratio = target_frequency / exact_frequency;

        let previous_pitch_shift_ratio = ctx.previous_pitch_shift_ratio.lock(|ppr| *ppr);

        let pitch_shift_ratio =
            current_pitch_shift_ratio * 0.999 + previous_pitch_shift_ratio * 0.001;

        let formant_ratio = match params.formant {
            1 => 0.5, // Lower formants
            2 => 2.0, // Raise formants
            _ => 1.0, // No formant shift
        };

        // shift all bins by the ratio
        for i in 0..FFT_SIZE / 2 {
            // Get residual (source magnitude divided by source envelope)
            let residual = if params.formant != 0 {
                buffers.analysis_magnitudes[i] / envelope[i].max(1e-6)
            } else {
                buffers.analysis_magnitudes[i]
            };

            //new bin for the pitch shift
            let new_bin = (floorf(i as f32 * pitch_shift_ratio + 0.5) * octave_factor) as usize;

            if new_bin < FFT_SIZE / 2 {
                // Get shifted envelope value
                let shifted_envelope = if params.formant != 0 {
                    // Find envelope at formant-shifted position
                    let env_pos = (i as f32 / formant_ratio).clamp(0.0, (FFT_SIZE / 2 - 1) as f32);
                    let env_idx = env_pos as usize;
                    let frac = env_pos - env_idx as f32;

                    if env_idx < (FFT_SIZE / 2) - 1 {
                        envelope[env_idx] * (1.0 - frac) + envelope[env_idx + 1] * frac
                    } else {
                        envelope[env_idx]
                    }
                } else {
                    1.0
                };

                // Final magnitude = residual * shifted_envelope
                let final_magnitude = residual * shifted_envelope;

                ctx.synthesis_magnitudes.lock(|synthesis_magnitudes| {
                    synthesis_magnitudes[new_bin] = final_magnitude;
                });
                ctx.synthesis_frequencies.lock(|synthesis_frequencies| {
                    synthesis_frequencies[new_bin] =
                        buffers.analysis_frequencies[i] * pitch_shift_ratio * octave_factor;
                });
            }
        }

        ctx.previous_pitch_shift_ratio
            .lock(|ppr| *ppr = pitch_shift_ratio);
    }

    // SYNTHESIS
    perform_phase_vocoder_synthesis(ctx, &mut buffers.full_spectrum);

    // Run the inverse FFT
    let res = microfft::inverse::ifft_1024(&mut buffers.full_spectrum);

    ctx.out_ring.lock(|rb| {
        for i in 0..FFT_SIZE {
            let windowed_sample = res[i].re * hann_window::HANN_WINDOW[i];
            rb.add_at_offset(i as u32, windowed_sample);
        }
    });
}

pub fn wrap_phase(phase_in: f32) -> f32 {
    if phase_in >= 0.0 {
        return fmodf(phase_in + PI, 2.0 * PI) - PI;
    }
    fmodf(phase_in - PI, -2.0 * PI) + PI
}

//find a n number of harmonics by the fundamental index
#[inline(always)]
pub fn collect_harmonics(fundamental_index: usize) -> [usize; 4] {
    let mut harmonics = [0; 4];
    for n in 1..=4 {
        let harmonic_index = fundamental_index * n;
        harmonics[n - 1] = harmonic_index;
    }
    harmonics
}
