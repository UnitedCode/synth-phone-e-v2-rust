//! Hardware-independent entry point shared by the firmware and audio lab.
use synthphone_e_vocal_dsp::{process_vocal_effects_1024, MusicalSettings, VocalEffectsConfig};

/// Keep the existing firmware configuration in one place for offline replay.
pub fn default_config() -> VocalEffectsConfig {
    VocalEffectsConfig::default()
}

#[allow(clippy::too_many_arguments)]
pub fn process_frame(
    input: &mut [f32; 1024],
    carrier: &mut [f32; 1024],
    input_phases: &mut [f32; 1024],
    output_phases: &mut [f32; 1024],
    harmony_envelope: &mut [f32; 512],
    harmony_inv_envelope: &mut [f32; 512],
    previous_ratio: f32,
    config: &VocalEffectsConfig,
    settings: &MusicalSettings,
) -> [f32; 1024] {
    process_vocal_effects_1024(
        input,
        Some(carrier),
        input_phases,
        output_phases,
        harmony_envelope,
        harmony_inv_envelope,
        previous_ratio,
        config,
        settings,
    )
}
