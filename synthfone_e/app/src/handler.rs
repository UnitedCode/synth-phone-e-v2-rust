use crate::{constants::*, display::screens::*, input::buttons::*};
use autotune::frequencies::{find_nearest_note_in_key, C_MAJOR_SCALE_FREQUENCIES};
use autotune::hann_window::{self, PI};
use autotune::keys::{get_scale_by_key, get_frequency};
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

pub fn update_handler(
    audio: &mut audio::Audio,
    buffer: &mut audio::AudioBuffer,
    shared: &mut crate::rtic_app::app::update_handler::SharedResources,
) {
    if audio.get_stereo(buffer) {
        for (_left, _right) in &buffer.as_slice()[..BLOCK_SIZE] {
            let mut out_sample = *_left;

            // Lock to write to in_buffer
            shared.in_buffer.lock(|in_buffer| {
                in_buffer.write(*_left);
            });

            shared.out_buffer.lock(|out_buffer| {
                out_sample = out_buffer.read_and_reset();
            });

            // Check and handle hop counter
            let mut local_hop_counter: u32 = 0;
            shared.hop_counter.lock(|count| {
                local_hop_counter = *count;
            });

            if local_hop_counter >= HOP_SIZE as u32 {
                shared.hop_counter.lock(|count| {
                    *count = 0;
                });

                // Run FFT Process in new software task
                if crate::rtic_app::app::dma1_stream0_software_task::spawn().is_err() {
                    warn!("Could not unwrap software task - underrun error");
                }

                // Lock to advance the output buffer's hop
                shared.out_buffer.lock(|out_buffer| {
                    out_buffer.next_hop();
                });
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
    // window + carrier block ------------------------------------------------
    let mut carrier_buf = [0.0_f32; FFT_SIZE];
    //let window = hann_window::HANN_WINDOW;

    // read state → frequency
    let (note, key, octave) = ctx.app_state_machine.lock(|m| {
        let s = m.snapshot();
        (s.note, s.key, s.octave)
    });
    let carrier_hz = get_frequency(key, note, octave);

    

    // 1. retune
    ctx.carrier_osc.lock(|osc| {
        osc.set_freq(carrier_hz);
        for i in 0..FFT_SIZE {
            carrier_buf[i] = osc.next();        // NO window for the test
        }
    });

    // 2. overlap-add – no FFT
    ctx.out_buffer.lock(|b| {
        for s in carrier_buf { b.add_value(s); }
        b.next_hop();
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
