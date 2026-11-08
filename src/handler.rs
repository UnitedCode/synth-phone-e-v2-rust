use crate::midi::get_midi_queue_status;
use crate::percussion::DRUM_CHANNEL;
use crate::state_machine::{AppEvent, AppState, ProcessingProfile};
use crate::{constants::*, input::buttons::*};
use app_lib::output_mix;
use app_lib::vocal_frame::{config_for_speed, process_frame};
use libdaisy::gpio::Daisy1;
use libdaisy::prelude::Input;
use libdaisy::{audio, hid};
use log::{info, warn};
use rotary_encoder_embedded::Direction;
use rtic::Mutex;
use synthphone_e_vocal_dsp::audio::{get_frequency, Oscillator, Waveform};
use synthphone_e_vocal_dsp::dsp::{bitcrush, sample_rate_reduce, Fft1024, FftOps};
use synthphone_e_vocal_dsp::ring_buffer::RingBuffer;
use synthphone_e_vocal_dsp::{MusicalSettings, ProcessingMode};

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
    let mut crush_voices = false;
    // In Vocode / Harmony the auto-switch channel (MIDI ch 3) drives the vocal
    // effect only — its notes are silenced as raw synth so you hear the processed
    // voice, not a synth playing over it. Every other mode plays the note normally.
    let mut mute_melody_synth = false;
    let mut voice_gain = 1.0f32;
    let mut melody_gain = 1.0f32;
    let mut drum_gain = 1.0f32;
    let mut master_gain = 1.0f32;
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
        let profile = match snapshot.current_state {
            AppState::Processing(p) | AppState::EffectsProfile(p) | AppState::Menu(_, p) => p,
            AppState::Splash => ProcessingProfile::PitchControl,
        };
        // In Percussion the crush keys shape the drums, ringers and dial tones
        // too, so the voices are mixed in before the crush instead of after.
        crush_voices = profile == ProcessingProfile::Percussion;
        mute_melody_synth = matches!(
            profile,
            ProcessingProfile::Vocode | ProcessingProfile::Harmony
        );
        voice_gain = output_mix::level_gain(snapshot.voice_volume);
        melody_gain = output_mix::synth_gain(snapshot.melody_volume);
        drum_gain = snapshot.drum_volume as f32 * 0.1;
        master_gain = snapshot.volume as f32 * 0.1;
    });

    if audio.get_stereo(buffer) {
        for (left, right) in &buffer.as_slice()[..BLOCK_SIZE] {
            let sample = match is_hangup_button_pressed {
                false => *right,
                true => *left,
            };

            shared
                .in_ring
                .lock(|in_ring| in_ring.push(sample * voice_gain));

            let mut out_sample = shared.out_ring.lock(|out_ring| out_ring.pop());

            if !crush_voices {
                // ************** SAMPLE-RATE REDUCE **************
                out_sample =
                    sample_rate_reduce(out_sample, sr_factor, sr_hold_counter, sr_held_value);

                // ************** BIT DEPTH REDUCE **************
                out_sample = bitcrush(out_sample, bit_depth as i8);
            }

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
                                        120 => {
                                            voice_manager.all_notes_off(Some(channel));
                                        }
                                        123 => voice_manager.release_channel_notes(channel),
                                        64 => voice_manager.set_sustain(channel, value >= 64),
                                        121 => {
                                            voice_manager.set_sustain(channel, false);
                                            voice_manager.apply_channel_pitch_bend(channel, 0.0);
                                        }
                                        47 => shared.app_state_machine.lock(|msm| {
                                            msm.set_harmony_lead_from_midi(value);
                                        }),
                                        48 => {
                                            voice_manager.enable_midi_lead_routing();
                                            shared.app_state_machine.lock(|msm| {
                                                msm.set_harmony_backing_from_midi(value);
                                            });
                                        }
                                        7 => {
                                            shared.app_state_machine.lock(|msm| {
                                                msm.set_volume_from_midi(value);
                                            });
                                        }
                                        34..=42 | 44..=46 => {
                                            shared.app_state_machine.lock(|msm| {
                                                msm.set_menu_value_from_cc(controller, value);
                                            });
                                        }
                                        43 => {
                                            shared.app_state_machine.lock(|msm| {
                                                msm.cycle_key_from_midi(value);
                                            });
                                            shared.display_needs_update.lock(|flag| *flag = true);
                                            crate::rtic_app::app::display_update_task::spawn().ok();
                                        }
                                        80 => {
                                            shared.app_state_machine.lock(|msm| {
                                                msm.set_octave_from_midi(value);
                                            });
                                            shared.display_needs_update.lock(|flag| *flag = true);
                                            crate::rtic_app::app::display_update_task::spawn().ok();
                                        }
                                        81 => {
                                            shared.app_state_machine.lock(|msm| {
                                                msm.set_formant_from_midi(value);
                                            });
                                            shared.display_needs_update.lock(|flag| *flag = true);
                                            crate::rtic_app::app::display_update_task::spawn().ok();
                                        }
                                        82 => {
                                            shared.app_state_machine.lock(|msm| {
                                                msm.set_crush_from_midi(value);
                                            });
                                            shared.display_needs_update.lock(|flag| *flag = true);
                                            crate::rtic_app::app::display_update_task::spawn().ok();
                                        }
                                        _ => {}
                                    },
                                    crate::midi::MidiEvent::PitchBend { channel, value } => {
                                        let bend_ratio = (value as f32 - 8192.0) / 8192.0;
                                        voice_manager.apply_channel_pitch_bend(channel, bend_ratio);
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
            let (melody, drums) = shared.voice_manager.lock(|vm| {
                vm.get_mixed_sources_with_gains(mute_melody_synth, melody_gain, drum_gain)
            });
            out_sample =
                output_mix::sum_sources(out_sample, melody, drums, wave_type) * master_gain;

            if crush_voices {
                // Percussion: crush the voice and every MIDI voice together.
                out_sample =
                    sample_rate_reduce(out_sample, sr_factor, sr_hold_counter, sr_held_value);
                out_sample = bitcrush(out_sample, bit_depth as i8);
            }

            // Normalize final output
            out_sample = output_mix::finish(out_sample, 1.0);
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
    local: &mut crate::rtic_app::app::interface_handler::LocalResources,
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

                let actual_col = 2 - col;
                let key_num = row * 3 + actual_col + 1;

                if is_pressed {
                    update_state = true;
                    let (current_state, percussion_set) = shared.app_state_machine.lock(|msm| {
                        let snapshot = msm.snapshot();
                        (snapshot.current_state, snapshot.percussion)
                    });

                    if let AppState::Processing(ProcessingProfile::Percussion) = current_state {
                        // In Percussion mode keypad presses play the selected sound set
                        // (drums, ringers or dial tones) — inject directly into the MIDI
                        // queue rather than routing through the state machine.
                        if let Some(note) = local.held_keys.press(percussion_set, key_num) {
                            shared.midi_events.lock(|events| {
                                let _ = crate::midi::try_enqueue_midi_event(
                                    events,
                                    crate::midi::MidiEvent::NoteOn {
                                        channel: DRUM_CHANNEL,
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
                    // Button has just been released. Stop whatever note its press
                    // started, whatever screen we're on now, so held ringers and
                    // dial tones never get stuck.
                    if let Some(note) = local.held_keys.release(key_num) {
                        shared.midi_events.lock(|events| {
                            let _ = crate::midi::try_enqueue_midi_event(
                                events,
                                crate::midi::MidiEvent::NoteOff {
                                    channel: DRUM_CHANNEL,
                                    key: note,
                                    velocity: 0,
                                },
                            );
                        });
                    }
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
    vocal_state: &mut app_lib::vocal_frame::VocalStreamState,
    carrier_buffer: &mut RingBuffer<BUFFER_SIZE>,
    osc: &mut Oscillator,
    carrier_oscs: &mut [Oscillator; 8],
) {
    let mut current_process = ProcessingProfile::PitchControl;
    let mut formant = 0;
    let mut formant_male_ratio = 0.5f32;
    let mut formant_female_ratio = 2.0f32;
    let mut note = 0;
    let mut synth_octave_ratio = 1.0f32;
    let mut synth_gain = 1.0f32;
    let mut key = 0i8;
    let mut pitch_ratio = 1.0f32;
    let mut autotune_speed = 5;
    let mut harmony_lead = 1.0;
    let mut harmony_backing = 64;
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
        autotune_speed = snapshot.autotune_speed;
        harmony_lead = snapshot.harmony_lead as f32 / 127.0;
        harmony_backing = snapshot.harmony_backing;
        synth_gain = output_mix::synth_gain(snapshot.melody_volume);
        key = snapshot.key;
        // Continuous pitch offset: semitones → frequency ratio (2^(st/12)),
        // so -12/0/+12 give 0.5×/1×/2× and everything in between is chromatic.
        pitch_ratio = libm::exp2f(snapshot.pitch_semitones as f32 / 12.0);
        note = snapshot.note;
        // -1/0/+1 octave → 0.5×/1×/2× on the keypad synth note only
        synth_octave_ratio = libm::exp2f(snapshot.synth_octave as f32);
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
        ProcessingProfile::Tuned => ProcessingMode::PitchControl,
        ProcessingProfile::Harmony => ProcessingMode::Harmony,
        ProcessingProfile::Percussion => ProcessingMode::Dry,
    };

    // In Percussion the keypad plays drum-channel voices and never updates
    // `note`, so whatever note was last played in another profile would keep
    // driving the Dry carrier. Treat it as silent here.
    if current_process == ProcessingProfile::Percussion {
        note = 0;
    }

    let (midi_frequencies, midi_gains) = ctx
        .voice_manager
        .lock(|vm| vm.get_harmony_voices(harmony_backing));

    let is_tuned = current_process == ProcessingProfile::Tuned;

    if mode == ProcessingMode::Vocode || mode == ProcessingMode::Dry || is_tuned {
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
            // Dry / Tuned mode: keypad note drives the synth carrier
            if note > 0 {
                let carrier_hz = get_frequency(key, note, pitch_ratio, true) * synth_octave_ratio;
                osc.set_waveform(wave_type);
                osc.set_freq(carrier_hz);
                for _ in 0..HOP_SIZE {
                    carrier_buffer
                        .push(osc.next_value() * output_mix::keypad_gain(wave_type) * synth_gain);
                }
            } else {
                for _ in 0..HOP_SIZE {
                    carrier_buffer.push(0.0);
                }
            }
        }
    }

    // In Tuned and Pitch Control the voice must always autotune to the musical
    // scale: the DSP only snaps to the nearest scale note when no MIDI note is
    // held, so hide held MIDI notes from it — otherwise singing while playing
    // force-tunes your voice to the held note. Tuned also hides the keypad note.
    // Keypad/MIDI notes still sound through the synth (mixed elsewhere). Other
    // modes keep the live frequencies (Vocode/Harmony are driven by them).
    let effect_midi_frequencies = if is_tuned || mode == ProcessingMode::PitchControl {
        [0.0; 8]
    } else {
        midi_frequencies
    };

    let musical_settings = MusicalSettings {
        formant,
        formant_male_ratio,
        formant_female_ratio,
        note: if is_tuned { 0 } else { note },
        midi_frequencies: effect_midi_frequencies,
        midi_gains,
        key,
        octave_ratio: pitch_ratio,
        mode,
    };
    let mut config = config_for_speed(autotune_speed);
    config.harmony_lead_gain = harmony_lead;
    let mut input_buffer = [0.0; FFT_SIZE];
    let write_idx = ctx.in_pointer_cached.lock(|in_pointer| *in_pointer);
    ctx.in_ring
        .lock(|rb| rb.block_from::<FFT_SIZE>(write_idx, &mut input_buffer));

    let mut carrier_unwrapped_buffer: [f32; FFT_SIZE] = [0.0; FFT_SIZE];
    let write_idx = carrier_buffer.write_index();
    carrier_buffer.block_from(write_idx, &mut carrier_unwrapped_buffer);

    let mut synthesis_output = process_frame(
        &mut input_buffer,
        &mut carrier_unwrapped_buffer,
        last_input_phases,
        last_output_phases,
        cached_harmony_envelope,
        cached_harmony_inv_envelope,
        vocal_state,
        &config,
        &musical_settings,
    );

    // PitchControl doesn't mix a carrier, so layer the keypad synth on top
    // here using the same balance and windowing as Dry mode.
    if is_tuned && note > 0 {
        let window = Fft1024::get_hann_window();
        for ((out, synth), w) in synthesis_output
            .iter_mut()
            .zip(carrier_unwrapped_buffer.iter())
            .zip(window.iter())
        {
            *out = *out * 0.96 + synth * 0.04 * w;
        }
    }

    ctx.out_ring.lock(|output_ring| {
        output_ring.write_overlapped_samples(&synthesis_output);
    });
}
