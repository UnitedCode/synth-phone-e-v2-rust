//! Independent audio gates plus explicit model limitations; no boot-only pass.
use super::{Result, RATE};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn artifact(prefix: &Path, suffix: &str) -> PathBuf {
    let mut path = prefix.as_os_str().to_os_string();
    path.push(suffix);
    path.into()
}

pub fn stat(prefix: &Path, key: &str) -> Result<u64> {
    let text = fs::read_to_string(artifact(prefix, ".wav.stats"))?;
    let marker = format!("{key}=");
    let mut matches = text.lines().filter_map(|line| line.strip_prefix(&marker));
    let value = matches
        .next()
        .ok_or_else(|| format!("Missing {key} counter"))?
        .parse()?;
    if matches.next().is_some() {
        return Err(format!("Duplicate {key} counter").into());
    }
    Ok(value)
}

pub fn boot_evidence(prefix: &Path) -> Result<()> {
    let host = fs::read_to_string(artifact(prefix, ".host.log"))?;
    if !host.contains("Python execution guard verified") || !host.contains("PC:") {
        return Err("Missing execution/guard evidence".into());
    }
    let rtt = fs::read_to_string(artifact(prefix, ".rtt"))?;
    if !rtt.contains("Startup done!!")
        || !rtt.contains("Startup complete, transitioning away from splash screen")
    {
        return Err(format!(
            "Firmware did not complete startup; see {}",
            artifact(prefix, ".rtt").display()
        )
        .into());
    }
    if ["Button pressed", "panicked", "ERROR", "HardFault"]
        .iter()
        .any(|bad| rtt.contains(bad))
    {
        return Err("Unexpected keypad event or firmware fault in idle boot".into());
    }
    let log = fs::read_to_string(artifact(prefix, ".log"))?;
    let unknown: Vec<_> = log
        .lines()
        .filter(|line| {
            (line.contains("[WARNING]") || line.contains("[ERROR]")) && !known_warning(line)
        })
        .collect();
    if !unknown.is_empty() {
        return Err(format!("Unexpected emulator warning/error:\n{}", unknown.join("\n")).into());
    }
    Ok(())
}

// Exact observed messages, without timestamp. These are intentional limitations
// of the functional clock/cache/bus model, documented in virtual-daisy.md.
fn known_warning(line: &str) -> bool {
    const ALLOWED: &str = include_str!("../known-warnings.txt");
    let Some((_, message)) = line.split_once("[WARNING] ") else {
        return false;
    };
    ALLOWED
        .lines()
        .any(|known| !known.is_empty() && message == known)
}

fn analyze(samples: &[f32]) -> (Vec<String>, String) {
    let metrics = audio_lab::metrics(samples);
    let pitch = audio_lab::estimate_pitch(samples);
    let cents = pitch.map(|hz| 1200.0 * (hz / 220.0).log2().abs());
    let mut failures = Vec::new();
    for (bad, label) in [
        (!(0.05..0.4).contains(&metrics.rms), "RMS outside 0.05..0.4"),
        (
            metrics.non_finite != 0 || metrics.over_full_scale != 0,
            "Invalid/clipped samples",
        ),
        (
            metrics.peak > 0.4 || metrics.dc.abs() > 0.02,
            "Peak/DC limit",
        ),
        (
            metrics.silent_hops != 0 || metrics.hop_rms_ratio < 0.7,
            "Sustained-tone dropout",
        ),
        (metrics.max_step > 0.05, "Discontinuous output"),
        (
            cents.is_none_or(|c| c > 15.0),
            "Autotune pitch exceeds 15 cents from A3",
        ),
    ] {
        if bad {
            failures.push(label.to_owned());
        }
    }
    (
        failures,
        format!(
            "RMS={:.6}, peak={:.6}, pitch={pitch:?} Hz, error={cents:?} cents, silent_hops={}",
            metrics.rms, metrics.peak, metrics.silent_hops
        ),
    )
}

fn evaluate(prefix: &Path) -> Result<(Vec<String>, String)> {
    let mut wav = hound::WavReader::open(artifact(prefix, ".wav"))?;
    let spec = wav.spec();
    if spec.channels != 2
        || spec.sample_rate != RATE
        || spec.sample_format != hound::SampleFormat::Float
        || spec.bits_per_sample != 32
    {
        return Err("Malformed virtual stereo capture format".into());
    }
    let audio = wav
        .samples::<f32>()
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if audio.len() % 2 != 0 {
        return Err("Truncated stereo capture".into());
    }
    let frames = stat(prefix, "frames")?;
    if audio.len() / 2 != frames as usize || !(46000..=48100).contains(&frames) {
        return Err(format!("Incomplete capture: {frames} frames").into());
    }
    let mut failures = Vec::new();
    for key in ["underruns", "overruns"] {
        if stat(prefix, key)? != 0 {
            failures.push(format!("SAI {key}"));
        }
    }
    let mut reports = Vec::new();
    for channel in 0..2 {
        let steady: Vec<f32> = audio
            .as_chunks::<2>()
            .0
            .iter()
            .skip(RATE as usize / 5)
            .map(|f| f[channel])
            .collect();
        let (bad, report) = analyze(&steady);
        failures.extend(
            bad.into_iter()
                .map(|bad| format!("channel {channel}: {bad}")),
        );
        reports.push(format!("channel {channel}: {report}"));
    }
    if audio
        .as_chunks::<2>()
        .0
        .iter()
        .any(|frame| (frame[0] - frame[1]).abs() > 1e-6)
    {
        failures.push("Stereo mismatch for duplicated mono fixture".into());
    }
    Ok((
        failures,
        format!(
            "frames={frames}, SAI underruns={}, overruns={}\n{}",
            stat(prefix, "underruns")?,
            stat(prefix, "overruns")?,
            reports.join("\n")
        ),
    ))
}

pub fn failures(prefix: &Path) -> Result<Vec<String>> {
    Ok(evaluate(prefix)?.0)
}
pub fn check(prefix: &Path) -> Result<String> {
    boot_evidence(prefix)?;
    let (failures, report) = evaluate(prefix)?;
    if !failures.is_empty() {
        return Err(format!("Audio gates failed: {}\n{report}", failures.join(", ")).into());
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gates_reject_silence_bypass_clipping_and_lost_audio() {
        let tone = |hz| audio_lab::tone(hz, 0.8, &[(1, 0.2)]);
        assert!(analyze(&tone(220.0)).0.is_empty());
        assert!(analyze(&vec![0.0; 38000])
            .0
            .iter()
            .any(|s| s.contains("RMS")));
        assert!(analyze(&tone(224.492958))
            .0
            .iter()
            .any(|s| s.contains("Autotune")));
        let mut broken = tone(220.0);
        broken[10000..11024].fill(0.0);
        assert!(analyze(&broken).0.iter().any(|s| s.contains("dropout")));
        broken[500] = f32::NAN;
        broken[501] = 2.0;
        assert!(analyze(&broken).0.iter().any(|s| s.contains("Invalid")));
    }
    #[test]
    fn warning_allowlist_does_not_hide_unknown_accesses() {
        // Renode MonitorCondition.Wait rechecks its predicate after an extra
        // host-thread wakeup; this exact diagnostic is not a lost virtual IRQ.
        assert!(known_warning(
            "[time]  [WARNING] Thread has been woken up 2 times, verify your condition"
        ));
        assert!(known_warning(
            "[time]  [WARNING] cpu: Trying to enable instruction cache, but it's not supported"
        ));
        assert!(!known_warning(
            "[time]  [WARNING] sysbus: Unhandled read from 0x90000000"
        ));
        assert!(!known_warning(
            "[ERROR] cpu: Trying to enable instruction cache, but it's not supported"
        ));
    }
}
