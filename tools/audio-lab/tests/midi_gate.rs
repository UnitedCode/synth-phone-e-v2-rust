use audio_lab::{constants::*, midi_gate, replay::Replay, tone, vocal_frame};
use synthphone_e_vocal_dsp::{MusicalSettings, ProcessingMode};

#[test]
fn midi_rests_are_silent_even_when_overlap_add_has_an_audible_tail() {
    let input = tone(200.0, 0.65, &[(1, 0.15)]);
    let held = |i: usize| {
        let seconds = i as f32 / SAMPLE_RATE;
        (0.1..0.3).contains(&seconds) || (0.4..0.5).contains(&seconds)
    };
    let mut config = vocal_frame::default_config();
    config.harmony_lead_gain = 0.0;
    let mut settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    let mut replay = Replay::new();
    let (mut audio, mut active) = (Vec::new(), Vec::new());
    for block in (0..input.len()).step_by(BLOCK_SIZE) {
        for (i, &sample) in input.iter().enumerate().skip(block).take(BLOCK_SIZE) {
            audio.push(replay.sample(sample));
            active.push(held(i));
        }
        settings.midi_frequencies[0] = if held(block) { 220.0 } else { 0.0 };
        replay.finish_block(&[0.0; HOP_SIZE], &config, &settings, None);
    }
    // This negative control demonstrates why just releasing the MIDI note
    // doesn't establish sample-exact silence at the output of the stream.
    assert!(audio
        .iter()
        .zip(&active)
        .any(|(&x, &on)| !on && x.abs() > 1e-4));
    midi_gate::apply(&mut audio, &active, SAMPLE_RATE as u32).unwrap();
    assert!(audio.iter().zip(&active).all(|(&x, &on)| on || x == 0.0));
    assert!(audio
        .iter()
        .zip(&active)
        .any(|(&x, &on)| on && x.abs() > 0.02));
}
