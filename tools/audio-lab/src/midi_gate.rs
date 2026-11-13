//! Audition-only output gating; never extends audio into a MIDI rest.

pub fn apply(audio: &mut [f32], active: &[bool], rate: u32) -> Result<(), &'static str> {
    if audio.len() != active.len() || rate == 0 {
        return Err("MIDI gate requires equal lengths and a positive sample rate");
    }
    let fade = (rate as usize / 500).max(1); // Two ms, entirely inside held notes.
    let mut cursor = 0;
    while cursor < audio.len() {
        if !active[cursor] {
            audio[cursor] = 0.0;
            cursor += 1;
            continue;
        }
        let start = cursor;
        while cursor < active.len() && active[cursor] {
            cursor += 1;
        }
        let length = cursor - start;
        for (i, sample) in audio[start..cursor].iter_mut().enumerate() {
            let edge = (i + 1).min(length - i);
            *sample *= (edge as f32 / fade as f32).min(1.0);
        }
    }
    Ok(())
}
