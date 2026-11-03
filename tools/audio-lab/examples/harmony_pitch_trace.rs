//! Compare legacy and Harmony pitch analysis with known oscillators or a vowel.
use audio_lab::{constants::*, vocal_frame};
use synthphone_e_vocal_dsp::dsp::{frequency_analysis, Fft1024, FftOps};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = vocal_frame::default_config();
    config.sample_rate = SAMPLE_RATE;
    if let Some(path) = std::env::args().nth(1) {
        let reader = hound::WavReader::open(path)?;
        if reader.spec().channels != 1
            || reader.spec().sample_rate != SAMPLE_RATE as u32
            || reader.spec().sample_format != hound::SampleFormat::Float
        {
            return Err("need mono 48014 Hz float WAV".into());
        }
        let input = reader
            .into_samples::<f32>()
            .collect::<Result<Vec<_>, _>>()?;
        if input.len() < 2048 || input.iter().any(|s| !s.is_finite()) {
            return Err("need at least 2048 finite samples".into());
        }
        println!("seconds,coarse_hz,resolved_hz,independent_hz,odd_to_even_energy");
        let mut previous = 0.0;
        let mut unvoiced = 0;
        for start in (0..input.len()).step_by(HOP_SIZE) {
            let history: Vec<_> = (start..start + 8192)
                .map(|i| input[i % input.len()])
                .collect();
            let coarse = frequency_analysis::find_pitch_harmony(&history[..2048], &config);
            let mut frame: [f32; 1024] = history[1024..2048].try_into().unwrap();
            for (sample, &window) in frame.iter_mut().zip(Fft1024::get_hann_window()) {
                *sample *= window;
            }
            let fft = Fft1024::forward_fft(&mut frame);
            let magnitudes: [f32; 512] =
                core::array::from_fn(|i| (fft[i].re * fft[i].re + fft[i].im * fft[i].im).sqrt());
            let resolved = frequency_analysis::resolve_harmony_octave_1024(
                coarse,
                &magnitudes,
                config.sample_rate,
                previous,
            );
            if resolved > 0.0 {
                previous = resolved;
                unvoiced = 0;
            } else {
                unvoiced += 1;
                if unvoiced >= 8 {
                    previous = 0.0;
                }
            }
            let (mut odd, mut even) = (0.0, 0.0);
            for h in 1..=8 {
                let bin = (coarse * h as f32 * 1024.0 / config.sample_rate).round() as usize;
                if bin >= 512 {
                    break;
                }
                if h % 2 == 0 {
                    even += magnitudes[bin].powi(2)
                } else {
                    odd += magnitudes[bin].powi(2)
                }
            }
            println!(
                "{},{coarse},{resolved},{},{}",
                start as f64 / SAMPLE_RATE as f64,
                audio_lab::estimate_pitch(&history).unwrap_or(0.0),
                odd / even
            );
        }
        return Ok(());
    }
    println!("source,seconds,legacy_yin,harmony_yin,refined,peak,harmonic,known_mean_hz");
    for base in [130.0, 200.0, 300.0] {
        let mut phase = 0.0_f64;
        let instantaneous: Vec<_> = (0..(SAMPLE_RATE * 2.0) as usize)
            .map(|i| {
                base * 2_f64.powf(
                    50.0 * (std::f64::consts::TAU * 5.0 * i as f64 / SAMPLE_RATE as f64).sin()
                        / 1200.0,
                )
            })
            .collect();
        let input: Vec<_> = instantaneous
            .iter()
            .map(|&hz| {
                phase += std::f64::consts::TAU * hz / SAMPLE_RATE as f64;
                (1..=10)
                    .map(|h| 0.15 / h as f64 * (phase * h as f64).sin())
                    .sum::<f64>() as f32
            })
            .collect();
        let mut phases = [0.0; FFT_SIZE];
        for start in (0..input.len() - FFT_SIZE).step_by(HOP_SIZE) {
            let mut frame: [f32; FFT_SIZE] = input[start..start + FFT_SIZE].try_into().unwrap();
            let legacy = frequency_analysis::find_pitch_yin(&frame, &config);
            let yin = frequency_analysis::find_pitch_harmony(&frame, &config);
            for (sample, &window) in frame.iter_mut().zip(Fft1024::get_hann_window()) {
                *sample *= window;
            }
            let fft = Fft1024::forward_fft(&mut frame);
            let (mut magnitudes, mut frequencies) = ([0.0; FFT_SIZE / 2], [0.0; FFT_SIZE / 2]);
            frequency_analysis::perform_phase_vocoder_analysis::<1024, 512>(
                fft,
                &mut phases,
                &mut magnitudes,
                &mut frequencies,
                HOP_SIZE,
            );
            let peak = (1..FFT_SIZE / 2)
                .max_by(|&a, &b| magnitudes[a].total_cmp(&magnitudes[b]))
                .unwrap();
            let measured = frequencies[peak] * SAMPLE_RATE / FFT_SIZE as f32;
            let harmonic = (measured / yin).round().max(1.0);
            let candidate = measured / harmonic;
            let refined = if (candidate / yin - 1.0).abs() < 0.03 {
                candidate
            } else {
                yin
            };
            let centre = start + FFT_SIZE / 2;
            let mean =
                instantaneous[centre - HOP_SIZE..centre].iter().sum::<f64>() / HOP_SIZE as f64;
            println!(
                "{base},{},{legacy},{yin},{refined},{peak},{harmonic},{mean}",
                start as f64 / SAMPLE_RATE as f64
            );
        }
    }
    Ok(())
}
