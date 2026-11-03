//! Actual-firmware keypad regression while formant processing is active.
use std::{error::Error, fs, path::Path};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
const KEYS: [usize; 14] = [9, 9, 7, 7, 8, 9, 7, 8, 9, 7, 9, 7, 8, 8];
const CONTROLS: [usize; 10] = [12, 10, 12, 10, 8, 9, 12, 10, 8, 8];
const CASES: [(&str, usize, &[usize]); 3] = [
    ("pitch", 0, &KEYS),
    ("tuned", 3, &KEYS),
    ("controls", 3, &CONTROLS),
];

fn stimulus(cycles: usize, keys: &[usize]) -> String {
    let mut events = Vec::new();
    for (step, cc, value) in [(5, 44, 64), (8, 45, 0), (41, 81, 127)] {
        for (i, byte) in [176, cc, value].into_iter().enumerate() {
            events.push((step + i, format!("{},uart,usart1,{byte}\n", step + i)));
        }
    }
    for i in 0..cycles {
        let step = 13 + i * 8;
        events.push((step, format!("{step},link,gpioPortC,1,gpioPortA,7\n")));
        events.push((
            step + 4,
            format!("{},unlink,gpioPortC,1,gpioPortA,7\n", step + 4),
        ));
    }
    for (i, &key) in keys.iter().enumerate() {
        let step = 80 + i * 7;
        let row = if key >= 10 {
            "gpioPortA,7"
        } else {
            "gpioPortB,1"
        };
        let (port, pin) = match key {
            9 | 12 => ("gpioPortC", 4),
            8 => ("gpioPortC", 1),
            _ => ("gpioPortA", 6),
        };
        events.push((step, format!("{step},link,{port},{pin},{row}\n")));
        events.push((
            step + 2,
            format!("{},unlink,{port},{pin},{row}\n", step + 2),
        ));
    }
    events.sort_by_key(|event| event.0);
    events.into_iter().map(|event| event.1).collect()
}

fn prepare(dir: &Path) -> Result<()> {
    fs::create_dir(dir)?;
    for (name, cycles, keys) in CASES {
        fs::write(dir.join(format!("{name}.stimulus")), stimulus(cycles, keys))?;
    }
    Ok(())
}

fn inspect(dir: &Path) -> Result<()> {
    let manifest = fs::read_to_string(dir.join("firmware.txt"))?;
    let hash = manifest
        .lines()
        .find_map(|line| line.strip_prefix("firmware_sha256="))
        .ok_or("missing ELF identity")?;
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid ELF identity".into());
    }
    let mut report = String::new();
    for (name, cycles, keys) in CASES {
        let done = fs::read_to_string(dir.join(format!("{name}.completed.txt")))?;
        if !done.lines().any(|line| line == "exit=0")
            || !done
                .lines()
                .any(|line| line == format!("firmware_sha256={hash}"))
        {
            return Err("missing image-specific successful completion".into());
        }
        let rtt = fs::read_to_string(dir.join(format!("{name}.rtt")))?;
        if !rtt.contains("Startup complete, transitioning away from splash screen")
            || [
                "Dropped",
                "Failed to spawn",
                "panicked",
                "HardFault",
                "ERROR",
            ]
            .iter()
            .any(|s| rtt.contains(s))
        {
            return Err(format!("{name}: boot or audio-task failure").into());
        }
        let actual: Vec<usize> = rtt
            .lines()
            .filter_map(|line| {
                line.split_once("Button pressed - Row:")
                    .and_then(|(_, line)| line.split_once("Key: "))
                    .and_then(|(_, key)| key.parse().ok())
            })
            .collect();
        let expected: Vec<_> = std::iter::repeat_n(11, cycles)
            .chain(keys.iter().copied())
            .collect();
        if actual != expected {
            return Err(format!(
                "{name}: lost/duplicated/out-of-order buttons: {actual:?}, expected {expected:?}"
            )
            .into());
        }
        let host = fs::read_to_string(dir.join(format!("{name}.host.log")))?;
        for guard in [
            "Python execution guard verified",
            "Native instruction-fetch tag verified",
            "IRQ-aware wait pacing adapter verified",
            "NVIC active-count adapter verified",
            "PC: ",
        ] {
            if !host.contains(guard) {
                return Err(format!("{name}: CPU guard absent").into());
            }
        }
        let faults = fs::read_to_string(dir.join(format!("{name}.cpu-faults.txt")))?;
        for address in ["0xE000ED28", "0xE000ED2C"] {
            if !faults
                .lines()
                .any(|line| line == format!("{address}=0x00000000"))
            {
                return Err("CPU fault".into());
            }
        }
        for line in fs::read_to_string(dir.join(format!("{name}.log")))?.lines() {
            if line.contains("[ERROR]") {
                return Err("emulator error".into());
            }
            if let Some((_, warning)) = line.split_once("[WARNING] ") {
                if !include_str!("../../virtual-daisy/known-warnings.txt")
                    .lines()
                    .any(|known| !known.is_empty() && known == warning)
                {
                    return Err(format!("unexpected emulator warning: {warning}").into());
                }
            }
        }
        let wav = hound::WavReader::open(dir.join(format!("{name}.wav")))?;
        let spec = wav.spec();
        let pcm = wav
            .into_samples::<f32>()
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if spec.channels != 2
            || spec.sample_format != hound::SampleFormat::Float
            || pcm.len() % 2 != 0
            || pcm
                .chunks_exact(2)
                .any(|s| !s[0].is_finite() || s[0] != s[1] || s[0].abs() > 0.80001)
            || pcm.len() / 2 < spec.sample_rate as usize * 19 / 10
        {
            return Err("invalid/truncated PCM".into());
        }
        let mono: Vec<_> = pcm.chunks_exact(2).map(|s| s[0]).collect();
        let rms = audio_lab::metrics(&mono[spec.sample_rate as usize..]).rms;
        if rms < 0.0001 {
            return Err("inaudible vocal fixture".into());
        }
        let stats = fs::read_to_string(dir.join(format!("{name}.wav.stats")))?;
        for counter in ["underruns=0", "overruns=0"] {
            if !stats.lines().any(|line| line == counter) {
                return Err("DMA FIFO error".into());
            }
        }
        report.push_str(&format!("{name}: all {} physical keypad presses recognized in order (including repeated same-key presses and formant-off); audible finite stereo RMS={rms:.6}, clear CPU faults, zero reported dropped tasks/DMA errors.\n", expected.len()));
    }
    report.push_str("Functional-model test of 20 ms presses; contact bounce, physical scan latency, display refresh and real CPU deadline margins require a board.\n");
    fs::write(dir.join("formant-audit.txt"), &report)?;
    print!("{report}");
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [mode, dir] if mode == "prepare" => prepare(Path::new(dir)),
        [mode, dir] if mode == "inspect" => inspect(Path::new(dir)),
        _ => Err("usage: formant_capture prepare NEW_DIRECTORY | inspect CAPTURE_DIRECTORY".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn twenty_ms_repeated_formant_buttons_and_real_uart_setup() {
        let script = stimulus(3, &KEYS);
        assert!(script.contains("41,uart,usart1,176\n42,uart,usart1,81\n43,uart,usart1,127\n"));
        assert!(
            script.contains("80,link,gpioPortC,4,gpioPortB,1\n82,unlink,gpioPortC,4,gpioPortB,1\n")
        );
        assert!(
            script.contains("87,link,gpioPortC,4,gpioPortB,1\n89,unlink,gpioPortC,4,gpioPortB,1\n")
        );
        assert_eq!(
            script
                .lines()
                .filter(|line| line.contains(",link,"))
                .count(),
            17
        );
    }
    #[test]
    fn ordinary_controls_use_the_correct_row_without_switching_profiles() {
        let script = stimulus(3, &CONTROLS);
        assert!(
            script.contains("80,link,gpioPortC,4,gpioPortA,7\n82,unlink,gpioPortC,4,gpioPortA,7\n")
        );
        assert!(
            script.contains("87,link,gpioPortA,6,gpioPortA,7\n89,unlink,gpioPortA,6,gpioPortA,7\n")
        );
        assert_eq!(
            script
                .lines()
                .filter(|line| line.contains(",link,"))
                .count(),
            13
        );
    }
}
