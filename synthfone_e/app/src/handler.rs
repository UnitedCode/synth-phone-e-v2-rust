use crate::{constants::*, display::screens::*, input::buttons::*};
use autotune::circular_buffer;
use autotune::ring_buffer::RingBuffer;
use autotune::frequencies::{find_nearest_note_in_key, C_MAJOR_SCALE_FREQUENCIES};
use autotune::hann_window::{self, PI};
use autotune::normal_phase_advance::{NORMAL_PHASE_ADVANCE};
use autotune::fade::{QUADRATIC_FADE};
use autotune::keys::{get_scale_by_key, get_frequency};
use autotune::process_frequencies::{
    bitcrush, cepstral_smoothing, find_fundamental_frequency, normalize_sample, sample_rate_reduce,
};
use libdaisy::audio;
use libm::{atan2f, cosf, floorf, fmodf, sinf, sqrtf, logf, powf, expf};
use log::{info, warn};
use rotary_encoder_embedded::Direction;
use rtic::Mutex;
use state_machines::{AppState, MenuState, ProcessingProfile};
use autotune::oscillator::{Oscillator, Waveform};
use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering; 

pub fn update_handler(
    audio: &mut audio::Audio,
    buffer: &mut audio::AudioBuffer,
    shared: &mut crate::rtic_app::app::update_handler::SharedResources,
) {
    if audio.get_stereo(buffer) {
        
        let snap = shared.app_state_machine.lock(|msm| msm.snapshot());
        let sr_factor      = snap.sample_reduction;
        let bit_depth      = snap.bit_rate;
        let note           = snap.note;
        let key            = snap.key;
        let octave         = snap.octave;

        let current_process = match snap.current_state {
            AppState::Processing(p)
            | AppState::EffectsProfile(p)
            | AppState::Menu(_, p) => p,
            AppState::Splash       => ProcessingProfile::Autotune,
        };

        for (_left, _right) in &buffer.as_slice()[..BLOCK_SIZE] {
            // Lock to write to in_buffer
            shared.in_ring.lock(|in_ring| in_ring.push(*_left));
            
            
            if(current_process == ProcessingProfile::Vocode || current_process == ProcessingProfile::Dry){
                
                let carrier_hz = get_frequency(key, note, octave, true);
                
                let sample = shared.carrier_osc.lock(|osc| {
                    osc.set_freq(carrier_hz);
                    osc.next()
                });
                
                shared.carrier_ring.lock(|carrier_ring| carrier_ring.push(sample));
            }
            
            let mut out_sample = shared.out_ring.lock(|out_ring| out_ring.pop());

            // ************** SAMPLE-RATE REDUCE **************
            // Apply the effect
            shared.sr_hold_counter.lock(|hold_ctr| {
                shared.sr_held_value.lock(|held_val| {
                    out_sample = sample_rate_reduce(out_sample, sr_factor, hold_ctr, held_val);
                });
            });

            // ************** BIT DEPTH REDUCE **************
            out_sample = bitcrush(out_sample, bit_depth as u8);

            // Normalize final output
            out_sample = normalize_sample(out_sample, 0.8);
            // **********************************************

            // Check and handle hop counter
            let mut local_hop_counter: u32 = 0;
            shared.hop_counter.lock(|count| {
                local_hop_counter = *count;
            });

            if local_hop_counter >= HOP_SIZE as u32 {
                shared.hop_counter.lock(|count| {
                    *count = 0;
                });

                let pointer = shared.in_ring.lock(|in_ring|{in_ring.write_index()});
                
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
        
        update_state = false;
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

//TODO: these have a lot of similar code and processes, deal with it
#[inline(always)]
pub fn process_dry(ctx: &mut crate::rtic_app::app::dma1_stream0_software_task::SharedResources){
    // Window and buffers
    let analysis_window_buffer: [f32; FFT_SIZE] = hann_window::HANN_WINDOW;
    let mut unwrapped_buffer: [f32; FFT_SIZE] = hann_window::HANN_WINDOW;
    let mut unwrapped_synth_buffer: [f32; FFT_SIZE] = hann_window::HANN_WINDOW;
    let mut full_spectrum: [microfft::Complex32; FFT_SIZE] = 
        [microfft::Complex32 { re: 0.0, im: 0.0 }; FFT_SIZE];
    let mut analysis_magnitudes = [0.0; FFT_SIZE / 2];
    let mut analysis_frequencies = [0.0; FFT_SIZE / 2];

    let write_idx = ctx.in_pointer_cached.lock(|in_pointer|{ in_pointer.load(Ordering::Relaxed)});

    ctx.in_ring.lock(|rb|  rb.block_from::<FFT_SIZE>(write_idx,   &mut unwrapped_buffer));
    ctx.carrier_ring.lock(|rb| rb.block_from::<FFT_SIZE>(write_idx,   &mut unwrapped_synth_buffer));    

    // Copy buffer into FFT input
    for i in 0..FFT_SIZE {
        unwrapped_buffer[i] *= analysis_window_buffer[i];
        unwrapped_synth_buffer[i] *= analysis_window_buffer[i];
    }

    // Forward FFT
    let fft = microfft::real::rfft_1024(&mut unwrapped_buffer);

    // Get settings from state machine
    let mut formant = 0;
    let mut pitch_shift_ratio = 1.0;
    let mut note = 0;
    ctx.app_state_machine.lock(|asm|{
        formant = asm.snapshot().formant;
        // Use octave as pitch control (0.5 = down octave, 2.0 = up octave)
        let octave_factor = asm.snapshot().octave as f32 * 0.5;
        pitch_shift_ratio = if octave_factor <= 0.4 { 1.0 } else { octave_factor };
        note = asm.snapshot().note;
        //formant_ratio = asm.snapshot().formant_factor;
    });

    // If no effects, just pass through
    if formant == 0 && (pitch_shift_ratio > 0.99 && pitch_shift_ratio < 1.01) {
        // Direct pass-through - just copy spectrum
        for i in 0..512 {
            full_spectrum[i] = fft[i];
        }
        for i in 1..512 {
            full_spectrum[FFT_SIZE - i] = fft[i].conj();
        }
    } else {
        // ANALYSIS - Phase vocoder
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

        // ENVELOPE EXTRACTION (only if formant shifting)
        let mut envelope = [1.0f32; FFT_SIZE / 2];
        
        if formant != 0 {
            const LIFTER_CUTOFF: usize = 64;
            let mut cepstrum_buffer = [0.0f32; FFT_SIZE];
            
            // Log magnitude spectrum
            for i in 0..(FFT_SIZE / 2) {
                let mag = analysis_magnitudes[i].max(1e-6);
                let log_mag = logf(mag);
                full_spectrum[i] = microfft::Complex32 { re: log_mag, im: 0.0 };
                if i != 0 {
                    full_spectrum[FFT_SIZE - i] = microfft::Complex32 { re: log_mag, im: 0.0 };
                }
            }
            
            // Get cepstrum
            let cepstrum = microfft::inverse::ifft_1024(&mut full_spectrum);
            
            // Lifter
            for i in 0..LIFTER_CUTOFF {
                cepstrum_buffer[i] = cepstrum[i].re;
            }
            for i in (FFT_SIZE - LIFTER_CUTOFF)..FFT_SIZE {
                cepstrum_buffer[i] = cepstrum[i].re;
            }
            
            // Get envelope
            let envelope_fft = microfft::real::rfft_1024(&mut cepstrum_buffer);
            for i in 0..(FFT_SIZE / 2) {
                envelope[i] = expf(envelope_fft[i].re);
            }
        }

        // Zero synthesis arrays
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

        // Formant shift ratio
        let formant_ratio = match formant {
            1 => 0.8,  // Lower formants
            2 => 1.3,  // Raise formants
            _ => 1.0,  // No formant shift
        };

        // PITCH AND FORMANT SHIFTING
        for i in 0..FFT_SIZE / 2 {
            // Get residual
            let residual = if formant != 0 {
                analysis_magnitudes[i] / envelope[i].max(1e-6)
            } else {
                analysis_magnitudes[i]
            };

            // New bin for pitch shift
            let new_bin = (floorf(i as f32 * pitch_shift_ratio + 0.5)) as usize;
            
            if new_bin < FFT_SIZE / 2 {
                // Get shifted envelope value
                let shifted_envelope = if formant != 0 {
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
                    synthesis_frequencies[new_bin] = analysis_frequencies[i] * pitch_shift_ratio;
                });
            }
        }

        // SYNTHESIS - Phase vocoder
        for i in 0..FFT_SIZE / 2 {
            let amplitude = ctx.synthesis_magnitudes.lock(|synthesis_magnitudes| synthesis_magnitudes[i]);
            let bin_deviation = ctx.synthesis_frequencies.lock(|synthesis_frequencies| synthesis_frequencies[i] - i as f32);
            
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
                im: amplitude * sinf(out_phase)
            };
            
            if i > 0 && i < (FFT_SIZE / 2) {
                full_spectrum[FFT_SIZE - i] = full_spectrum[i].conj();
            }
        }
    }

    // Inverse FFT
    let res = microfft::inverse::ifft_1024(&mut full_spectrum);

    let playing_note = note != 0;

    // Overlap-add to output
    let mut synth_frame = [0.0f32; FFT_SIZE];
    for i in 0..FFT_SIZE {
        let vocals = res[i].re;
        let synth = unwrapped_synth_buffer[i];
        let mixed = if playing_note { vocals * 0.96 + synth * 0.04 } else { vocals };
        synth_frame[i] = mixed * analysis_window_buffer[i];
    }

    ctx.out_ring.lock(|rb| {
        for (i, &sample) in synth_frame.iter().enumerate() {
            rb.add_at_offset(i as u32, sample);
        }
    });
}

#[inline(always)]
pub fn process_vocode(ctx: &mut crate::rtic_app::app::dma1_stream0_software_task::SharedResources){
    let mut input_unwrapped_buffer: [f32; FFT_SIZE] = hann_window::HANN_WINDOW;
    let mut carrier_unwrapped_buffer: [f32; FFT_SIZE] = hann_window::HANN_WINDOW;
    let mut full_spectrum: [microfft::Complex32; FFT_SIZE] = [microfft::Complex32 { re: 0.0, im: 0.0 }; FFT_SIZE];
    let analysis_window_buffer: [f32; FFT_SIZE] = hann_window::HANN_WINDOW;

    let write_idx = ctx.in_pointer_cached.lock(|in_pointer|{ in_pointer.load(Ordering::Relaxed)});

    ctx.in_ring.lock(|rb|  rb.block_from::<FFT_SIZE>(write_idx,   &mut input_unwrapped_buffer));
    ctx.carrier_ring.lock(|rb| rb.block_from::<FFT_SIZE>(write_idx,   &mut carrier_unwrapped_buffer));    

    // Copy buffer into FFT input
    for i in 0..FFT_SIZE {
        input_unwrapped_buffer[i] *= analysis_window_buffer[i];
        carrier_unwrapped_buffer[i] *= analysis_window_buffer[i];
    }

    let modulator_fft = microfft::real::rfft_1024(&mut input_unwrapped_buffer);
    let carrier_fft = microfft::real::rfft_1024(&mut carrier_unwrapped_buffer);

    // Copy the first half (including DC and Nyquist)
    for i in 0..(FFT_SIZE / 2) {
        // Get modulator magnitude
        let mod_mag = libm::sqrtf(modulator_fft[i].re * modulator_fft[i].re + modulator_fft[i].im * modulator_fft[i].im);
        
        // Get carrier magnitude
        let car_mag = libm::sqrtf(carrier_fft[i].re * carrier_fft[i].re + carrier_fft[i].im * carrier_fft[i].im);
        
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
            let windowed_sample = res[i].re * analysis_window_buffer[i];
            rb.add_at_offset(i as u32, windowed_sample);
        }
    });
}

#[inline(always)]
pub fn process_autotune(ctx: &mut crate::rtic_app::app::dma1_stream0_software_task::SharedResources){

     // START ACTUAL FFT PROCESSING
    let analysis_window_buffer: [f32; FFT_SIZE] = hann_window::HANN_WINDOW;

    let mut unwrapped_buffer: [f32; FFT_SIZE] = hann_window::HANN_WINDOW;
    let mut full_spectrum: [microfft::Complex32; FFT_SIZE] =
        [microfft::Complex32 { re: 0.0, im: 0.0 }; FFT_SIZE];
    let mut analysis_magnitudes = [0.0; FFT_SIZE / 2];
    let mut analysis_frequencies = [0.0; FFT_SIZE / 2];
    let mut _synthesis_count = [0; FFT_SIZE / 2];

    let write_idx = ctx.in_pointer_cached.lock(|in_pointer|{ in_pointer.load(Ordering::Relaxed)});
    ctx.in_ring.lock(|rb|  rb.block_from::<FFT_SIZE>(write_idx,   &mut unwrapped_buffer));

    // Copy buffer into FFT input
    for i in 0..FFT_SIZE {
        unwrapped_buffer[i] *= analysis_window_buffer[i];
    }

    // Process the FFT based on the time domain input
    let fft = microfft::real::rfft_1024(&mut unwrapped_buffer);

    let mut formant = 0;
    let mut note = 0;
    ctx.app_state_machine.lock(|asm|{
        formant = asm.snapshot().formant;
        note = asm.snapshot().note
    });

    let is_auto = note == 0;

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

    // Now compute the envelope using cepstral smoothing.
    let mut envelope = [1.0f32; FFT_SIZE / 2];

    if formant != 0 {
        // SIMPLE METHOD: Moving average of magnitude spectrum
        // Much faster than cepstral analysis
        const WINDOW_SIZE: usize = 16; // Adjust for smoothness vs detail
        
        for i in 0..(FFT_SIZE / 2) {
            let start = i.saturating_sub(WINDOW_SIZE / 2);
            let end = ((i + WINDOW_SIZE / 2) + 1).min(FFT_SIZE / 2);
            
            let mut sum = 0.0;
            let mut count = 0;
            
            for j in start..end {
                sum += analysis_magnitudes[j];
                count += 1;
            }
            
            envelope[i] = if count > 0 { sum / count as f32 } else { 1.0 };
        }
        
        // Optional: Apply a second smoothing pass for better results
        let mut smooth_envelope = envelope;
        for i in 1..(FFT_SIZE / 2 - 1) {
            smooth_envelope[i] = 0.25 * envelope[i-1] + 0.5 * envelope[i] + 0.25 * envelope[i+1];
        }
        envelope = smooth_envelope;
    }

    // Get the fundamental frequency (Loudest)
    let fundamental_index = find_fundamental_frequency(&analysis_magnitudes);
    //let _harmonics = collect_harmonics(fundamental_index);

    // Exact frequency is tied to the bin.
    let exact_frequency = analysis_frequencies[fundamental_index] * crate::constants::BIN_WIDTH;

        // Zero synthesis arrays
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

    // We cannot divide by 0
    if exact_frequency > 0.001 {
        let mut scale_frequencies = &C_MAJOR_SCALE_FREQUENCIES;

        let mut octave_factor = 1.0;
        let mut key = 0;
        let mut octave = 2;
        ctx.app_state_machine.lock(|msm| {
            octave = msm.snapshot().octave;
            octave_factor = octave as f32 * 0.5;
            if octave_factor <= 0.4 {
                octave_factor = 1.0;
            }

            key = msm.snapshot().key;
            scale_frequencies = get_scale_by_key(key);

        });

        let target_frequency = if(is_auto){
            find_nearest_note_in_key(exact_frequency, scale_frequencies)
        }else{
            get_frequency(key, note, octave, false)
        };
        let current_pitch_shift_ratio = target_frequency / exact_frequency;

        let previous_pitch_shift_ratio = ctx.previous_pitch_shift_ratio.lock(|ppr| *ppr);

        let pitch_shift_ratio =
            0.999 * current_pitch_shift_ratio + 0.001 * previous_pitch_shift_ratio;

        let formant_ratio = match formant {
            1 => 0.5,  // Lower formants
            2 => 2.0,  // Raise formants
            _ => 1.0,  // No formant shift
        };

        // shift all bins by the ratio
        for i in 0..FFT_SIZE / 2 {

            // Get residual (source magnitude divided by source envelope)
            let residual = if formant != 0 {
                analysis_magnitudes[i] / envelope[i].max(1e-6)
            } else {
                analysis_magnitudes[i]
            };

            //new bin for the pitch shift
            let new_bin = (floorf(i as f32 * pitch_shift_ratio + 0.5) * octave_factor) as usize;
            
            if new_bin < FFT_SIZE / 2 {
                // Get shifted envelope value
                let shifted_envelope = if formant != 0 {
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
                        analysis_frequencies[i] * pitch_shift_ratio * octave_factor;
                });
            }
        }
    }

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

    ctx.out_ring.lock(|rb| {
        for i in 0..FFT_SIZE {
            let windowed_sample = res[i].re * analysis_window_buffer[i];
            rb.add_at_offset(i as u32, windowed_sample);
        }
    });
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
