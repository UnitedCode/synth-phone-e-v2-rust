use crate::{constants::*, display::screens::*, input::buttons::*};
use autotune::frequencies::{find_nearest_note_in_key, C_MAJOR_SCALE_FREQUENCIES};
use autotune::hann_window::{self, PI};
use autotune::keys::get_scale_by_key;
use autotune::process_frequencies::{
    bitcrush, cepstral_smoothing, find_fundamental_frequency, normalize_sample, sample_rate_reduce,
};
use libdaisy::audio;
use libm::{atan2f, cosf, floorf, fmodf, sinf, sqrtf};
use log::{info, warn};
use rotary_encoder_embedded::Direction;
use rtic::Mutex;
use state_machines::{AppState, MenuState, ProcessingProfile};
use autotune::oscillator::{Oscillator, Waveform};
static mut CARRIER_OSC: Option<Oscillator> = None;

pub fn update_handler(
    audio: &mut audio::Audio,
    buffer: &mut audio::AudioBuffer,
    shared: &mut crate::rtic_app::app::update_handler::SharedResources,
) {
    if audio.get_stereo(buffer) {
        for (_left, _right) in &buffer.as_slice()[..BLOCK_SIZE] {
            // Generate saw wave sample
            let saw_sample = shared.carrier_osc.lock(|osc| osc.next());
            
            let out_sample = saw_sample * 0.1;  // 20% volume
            
            // Apply a hard limiter as safety (optional)
            let out_sample = out_sample.clamp(-0.95, 0.95);

            if audio.push_stereo((out_sample, out_sample)).is_err() {
                warn!("Failed to write audio data");
            }
        }
        //     // Get current processing profile
        //     let mut current_process = ProcessingProfile::Autotune;
        //     shared.app_state_machine.lock(|msm| {
        //         let snapshot = msm.snapshot();
        //         match snapshot.current_state {
        //             AppState::Processing(process)
        //             | AppState::EffectsProfile(process)
        //             | AppState::Menu(_, process) => {
        //                 current_process = process;
        //             }
        //             AppState::Splash => {}
        //         }
        //     });

        //     // Lock to write to in_buffer
        //     shared.in_buffer.lock(|in_buffer| {
        //         in_buffer.write(*left);
        //     });

        //     // Process based on current processing profile
        //     let apply_effects = match current_process {
        //         ProcessingProfile::Autotune => true, // Apply autotune
        //         ProcessingProfile::Vocode => true,   // Apply vocoder
        //         ProcessingProfile::Dry => false,     // Passthrough (no processing)
        //     };

        //     // Get the processed audio if effects are enabled
        //     if apply_effects {
        //         shared.out_buffer.lock(|out_buffer| {
        //             out_sample = out_buffer.read_and_reset();
        //         });
        //     } else {
        //         out_sample = *left
        //     }

        //     // ************** SAMPLE-RATE REDUCE **************
        //     let mut sr_factor = 1;

        //     shared.app_state_machine.lock(|msm| {
        //         sr_factor = msm.snapshot().sample_reduction;
        //     });

        //     // Apply the effect
        //     shared.sr_hold_counter.lock(|hold_ctr| {
        //         shared.sr_held_value.lock(|held_val| {
        //             out_sample = sample_rate_reduce(out_sample, sr_factor, hold_ctr, held_val);
        //         });
        //     });

        //     // ************** BIT DEPTH REDUCE **************
        //     let mut bit_depth = 32;
        //     shared.app_state_machine.lock(|msm| {
        //         bit_depth = msm.snapshot().bit_rate;
        //     });
        //     out_sample = bitcrush(out_sample, bit_depth as u8);

        //     // Normalize final output
        //     out_sample = normalize_sample(out_sample, 0.8);

        //     // Check and handle hop counter
        //     let mut local_hop_counter: u32 = 0;
        //     shared.hop_counter.lock(|count| {
        //         local_hop_counter = *count;
        //     });

        //     if local_hop_counter >= HOP_SIZE as u32 {
        //         shared.hop_counter.lock(|count| {
        //             *count = 0;
        //         });

        //         // Run FFT Process in new software task
        //         if crate::rtic_app::app::dma1_stream0_software_task::spawn().is_err() {
        //             warn!("Could not unwrap software task - underrun error");
        //         }

        //         // Lock to advance the output buffer's hop
        //         shared.out_buffer.lock(|out_buffer| {
        //             out_buffer.next_hop();
        //         });
        //     }

        //     shared.hop_counter.lock(|count| {
        //         *count += 1;
        //     });

        //     // Output the processed audio
        //     if audio.push_stereo((out_sample, out_sample)).is_err() {
        //         warn!("Failed to write audio data");
        //     }
        //}
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
    // Check for single press if not a double press
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

    // Update display if state changed
    shared.app_state_machine.lock(|msm| {
        if update_state {
            let snapshot = msm.snapshot();
            info!("state - {:?} -", snapshot.current_state);

            match snapshot.current_state {
                // For splash screen
                AppState::Splash => {
                    draw_splash_screen(&mut local.display);
                }

                // For processing profiles
                AppState::Processing(process) => {
                    draw_processing_screen(
                        process,
                        snapshot.key,
                        snapshot.octave,
                        snapshot.note,
                        snapshot.volume,
                        &mut local.display,
                    );
                }

                // For effects screen
                AppState::EffectsProfile(process) => {
                    draw_effects_screen(
                        process,
                        snapshot.key,
                        snapshot.octave,
                        snapshot.crush,
                        snapshot.formant,
                        snapshot.key_down_pressed,
                        snapshot.process_cycle_pressed,
                        snapshot.key_up_pressed,
                        &mut local.display,
                    );
                }

                // For menu screens
                AppState::Menu(nav_state, _) => match nav_state {
                    MenuState::Selecting(_idx) => {
                        let menu_context = msm.current();
                        draw_menu_screen(
                            menu_context.previous_item,
                            menu_context.current_item,
                            menu_context.next_item,
                            false,
                            &mut local.display,
                        );
                    }
                    MenuState::Editing(_idx) => {
                        let menu_context = msm.current();
                        draw_menu_screen(
                            menu_context.previous_item,
                            menu_context.current_item,
                            menu_context.next_item,
                            true,
                            &mut local.display,
                        );
                    }
                },
            }

            local.display.flush().expect("could not draw to screen");
            update_state = false;
        }
    });
}

pub fn dma1_stream0_software_task(
    ctx: &mut crate::rtic_app::app::dma1_stream0_software_task::SharedResources,
) {
    // START ACTUAL FFT PROCESSING
    let analysis_window_buffer: [f32; FFT_SIZE] = hann_window::HANN_WINDOW;

    let mut unwrapped_buffer: [f32; FFT_SIZE] = hann_window::HANN_WINDOW;
    let mut full_spectrum: [microfft::Complex32; FFT_SIZE] =
        [microfft::Complex32 { re: 0.0, im: 0.0 }; FFT_SIZE];
    let mut analysis_magnitudes = [0.0; FFT_SIZE / 2];
    let mut analysis_frequencies = [0.0; FFT_SIZE / 2];
    let mut _synthesis_count = [0; FFT_SIZE / 2];

    // Copy buffer into FFT input, starting one window ago
    ctx.in_buffer.lock(|in_buffer| {
        in_buffer.push_read_back(FFT_SIZE - HOP_SIZE);
    });

    for n in 0..FFT_SIZE {
        ctx.in_buffer.lock(|in_buffer| {
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
        ctx.last_input_phases.lock(|last_input_phases| {
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
        ctx.last_input_phases.lock(|last_input_phases| {
            last_input_phases[i] = phase;
        });
    }

    // Zero out the synthesis bins, ready for new data
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

    let mut analysis_magnitudes_full = [0.0f32; FFT_SIZE];
    // Set the DC component.
    analysis_magnitudes_full[0] = analysis_magnitudes[0];
    // For bins 1 to FFT_SIZE/2 - 1, mirror the half-spectrum.
    for i in 1..(FFT_SIZE / 2) {
        analysis_magnitudes_full[i] = analysis_magnitudes[i];
        analysis_magnitudes_full[FFT_SIZE - i] = analysis_magnitudes[i];
    }

    //start here
    //1) compute the envelope using cepstral smoothing.
    let envelope = cepstral_smoothing(&analysis_magnitudes_full);

    // let mut carrier_osc = Oscillator::new(440.0, SAMPLE_RATE, Waveform::Saw);

    // // Create a time-domain carrier frame
    // let mut carrier_frame: [f32; FFT_SIZE] = [0.0; FFT_SIZE];
    // for i in 0..FFT_SIZE {
    //     carrier_frame[i] = carrier_osc.next();
    // }

    // // Apply windowing (same as modulator)
    // for i in 0..FFT_SIZE {
    //     carrier_frame[i] *= analysis_window_buffer[i];
    // }

    // // FFT the carrier
    // let fft_out = microfft::real::rfft_1024(&mut carrier_frame);
    // let mut carrier_fft = *fft_out; // copy the contents into a mutable array

    // // Apply the modulator’s envelope to the carrier's magnitude
    // for i in 0..FFT_SIZE / 2 {
    //     let phase = atan2f(carrier_fft[i].im, carrier_fft[i].re);
    //     let envelope_mag = envelope[i]; // Already cepstrally smoothed

    //     // Reconstruct real + imag from envelope magnitude + carrier phase
    //     let re = envelope_mag * cosf(phase);
    //     let im = envelope_mag * sinf(phase);

    //     ctx.synthesis_magnitudes.lock(|mags| {
    //         mags[i] = envelope_mag;
    //     });

    //     ctx.synthesis_frequencies.lock(|freqs| {
    //         freqs[i] = i as f32;
    //     });

    //     // Write into the FFT array used for synthesis
    //     carrier_fft[i].re = re;
    //     carrier_fft[i].im = im;

    //     full_spectrum[i] = carrier_fft[i];
    //     if i > 0 && i < FFT_SIZE / 2 {
    //         // Conjugate symmetry
    //         full_spectrum[FFT_SIZE - i] = Complex32 {
    //             re,
    //             im: -im,
    //         };
    //     }
    // }

    // TODO: the fundimental can now be found from the spectral analysis

    // // Get the fundamental frequency (Loudest)
    // let fundamental_index = find_fundamental_frequency(&analysis_magnitudes);
    // let _harmonics = collect_harmonics(fundamental_index);

    // // Exact frequency is tied to the bin.
    // let exact_frequency = analysis_frequencies[fundamental_index] * crate::constants::BIN_WIDTH;

    // // We cannot divide by 0
    // if exact_frequency > 0.001 {
    //     let mut scale_frequencies = &C_MAJOR_SCALE_FREQUENCIES;

    //     let mut octave_factor = 1.0;
    //     ctx.app_state_machine.lock(|msm| {
    //         octave_factor = msm.snapshot().octave as f32 * 0.5;
    //         if octave_factor <= 0.4 {
    //             octave_factor = 1.0;
    //         }
    //         scale_frequencies = get_scale_by_key(msm.snapshot().key);
    //     });

    //     let target_frequency = find_nearest_note_in_key(exact_frequency, scale_frequencies);
    //     let current_pitch_shift_ratio = target_frequency / exact_frequency;

    //     let previous_pitch_shift_ratio = ctx.previous_pitch_shift_ratio.lock(|ppr| *ppr);

    //     let pitch_shift_ratio =
    //         0.999 * current_pitch_shift_ratio + 0.001 * previous_pitch_shift_ratio;

    //     let formant_ratio = 1.0;

    //     // shift all bins by the ratio
    //     for i in 0..FFT_SIZE / 2 {
    //         let amplitude_in = analysis_magnitudes[i];
    //         let old_envelope = envelope[i].max(1e-9);
    //         let new_bin = (floorf(i as f32 * pitch_shift_ratio + 0.5) * octave_factor) as usize;
    //         if new_bin < FFT_SIZE / 2 {
    //             // find new envelope at new_bin
    //             let mut shifted_env_bin_f32 =
    //                 (i as f32 * formant_ratio).clamp(0.0, FFT_SIZE as f32 / 2.0 - 1.0);

    //             shifted_env_bin_f32 = floorf(shifted_env_bin_f32 + 0.5);

    //             let shifted_env_bin = shifted_env_bin_f32 as usize;
    //             let shifted_env_bin = shifted_env_bin.min(FFT_SIZE / 2 - 1);

    //             let new_envelope = envelope[shifted_env_bin];
    //             let adjusted_mag = amplitude_in * (new_envelope / old_envelope);

    //             ctx.synthesis_magnitudes.lock(|synthesis_magnitudes| {
    //                 synthesis_magnitudes[new_bin] = adjusted_mag;
    //             });
    //             ctx.synthesis_frequencies.lock(|synthesis_frequencies| {
    //                 synthesis_frequencies[new_bin] =
    //                     analysis_frequencies[i] * pitch_shift_ratio * octave_factor;
    //             });
    //         }
    //     }
    // }

    // SYNTHESIS
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
        });

        fft[i].re = amplitude * cosf(out_phase);
        fft[i].im = amplitude * sinf(out_phase);

        // Also store the complex conjugate in the upper half of the spectrum
        full_spectrum[i] = fft[i]; // First half directly
        if i > 0 && i < (FFT_SIZE / 2) {
            // Conjugate symmetry for the second half
            full_spectrum[FFT_SIZE - i] = fft[i].conj();
        }

        // Save the phase for the next hop
        ctx.last_output_phases.lock(|last_output_phases| {
            last_output_phases[i] = out_phase;
        });
    }

    // Run the inverse FFT
    let res = microfft::inverse::ifft_1024(&mut full_spectrum);

    // Add time domain into the output buffer
    for (n, val) in res.iter().enumerate() {
        let windowed_val = val.re * analysis_window_buffer[n]; // Window again and scale
        ctx.out_buffer.lock(|out_buffer| {
            out_buffer.add_value(windowed_val);
        });
    }
}

#[inline(always)]
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
