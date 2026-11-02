//! Device-source level calibration through shared DSP, oscillators and mixer.
use crate::{
    constants::SAMPLE_RATE, metrics, output_mix, render, steady, vocal_frame,
    voice_generator::VoiceManager,
};
use std::{error::Error, fs, io::Write, path::Path};
use synthphone_e_vocal_dsp::{
    audio::{Oscillator, Waveform},
    dsp::bitcrush,
    MusicalSettings, ProcessingMode,
};

pub const WAVES: [(&str, Waveform); 4] = [
    ("triangle", Waveform::Triangle),
    ("sine", Waveform::Sine),
    ("square", Waveform::Square),
    ("saw", Waveform::Saw),
];
pub struct Comparison {
    pub vocal: Vec<f32>,
    pub keypad: Vec<f32>,
    pub midi: Vec<f32>,
}

/// Compare A3 sources at the same master setting. The live-input reference is
/// 0.2 peak, matching the existing virtual-Daisy fixture. MIDI velocity is kept.
pub fn compare(wave: Waveform, velocity: u8, input_peak: f64, master: f32) -> Comparison {
    compare_with_levels(wave, velocity, input_peak, master, 10, 10)
}

pub fn compare_with_levels(
    wave: Waveform,
    velocity: u8,
    input_peak: f64,
    master: f32,
    voice_level: i8,
    synth_level: i8,
) -> Comparison {
    let input: Vec<_> = crate::tone(224.492958, 1.4, &[(1, input_peak)])
        .into_iter()
        .map(|x| x * output_mix::level_gain(voice_level))
        .collect();
    let synth_gain = output_mix::level_gain(synth_level);
    let vocal_raw = render(
        &input,
        &[],
        &vocal_frame::default_config(),
        &MusicalSettings::default(),
    )
    .raw;
    let vocal = vocal_raw
        .iter()
        .map(|&x| output_mix::mix(bitcrush(x, 32), 0.0, 0.0, wave, master))
        .collect();
    let mut oscillator = Oscillator::new(220.0, SAMPLE_RATE, wave);
    let carrier: Vec<_> = (0..input.len())
        .map(|_| oscillator.next_value() * output_mix::keypad_gain(wave) * synth_gain)
        .collect();
    let settings = MusicalSettings {
        mode: ProcessingMode::Dry,
        note: 6,
        ..MusicalSettings::default()
    };
    let keypad_raw = render(
        &vec![0.0; input.len()],
        &carrier,
        &vocal_frame::default_config(),
        &settings,
    )
    .raw;
    let keypad = keypad_raw
        .iter()
        .map(|&x| output_mix::mix(bitcrush(x, 32), 0.0, 0.0, wave, master))
        .collect();
    let mut voices = VoiceManager::<8>::new(SAMPLE_RATE);
    voices.set_waveform(wave);
    voices.note_on(57, velocity, 1); // Ch2 is sound-only: no vocal pitch control.
    let midi = (0..keypad_raw.len())
        .map(|_| {
            let (melody, drums) = voices.get_mixed_sources();
            output_mix::mix_with_synth_gain(0.0, melody, drums, wave, master, synth_gain)
        })
        .collect();
    Comparison {
        vocal,
        keypad,
        midi,
    }
}

pub fn db_ratio(a: f64, b: f64) -> f64 {
    20.0 * (a / b).log10()
}

fn wav(path: &Path, samples: &[f32]) -> Result<(), Box<dyn Error>> {
    let mut writer = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE.round() as u32,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )?;
    for &x in samples {
        writer.write_sample(x)?;
    }
    writer.finalize()?;
    Ok(())
}

pub fn run(directory: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(directory)?;
    // Physical GPIO links simulate held matrix keys; UART bytes are spaced by
    // 10ms. Firmware interprets every input through its normal interrupt paths.
    fs::write(
        directory.join("midi.stimulus"),
        "10,uart,usart1,145\n11,uart,usart1,57\n12,uart,usart1,127\n",
    )?;
    fs::write(
        directory.join("keypad.stimulus"),
        concat!(
            "5,link,gpioPortC,1,gpioPortA,7\n9,unlink,gpioPortC,1,gpioPortA,7\n",
            "13,link,gpioPortC,1,gpioPortA,7\n17,unlink,gpioPortC,1,gpioPortA,7\n",
            "19,link,gpioPortC,4,gpioPortC,0\n20,unlink,gpioPortC,4,gpioPortC,0\n",
            "21,gpio,gpioPortC,10,0\n29,gpio,gpioPortC,10,1\n",
            "36,link,gpioPortC,4,gpioPortA,3\n"
        ),
    )?;
    let mut csv = fs::File::create(directory.join("levels.csv"))?;
    writeln!(
        csv,
        "waveform,input_peak,velocity,source,rms,peak,db_vs_vocal,non_finite,over_full_scale"
    )?;
    for (name, wave) in WAVES {
        let c = compare(wave, 127, 0.2, 1.0);
        let reference = metrics(steady(&c.vocal)).rms;
        let mut ab = Vec::new();
        for (source, audio) in [
            ("autotune", &c.vocal),
            ("keypad", &c.keypad),
            ("midi", &c.midi),
        ] {
            let clip = steady(audio);
            let m = metrics(clip);
            writeln!(
                csv,
                "{name},0.2,127,{source},{:.9},{:.9},{:.5},{},{}",
                m.rms,
                m.peak,
                db_ratio(m.rms, reference),
                m.non_finite,
                m.over_full_scale
            )?;
            wav(&directory.join(format!("{name}-{source}.wav")), clip)?;
            ab.extend_from_slice(clip);
            ab.resize(ab.len() + (SAMPLE_RATE * 0.25) as usize, 0.0);
        }
        wav(&directory.join(format!("{name}-comparison.wav")), &ab)?;
    }
    // Expose input sensitivity and velocity, rather than pretending fixed tones
    // can equal every microphone level or normalizing the resulting files.
    for peak in [0.05, 0.1, 0.4] {
        let c = compare(Waveform::Triangle, 127, peak, 1.0);
        let reference = metrics(steady(&c.vocal)).rms;
        for (source, audio) in [
            ("autotune", c.vocal),
            ("keypad", c.keypad),
            ("midi", c.midi),
        ] {
            let m = metrics(steady(&audio));
            writeln!(
                csv,
                "triangle,{peak},127,{source},{:.9},{:.9},{:.5},{},{}",
                m.rms,
                m.peak,
                db_ratio(m.rms, reference),
                m.non_finite,
                m.over_full_scale
            )?;
        }
    }
    println!(
        "Source order: autotune, keypad, MIDI; 250ms gaps; original levels retained.\n{}",
        fs::read_to_string(directory.join("levels.csv"))?
    );
    Ok(())
}
