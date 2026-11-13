//! Independent, desktop-only WORLD reference; never linked into the phone.
use audio_lab::{midi_gate, wav};
use std::{error::Error, fs, path::Path};
use world_rs::{cheaptrick, d4c, dio, stonemask, synthesis};
mod psola;
type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Clone)]
struct Targets {
    time: f64,
    frequencies: [f64; 8],
    gains: [f64; 8],
}

fn targets(text: &str) -> Result<Vec<Targets>> {
    let mut frames = Vec::new();
    for row in text.lines().skip(1) {
        let values = row
            .split(',')
            .map(str::parse::<f64>)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if values.len() != 17 || values.iter().any(|v| !v.is_finite() || *v < 0.0) {
            return Err("invalid target trace".into());
        }
        if frames
            .last()
            .is_some_and(|last: &Targets| last.time >= values[0])
        {
            return Err("target timestamps must increase".into());
        }
        frames.push(Targets {
            time: values[0],
            frequencies: core::array::from_fn(|i| values[1 + 2 * i]),
            gains: core::array::from_fn(|i| values[2 + 2 * i]),
        });
    }
    if frames.is_empty() {
        return Err("empty target trace".into());
    }
    Ok(frames)
}

fn at(frames: &[Targets], time: f64) -> Option<&Targets> {
    frames
        .partition_point(|frame| frame.time <= time)
        .checked_sub(1)
        .map(|i| &frames[i])
}

struct Model {
    times: Vec<f64>,
    f0: Vec<f64>,
    spectrum: Vec<Vec<f64>>,
    noise: Vec<Vec<f64>>,
    fft: usize,
    rate: f64,
    length: usize,
}

fn analyze(input: &[f32], rate: u32) -> Result<Model> {
    let signal: Vec<_> = input.iter().map(|&x| f64::from(x)).collect();
    let rate = f64::from(rate);
    let mut option = dio::initialize_dio_option();
    option.f0_floor = 50.0;
    option.f0_ceil = 600.0;
    let contour = dio::dio(&signal, rate, &option).map_err(|e| format!("DIO: {e:?}"))?;
    let f0 = stonemask::stone_mask(
        &signal,
        signal.len(),
        rate,
        &contour.temporal_positions,
        &contour.f0,
        contour.f0_length,
    )
    .map_err(|e| format!("StoneMask: {e:?}"))?;
    let mut option = cheaptrick::initialize_cheaptrick_option(rate);
    option.f0_floor = 50.0;
    option.fft_size = cheaptrick::get_fft_size_for_cheaptrick(rate, &option);
    let spectrum = cheaptrick::cheaptrick(&signal, rate, &contour.temporal_positions, &f0, &option)
        .map_err(|e| format!("CheapTrick: {e:?}"))?;
    let noise = d4c::d4c(
        &signal,
        rate,
        &contour.temporal_positions,
        &f0,
        option.fft_size,
        &d4c::initialize_d4c_option(),
    )
    .map_err(|e| format!("D4C: {e:?}"))?;
    Ok(Model {
        times: contour.temporal_positions,
        f0,
        spectrum,
        noise,
        fft: option.fft_size as usize,
        rate,
        length: input.len(),
    })
}

impl Model {
    fn synthesize(&self, f0: &[f64]) -> Result<Vec<f32>> {
        let output = synthesis::synthesis(
            f0,
            f0.len(),
            &self.spectrum,
            &self.noise,
            self.fft,
            5.0,
            self.rate,
            self.length,
        )
        .map_err(|e| format!("Synthesis: {e:?}"))?;
        if output
            .iter()
            .any(|x| !x.is_finite() || x.abs() > f64::from(f32::MAX))
        {
            return Err("invalid reference output".into());
        }
        Ok(output.into_iter().map(|x| x as f32).collect())
    }
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !(6..=7).contains(&args.len()) || args.get(6).is_some_and(|flag| flag != "--psola") {
        return Err("usage: harmony-reference INPUT_MONO.wav TARGETS.csv MIDI_GATE.csv START_SECONDS DURATION_SECONDS NEW_OUTPUT_DIR [--psola]".into());
    }
    let waveform = args.len() == 7;
    let source = wav::read(Path::new(&args[0]))?;
    let frames = targets(&fs::read_to_string(&args[1])?)?;
    let start: f64 = args[3].parse()?;
    let duration: f64 = args[4].parse()?;
    let rate = source.sample_rate;
    if !start.is_finite()
        || !duration.is_finite()
        || start < 0.0
        || duration <= 0.0
        || start + duration > source.samples.len() as f64 / f64::from(rate)
    {
        return Err("invalid window".into());
    }
    let offset = (start * f64::from(rate)).round() as usize;
    let length = (duration * f64::from(rate)).round() as usize;
    let input = source
        .samples
        .get(offset..offset + length)
        .ok_or("source shorter than window")?;
    if frames.last().unwrap().time < start + duration {
        return Err("target trace shorter than window".into());
    }
    let mut gate = vec![false; length];
    for row in fs::read_to_string(&args[2])?.lines().skip(1) {
        let columns: Vec<_> = row.split(',').collect();
        if columns.len() != 3 {
            return Err("invalid MIDI gate row".into());
        }
        let begin: usize = columns[0].parse()?;
        let end: usize = columns[1].parse()?;
        if begin > end || !["0", "1"].contains(&columns[2]) {
            return Err("invalid MIDI gate interval".into());
        }
        if columns[2] == "1" {
            let begin = begin.saturating_sub(offset).min(length);
            let end = end.saturating_sub(offset).min(length);
            gate[begin..end].fill(true);
        }
    }
    let out = Path::new(&args[5]);
    fs::create_dir(out)?;
    eprintln!("analyzing {duration}s at {rate} Hz with WORLD");
    let model = analyze(input, rate)?;
    let reconstructed = model.synthesize(&model.f0)?;
    wav::write(&out.join("source-reconstructed.wav"), &reconstructed, rate)?;
    let mut mix = vec![0.0_f32; length];
    for voice in 0..8 {
        let f0: Vec<_> = model
            .times
            .iter()
            .zip(&model.f0)
            .map(|(&time, &pitch)| {
                if pitch > 0.0 || waveform {
                    at(&frames, start + time).map_or(0.0, |t| t.frequencies[voice])
                } else {
                    0.0
                }
            })
            .collect();
        if f0.iter().all(|&f| f == 0.0) {
            continue;
        }
        let output = if waveform {
            psola::shift(input, rate, &model.f0, &f0, 5.0)
        } else {
            model.synthesize(&f0)?
        };
        for (i, (&sample, sum)) in output.iter().zip(&mut mix).enumerate() {
            if let Some(frame) = at(&frames, start + i as f64 / f64::from(rate)) {
                let count = frame.frequencies.iter().filter(|&&f| f > 0.0).count();
                if frame.frequencies[voice] > 0.0 && count > 0 {
                    *sum += sample * (frame.gains[voice] / count as f64) as f32;
                }
            }
        }
        eprintln!("synthesized voice {}", voice + 1);
    }
    midi_gate::apply(&mut mix, &gate, rate)?;
    if mix.iter().zip(&gate).any(|(&x, &on)| !on && x != 0.0) {
        return Err("reference leaked into MIDI rest".into());
    }
    wav::write(&out.join("same-midi-chords.wav"), &mix, rate)?;
    wav::write(&out.join("input.wav"), input, rate)?;
    let mut trace = String::from("song_seconds,world_f0_hz\n");
    for (&time, &pitch) in model.times.iter().zip(&model.f0) {
        trace.push_str(&format!("{},{pitch}\n", start + time));
    }
    fs::write(out.join("world-f0.csv"), trace)?;
    fs::write(out.join("provenance.txt"), format!("world-rs=0.1.0\nwaveform_psola={waveform}\ninput={}\ntargets={}\ngate={}\nstart={start}\nduration={duration}\nrate={rate}\nDIO floor=50 ceiling=600 Hz followed by StoneMask\nCheapTrick floor=50 Hz fft={}\nD4C defaults; frame period=5ms\nOriginal-pitch WORLD resynthesis is an analysis control. MIDI output uses the exported targets; --psola substitutes waveform epoch overlap-add for WORLD synthesis, retaining unvoiced input through a 10ms routing fade.\nOffline desktop reference with independent pitch analysis, not phone firmware or a listening-quality verdict. No source loops, splices, resampling or voiced-only muting.\n", args[0], args[1], args[2], model.fft))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_engine_retargets_a_known_periodic_voice() -> Result<()> {
        let rate = 16000;
        let input: Vec<_> = (0..rate)
            .map(|i| {
                let p = std::f64::consts::TAU * 100.0 * f64::from(i) / f64::from(rate);
                (0.1 * p.sin() + 0.05 * (2.0 * p).sin() + 0.02 * (3.0 * p).sin()) as f32
            })
            .collect();
        let model = analyze(&input, rate)?;
        eprintln!(
            "source frames={} voiced={} spectrum_peak={} mean_ap={} output_fft={}",
            model.f0.len(),
            model.f0.iter().filter(|&&f| f > 0.0).count(),
            model.spectrum[100].iter().copied().fold(0.0_f64, f64::max),
            model.noise[100].iter().sum::<f64>() / model.noise[100].len() as f64,
            model.fft
        );
        let pitch: Vec<_> = model
            .f0
            .iter()
            .map(|&f| if f > 0.0 { 220.0 } else { 0.0 })
            .collect();
        let output = model.synthesize(&pitch)?;
        // Retained noise can make autocorrelation choose a subharmonic.
        // Measure the requested line and reject source/subharmonic alternatives.
        let component = |hz: f64| {
            let (mut re, mut im) = (0.0, 0.0);
            for (i, &sample) in output[4000..12000].iter().enumerate() {
                let p = std::f64::consts::TAU * hz * i as f64 / f64::from(rate);
                re += f64::from(sample) * p.cos();
                im += f64::from(sample) * p.sin();
            }
            2.0 * re.hypot(im) / 8000.0
        };
        let target = component(220.0);
        eprintln!(
            "target={target}, source={}, subharmonic={}",
            component(100.0),
            component(110.0)
        );
        assert!(target > 0.01);
        assert!(target > component(100.0) * 5.0);
        assert!(target > component(110.0) * 5.0);
        assert_eq!(output.len(), input.len());
        Ok(())
    }
}
