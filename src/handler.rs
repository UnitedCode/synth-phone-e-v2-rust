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
    sr_hold_counter: &mut i32,
    sr_held_value: &mut f32,
    shared: &mut crate::rtic_app::app::update_handler::SharedResources,
) {
    hangup_button.update();
    let is_hangup_button_pressed = hangup_button.is_high();

    let mut sr_factor = 1;
    let mut wave_type = Waveform::Sine;
    let mut bit_depth = 32;
    let mut volume_gain = 1.0f32;
    let mut waveform_compensation = 1.0f32;
    shared.app_state_machine.lock(|msm| {
        let snapshot = msm.snapshot();
        sr_factor = snapshot.sample_reduction;
        wave_type = match snapshot.waveform {
            0 => Waveform::Triangle,
            1 => Waveform::Square,
            2 => Waveform::Saw,
            _ => Waveform::Sine,
        };
        bit_depth = snapshot.bit_rate;
        volume_gain = snapshot.volume as f32 / 10.0;
        // Sine and triangle are perceptually quieter than saw/square at the same
        // peak amplitude — they're spectrally pure, while saw/square spread energy
        // across many harmonics, which the ear perceives as louder. Compensate so
        // waveform choice doesn't also change perceived volume. Applied before the
        // peak limiter below (not after) so boosted peaks still get clamped instead
        // of hard-clipping at the DAC.
        waveform_compensation = match wave_type {
            Waveform::Sine => 1.4,
            Waveform::Triangle => 1.2,
            Waveform::Square | Waveform::Saw => 1.0,
        };
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
            out_sample = sample_rate_reduce(out_sample, sr_factor, sr_hold_counter, sr_held_value);

            // ************** BIT DEPTH REDUCE **************
            out_sample = bitcrush(out_sample, bit_depth as i8);

            // ************** PROCESS MIDI EVENTS **************
            // Process MIDI events when we have available CPU cycles
            // Only process a small batch per audio frame to prevent underruns
            //
            // All variants are handled inline here rather than deferred to a lower-
            // priority batch task. This runs far more often (every BLOCK_SIZE samples)
            // than a byte-rate-limited MIDI message can arrive, so a deferred task can
            // never win a fair race against it — it would just see the event
            // dequeued-and-requeued on every pass and starve indefinitely.
            shared.midi_events.lock(|events| {
                if !events.is_empty() {
                    shared.voice_manager.lock(|voice_manager| {
                        voice_manager.set_waveform(wave_type);
                        // Process up to 2 events per audio frame to maintain real-time performance
                        for _ in 0..2 {
                            if let Some(event) = events.dequeue() {
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
                                    crate::midi::MidiEvent::ControlChange {
                                        channel,
                                        controller,
                                        value,
                                    } => match controller {
                                        123 => {
                                            voice_manager.all_notes_off(Some(channel));
                                        }
                                        7 => {
                                            shared.app_state_machine.lock(|msm| {
                                                msm.set_volume_from_midi(value);
                                            });
                                        }
                                        34..=42 => {
                                            shared.app_state_machine.lock(|msm| {
                                                msm.set_menu_value_from_cc(controller, value);
                                            });
                                        }
                                        43 => {
                                            shared.app_state_machine.lock(|msm| {
                                                msm.cycle_key_from_midi(value);
                                            });
                                        }
                                        _ => {}
                                    },
                                    crate::midi::MidiEvent::PitchBend { channel: _, value } => {
                                        let bend_ratio = (value as f32 - 8192.0) / 8192.0;
                                        voice_manager.apply_pitch_bend(bend_ratio);
                                    }
                                    crate::midi::MidiEvent::ProgramChange { channel, program } => {
                                        shared.app_state_machine.lock(|msm| {
                                            msm.set_waveform_from_midi(channel, program);
                                        });
                                    }
                                    crate::midi::MidiEvent::Other => {
                                        // MIDI clock / active-sense / reset: discard.
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
            // Get MIDI sample and mix it with the processed audio. Waveform
            // compensation applies only to the synthesized note, not the live
            // audio-in signal already folded into out_sample above.
            let midi_sample = shared.voice_manager.lock(|vm| vm.get_mixed_sample());
            out_sample = out_sample + (midi_sample * waveform_compensation) * 0.1;

            // Normalize final output
            out_sample = normalize_sample(out_sample, 0.8) * volume_gain;
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
                    let current_state = shared
                        .app_state_machine
                        .lock(|msm| msm.snapshot().current_state);

                    if let AppState::Processing(ProcessingProfile::Percussion) = current_state {
                        // In Percussion mode keypad presses are drum notes — inject directly
                        // into the MIDI queue rather than routing through the state machine.
                        let actual_col = 2 - col;
                        let key_num = row * 3 + actual_col + 1;
                        if let Some(note) = keypad_to_drum_note(key_num) {
                            shared.midi_events.lock(|events| {
                                let _ = crate::midi::try_enqueue_midi_event(
                                    events,
                                    crate::midi::MidiEvent::NoteOn {
                                        channel: 9,
                                        key: note,
                                        velocity: 100,
                                    },
                                );
                            });
                        }
                    } else {
                        shared.app_state_machine.lock(|msm| {
                            msm.handle_event(handle_button_press(row, col, current_state));
                        });
                    }
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
    cached_harmony_envelope: &mut [f32; FFT_SIZE / 2],
    cached_harmony_inv_envelope: &mut [f32; FFT_SIZE / 2],
    previous_pitch_shift_ratio: &mut f32,
    carrier_buffer: &mut RingBuffer<BUFFER_SIZE>,
    osc: &mut Oscillator,
    carrier_oscs: &mut [Oscillator; 8],
) {
    let mut current_process = ProcessingProfile::PitchControl;
    let mut formant = 0;
    let mut formant_male_ratio = 0.5f32;
    let mut formant_female_ratio = 2.0f32;
    let mut note = 0;
    let mut key = 0i8;
    let mut pitch_ratio = 1.0f32;
    let mut percussion = 0;
    let mut wave_type = Waveform::Sine;
    ctx.app_state_machine.lock(|asm| {
        let snapshot = asm.snapshot();
        match snapshot.current_state {
            AppState::Processing(process)
            | AppState::EffectsProfile(process)
            | AppState::Menu(_, process) => {
                current_process = process;
            }
            AppState::Splash => {}
        }
        formant = snapshot.formant;
        key = snapshot.key;
        percussion = snapshot.percussion;
        // Continuous pitch offset: semitones → frequency ratio (2^(st/12)),
        // so -12/0/+12 give 0.5×/1×/2× and everything in between is chromatic.
        pitch_ratio = libm::exp2f(snapshot.pitch_semitones as f32 / 12.0);
        note = snapshot.note;
        wave_type = match snapshot.waveform {
            0 => Waveform::Triangle,
            1 => Waveform::Square,
            2 => Waveform::Saw,
            _ => Waveform::Sine,
        };
        let values = asm.get_values();
        // value 5 → 0.5 / 2.0 to match the previous hardcoded defaults
        formant_male_ratio = values.formant_male as f32 * 0.1;
        formant_female_ratio = values.formant_female as f32 * 0.4;
    });

    let mode = match current_process {
        ProcessingProfile::PitchControl => ProcessingMode::PitchControl,
        ProcessingProfile::Vocode => ProcessingMode::Vocode,
        ProcessingProfile::Dry => ProcessingMode::Dry,
        ProcessingProfile::Harmony => ProcessingMode::Harmony,
        ProcessingProfile::Percussion => ProcessingMode::Dry,
    };

    let midi_frequencies = ctx.voice_manager.lock(|vm| vm.get_cached_frequencies());

    if mode == ProcessingMode::Vocode || mode == ProcessingMode::Dry {
        if mode == ProcessingMode::Vocode {
            let active_count = midi_frequencies.iter().filter(|&&f| f > 0.0).count();
            if active_count > 0 {
                let scale = 1.0 / active_count as f32;
                for (i, &freq) in midi_frequencies.iter().enumerate() {
                    if freq > 0.0 {
                        carrier_oscs[i].set_waveform(wave_type);
                        carrier_oscs[i].set_freq(freq);
                    }
                }
                for _ in 0..HOP_SIZE {
                    let mut sample = 0.0f32;
                    for (i, &freq) in midi_frequencies.iter().enumerate() {
                        if freq > 0.0 {
                            sample += carrier_oscs[i].next_value();
                        }
                    }
                    carrier_buffer.push(sample * scale);
                }
            } else {
                // No MIDI notes held — fall back to keypad-driven pitch
                if note > 0 {
                    let carrier_hz = get_frequency(key, note, pitch_ratio, true);
                    osc.set_waveform(wave_type);
                    osc.set_freq(carrier_hz);
                    for _ in 0..HOP_SIZE {
                        carrier_buffer.push(osc.next_value());
                    }
                } else {
                    for _ in 0..HOP_SIZE {
                        carrier_buffer.push(0.0);
                    }
                }
            }
        } else {
            // Dry mode
            if note > 0 {
                let carrier_hz = get_frequency(key, note, pitch_ratio, true);
                osc.set_waveform(wave_type);
                osc.set_freq(carrier_hz);
                for _ in 0..HOP_SIZE {
                    carrier_buffer.push(osc.next_value());
                }
            } else {
                for _ in 0..HOP_SIZE {
                    carrier_buffer.push(0.0);
                }
            }
        }
    }

    if current_process == ProcessingProfile::Percussion {
        match percussion {
            0 => {
                //dial tones
            }
            1 => {
                //ringer
            }
            2 => {
                //drums
                //rech out to the voice manager and trigger drums
                //or get the drum sampler directly

                //let mut drum = DrumSampler::new();
                //drum.set_drum_type(DrumType::Cymbal);
                //drum.trigger();
                //carrier_buffer.push(drum.next_value());
            }
            _ => {
                //dial tone
            }
        }
    }

    let musical_settings = MusicalSettings {
        formant,
        formant_male_ratio,
        formant_female_ratio,
        note,
        midi_frequencies,
        key,
        octave_ratio: pitch_ratio,
        mode,
    };
    let config = VocalEffectsConfig::default();
    let mut input_buffer = [0.0; FFT_SIZE];
    let write_idx = ctx.in_pointer_cached.lock(|in_pointer| *in_pointer);
    ctx.in_ring
        .lock(|rb| rb.block_from::<FFT_SIZE>(write_idx, &mut input_buffer));

    let mut carrier_unwrapped_buffer: [f32; FFT_SIZE] = [0.0; FFT_SIZE];
    let write_idx = carrier_buffer.write_index();
    carrier_buffer.block_from(write_idx, &mut carrier_unwrapped_buffer);

    let synthesis_output = process_vocal_effects_1024(
        &mut input_buffer,
        Some(&mut carrier_unwrapped_buffer),
        last_input_phases,
        last_output_phases,
        cached_harmony_envelope,
        cached_harmony_inv_envelope,
        *previous_pitch_shift_ratio,
        &config,
        &musical_settings,
    );

    ctx.out_ring.lock(|output_ring| {
        output_ring.write_overlapped_samples(&synthesis_output);
    });
}
