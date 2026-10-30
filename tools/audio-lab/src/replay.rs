//! Allocation-free vocal stream model, shared by host tests and the ARM guest.
use crate::{constants::*, vocal_frame};
use synthphone_e_vocal_dsp::{ring_buffer::RingBuffer, MusicalSettings, VocalEffectsConfig};

pub struct Replay {
    input: RingBuffer<BUFFER_SIZE>,
    output: RingBuffer<BUFFER_SIZE>,
    carrier: RingBuffer<BUFFER_SIZE>,
    input_phases: [f32; FFT_SIZE],
    output_phases: [f32; FFT_SIZE],
    envelope: [f32; FFT_SIZE / 2],
    inv_envelope: [f32; FFT_SIZE / 2],
    counter: usize,
    pending: Option<u32>,
    pub hops: usize,
}

impl Default for Replay {
    fn default() -> Self {
        Self::new()
    }
}

impl Replay {
    pub const fn new() -> Self {
        Self {
            input: RingBuffer::new(),
            output: RingBuffer::new(),
            carrier: RingBuffer::new(),
            input_phases: [0.0; FFT_SIZE],
            output_phases: [0.0; FFT_SIZE],
            envelope: [1.0; FFT_SIZE / 2],
            inv_envelope: [1.0; FFT_SIZE / 2],
            counter: 0,
            pending: None,
            hops: 0,
        }
    }

    /// Clear in place without placing another 60 KB state object on the ARM stack.
    pub fn reset(&mut self) {
        for _ in 0..BUFFER_SIZE {
            self.input.push(0.0);
            self.carrier.push(0.0);
            self.output.pop();
        }
        self.input_phases.fill(0.0);
        self.output_phases.fill(0.0);
        self.envelope.fill(1.0);
        self.inv_envelope.fill(1.0);
        self.counter = 0;
        self.pending = None;
        self.hops = 0;
    }

    pub fn sample(&mut self, input: f32) -> f32 {
        self.input.push(input);
        let output = self.output.pop();
        // Match the production handler, including the first 257-sample hop.
        if self.counter >= HOP_SIZE {
            self.counter = 0;
            self.pending = Some(self.input.write_index());
        }
        self.counter += 1;
        output
    }

    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Run the pending lower-priority FFT task after a complete DMA block.
    pub fn finish_block(
        &mut self,
        carrier: &[f32; HOP_SIZE],
        config: &VocalEffectsConfig,
        settings: &MusicalSettings,
        omit_hop: Option<usize>,
    ) {
        let Some(write_index) = self.pending.take() else {
            return;
        };
        for &sample in carrier {
            self.carrier.push(sample);
        }
        let mut frame = [0.0; FFT_SIZE];
        let mut carrier_frame = [0.0; FFT_SIZE];
        self.input.block_from(write_index, &mut frame);
        self.carrier
            .block_from(self.carrier.write_index(), &mut carrier_frame);
        let synthesis = vocal_frame::process_frame(
            &mut frame,
            &mut carrier_frame,
            &mut self.input_phases,
            &mut self.output_phases,
            &mut self.envelope,
            &mut self.inv_envelope,
            1.0,
            config,
            settings,
        );
        if omit_hop != Some(self.hops) {
            self.output.write_overlapped_samples(&synthesis);
        }
        self.hops += 1;
    }
}
