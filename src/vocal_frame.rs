//! Hardware-independent entry point shared by the firmware and audio lab.
pub use synthphone_e_vocal_dsp::VocalStreamState;
use synthphone_e_vocal_dsp::{
    process_vocal_effects_1024_stream, MusicalSettings, VocalEffectsConfig,
};

/// Keep the existing firmware configuration in one place for offline replay.
pub fn default_config() -> VocalEffectsConfig {
    VocalEffectsConfig {
        // Preserve the stateless path's 99% steady-state correction.
        pitch_correction_strength: 0.99,
        // Keep the fresh default fast; lower menu settings now glide.
        transition_speed: 1.0,
        ..VocalEffectsConfig::default()
    }
}

/// Higher menu values correct faster. Coefficients are fractions per FFT hop.
/// At the device hop rate, level 1 has about 530 ms time constant; 10 snaps.
pub fn config_for_speed(speed: i8) -> VocalEffectsConfig {
    const SPEEDS: [f32; 10] = [0.01, 0.018, 0.032, 0.056, 0.1, 0.18, 0.32, 0.56, 0.8, 1.0];
    let mut config = default_config();
    config.transition_speed = SPEEDS[(speed.clamp(1, 10) - 1) as usize];
    config
}

#[allow(clippy::too_many_arguments)]
pub fn process_frame(
    input: &mut [f32; 1024],
    carrier: &mut [f32; 1024],
    input_phases: &mut [f32; 1024],
    output_phases: &mut [f32; 1024],
    harmony_envelope: &mut [f32; 512],
    harmony_inv_envelope: &mut [f32; 512],
    state: &mut VocalStreamState,
    config: &VocalEffectsConfig,
    settings: &MusicalSettings,
) -> [f32; 1024] {
    process_vocal_effects_1024_stream(
        input,
        Some(carrier),
        input_phases,
        output_phases,
        harmony_envelope,
        harmony_inv_envelope,
        state,
        config,
        settings,
    )
}
