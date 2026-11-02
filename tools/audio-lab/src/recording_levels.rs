//! Original-level A/B replay of a real vocal recording and generated notes.
use crate::{
    constants::SAMPLE_RATE, levels::db_ratio, metrics, output_mix, render, steady, vocal_frame,
    voice_generator::VoiceManager,
};
use std::{error::Error, fs, io::Write, path::Path};
use synthphone_e_vocal_dsp::{
    audio::{Oscillator, Waveform},
    dsp::{bitcrush, normalize_sample},
    MusicalSettings, ProcessingMode,
};

fn wav(path: &Path, audio: &[f32]) -> Result<(), Box<dyn Error>> {
    let mut out = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE.round() as u32,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )?;
    for &x in audio {
        out.write_sample(x)?;
    }
    out.finalize()?;
    Ok(())
}

fn finish(
    processed: f32,
    melody: f32,
    legacy: bool,
    synth: f32,
    settings_voice: Option<f32>,
) -> f32 {
    let processed = bitcrush(processed, 32);
    if let Some(voice) = settings_voice {
        // Existing volume-settings branch c9d1a58: Voice Volume scales the
        // entire DSP output (including keypad); Melody Volume only scales MIDI.
        normalize_sample(processed * voice + melody * 1.2 * 0.1 * synth, 0.8)
    } else if legacy {
        // Frozen triangle mix from main339b3f4; diagnostic A/B reference only.
        normalize_sample(processed * 0.75 + melody * 1.2 * 0.1, 0.8)
    } else {
        output_mix::mix_with_synth_gain(processed, melody, 0.0, Waveform::Triangle, 1.0, synth)
    }
}

pub fn run(input: &[f32], directory: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(directory)?;
    wav(&directory.join("input.wav"), input)?;
    let mut csv = fs::File::create(directory.join("levels.csv"))?;
    writeln!(
        csv,
        "version,voice_level,synth_level,source,rms,peak,db_vs_voice,non_finite,over_full_scale"
    )?;
    let config = vocal_frame::default_config();
    let default = MusicalSettings::default();
    for (version, legacy, settings, voice_level, synth_level) in [
        ("before", true, false, 10, 10),
        ("before-settings", true, true, 10, 10),
        ("before-settings-adjusted", true, true, 5, 20),
        ("after", false, false, 10, 10),
        ("after-balanced", false, false, 10, 8),
        ("after-voice-pad", false, false, 5, 4),
        ("after-adjusted", false, false, 5, 20),
    ] {
        let settings_voice = settings.then(|| output_mix::level_gain(voice_level));
        let signal: Vec<_> = input
            .iter()
            .map(|x| {
                x * if settings {
                    1.0
                } else {
                    output_mix::level_gain(voice_level)
                }
            })
            .collect();
        let voice_raw = render(&signal, &[], &config, &default).raw;
        let mut osc = Oscillator::new(110.0, SAMPLE_RATE, Waveform::Triangle);
        let synth_gain = output_mix::level_gain(synth_level);
        let drive = if legacy {
            1.0
        } else {
            output_mix::keypad_gain(Waveform::Triangle) * synth_gain
        };
        let carrier: Vec<_> = (0..input.len()).map(|_| osc.next_value() * drive).collect();
        let dry = MusicalSettings {
            mode: ProcessingMode::Dry,
            note: 6,
            ..default
        };
        let keypad_raw = render(&vec![0.0; input.len()], &carrier, &config, &dry).raw;
        let keypad_mix_raw = render(&signal, &carrier, &config, &dry).raw;
        let mut voices = VoiceManager::<8>::new(SAMPLE_RATE);
        voices.set_waveform(Waveform::Triangle);
        voices.note_on(45, 127, 1); // A2, matching the keypad's default note6.
        let melody: Vec<_> = (0..voice_raw.len())
            .map(|_| voices.get_mixed_sample())
            .collect();
        let voice: Vec<_> = voice_raw
            .iter()
            .map(|&v| finish(v, 0.0, legacy, 1.0, settings_voice))
            .collect();
        let keypad: Vec<_> = keypad_raw
            .iter()
            .map(|&v| finish(v, 0.0, legacy, 1.0, settings_voice))
            .collect();
        let midi: Vec<_> = melody
            .iter()
            .map(|&v| finish(0.0, v, legacy, synth_gain, settings_voice))
            .collect();
        let midi_mix: Vec<_> = voice_raw
            .iter()
            .zip(&melody)
            .map(|(&v, &m)| finish(v, m, legacy, synth_gain, settings_voice))
            .collect();
        let keypad_mix: Vec<_> = keypad_mix_raw
            .iter()
            .map(|&v| finish(v, 0.0, legacy, 1.0, settings_voice))
            .collect();
        let reference = metrics(steady(&voice)).rms;
        for (name, audio) in [
            ("autotune", &voice),
            ("keypad", &keypad),
            ("midi", &midi),
            ("keypad-mix", &keypad_mix),
            ("midi-mix", &midi_mix),
        ] {
            let m = metrics(steady(audio));
            writeln!(
                csv,
                "{version},{voice_level},{synth_level},{name},{:.9},{:.9},{:.5},{},{}",
                m.rms,
                m.peak,
                db_ratio(m.rms, reference),
                m.non_finite,
                m.over_full_scale
            )?;
            wav(&directory.join(format!("{version}-{name}.wav")), audio)?;
        }
        // Recorded/autotuned voice, then isolated keypad and MIDI. No separate
        // normalization; the first six seconds of the vocal stay at their level.
        let mut sequence = voice[..voice.len().min((SAMPLE_RATE * 6.0) as usize)].to_vec();
        for tone in [&keypad, &midi] {
            sequence.resize(sequence.len() + (SAMPLE_RATE * 0.35) as usize, 0.0);
            let clip = steady(tone);
            sequence.extend_from_slice(&clip[..clip.len().min((SAMPLE_RATE * 2.0) as usize)]);
        }
        wav(
            &directory.join(format!("{version}-comparison.wav")),
            &sequence,
        )?;
    }
    println!("{}", fs::read_to_string(directory.join("levels.csv"))?);
    Ok(())
}
