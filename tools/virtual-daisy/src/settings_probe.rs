//! Actual firmware save/IRQ regression; inputs enter through USART1.
use super::{grade, Host, Result};
use std::{fs, path::Path};

pub fn run(host: &Host<'_>, silence: &Path) -> Result<()> {
    let result = host.directory.join("settings-summary.txt");
    if result.exists() {
        fs::remove_file(&result)?;
    }
    fs::write(
        host.directory.join("settings.stimulus"),
        concat!(
            "5,uart,usart1,176\n6,uart,usart1,7\n7,uart,usart1,64\n",
            "210,uart,usart1,145\n211,uart,usart1,57\n212,uart,usart1,127\n",
            "225,uart,usart1,129\n226,uart,usart1,57\n227,uart,usart1,0\n"
        ),
    )?;
    let capture = host.boot("settings", silence, "settings")?;
    let rtt = fs::read_to_string(grade::artifact(&capture, ".rtt"))?;
    if !rtt.contains("settings saved") || rtt.contains("settings save failed") {
        return Err("MIDI CC settings change was not saved".into());
    }
    if grade::stat(&capture, "underruns")? != 0 || grade::stat(&capture, "overruns")? != 0 {
        return Err("Settings save disrupted audio DMA".into());
    }
    let reader = hound::WavReader::open(grade::artifact(&capture, ".wav"))?;
    let spec = reader.spec();
    let audio = reader
        .into_samples::<f32>()
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if spec.channels != 2
        || audio.len() % 2 != 0
        || audio
            .chunks_exact(2)
            .any(|x| !x[0].is_finite() || x[0] != x[1] || x[0].abs() > 0.80001)
    {
        return Err("Invalid settings capture".into());
    }
    let mono: Vec<_> = audio.chunks_exact(2).map(|x| x[0]).collect();
    if mono.len() < (f64::from(spec.sample_rate) * 2.95) as usize
        || mono.len() != grade::stat(&capture, "frames")? as usize
    {
        return Err("Truncated settings capture".into());
    }
    let window = |start: f64, end: f64| {
        audio_lab::metrics(
            &mono[(start * f64::from(spec.sample_rate)) as usize
                ..(end * f64::from(spec.sample_rate)) as usize],
        )
        .rms
    };
    let on = window(2.18, 2.22);
    let off = window(2.65, 2.95);
    if on < 0.05 || off > 0.00001 {
        return Err(format!("MIDI during flash save failed: note_on={on}, note_off={off}").into());
    }
    fs::write(&result,format!("PASS: actual firmware saved a MIDI-originated master-volume change, received NoteOn/NoteOff during/after the flash-save interval, produced finite matching stereo and zero DMA underruns/overruns.\nnote_on_rms={on}\nnote_off_rms={off}\nFlash retention across power cycles and physical timing remain unverified.\n"))?;
    print!("{}", fs::read_to_string(result)?);
    Ok(())
}
