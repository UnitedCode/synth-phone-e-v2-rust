//! Pitch-synchronous waveform overlap-add, for desktop comparison only.
fn value(curve: &[f64], position: f64, step: f64, interpolate: bool) -> f64 {
    let index = (position / step) as usize;
    let a = curve[index.min(curve.len() - 1)];
    let b = curve[(index + 1).min(curve.len() - 1)];
    if interpolate && a > 0.0 && b > 0.0 {
        a + (b - a) * (position / step - index as f64)
    } else {
        a
    }
}

struct Mark {
    position: usize,
    period: f64,
}

fn epochs(input: &[f32], rate: f64, source: &[f64], step: f64) -> Vec<Mark> {
    let mut marks = Vec::new();
    let mut cursor = 0;
    while cursor < input.len() {
        let pitch = value(source, cursor as f64, step, true);
        if pitch <= 0.0 {
            cursor += step.max(1.0) as usize;
            continue;
        }
        let period = (rate / pitch).clamp(16.0, rate / 40.0);
        let end = (cursor + period.ceil() as usize).min(input.len());
        let mut previous = (cursor..end)
            .max_by(|&a, &b| input[a].abs().total_cmp(&input[b].abs()))
            .unwrap();
        loop {
            let pitch = value(source, previous as f64, step, true);
            if pitch <= 0.0 {
                cursor = previous + step.max(1.0) as usize;
                break;
            }
            let period = (rate / pitch).clamp(16.0, rate / 40.0);
            marks.push(Mark {
                position: previous,
                period,
            });
            let prediction = previous as f64 + period;
            if prediction + period * 0.2 >= input.len() as f64 {
                cursor = input.len();
                break;
            }
            if value(source, prediction, step, true) <= 0.0 {
                cursor = prediction as usize;
                break;
            }
            let radius = (period * 0.2) as usize;
            let half = (period * 0.35) as isize;
            let centre = prediction.round() as usize;
            let mut best = centre;
            let mut best_score = f64::NEG_INFINITY;
            for candidate in centre - radius..=centre + radius {
                let (mut dot, mut power) = (0.0, 0.0);
                for offset in (-half..=half).step_by(2) {
                    let a = previous as isize + offset;
                    let b = candidate as isize + offset;
                    if a < 0 || b < 0 || a as usize >= input.len() || b as usize >= input.len() {
                        continue;
                    }
                    let (a, b) = (f64::from(input[a as usize]), f64::from(input[b as usize]));
                    dot += a * b;
                    power += b * b;
                }
                // The previous window's energy is constant across candidates.
                // Keep polarity: a negative correlation is not another epoch.
                let score = if dot > 0.0 {
                    dot * dot / power.max(1e-20)
                } else {
                    -1.0
                };
                if score > best_score {
                    best_score = score;
                    best = candidate;
                }
            }
            previous = best;
        }
    }
    marks
}

pub fn shift(input: &[f32], rate: u32, source: &[f64], target: &[f64], frame_ms: f64) -> Vec<f32> {
    assert!(!source.is_empty() && !target.is_empty() && rate > 0 && frame_ms > 0.0);
    let rate = f64::from(rate);
    let step = rate * frame_ms / 1000.0;
    let marks = epochs(input, rate, source, step);
    let mut output = vec![0.0_f32; input.len()];
    let mut position = 0.0;
    while position < input.len() as f64 {
        let target_pitch = value(target, position, step, false);
        if target_pitch <= 0.0 {
            position += step.max(1.0);
            continue;
        }
        let target_period = rate / target_pitch;
        if value(source, position, step, true) > 0.0 && !marks.is_empty() {
            let index = marks.partition_point(|mark| (mark.position as f64) < position);
            let a = index.saturating_sub(1);
            let b = index.min(marks.len() - 1);
            let mark = if (marks[a].position as f64 - position).abs()
                < (marks[b].position as f64 - position).abs()
            {
                &marks[a]
            } else {
                &marks[b]
            };
            let half = mark.period.ceil() as isize;
            let gain = target_period / mark.period;
            for offset in -half..=half {
                let source_position = mark.position as isize + offset;
                let destination = position + offset as f64;
                if source_position < 0
                    || source_position as usize >= input.len()
                    || destination < 0.0
                {
                    continue;
                }
                let bin = destination as usize;
                if bin + 1 >= output.len() {
                    continue;
                }
                let x = offset as f64 / mark.period;
                if x.abs() > 1.0 {
                    continue;
                }
                let window = 0.5 + 0.5 * (std::f64::consts::PI * x).cos();
                let sample = (f64::from(input[source_position as usize]) * window * gain) as f32;
                let fraction = (destination - bin as f64) as f32;
                output[bin] += sample * (1.0 - fraction);
                output[bin + 1] += sample * fraction;
            }
        }
        position += target_period.max(1.0);
    }
    // Retain unvoiced waveform rather than deleting consonants. Smooth the
    // route change; this desktop trial never inserts a voiced-only audio gate.
    let slew = (1.0 - (-1.0 / (rate * 0.01)).exp()) as f32;
    let dc_slew = (1.0 - (-std::f64::consts::TAU * 30.0 / rate).exp()) as f32;
    let (mut voiced, mut dc) = (0.0, 0.0);
    for (i, sample) in output.iter_mut().enumerate() {
        dc += (*sample - dc) * dc_slew;
        let wanted = if value(source, i as f64, step, true) > 0.0 {
            1.0
        } else {
            0.0
        };
        voiced += (wanted - voiced) * slew;
        *sample = if value(target, i as f64, step, false) > 0.0 {
            (*sample - dc) * voiced + input[i] * (1.0 - voiced)
        } else {
            0.0
        };
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_held_note_removes_source_vibrato_without_pitch_wobble() {
        let rate = 44_100;
        let seconds = 1.4;
        let mut phase = 0.0_f64;
        let hz = |time: f64| {
            220.0 * 2_f64.powf(60.0 * (std::f64::consts::TAU * 5.0 * time).sin() / 1200.0)
        };
        let input: Vec<_> = (0..(f64::from(rate) * seconds) as usize)
            .map(|i| {
                phase += std::f64::consts::TAU * hz(i as f64 / f64::from(rate)) / f64::from(rate);
                (0.15 * phase.sin() + 0.05 * (2.0 * phase).sin() + 0.03 * (3.0 * phase).sin())
                    as f32
            })
            .collect();
        let source: Vec<_> = (0..=280).map(|i| hz(i as f64 * 0.005)).collect();
        for hz in [174.614, 350.0] {
            let target = vec![hz; source.len()];
            let output = shift(&input, rate, &source, &target, 5.0);
            let mut worst = 0.0_f64;
            for start in (13_230..44_100).step_by(2646) {
                let pitch = audio_lab::estimate_pitch_at_rate(
                    &output[start..start + 5292],
                    f64::from(rate),
                )
                .expect("held note must remain voiced");
                worst = worst.max(audio_lab::cents(pitch, hz).abs());
            }
            eprintln!("waveform trial {hz} Hz worst target error: {worst:.3} cents");
            assert!(
                worst < 5.0,
                "waveform trial pitch wobbles by {worst:.2} cents"
            );
            assert_eq!(output.len(), input.len());
            assert!(output.iter().all(|x| x.is_finite()));
        }
    }

    #[test]
    fn unvoiced_audio_is_preserved_and_absent_midi_is_silent() {
        let input: Vec<_> = (0..1600)
            .map(|i| ((i * 7919 % 997) as f32 / 997.0 - 0.5) * 0.1)
            .collect();
        let unvoiced = vec![0.0; 21];
        assert_eq!(shift(&input, 16000, &unvoiced, &[220.0; 21], 5.0), input);
        assert!(shift(&input, 16000, &unvoiced, &unvoiced, 5.0)
            .iter()
            .all(|&x| x == 0.0));
    }
}
