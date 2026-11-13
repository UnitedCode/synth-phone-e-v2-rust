//! Carrier generation shared by the device and host audio regressions.
use crate::constants::HOP_SIZE;
use synthphone_e_vocal_dsp::audio::{Oscillator, Waveform};

/// Returns false only when no MIDI notes are assigned (allowing keypad fallback).
/// Muted assigned notes still advance phase and do not enable keypad fallback.
pub fn render_midi_hop(
    oscillators: &mut [Oscillator; 8],
    frequencies: &[f32; 8],
    gains: &[f32; 8],
    waveform: Waveform,
    output: &mut [f32; HOP_SIZE],
) -> bool {
    let active = frequencies
        .iter()
        .filter(|&&f| f.is_finite() && f > 0.0)
        .count();
    output.fill(0.0);
    if active == 0 {
        return false;
    }
    for (i, &frequency) in frequencies.iter().enumerate() {
        if frequency.is_finite() && frequency > 0.0 {
            oscillators[i].set_waveform(waveform);
            oscillators[i].set_freq(frequency);
            let gain = if gains[i].is_finite() {
                gains[i].clamp(0.0, 8.0)
            } else {
                0.0
            } / active as f32;
            for sample in output.iter_mut() {
                *sample += oscillators[i].next_value() * gain;
            }
        }
    }
    true
}
