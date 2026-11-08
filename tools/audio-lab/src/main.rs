use audio_lab::{
    cents, constants::SAMPLE_RATE, estimate_pitch, metrics, render, steady, tone, vocal_frame,
};
use std::{collections::BTreeSet, error::Error, fs, io::Write, path::Path};
use synthphone_e_vocal_dsp::{MusicalSettings, ProcessingMode};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn write_wav(path: &Path, audio: &[f32]) -> Result<()> {
    audio_lab::wav::write(path, audio, SAMPLE_RATE.round() as u32)
}

fn sweep(directory: &Path, strict: bool, speed: i8) -> Result<()> {
    fs::create_dir_all(directory)?;
    let mut csv = fs::File::create(directory.join("pitch.csv"))?;
    writeln!(csv, "key,input_hz,shape,formant,target_hz,output_hz,error_cents,rms,peak,dc,silent_hops,non_finite,raw_over_full_scale,host_p95_us,host_max_us,pass")?;
    let mut failures = 0;
    let known_failures: BTreeSet<&str> = include_str!("../known-pitch-failures.csv")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    let mut regressions = 0;
    let mut improvements = 0;
    let mut count = 0;
    let config = vocal_frame::config_for_speed(speed);
    for key in 0..24 {
        for base in [110.0, 146.8324, 220.0, 329.6276, 440.0] {
            for (shape, harmonics) in [
                ("sine", &[(1, 0.25)][..]),
                ("bright", &[(1, 0.06), (2, 0.18), (3, 0.08), (4, 0.04)][..]),
            ] {
                for formant in 0..=2 {
                    let hz = base * 2_f64.powf(35.0 / 1200.0);
                    let input = tone(hz, 0.65, harmonics);
                    let settings = MusicalSettings {
                        key: key as i8,
                        formant,
                        ..Default::default()
                    };
                    let output = render(&input, &[], &config, &settings);
                    let samples = steady(&output.audio);
                    let m = metrics(samples);
                    let raw = metrics(&output.raw);
                    let target = audio_lab::tuning::nearest_scale_hz(hz, key)
                        .expect("validated fixture pitch and key");
                    let pitch = estimate_pitch(samples);
                    let error = pitch.map(|p| cents(p, target));
                    let pass = error.is_some_and(|c| c.abs() <= 20.0)
                        && m.non_finite == 0
                        && raw.non_finite == 0
                        && m.rms > 0.001
                        && m.silent_hops == 0;
                    failures += usize::from(!pass);
                    let id = format!("{key},{hz:.6},{shape},{formant}");
                    regressions += usize::from(!pass && !known_failures.contains(id.as_str()));
                    improvements += usize::from(pass && known_failures.contains(id.as_str()));
                    count += 1;
                    let mut timings = output.hop_micros;
                    timings.sort_by(f64::total_cmp);
                    writeln!(csv, "{key},{hz:.6},{shape},{formant},{target:.6},{},{},{:.6},{:.6},{:.6},{},{},{},{:.3},{:.3},{pass}",
                        pitch.map(|p| format!("{p:.6}")).unwrap_or_default(),
                        error.map(|c| format!("{c:.3}")).unwrap_or_default(),
                        m.rms, m.peak, m.dc, m.silent_hops, m.non_finite, raw.over_full_scale,
                        timings[timings.len() * 95 / 100], timings.last().unwrap())?;
                    // Small, repeatable listening pack instead of hundreds of files.
                    if key == 0 && base == 220.0 {
                        if formant == 0 {
                            write_wav(&directory.join(format!("{shape}-input.wav")), &input)?;
                        }
                        write_wav(
                            &directory.join(format!("{shape}-formant-{formant}-output.wav")),
                            &output.audio,
                        )?;
                    }
                }
            }
        }
    }
    let control_input = tone(224.5, 0.65, &[(1, 0.25)]);
    let settings = MusicalSettings::default();
    let reference = render(&control_input, &[], &config, &settings);
    let mut controls = fs::File::create(directory.join("controls.csv"))?;
    writeln!(controls, "control,value,maximum_output_difference,responds")?;
    let mut unresponsive_controls = 0;
    for (name, value) in [
        (
            "transition_speed",
            if config.transition_speed == 1.0 {
                0.01
            } else {
                1.0
            },
        ),
        ("pitch_correction_strength", 0.0),
    ] {
        let mut changed = config;
        if name == "transition_speed" {
            changed.transition_speed = value;
        } else {
            changed.pitch_correction_strength = value;
        }
        let output = render(&control_input, &[], &changed, &settings);
        let difference = reference
            .raw
            .iter()
            .zip(output.raw)
            .map(|(a, b)| (*a - b).abs())
            .fold(0.0, f32::max);
        let responds = difference > 1e-5;
        unresponsive_controls += usize::from(!responds);
        writeln!(controls, "{name},{value},{difference},{responds}")?;
    }
    let summary = format!(
        "Speed preset {speed}; fixed 0.65s fixtures, 0.2s measurement warmup (slow settings may still be gliding).\n\
        {count} pitch cases; {} pass; {failures} fail (20-cent tolerance).\n\
        {regressions} new failures; {improvements} improvements against the recorded failure list.\n\
        {unresponsive_controls} of 2 correction controls produced no measurable response.\n\
        Any reported quality failure remains a failure; missing pitch also fails.\n\
        Host timings are relative benchmarks, not Daisy CPU timings.\n\
        Model: current firmware DSP pin, 1024-point FFT, 256-sample hop, 2-sample DMA block.\n\
        Scope: steady synthetic monophonic input; startup, consonants, expressive singing,\n\
        MIDI/UI interrupts, codec latency and physical noise require additional tests.\n",
        count - failures
    );
    fs::write(directory.join("summary.txt"), &summary)?;
    print!("{summary}");
    if regressions != 0 {
        return Err("new pitch regressions; see pitch.csv".into());
    }
    if strict && (failures != 0 || unresponsive_controls != 0) {
        return Err("quality acceptance failed; see pitch.csv and controls.csv".into());
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("levels-recording") if args.len() == 3 => {
            let input = audio_lab::wav::read(Path::new(&args[1]))?;
            audio_lab::recording_levels::run_at_rate(&input.samples, input.sample_rate, Path::new(&args[2]))
        },
        Some("levels") if args.len() == 2 => audio_lab::levels::run(Path::new(&args[1])),
        Some("emulate") if args.len() == 2 => audio_lab::emulator::run(Path::new(&args[1])),
        Some("sweep") if args.len() == 2 || (args.len() == 3 && args[2] == "--strict") => {
            sweep(Path::new(&args[1]), args.len() == 3, 10)
        }
        Some("sweep-speed") if args.len() == 3 => {
            let speed: i8 = args[2].parse()?;
            if !(1..=10).contains(&speed) { return Err("Speed must be 1..10".into()); }
            sweep(Path::new(&args[1]), false, speed)
        }
        Some("render") if (3..=5).contains(&args.len()) => {
            let mode = match args.get(3).map(String::as_str).unwrap_or("pitch") {
                "pitch" => ProcessingMode::PitchControl, "dry" => ProcessingMode::Dry,
                "harmony" => ProcessingMode::Harmony, "vocode" => ProcessingMode::Vocode,
                _ => return Err("mode must be pitch, dry, harmony or vocode".into()),
            };
            let key: i8 = args.get(4).map(|s| s.parse()).transpose()?.unwrap_or(0);
            if !(0..24).contains(&key) { return Err("key must be 0..23".into()); }
            let settings = MusicalSettings { key, mode, midi_frequencies: if mode == ProcessingMode::Harmony { [audio_lab::midi_notes::frequency(57), audio_lab::midi_notes::frequency(64), 0.0, 0.0, 0.0, 0.0, 0.0, 0.0] } else { [0.0; 8] }, ..Default::default() };
            let output = audio_lab::wav::render(Path::new(&args[1]), Path::new(&args[2]), &vocal_frame::default_config(), &settings)?;
            println!("Output: {:?}\nRaw before limiter: {:?}", metrics(&output.audio), metrics(&output.raw));
            Ok(())
        }
        _ => Err("Usage: audio-lab sweep OUTPUT_DIRECTORY [--strict]\n       audio-lab sweep-speed OUTPUT_DIRECTORY SPEED_1_TO_10\n       audio-lab levels OUTPUT_DIRECTORY\n       audio-lab levels-recording INPUT.wav OUTPUT_DIRECTORY\n       audio-lab emulate OUTPUT_DIRECTORY\n       audio-lab render INPUT.wav OUTPUT.wav [pitch|dry|harmony|vocode] [KEY_0_TO_23]".into()),
    }
}
