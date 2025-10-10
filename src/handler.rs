use crate::midi::get_midi_queue_status;
use crate::state_machine::{AppEvent, AppState, ProcessingProfile};
use crate::{constants::*, input::buttons::*};
use libdaisy::gpio::Daisy1;
use libdaisy::prelude::Input;
use libdaisy::{audio, hid};
use log::{info, warn};
use rotary_encoder_embedded::Direction;
use rtic::Mutex;
use synthphone_e_vocal_dsp::audio::{get_frequency, Oscillator, Waveform};
use synthphone_e_vocal_dsp::dsp::{bitcrush, normalize_sample, sample_rate_reduce};
use synthphone_e_vocal_dsp::ring_buffer::RingBuffer;
use synthphone_e_vocal_dsp::{
    process_vocal_effects_1024, MusicalSettings, ProcessingMode, VocalEffectsConfig,
};

pub fn audio_handler(
    audio: &mut audio::Audio,
    buffer: &mut audio::AudioBuffer,
    hangup_button: &mut hid::Switch<Daisy1<Input>>,
    hop_counter: &mut u32,
    shared: &mut crate::rtic_app::app::update_handler::SharedResources,
) {
    hangup_button.update();
    let is_hangup_button_pressed = hangup_button.is_high();

    let mut sr_factor = 1;
    let mut wave_type = Waveform::Sine;
    let mut bit_depth = 32;
    shared.app_state_machine.lock(|msm| {
        sr_factor = msm.snapshot().sample_reduction;
        wave_type = match msm.snapshot().waveform {
            0 => Waveform::Triangle,
            1 => Waveform::Square,
            2 => Waveform::Saw,
            _ => Waveform::Sine,
        };
        bit_depth = msm.snapshot().bit_rate;
    });

    if audio.get_stereo(buffer) {
        for (left, right) in &buffer.as_slice()[..BLOCK_SIZE] {
            let sample = match is_hangup_button_pressed {
                false => *right,
                true => *left,
            };

            shared.in_ring.lock(|in_ring| in_ring.push(sample));

            let mut out_sample = shared.out_ring.lock(|out_ring| out_ring.pop());

            // ************** SAMPLE-RATE REDUCE **************
            // Apply the effect
            shared.sr_hold_counter.lock(|hold_ctr| {
                shared.sr_held_value.lock(|held_val| {
                    out_sample = sample_rate_reduce(out_sample, sr_factor, hold_ctr, held_val);
                });
            });

            // ************** BIT DEPTH REDUCE **************
            out_sample = bitcrush(out_sample, bit_depth as i8);

            // ************** PROCESS MIDI EVENTS **************
            // Process MIDI events when we have available CPU cycles
            // Only process a small batch per audio frame to prevent underruns
            shared.midi_events.lock(|events| {
                if !events.is_empty() {
                    shared.voice_manager.lock(|voice_manager| {
                        voice_manager.set_waveform(wave_type);
                        // Process up to 2 events per audio frame to maintain real-time performance
                        for _ in 0..2 {
                            if let Some(event) = events.dequeue() {
                                // Quick inline processing for critical events
                                match event {
                                    crate::midi::MidiEvent::NoteOn {
                                        channel,
                                        key,
                                        velocity,
                                    } => {
                                        voice_manager.note_on(key, velocity, channel);
                                    }
                                    crate::midi::MidiEvent::NoteOff { channel, key, .. } => {
                                        voice_manager.note_off(key, channel);
                                    }
                                    _ => {
                                        // Re-queue non-critical events for batch processing
                                        let _ = events.enqueue(event);
                                        break;
                                    }
                                }
                            } else {
                                break;
                            }
                        }
                    });
                }
            });

            // ************** ADD MIDI OUTPUT **************
            // Get MIDI sample and mix it with the processed audio
            let midi_sample = shared.voice_manager.lock(|vm| vm.get_mixed_sample());
            out_sample = out_sample + midi_sample * 0.1; // Mix at 10% volume

            // Normalize final output
            out_sample = normalize_sample(out_sample, 0.8);
            // **********************************************

            // Check and handle hop counter
            if *hop_counter >= HOP_SIZE as u32 {
                *hop_counter = 0;

                let pointer = shared.in_ring.lock(|in_ring| in_ring.write_index());

                shared.in_pointer_cached.lock(|cache| {
                    *cache = pointer;
                });

                if crate::rtic_app::app::dma1_stream0_fft_task::spawn().is_err() {
                    warn!("Could not unwrap software task - underrun error");
                }
            };

            *hop_counter += 1;

            if audio.push_stereo((out_sample, out_sample)).is_err() {
                warn!("Failed to write audio data");
            }
        }

        // Monitor MIDI queue status periodically using hop_counter
        if *hop_counter % 4800 == 0 {
            // Check every ~100ms at 48kHz
            shared.midi_events.lock(|events| {
                let (len, capacity) = get_midi_queue_status(events);
                if len > capacity * 3 / 4 {
                    // Warn if queue is >75% full
                    warn!("MIDI queue high: {}/{}", len, capacity);
                }
            });
        }
    } else {
        warn!("Error reading data!");
    }
}

pub fn interface_handler(
    local: crate::rtic_app::app::interface_handler::LocalResources,
    shared: &mut crate::rtic_app::app::interface_handler::SharedResources,
) {
    local.timer2.clear_irq();

    let mut update_state: bool = false;

    let new_matrix_state = scan_button_matrix(
        local.col_1_pin,
        local.col_2_pin,
        local.col_3_pin,
        local.row_1_pin,
        local.row_2_pin,
        local.row_3_pin,
        local.row_4_pin,
    );

    for row in 0..4 {
        for col in 0..3 {
            let was_pressed = shared.old_matrix_state.lock(|oms| oms[row][col]);
            let is_pressed = new_matrix_state[row][col];

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
            msm.handle_event(AppEvent::EncoderDoublePress);
        });
    }
    //Check for single press if not a double press
    else if local.encoder_button.is_falling() {
        info!("Encoder single press detected!");
        update_state = true;

        shared.app_state_machine.lock(|msm| {
            msm.handle_event(AppEvent::EncoderPress);
        });
    }

    // Handle encoder rotation
    match local.knob_1.rotary_encoder.update() {
        Direction::Clockwise => {
            update_state = true;
            shared.app_state_machine.lock(|msm| {
                msm.handle_event(AppEvent::EncoderRotate(-1));
            });
        }
        Direction::Anticlockwise => {
            update_state = true;
            shared.app_state_machine.lock(|msm| {
                msm.handle_event(AppEvent::EncoderRotate(1));
            });
        }
        Direction::None => {}
    }

    if update_state {
        // Set the flag
        shared.display_needs_update.lock(|flag| *flag = true);

        // Spawn the display task to run
        crate::rtic_app::app::display_update_task::spawn().ok();

        // update_state = false;
    }
}

pub fn handle_vocal_effects(
    ctx: &mut crate::rtic_app::app::dma1_stream0_fft_task::SharedResources,
    last_input_phases: &mut [f32; FFT_SIZE],
    last_output_phases: &mut [f32; FFT_SIZE],
    previous_pitch_shift_ratio: &mut f32,
    carrier_buffer: &mut RingBuffer<FFT_SIZE>,
    osc: &mut Oscillator,
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
    // process_vocal_effects_config!(process_vocal_effects, 1024, 48_014.312);
    let mut formant = 0;
    let mut pitch_shift_ratio = 1.0;
    let mut note = 0;
    let key = 0;
    let mut octave = 2;
    let mut wave_type = Waveform::Sine;
    ctx.app_state_machine.lock(|asm| {
        formant = asm.snapshot().formant;
        // Use octave as pitch control (0.5 = down octave, 2.0 = up octave)
        octave = asm.snapshot().octave;
        let octave_factor = octave as f32 * 0.5;
        pitch_shift_ratio = if octave_factor <= 0.4 {
            1.0
        } else {
            octave_factor
        };
        note = asm.snapshot().note;
        //formant_ratio = asm.snapshot().formant_factor;
        //
        wave_type = match asm.snapshot().waveform {
            0 => Waveform::Triangle,
            1 => Waveform::Square,
            2 => Waveform::Saw,
            _ => Waveform::Sine,
        }
    });

    let mode = match current_process {
        ProcessingProfile::Autotune => ProcessingMode::Autotune,
        ProcessingProfile::Vocode => ProcessingMode::Vocode,
        ProcessingProfile::Dry => ProcessingMode::Dry,
    };

    if mode == ProcessingMode::Vocode || mode == ProcessingMode::Dry {
        let carrier_hz = get_frequency(key, note, octave, true);

        osc.set_waveform(wave_type);

        osc.set_freq(carrier_hz);
        for _ in 0..FFT_SIZE {
            let sample = osc.next_value();
            carrier_buffer.push(sample);
        }
    }

    let musical_settings = MusicalSettings {
        formant: formant,
        note: note,
        key: key,
        octave: octave,
        mode: mode,
    };
    let config = VocalEffectsConfig::default();
    let mut input_buffer = [0.0; FFT_SIZE];
    let write_idx = ctx.in_pointer_cached.lock(|in_pointer| *in_pointer);
    ctx.in_ring
        .lock(|rb| rb.block_from::<FFT_SIZE>(write_idx, &mut input_buffer));

    let mut carrier_unwrapped_buffer: [f32; FFT_SIZE] = [0.0; FFT_SIZE];
    let write_idx = 0;
    carrier_buffer.block_from(write_idx, &mut carrier_unwrapped_buffer);

    let synthesis_output = process_vocal_effects_1024(
        &mut input_buffer,
        Some(&mut carrier_unwrapped_buffer),
        last_input_phases,
        last_output_phases,
        *previous_pitch_shift_ratio,
        &config,
        &musical_settings,
    );

    ctx.out_ring.lock(|output_ring| {
        output_ring.write_overlapped_samples(&synthesis_output);
    });
}
