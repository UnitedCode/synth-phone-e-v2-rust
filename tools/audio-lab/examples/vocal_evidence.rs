//! Generate auditable A/B files from the unchanged production DSP.
use audio_lab::{metrics, vocal_evidence::*, wav};
use std::{error::Error, fs, path::Path};
use synthphone_e_vocal_dsp::ProcessingMode;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn write_pair(directory: &Path, name: &str, a: &[f32], b: &[f32]) -> Result<()> {
    // One shared gain per comparison. Never normalize A and B independently:
    // a missing level response must remain audible as identical loudness.
    if a.len() != b.len() || a.is_empty() || metrics(a).non_finite + metrics(b).non_finite > 0 {
        return Err("comparison requires equal-length finite audio".into());
    }
    let peak = metrics(a).peak.max(metrics(b).peak);
    let gain = (0.4 / peak.max(1e-9)).min(1.0);
    let a: Vec<_> = a.iter().map(|s| s * gain).collect();
    let b: Vec<_> = b.iter().map(|s| s * gain).collect();
    wav::write(&directory.join(format!("{name}-a.wav")), &a, RATE)?;
    wav::write(&directory.join(format!("{name}-b.wav")), &b, RATE)?;
    let mut ab = a;
    ab.resize(ab.len() + RATE as usize / 3, 0.0);
    ab.extend_from_slice(&b);
    wav::write(&directory.join(format!("{name}-ab.wav")), &ab, RATE)?;
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 1 {
        return Err("usage: vocal_evidence NEW_OUTPUT_DIRECTORY".into());
    }
    let directory = Path::new(&args[0]);
    fs::create_dir(directory)?;
    let mut report = String::from("case,measurement,value,criterion\n");
    for source in SOURCES {
        let input = vowel(source, SECONDS, 1.0);
        for target in TARGETS {
            let signal = carrier(&[target], SECONDS);
            let output = vocode(&input, &signal, vocoder_settings());
            let fraction = harmonic_fraction(middle(&output.raw), target);
            report.push_str(&format!(
                "vocode-{source}-{target},carrier_grid_energy_fraction,{fraction:.9},>0.9\n"
            ));
        }
    }
    let input = vowel(200.0, SECONDS, 1.0);
    let signal = carrier(&[440.0], SECONDS);
    let output = vocode(&input, &signal, vocoder_settings());
    // A known source/filter fixture at the target pitch is a listening reference,
    // not output from a repaired vocoder and not a bit-exact timbre oracle.
    let reference = vowel(440.0, SECONDS, 1.0);
    let mut delayed_reference = vec![0.0; 1025];
    delayed_reference.extend_from_slice(&reference);
    delayed_reference.resize(output.raw.len(), 0.0);
    write_pair(directory, "vocoder-grid", &output.raw, &delayed_reference)?;
    wav::write(&directory.join("synthetic-vowel-input.wav"), &input, RATE)?;
    wav::write(&directory.join("carrier-440hz.wav"), &signal, RATE)?;
    for (name, pair) in [
        ("unvoiced", unvoiced_pair()),
        ("vocoder-formant", formant_pair(ProcessingMode::Vocode)),
        ("vocoder-backing", backing_pair(ProcessingMode::Vocode)),
        (
            "pitch-formant-positive-control",
            formant_pair(ProcessingMode::PitchControl),
        ),
        (
            "harmony-backing-positive-control",
            backing_pair(ProcessingMode::Harmony),
        ),
    ] {
        let difference = relative_difference(middle(&pair.a), middle(&pair.b));
        report.push_str(&format!(
            "{name},relative_rms_difference,{difference:.9},{}\n",
            if name == "unvoiced" { "<0.01" } else { ">0.05" }
        ));
        write_pair(directory, name, &pair.a, &pair.b)?;
    }
    let quiet: Vec<_> = signal.iter().map(|s| s * 0.5).collect();
    let attenuated = vocode(&input, &quiet, vocoder_settings());
    let db =
        20.0 * (metrics(middle(&attenuated.raw)).rms / metrics(middle(&output.raw)).rms).log10();
    report.push_str(&format!(
        "carrier-level,output_change_db_for_minus_6db_carrier,{db:.9},diagnostic_only\n"
    ));
    fs::write(directory.join("measurements.csv"), &report)?;
    fs::write(directory.join("provenance.txt"), format!(
        "Synthetic vowel/noise fixtures; host replay of production DSP.\nsample_rate_hz={RATE}\nfixture_seconds={SECONDS}\nDSP dependency: {}\nA/Bs share one gain (never independently normalized).\nVocoder grid B is a known-pitch synthetic reference, not a proposed fix.\nNo hardware, real-vocal intelligibility or listening acceptance is claimed.\n",
        include_str!("../../../Cargo.toml").lines().find(|s| s.starts_with("synthphone-e-vocal-dsp =")).ok_or("missing firmware DSP pin")?
    ))?;
    println!("{report}");
    Ok(())
}
