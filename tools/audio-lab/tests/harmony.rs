use audio_lab::{constants::SAMPLE_RATE, render, steady, tone, vocal_frame};
use synthphone_e_vocal_dsp::{MusicalSettings, ProcessingMode};

// Independent sinusoidal projection with a Hann taper. A monophonic pitch
// estimator cannot establish that every note of a chord is present.
fn component(samples: &[f32], hz: f64) -> f64 {
    (-3..=3)
        .map(|step| {
            let hz = hz * 2_f64.powf(step as f64 * 5.0 / 1200.0);
            let (mut re, mut im, mut weight) = (0.0, 0.0, 0.0);
            for (i, &sample) in samples.iter().enumerate() {
                let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / samples.len() as f64).cos();
                let phase = std::f64::consts::TAU * hz * i as f64 / SAMPLE_RATE as f64;
                re += sample as f64 * w * phase.cos();
                im += sample as f64 * w * phase.sin();
                weight += w;
            }
            2.0 * (re * re + im * im).sqrt() / weight
        })
        .fold(0.0, f64::max)
}

#[test]
fn a_four_note_vocal_chord_contains_every_requested_note() {
    let input = tone(200.0, 0.85, &[(1, 0.2)]);
    let notes = [220.0_f32, 246.94165, 261.62558, 293.66476];
    let mut settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    settings.midi_frequencies[..4].copy_from_slice(&notes);
    let result = render(&input, &[], &vocal_frame::default_config(), &settings);
    for note in notes {
        let amplitude = component(steady(&result.audio), note as f64);
        assert!(
            amplitude > 0.015,
            "missing chord note {note} Hz: amplitude {amplitude}"
        );
    }
}

#[test]
fn a_unison_held_through_silence_does_not_cancel_the_vocal() {
    let mut input = vec![0.0; (SAMPLE_RATE * 0.25) as usize];
    input.extend(tone(220.0, 0.7, &[(1, 0.2)]));
    let mut settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    let config = vocal_frame::default_config();
    let dry = render(&input, &[], &config, &settings);
    settings.midi_frequencies[0] = 220.0;
    let unison = render(&input, &[], &config, &settings);
    let range = (SAMPLE_RATE * 0.5) as usize..(SAMPLE_RATE * 0.85) as usize;
    let ratio =
        component(&unison.audio[range.clone()], 220.0) / component(&dry.audio[range], 220.0);
    assert!(
        ratio > 0.85,
        "unison cancels the lead: amplitude ratio {ratio}"
    );
}

#[test]
fn midi_only_harmony_is_silent_without_notes() {
    let input = tone(200.0, 0.5, &[(1, 0.2)]);
    let mut config = vocal_frame::default_config();
    config.harmony_lead_gain = 0.0;
    config.sample_rate = SAMPLE_RATE;
    let settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    let output = render(&input, &[], &config, &settings);
    assert!(output.audio.iter().all(|x| x.abs() < 1e-7));
}

#[test]
fn chord_note_order_does_not_change_the_audio() {
    let input = tone(200.0, 0.6, &[(1, 0.2), (2, 0.05)]);
    let mut settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    settings.midi_frequencies = [
        220.0, 246.94165, 261.62558, 293.66476, 329.62756, 349.22824, 391.99542, 440.0,
    ];
    let config = vocal_frame::default_config();
    let forward = render(&input, &[], &config, &settings);
    settings.midi_frequencies.reverse();
    let reverse = render(&input, &[], &config, &settings);
    assert_eq!(forward.audio, reverse.audio);
    assert_eq!(audio_lab::metrics(&forward.audio).non_finite, 0);
    for note in settings.midi_frequencies {
        assert!(
            component(steady(&forward.audio), note as f64) > 0.007,
            "missing {note}"
        );
    }
}

#[test]
fn held_polyphony_matches_the_average_of_independently_rendered_voices() {
    let input: Vec<_> = (0..(SAMPLE_RATE * 0.65) as usize)
        .map(|i| {
            let t = i as f64 / SAMPLE_RATE as f64;
            let phase =
                std::f64::consts::TAU * 196.0 * t + 0.15 * (std::f64::consts::TAU * 5.0 * t).sin();
            (0.07 * phase.sin() + 0.04 * (phase * 2.0).sin() + 0.02 * (phase * 5.0).sin()) as f32
        })
        .collect();
    let keys = [57, 64, 60, 67, 48, 52, 55, 69];
    let notes = keys.map(|key| 440.0 * 2_f32.powf((key as f32 - 69.0) / 12.0));
    let mut config = vocal_frame::default_config();
    config.sample_rate = SAMPLE_RATE;
    config.harmony_lead_gain = 0.0;
    let mut settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    let separate: Vec<_> = notes
        .iter()
        .map(|&note| {
            settings.midi_frequencies = [0.0; 8];
            settings.midi_frequencies[0] = note;
            render(&input, &[], &config, &settings).raw
        })
        .collect();
    for count in [2, 3, 4, 6, 8] {
        settings.midi_frequencies = [0.0; 8];
        settings.midi_frequencies[..count].copy_from_slice(&notes[..count]);
        let mixed = render(&input, &[], &config, &settings);
        // This equality is for voiced frames. Onsets/consonants have one shared
        // noise path, whose polyphony-dependent power is checked separately.
        let region = (SAMPLE_RATE * 0.2) as usize..(SAMPLE_RATE * 0.6) as usize;
        let peak = mixed.raw[region.clone()]
            .iter()
            .enumerate()
            .map(|(i, &sample)| {
                let expected = separate[..count]
                    .iter()
                    .map(|voice| voice[i + region.start] as f64)
                    .sum::<f64>()
                    / count as f64;
                (sample as f64 - expected).abs()
            })
            .fold(0.0, f64::max);
        eprintln!("{count} held voices: peak difference from independent sum {peak}");
        assert!(peak < 0.00001, "{count} held voices interfere: {peak}");
    }
}

#[test]
fn shifting_one_clean_tone_does_not_add_audible_sidebands() {
    let input = tone(200.0, 1.3, &[(1, 0.2)]);
    let mut config = vocal_frame::default_config();
    config.harmony_lead_gain = 0.0;
    config.sample_rate = SAMPLE_RATE;
    let mut settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    for target in [220.0, 246.94165, 293.66476] {
        settings.midi_frequencies[0] = target;
        let output = render(&input, &[], &config, &settings);
        let x = &output.audio[(SAMPLE_RATE * 0.3) as usize..(SAMPLE_RATE * 1.1) as usize];
        // Independent least-squares projection onto the requested sinusoid.
        // Energy outside this sinusoid is distortion, even if a pitch counter
        // still reports the correct fundamental.
        let (mut ss, mut cc, mut sc, mut xs, mut xc, mut xx) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        for (i, &sample) in x.iter().enumerate() {
            let phase = std::f64::consts::TAU * target as f64 * i as f64 / SAMPLE_RATE as f64;
            let (s, c) = phase.sin_cos();
            let value = sample as f64;
            ss += s * s;
            cc += c * c;
            sc += s * c;
            xs += value * s;
            xc += value * c;
            xx += value * value;
        }
        let det = ss * cc - sc * sc;
        let (a, b) = ((xs * cc - xc * sc) / det, (xc * ss - xs * sc) / det);
        let residual = ((xx - a * xs - b * xc) / xx).max(0.0);
        eprintln!("target {target}: unwanted energy {:.2}%", residual * 100.0);
        assert!(
            residual < 0.01,
            "{target} Hz: {:.2}% unwanted energy",
            residual * 100.0
        );
    }
}

#[test]
fn a_held_midi_note_removes_vibrato_without_garbled_sidebands() {
    let mut phase = 0.0_f64;
    let input: Vec<_> = (0..(SAMPLE_RATE * 2.0) as usize)
        .map(|i| {
            let time = i as f64 / SAMPLE_RATE as f64;
            let hz = 200.0 * 2_f64.powf(50.0 * (std::f64::consts::TAU * 5.0 * time).sin() / 1200.0);
            phase += std::f64::consts::TAU * hz / SAMPLE_RATE as f64;
            (0.2 * phase.sin()) as f32
        })
        .collect();
    let mut config = vocal_frame::default_config();
    config.sample_rate = SAMPLE_RATE;
    config.harmony_lead_gain = 0.0;
    let mut settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    settings.midi_frequencies[0] = 220.0;
    let output = render(&input, &[], &config, &settings);
    let samples = &output.audio[(SAMPLE_RATE * 0.4) as usize..(SAMPLE_RATE * 1.8) as usize];
    let mut energy = 0.0_f64;
    let (mut re, mut im) = (0.0, 0.0);
    for (i, &x) in samples.iter().enumerate() {
        let phase = std::f64::consts::TAU * 220.0 * i as f64 / SAMPLE_RATE as f64;
        re += x as f64 * phase.cos();
        im += x as f64 * phase.sin();
        energy += (x as f64).powi(2);
    }
    let unwanted = 1.0 - 2.0 * (re * re + im * im) / (samples.len() as f64 * energy);
    eprintln!("vibrato retune: unwanted energy {:.2}%", unwanted * 100.0);
    assert!(
        unwanted < 0.05,
        "vibrato becomes distortion: {:.2}% unwanted energy",
        unwanted * 100.0
    );
}

#[test]
fn rich_voiced_vibrato_stays_on_the_requested_harmonic_grid() {
    let mut phase = 0.0_f64;
    let input: Vec<_> = (0..(SAMPLE_RATE * 2.0) as usize)
        .map(|i| {
            let t = i as f64 / SAMPLE_RATE as f64;
            let hz = 200.0 * 2_f64.powf(50.0 * (std::f64::consts::TAU * 5.0 * t).sin() / 1200.0);
            phase += std::f64::consts::TAU * hz / SAMPLE_RATE as f64;
            (1..=10)
                .map(|h| 0.15 / h as f64 * (phase * h as f64).sin())
                .sum::<f64>() as f32
        })
        .collect();
    let mut config = vocal_frame::default_config();
    config.sample_rate = SAMPLE_RATE;
    config.harmony_lead_gain = 0.0;
    let mut settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    let mut failures = Vec::new();
    for (name, notes, grid, harmonics) in [
        ("one-220", &[220.0_f32][..], 220.0, 20),
        ("one-440", &[440.0][..], 440.0, 20),
        ("two-octaves", &[220.0, 440.0][..], 220.0, 40),
        (
            "eight-harmonic-pitches",
            &[110.0, 220.0, 330.0, 440.0, 550.0, 660.0, 770.0, 880.0][..],
            110.0,
            100,
        ),
    ] {
        settings.midi_frequencies = [0.0; 8];
        settings.midi_frequencies[..notes.len()].copy_from_slice(notes);
        let output = render(&input, &[], &config, &settings);
        let x = &output.audio[(SAMPLE_RATE * 0.4) as usize..(SAMPLE_RATE * 1.8) as usize];
        let energy = x.iter().map(|&s| (s as f64).powi(2)).sum::<f64>();
        let mut captured = 0.0;
        for h in 1..=harmonics {
            let (mut re, mut im) = (0.0, 0.0);
            for (i, &s) in x.iter().enumerate() {
                let phase = std::f64::consts::TAU * grid * h as f64 * i as f64 / SAMPLE_RATE as f64;
                re += s as f64 * phase.cos();
                im += s as f64 * phase.sin();
            }
            captured += 2.0 * (re * re + im * im) / x.len() as f64;
        }
        let outside = (1.0 - captured / energy).max(0.0);
        eprintln!(
            "rich voiced {name}: {:.3}% outside target harmonics",
            outside * 100.0
        );
        if outside >= 0.05 {
            failures.push((name, outside));
        }
    }
    assert!(
        failures.is_empty(),
        "rich voiced targets miss their harmonic grids: {failures:?}"
    );
}

#[test]
fn unvoiced_noise_does_not_jump_above_the_polyphonic_voice_power_scale() {
    let mut random = 0x12345678_u32;
    let input: Vec<_> = (0..(SAMPLE_RATE * 0.6) as usize)
        .map(|_| {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            ((random as f64 / u32::MAX as f64) * 0.2 - 0.1) as f32
        })
        .collect();
    let mut config = vocal_frame::default_config();
    config.sample_rate = SAMPLE_RATE;
    config.harmony_lead_gain = 0.0;
    let mut reference = 0.0;
    for count in [2, 4, 8] {
        let mut settings = MusicalSettings {
            mode: ProcessingMode::Harmony,
            ..Default::default()
        };
        for i in 0..count {
            settings.midi_frequencies[i] = 220.0 + i as f32 * 35.0;
        }
        let output = render(&input, &[], &config, &settings);
        let rms = audio_lab::metrics(
            &output.raw[(SAMPLE_RATE * 0.2) as usize..(SAMPLE_RATE * 0.5) as usize],
        )
        .rms;
        if count == 2 {
            reference = rms;
        }
        let expected = (2.0 / count as f64).sqrt();
        assert!(
            (rms / reference - expected).abs() < 0.03,
            "{count} unvoiced voices: {} power ratio, expected {expected}",
            rms / reference
        );
    }
}

#[test]
fn designated_midi_lead_below_backing_obeys_the_group_fader() {
    use audio_lab::voice_generator::VoiceManager;
    let input = tone(200.0, 1.0, &[(1, 0.15)]);
    let mut config = vocal_frame::default_config();
    config.sample_rate = SAMPLE_RATE;
    config.harmony_lead_gain = 0.0;
    let mut manager = VoiceManager::<8>::new(SAMPLE_RATE);
    manager.note_on(57, 100, 3);
    manager.note_on(64, 100, 0);
    let mut settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    let (freq, gains) = manager.get_harmony_voices(64);
    settings.midi_frequencies = freq;
    let equal = render(&input, &[], &config, &settings);
    settings.midi_gains = gains;
    let foreground = render(&input, &[], &config, &settings);
    let ratio =
        |audio: &[f32]| component(steady(audio), 220.0) / component(steady(audio), 329.62756);
    let change = 20.0 * (ratio(&foreground.audio) / ratio(&equal.audio)).log10();
    assert!(
        (change - 20.0 * (127.0_f64 / 64.0).log10()).abs() < 0.15,
        "lead fader response {change} dB"
    );
    settings.midi_gains = manager.get_harmony_voices(0).1;
    let muted = render(&input, &[], &config, &settings);
    assert!(component(steady(&muted.audio), 220.0) > 0.05);
    assert!(component(steady(&muted.audio), 329.62756) < 0.001);
}

#[test]
fn muting_a_midi_voice_retains_its_phase_for_restoring_the_gain() {
    use audio_lab::{constants::*, replay::Replay};
    let input = tone(200.0, 1.0, &[(1, 0.1), (2, 0.03)]);
    let mut config = vocal_frame::default_config();
    config.sample_rate = SAMPLE_RATE;
    config.harmony_lead_gain = 0.0;
    let mut settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    settings.midi_frequencies[0] = 220.0;
    let reference = render(&input, &[], &config, &settings);
    let mut replay = Replay::new();
    let mut actual = Vec::new();
    for block in (0..input.len()).step_by(BLOCK_SIZE) {
        for &sample in &input[block..(block + BLOCK_SIZE).min(input.len())] {
            actual.push(replay.sample(sample));
        }
        settings.midi_gains[0] = if block < (SAMPLE_RATE * 0.4) as usize {
            0.0
        } else {
            1.0
        };
        replay.finish_block(&[0.0; HOP_SIZE], &config, &settings, None);
    }
    let start = (SAMPLE_RATE * 0.65) as usize;
    let peak = actual[start..]
        .iter()
        .zip(&reference.raw[start..])
        .map(|(&a, &b)| (a - b).abs())
        .fold(0.0_f32, f32::max);
    assert!(
        peak < 1e-6,
        "gain restore changed pitch-shift phase: {peak}"
    );
}

#[test]
fn backing_pcm_level_stays_constant_when_the_midi_lead_rests() {
    use audio_lab::{constants::*, replay::Replay, voice_generator::VoiceManager};
    let input = tone(200.0, 1.2, &[(1, 0.1)]);
    let mut config = vocal_frame::default_config();
    config.sample_rate = SAMPLE_RATE;
    config.harmony_lead_gain = 0.0;
    let mut manager = VoiceManager::<8>::new(SAMPLE_RATE);
    manager.note_on(57, 100, 3);
    manager.note_on(64, 100, 0);
    manager.note_on(67, 100, 0);
    let mut settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    let mut replay = Replay::new();
    let mut audio = Vec::new();
    for block in (0..input.len()).step_by(BLOCK_SIZE) {
        if block >= (SAMPLE_RATE * 0.6) as usize {
            manager.note_off(57, 3);
        }
        for &s in &input[block..(block + BLOCK_SIZE).min(input.len())] {
            audio.push(replay.sample(s));
        }
        (settings.midi_frequencies, settings.midi_gains) = manager.get_harmony_voices(64);
        replay.finish_block(&[0.0; HOP_SIZE], &config, &settings, None);
    }
    let a = &audio[(SAMPLE_RATE * 0.3) as usize..(SAMPLE_RATE * 0.5) as usize];
    let b = &audio[(SAMPLE_RATE * 0.9) as usize..(SAMPLE_RATE * 1.1) as usize];
    for target in [329.62756, 391.99542] {
        let ratio = component(b, target) / component(a, target);
        assert!(
            (ratio - 1.0).abs() < 0.04,
            "backing {target} jumps at lead release: {ratio}"
        );
    }
}

#[test]
fn held_harmony_stays_in_tune_when_the_singer_changes_register() {
    let sources = [90.0, 130.0, 200.0, 300.0, 440.0, 590.0, 150.0, 300.0, 150.0];
    let segment = (SAMPLE_RATE * 0.45) as usize;
    let mut phase = 0.0_f64;
    let mut input = Vec::new();
    for source in sources {
        input.extend((0..segment).map(|_| {
            phase += std::f64::consts::TAU * source / f64::from(SAMPLE_RATE);
            (0.15 * phase.sin()) as f32
        }));
    }
    let mut config = vocal_frame::default_config();
    config.harmony_lead_gain = 0.0;
    let mut settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    settings.midi_frequencies[0] = 220.0;
    let output = render(&input, &[], &config, &settings);
    let mut failures = Vec::new();
    for (i, source) in sources.into_iter().enumerate() {
        let start = i * segment + (SAMPLE_RATE * 0.22) as usize;
        let end = i * segment + (SAMPLE_RATE * 0.4) as usize;
        let actual = audio_lab::estimate_pitch(&output.raw[start..end]);
        let spur_ratio =
            component(&output.raw[start..end], 350.0) / component(&output.raw[start..end], 220.0);
        if source == 90.0 {
            eprintln!(
                "90 Hz source -> 220 Hz: measured {actual:?}, 350 Hz spur ratio {spur_ratio}"
            );
        }
        if actual.is_none_or(|hz| audio_lab::cents(hz, 220.0).abs() > 20.0)
            || (source == 90.0 && spur_ratio > 0.03)
        {
            eprintln!("source {source}: unwanted 350 Hz amplitude / target = {spur_ratio}");
            failures.push((source, actual));
        }
    }
    assert!(
        failures.is_empty(),
        "held 220 Hz harmony lost tuning: {failures:?}"
    );
}

#[test]
fn unvoiced_consonants_include_the_lead_in_voice_power_normalization() {
    let input = audio_lab::vocal_evidence::noise(0.6);
    let mut config = vocal_frame::default_config();
    config.harmony_lead_gain = 0.0;
    let mut settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    settings.midi_frequencies[0] = 220.0;
    let reference = render(&input, &[], &config, &settings);
    let region = (SAMPLE_RATE * 0.2) as usize..(SAMPLE_RATE * 0.5) as usize;
    let reference_rms = audio_lab::metrics(&reference.raw[region.clone()]).rms;
    for lead in [0.0_f32, 0.5, 1.0] {
        config.harmony_lead_gain = lead;
        for count in [1, 2, 4] {
            settings.midi_frequencies = [0.0; 8];
            for i in 0..count {
                settings.midi_frequencies[i] = 220.0 + i as f32 * 35.0;
            }
            let result = render(&input, &[], &config, &settings);
            let ratio = audio_lab::metrics(&result.raw[region.clone()]).rms / reference_rms;
            // Independent voices add power, then the mixer averages their
            // amplitudes. The original lead participates in both terms.
            let expected =
                (f64::from(lead).powi(2) + count as f64).sqrt() / (f64::from(lead) + count as f64);
            assert!(
                (ratio - expected).abs() < 0.015,
                "lead {lead}, {count} harmonies: unvoiced gain {ratio}, expected {expected}"
            );
        }
    }
}

#[test]
fn wide_sung_vibrato_does_not_make_a_held_harmony_warble() {
    let mut phase = 0.0_f64;
    let input: Vec<_> = (0..(SAMPLE_RATE * 3.0) as usize)
        .map(|i| {
            let time = i as f64 / f64::from(SAMPLE_RATE);
            let source =
                150.0 * 2_f64.powf(150.0 * (std::f64::consts::TAU * 6.0 * time).sin() / 1200.0);
            phase += std::f64::consts::TAU * source / f64::from(SAMPLE_RATE);
            (0.15 * phase.sin()) as f32
        })
        .collect();
    let mut config = vocal_frame::default_config();
    config.harmony_lead_gain = 0.0;
    let mut settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    settings.midi_frequencies[0] = 220.0;
    let output = render(&input, &[], &config, &settings);
    let mut worst = 0.0_f64;
    for start in ((SAMPLE_RATE * 0.5) as usize..(SAMPLE_RATE * 2.7) as usize)
        .step_by((SAMPLE_RATE * 0.06) as usize)
    {
        let end = start + (SAMPLE_RATE * 0.12) as usize;
        let pitch = audio_lab::estimate_pitch(&output.raw[start..end])
            .expect("a held monophonic target must remain periodic");
        worst = worst.max(audio_lab::cents(pitch, 220.0).abs());
    }
    eprintln!("wide vibrato worst pitch error: {worst:.2} cents");
    assert!(
        worst < 20.0,
        "held 220 Hz harmony warbles by {worst:.2} cents"
    );
}

#[test]
fn a_fast_voiced_slide_keeps_harmony_on_its_held_target() {
    let mut phase = 0.0_f64;
    let input: Vec<_> = (0..SAMPLE_RATE as usize)
        .map(|i| {
            let time = i as f64 / f64::from(SAMPLE_RATE);
            let progress = ((time - 0.4) / 0.08).clamp(0.0, 1.0);
            let source = 200.0 * 1.5_f64.powf(progress);
            phase += std::f64::consts::TAU * source / f64::from(SAMPLE_RATE);
            (0.15 * phase.sin() + 0.06 * (2.0 * phase).sin() + 0.03 * (3.0 * phase).sin()) as f32
        })
        .collect();
    let mut config = vocal_frame::default_config();
    config.harmony_lead_gain = 0.0;
    let mut settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    settings.midi_frequencies[0] = 350.0;
    let output = render(&input, &[], &config, &settings);
    let mut worst = 0.0_f64;
    for start in ((SAMPLE_RATE * 0.35) as usize..(SAMPLE_RATE * 0.6) as usize).step_by(256) {
        let end = start + (SAMPLE_RATE * 0.12) as usize;
        let pitch = audio_lab::estimate_pitch(&output.raw[start..end])
            .expect("voiced glide must retain a periodic harmony");
        worst = worst.max(audio_lab::cents(pitch, 350.0).abs());
    }
    eprintln!("fast voiced slide worst target error: {worst:.2} cents");
    assert!(
        worst < 20.0,
        "held harmony follows the source slide by {worst:.2} cents"
    );
}

#[test]
fn noise_after_singing_is_not_locked_to_the_previous_pitch() {
    let mut input = tone(220.0, 0.3, &[(1, 0.15), (2, 0.05)]);
    input.extend(audio_lab::vocal_evidence::noise(0.6));
    let mut config = vocal_frame::default_config();
    config.harmony_lead_gain = 0.0;
    let mut settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    settings.midi_frequencies[0] = 350.0;
    let harmony = render(&input, &[], &config, &settings);
    settings.mode = ProcessingMode::Dry;
    let dry = render(&input, &[], &config, &settings);
    let range = (SAMPLE_RATE * 0.42) as usize..(SAMPLE_RATE * 0.52) as usize;
    let peak_error = harmony.raw[range.clone()]
        .iter()
        .zip(&dry.raw[range])
        .map(|(&a, &b)| (a - b).abs())
        .fold(0.0_f32, f32::max);
    assert!(
        peak_error < 1e-5,
        "unvoiced noise retained a remembered pitch: {peak_error}"
    );
}

#[test]
#[ignore = "Mixed-pitch dominant-vocal ambiguity remains unresolved; not single-source ground truth (docs/audio-testing.md)"]
fn quiet_low_frequency_leakage_does_not_replace_the_dominant_vocal_register() {
    dominant_vocal_with_leakage(None);
}

#[test]
fn an_explicit_vocal_range_can_exclude_low_background_periods() {
    dominant_vocal_with_leakage(Some(150.0));
}

fn dominant_vocal_with_leakage(floor: Option<f32>) {
    let input: Vec<_> = (0..SAMPLE_RATE as usize)
        .map(|i| {
            let phase = std::f64::consts::TAU * 220.0 * i as f64 / f64::from(SAMPLE_RATE);
            (0.15 * phase.sin()
                + 0.04 * (2.0 * phase).sin()
                + 0.02 * (3.0 * phase).sin()
                + 0.025 * (phase / 3.0).sin()) as f32
        })
        .collect();
    // The intended vocal is 220 Hz; the quieter 73.33 Hz component models
    // leaked backing/rumble rather than changing the singer's note.
    let mut config = vocal_frame::default_config();
    config.harmony_lead_gain = 0.0;
    if let Some(floor) = floor {
        config.min_frequency = floor;
    }
    let pitch = synthphone_e_vocal_dsp::dsp::frequency_analysis::find_pitch_harmony(
        &input[16_000..18_048],
        &config,
    );
    eprintln!("dominant 220 Hz vocal plus low leakage: detected {pitch} Hz");
    assert!(
        audio_lab::cents(f64::from(pitch), 220.0).abs() < 10.0,
        "low leakage replaced vocal register: {pitch}"
    );
    let mut settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    settings.midi_frequencies[0] = 350.0;
    let output = render(&input, &[], &config, &settings);
    assert!(
        component(steady(&output.raw), 350.0) > 0.02,
        "requested fundamental missing after leakage changed the register"
    );
}

#[test]
fn changing_vowel_balance_does_not_create_inharmonic_harmony() {
    let input: Vec<_> = (0..(SAMPLE_RATE * 3.0) as usize)
        .map(|i| {
            let t = i as f64 / f64::from(SAMPLE_RATE);
            let phase = std::f64::consts::TAU * 150.0 * t;
            let odd = (0.5 + 0.5 * (std::f64::consts::TAU * 2.0 * t).sin()).powi(2) * 0.15;
            ((0.005 + odd * 0.2) * phase.sin()
                + 0.12 * (2.0 * phase).sin()
                + (0.01 + odd * 0.3) * (3.0 * phase).sin()
                + 0.08 * (4.0 * phase).sin()
                + (0.005 + odd * 0.1) * (5.0 * phase).sin()
                + 0.04 * (6.0 * phase).sin()) as f32
        })
        .collect();
    let mut config = vocal_frame::default_config();
    config.harmony_lead_gain = 0.0;
    let mut settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    settings.midi_frequencies[0] = 220.0;
    let output = render(&input, &[], &config, &settings);
    let region = (SAMPLE_RATE * 0.5) as usize..(SAMPLE_RATE * 2.5) as usize;
    for (label, samples, fundamental, tolerance) in [
        ("known input", &input[region.clone()], 150.0, 0.01),
        ("held harmony", &output.raw[region], 220.0, 0.05),
    ] {
        let energy = samples.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>();
        let mut captured = 0.0;
        // The known vowel modulation is periodic at 2 Hz. Include its legitimate
        // amplitude sidebands around each target harmonic, excluding unrelated
        // tones between the harmonic bands.
        for harmonic in 1..=12 {
            for sideband in -5..=5 {
                let frequency = harmonic as f64 * fundamental + sideband as f64 * 2.0;
                let angle = std::f64::consts::TAU * frequency / f64::from(SAMPLE_RATE);
                let (step_s, step_c) = angle.sin_cos();
                let (mut s, mut c, mut re, mut im) = (0.0, 1.0, 0.0, 0.0);
                for &sample in samples {
                    re += f64::from(sample) * c;
                    im += f64::from(sample) * s;
                    (s, c) = (s * step_c + c * step_s, c * step_c - s * step_s);
                }
                captured += 2.0 * (re * re + im * im) / samples.len() as f64;
            }
        }
        let outside = (1.0 - captured / energy).max(0.0);
        eprintln!(
            "changing vowel {label}: {:.2}% outside target bands",
            outside * 100.0
        );
        assert!(
            outside < tolerance,
            "{label}: vowel changes generate {:.2}% inharmonic energy",
            outside * 100.0
        );
    }
}

#[test]
fn bass_vocal_below_80_hz_stays_on_the_held_midi_note() {
    let rate = 44_100.0;
    let input: Vec<_> = (0..(rate * 2.0) as usize)
        .map(|i| {
            let phase = std::f64::consts::TAU * 73.6 * i as f64 / rate;
            (0.03 * phase.sin()
                + 0.06 * (2.0 * phase).sin()
                + 0.2 * (3.0 * phase).sin()
                + 0.08 * (4.0 * phase).sin()
                + 0.035 * (5.0 * phase).sin()) as f32
        })
        .collect();
    let source = audio_lab::estimate_pitch_at_rate(&input[10_000..18_192], rate).unwrap();
    assert!(audio_lab::cents(source, 73.6).abs() < 5.0);
    let mut config = vocal_frame::default_config();
    config.sample_rate = rate as f32;
    config.harmony_lead_gain = 0.0;
    let mut settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    settings.midi_frequencies[0] = 220.0;
    let output = render(&input, &[], &config, &settings);
    let analysis = &output.raw[22_050..66_150];
    let projection = |samples: &[f32], hz: f64| {
        let (step_s, step_c) = (std::f64::consts::TAU * hz / rate).sin_cos();
        let (mut s, mut c, mut re, mut im) = (0.0, 1.0, 0.0, 0.0);
        for &x in samples {
            re += f64::from(x) * c;
            im += f64::from(x) * s;
            (s, c) = (s * step_c + c * step_s, c * step_c - s * step_s);
        }
        2.0 * (re * re + im * im).sqrt() / samples.len() as f64
    };
    let fraction = |samples: &[f32]| {
        let energy =
            samples.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>() / samples.len() as f64;
        (1..=20)
            .map(|h| projection(samples, 220.0 * h as f64).powi(2) * 0.5)
            .sum::<f64>()
            / energy
    };
    // A dominant upper partial can make a monophonic autocorrelation oracle
    // choose a subharmonic. Independently measure the full requested grid and
    // require the target fundamental itself, with the unshifted input as a
    // negative control for this measurement.
    assert!(fraction(&input[22_050..66_150]) < 0.2);
    let on_target = fraction(analysis);
    eprintln!(
        "73.6 Hz bass -> 220 Hz: {:.3}% on target harmonic grid",
        on_target * 100.0
    );
    assert!(
        on_target > 0.95,
        "bass target retained only {:.2}% on its harmonic grid",
        on_target * 100.0
    );
    assert!(projection(analysis, 220.0) > 0.01);
}

#[test]
#[ignore = "Known unresolved low-voice overlap distortion; run explicitly to reproduce (docs/audio-testing.md)"]
fn overlapping_upper_harmonics_do_not_wobble_a_low_voices_target() {
    overlapping_upper_harmonics(false);
}

#[test]
fn a_longer_window_resolves_overlapping_low_voice_harmonics() {
    overlapping_upper_harmonics(true);
}

#[test]
fn longer_window_preserves_level_and_has_the_declared_extra_delay() {
    let mut input = vec![0.0; 8192];
    input[4096] = 0.2;
    let config = vocal_frame::default_config();
    let settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    let before = render(&input, &[], &config, &settings).raw;
    let after = audio_lab::harmony_window::render(&input, &config, &settings);
    let peak = |audio: &[f32]| {
        audio
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.abs().total_cmp(&b.abs()))
            .unwrap()
            .0
    };
    let a = peak(&before);
    let b = peak(&after);
    assert_eq!(b - a, audio_lab::harmony_window::EXTRA_DELAY);
    assert!(
        (after[b] / before[a] - 1.0).abs() < 0.002,
        "long-window overlap normalization changed unity gain"
    );
    assert!(after.iter().all(|x| x.is_finite()));
    let silence = audio_lab::harmony_window::render(&vec![0.0; 8192], &config, &settings);
    assert!(silence.iter().all(|&x| x == 0.0));
}

fn overlapping_upper_harmonics(long_window: bool) {
    let rate = 44_100.0;
    let input: Vec<_> = (0..(rate * 3.0) as usize)
        .map(|i| {
            let t = i as f64 / rate;
            let phase = std::f64::consts::TAU * 75.0 * t;
            let blend = 0.5 + 0.45 * (std::f64::consts::TAU * 2.0 * t).sin();
            (0.03 * phase.sin()
                + 0.06 * (2.0 * phase).sin()
                + 0.2 * (3.0 * phase).sin()
                + 0.08 * (4.0 * phase).sin()
                + 0.035 * (5.0 * phase).sin()
                + 0.25 * blend * (9.0 * phase).sin()
                + 0.25 * (1.0 - blend) * (10.0 * phase).sin()) as f32
        })
        .collect();
    let mut config = vocal_frame::default_config();
    config.sample_rate = rate as f32;
    config.harmony_lead_gain = 0.0;
    let mut settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    settings.midi_frequencies[0] = 350.0;
    let output = if long_window {
        audio_lab::harmony_window::render(&input, &config, &settings)
    } else {
        render(&input, &[], &config, &settings).raw
    };
    for (name, samples, fundamental) in [
        ("source control", &input[22_050..110_250], 75.0),
        ("held harmony", &output[22_050..110_250], 350.0),
    ] {
        let energy = samples.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>();
        let mut captured = 0.0;
        // Allow the source's legitimate 2 Hz amplitude-modulation sidebands.
        for h in 1..=20 {
            for side in -5..=5 {
                let hz = fundamental * h as f64 + side as f64 * 2.0;
                let (step_s, step_c) = (std::f64::consts::TAU * hz / rate).sin_cos();
                let (mut s, mut c, mut re, mut im) = (0.0, 1.0, 0.0, 0.0);
                for &x in samples {
                    re += f64::from(x) * c;
                    im += f64::from(x) * s;
                    (s, c) = (s * step_c + c * step_s, c * step_c - s * step_s);
                }
                captured += 2.0 * (re * re + im * im) / samples.len() as f64;
            }
        }
        let outside = (1.0 - captured / energy).max(0.0);
        eprintln!(
            "overlapping harmonics {name}: {:.3}% outside target bands",
            outside * 100.0
        );
        assert!(
            outside < if long_window { 0.005 } else { 0.05 },
            "{name} created {:.3}% off-grid energy",
            outside * 100.0
        );
    }
}
