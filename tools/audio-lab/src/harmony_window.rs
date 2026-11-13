//! Host-only longer-window trial. Production DMA blocks and the 256-sample
//! cadence still run through Replay; a private OLA holds the longer frame.
use crate::constants::{FFT_SIZE, HOP_SIZE};
use synthphone_e_vocal_dsp::{
    dsp::Fft2048,
    effects::harmony_stream::{process_with_fft, HarmonyState},
    MusicalSettings, VocalEffectsConfig,
};

pub const WINDOW: usize = 2048;
pub const EXTRA_DELAY: usize = WINDOW - FFT_SIZE;

pub struct LongHarmony {
    history: [f32; WINDOW],
    phases: [f32; WINDOW],
    envelope: [f32; WINDOW / 2],
    inverse: [f32; WINDOW / 2],
    overlap: [f32; WINDOW],
    state: HarmonyState,
}

impl Default for LongHarmony {
    fn default() -> Self {
        Self {
            history: [0.0; WINDOW],
            phases: [0.0; WINDOW],
            envelope: [1.0; WINDOW / 2],
            inverse: [1.0; WINDOW / 2],
            overlap: [0.0; WINDOW],
            state: HarmonyState::new(),
        }
    }
}

impl LongHarmony {
    pub fn process(
        &mut self,
        input: &[f32; FFT_SIZE],
        config: &VocalEffectsConfig,
        settings: &MusicalSettings,
    ) -> [f32; FFT_SIZE] {
        self.history.copy_within(HOP_SIZE.., 0);
        self.history[WINDOW - HOP_SIZE..].copy_from_slice(&input[FFT_SIZE - HOP_SIZE..]);
        let mut frame = self.history;
        let mut config = *config;
        config.fft_size = WINDOW;
        config.hop_size = HOP_SIZE;
        config.hop_ratio = HOP_SIZE as f32 / WINDOW as f32;
        let synthesis = process_with_fft::<WINDOW, { WINDOW / 2 }, Fft2048>(
            &mut frame,
            &mut self.phases,
            &mut self.envelope,
            &mut self.inverse,
            &mut self.state,
            &config,
            settings,
        );
        self.overlap.copy_within(HOP_SIZE.., 0);
        self.overlap[WINDOW - HOP_SIZE..].fill(0.0);
        for (sum, sample) in self.overlap.iter_mut().zip(synthesis) {
            *sum += sample;
        }
        let mut output = [0.0; FFT_SIZE];
        // Replay applies the device's 2/3 gain. Eight overlapping Hann-squared
        // windows need another factor of 1/2 versus the device's four windows.
        for (sample, &sum) in output[..HOP_SIZE].iter_mut().zip(&self.overlap) {
            *sample = sum * (FFT_SIZE as f32 / WINDOW as f32);
        }
        output
    }
}

pub fn render(input: &[f32], config: &VocalEffectsConfig, settings: &MusicalSettings) -> Vec<f32> {
    let mut replay = crate::replay::Replay::new();
    let mut harmony = LongHarmony::default();
    let total = (input.len() + WINDOW * 2).div_ceil(crate::constants::BLOCK_SIZE)
        * crate::constants::BLOCK_SIZE;
    let mut output = Vec::with_capacity(total);
    for block in (0..total).step_by(crate::constants::BLOCK_SIZE) {
        for i in block..block + crate::constants::BLOCK_SIZE {
            output.push(replay.sample(input.get(i).copied().unwrap_or(0.0)));
        }
        replay.finish_block_with_kernel(&[0.0; HOP_SIZE], None, |input, _, _, _, _, _, _| {
            harmony.process(input, config, settings)
        });
    }
    output
}
