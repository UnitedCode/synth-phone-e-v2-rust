//! Real-recording MIDI vocal-effects audition, including the frozen original DSP.
use audio_lab::{
    constants::{BLOCK_SIZE, FFT_SIZE, HOP_SIZE},
    metrics, output_mix,
    replay::Replay,
    vocal_carrier, vocal_frame,
    voice_generator::VoiceManager,
    wav,
};
use midly::{Format, MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};
use std::{collections::BTreeSet, error::Error, fs, path::Path};
use synthphone_e_vocal_dsp::{
    audio::{Oscillator, Waveform},
    MusicalSettings, ProcessingMode,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

struct Event {
    sample: usize,
    message: MidiMessage,
}
struct Schedule {
    events: Vec<Event>,
    samples: usize,
    report: String,
}

#[derive(Default)]
struct Options {
    preview_start: f64,
    window: Option<(f64, f64)>,
    legato: bool,
    mode: Option<ProcessingMode>,
}

fn options(args: &[String]) -> Result<Options> {
    let mut options = Options::default();
    let mut args = args.iter().peekable();
    if args.peek().is_some_and(|s| !s.starts_with("--")) {
        options.preview_start = args.next().unwrap().parse()?;
    }
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--legato" => options.legato = true,
            "--mode" => {
                options.mode = Some(match args.next().map(String::as_str) {
                    Some("vocoder") => ProcessingMode::Vocode,
                    Some("pitch") => ProcessingMode::PitchControl,
                    Some("harmony") => ProcessingMode::Harmony,
                    _ => return Err("--mode requires vocoder, pitch or harmony".into()),
                });
            }
            "--window" => {
                let start = args
                    .next()
                    .ok_or("--window requires start and duration")?
                    .parse()?;
                let duration = args.next().ok_or("--window requires duration")?.parse()?;
                options.window = Some((start, duration));
            }
            _ => return Err(format!("unknown option {flag}").into()),
        }
    }
    Ok(options)
}

fn schedule(data: &[u8], selected: usize, rate: u32) -> Result<Schedule> {
    let midi = Smf::parse(data).map_err(|e| format!("invalid MIDI: {e:?}"))?;
    if midi.header.format == Format::Sequential {
        return Err("independent sequential tracks cannot share a timeline".into());
    }
    let Timing::Metrical(ticks) = midi.header.timing else {
        return Err("SMPTE timing is unsupported".into());
    };
    if ticks.as_int() == 0 || selected >= midi.tracks.len() || rate == 0 {
        return Err("invalid timebase, track or sample rate".into());
    }
    let mut events = Vec::new();
    let mut report = format!(
        "selected_track_index={selected}\nticks_per_beat={}\n",
        ticks.as_int()
    );
    for (track, sequence) in midi.tracks.iter().enumerate() {
        let (mut tick, mut count) = (0_u64, 0);
        let mut name = String::new();
        for event in sequence {
            tick += u64::from(event.delta.as_int());
            if let TrackEventKind::Meta(MetaMessage::TrackName(text)) = event.kind {
                name = String::from_utf8_lossy(text).into_owned();
            }
            if matches!(event.kind, TrackEventKind::Midi { message: MidiMessage::NoteOn { vel, .. }, .. } if vel.as_int() > 0)
            {
                count += 1;
            }
            events.push((tick, track, event.kind));
        }
        report.push_str(&format!("track {track}: {name:?}, note_ons={count}\n"));
    }
    events.sort_by_key(|event| event.0);
    let denominator = u128::from(ticks.as_int()) * 1_000_000;
    let (mut last, mut elapsed, mut tempo) = (0_u64, 0_u128, 500_000_u128);
    let mut timed = Vec::new();
    let (mut channels, mut held) = (BTreeSet::new(), BTreeSet::new());
    let (mut maximum, mut note_count) = (0, 0);
    for (tick, track, kind) in events {
        elapsed += u128::from(tick - last) * tempo;
        last = tick;
        match kind {
            TrackEventKind::Meta(MetaMessage::Tempo(value)) => {
                if value.as_int() == 0 {
                    return Err("zero MIDI tempo".into());
                }
                tempo = u128::from(value.as_int());
            }
            TrackEventKind::Midi { channel, message }
                if track == selected && channel.as_int() != 9 =>
            {
                match message {
                    MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                        channels.insert(channel.as_int());
                        held.insert(key.as_int());
                        note_count += 1;
                    }
                    MidiMessage::NoteOn { key, .. } | MidiMessage::NoteOff { key, .. } => {
                        held.remove(&key.as_int());
                    }
                    _ => (),
                }
                maximum = maximum.max(held.len());
                timed.push(Event {
                    sample: ((elapsed * u128::from(rate) + denominator / 2) / denominator) as usize,
                    message,
                });
            }
            _ => (),
        }
    }
    if note_count == 0 || channels.len() != 1 || maximum > 8 {
        return Err(
            "choose one melodic channel/track with 1..8 simultaneous notes; drums are excluded"
                .into(),
        );
    }
    let samples = ((elapsed * u128::from(rate) + denominator / 2) / denominator) as usize;
    if samples == 0 || samples > rate as usize * 600 {
        return Err("MIDI duration must be 0..600 seconds".into());
    }
    report.push_str(&format!("duration_seconds={:.6}\nselected_note_ons={note_count}\nselected_maximum_held={maximum}\nsource_channels_zero_based={channels:?}\n", samples as f64 / rate as f64));
    Ok(Schedule {
        events: timed,
        samples,
        report,
    })
}

fn apply(manager: &mut VoiceManager<8>, message: MidiMessage) {
    // Route the selected part to the phone's vocal-control channel, never mix
    // its ordinary synth audio into the result. Original program/volume CCs
    // do not change the explicitly selected saw carrier or audition levels.
    match message {
        MidiMessage::NoteOn { key, vel } => manager.note_on(key.as_int(), vel.as_int(), 0),
        MidiMessage::NoteOff { key, .. } => manager.note_off(key.as_int(), 0),
        MidiMessage::Controller { controller, value } => match controller.as_int() {
            64 => manager.set_sustain(0, value.as_int() >= 64),
            120 => manager.all_notes_off(Some(0)),
            123 => manager.release_channel_notes(0),
            _ => (),
        },
        MidiMessage::PitchBend { bend } => manager.apply_channel_pitch_bend(0, bend.as_f32()),
        _ => (),
    }
}

fn apply_group(manager: &mut VoiceManager<8>, group: &[Event], legato: bool) {
    if !legato {
        for event in group {
            apply(manager, event.message);
        }
        return;
    }
    // Audition-only legato: keep the last chord through rests, replacing it
    // with the complete next attack group. This never edits the user's SMF.
    if group
        .iter()
        .any(|e| matches!(e.message, MidiMessage::NoteOn { vel, .. } if vel.as_int() > 0))
    {
        manager.all_notes_off(Some(0));
    }
    for event in group {
        match event.message {
            MidiMessage::NoteOn { vel, .. } if vel.as_int() > 0 => apply(manager, event.message),
            MidiMessage::PitchBend { .. } => apply(manager, event.message),
            _ => (),
        }
    }
}

fn longest_quiet_gap(samples: &[f32], rate: u32) -> (f64, f64) {
    let window = (rate as usize / 100).max(1);
    let (mut run, mut maximum, mut start, mut offset) = (0, 0, 0, 0);
    for frame in samples.chunks(window) {
        offset += frame.len();
        let energy = frame.iter().map(|&x| (x as f64).powi(2)).sum::<f64>() / frame.len() as f64;
        if energy < 1e-6 {
            run += frame.len();
        } else {
            run = 0;
        }
        if run > maximum {
            maximum = run;
            start = offset - run;
        }
    }
    (maximum as f64 / rate as f64, start as f64 / rate as f64)
}

fn pcm16(path: &Path, samples: impl IntoIterator<Item = f32>, rate: u32, gain: f64) -> Result<()> {
    let mut writer = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for sample in samples {
        let value = sample as f64 * gain;
        if !value.is_finite() || value.abs() > 1.0 {
            return Err("non-finite or clipped playback audio".into());
        }
        writer.write_sample((value * i16::MAX as f64).round() as i16)?;
    }
    writer.finalize()?;
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 4 {
        return Err("usage: midi_vocoder FILE.mid REAL_VOCAL_MONO.wav NEW_OUTPUT_DIRECTORY TRACK_INDEX [PREVIEW_START_SECONDS] [--window MIDI_START_SECONDS DURATION_SECONDS] [--legato] [--mode vocoder|pitch|harmony]".into());
    }
    let options = options(&args[4..])?;
    let source = wav::read(Path::new(&args[1]))?;
    let rate = source.sample_rate;
    let midi = schedule(&fs::read(&args[0])?, args[3].parse()?, rate)?;
    let preview_start = options.preview_start;
    let (window_start, duration) = options
        .window
        .unwrap_or((0.0, midi.samples as f64 / rate as f64));
    if !window_start.is_finite() || window_start < 0.0 || !duration.is_finite() || duration <= 0.0 {
        return Err("invalid MIDI window".into());
    }
    let window_start = (window_start * rate as f64).round() as usize;
    let duration = (duration * rate as f64).round() as usize;
    let window_end = window_start
        .checked_add(duration)
        .ok_or("MIDI window overflow")?;
    if duration == 0 || window_end > midi.samples {
        return Err("window extends beyond MIDI timeline".into());
    }
    if !preview_start.is_finite()
        || preview_start < 0.0
        || preview_start * rate as f64 >= duration as f64
    {
        return Err("invalid preview start".into());
    }
    if source.samples.len() < duration {
        return Err("use a real vocal recording at least as long as the MIDI window; no automatic looping or synthetic replacement".into());
    }
    let out = Path::new(&args[2]);
    fs::create_dir(out)?;
    let (mut before, mut after) = (Replay::new(), Replay::new());
    let mut manager = VoiceManager::<8>::new(rate as f32);
    if options.legato {
        let first = midi
            .events
            .iter()
            .position(|e| matches!(e.message, MidiMessage::NoteOn { vel, .. } if vel.as_int() > 0))
            .ok_or("no carrier notes")?;
        let end = first
            + midi.events[first..]
                .iter()
                .take_while(|e| e.sample == midi.events[first].sample)
                .count();
        apply_group(&mut manager, &midi.events[first..end], true);
    }
    let mut oscillators =
        core::array::from_fn(|_| Oscillator::new(440.0, rate as f32, Waveform::Saw));
    let mut config = vocal_frame::config_for_controls(10, 10);
    config.sample_rate = rate as f32;
    config.harmony_lead_gain = 0.0;
    let mut settings = MusicalSettings {
        mode: options.mode.unwrap_or(ProcessingMode::Vocode),
        ..Default::default()
    };
    let baseline_config = baseline_vocal_dsp::VocalEffectsConfig {
        sample_rate: rate as f32,
        transition_speed: config.transition_speed,
        pitch_correction_strength: config.pitch_correction_strength,
        harmony_lead_gain: config.harmony_lead_gain,
        ..Default::default()
    };
    let mut baseline_settings = baseline_vocal_dsp::MusicalSettings {
        mode: match settings.mode {
            ProcessingMode::PitchControl => baseline_vocal_dsp::ProcessingMode::PitchControl,
            ProcessingMode::Harmony => baseline_vocal_dsp::ProcessingMode::Harmony,
            _ => baseline_vocal_dsp::ProcessingMode::Vocode,
        },
        ..Default::default()
    };
    let mut baseline_state = baseline_vocal_dsp::VocalStreamState::new();
    let total = (window_end + FFT_SIZE * 2).div_ceil(BLOCK_SIZE) * BLOCK_SIZE;
    let (mut old, mut new, mut carrier_audio) = (
        Vec::with_capacity(total),
        Vec::with_capacity(total),
        vec![0.0; total],
    );
    let mut next = 0;
    for block in (0..total).step_by(BLOCK_SIZE) {
        for i in block..block + BLOCK_SIZE {
            while next < midi.events.len() && midi.events[next].sample <= i {
                let end = next
                    + midi.events[next..]
                        .iter()
                        .take_while(|e| e.sample == midi.events[next].sample)
                        .count();
                apply_group(&mut manager, &midi.events[next..end], options.legato);
                next = end;
            }
            let input = if (window_start..window_end).contains(&i) {
                source.samples[i - window_start]
            } else {
                0.0
            };
            old.push(before.sample(input));
            new.push(after.sample(input));
        }
        if after.pending() {
            let (frequencies, gains) = manager.get_harmony_voices(64);
            settings.midi_frequencies = frequencies;
            settings.midi_gains = gains;
            baseline_settings.midi_frequencies = frequencies;
            baseline_settings.midi_gains = gains;
            let mut carrier = [0.0; HOP_SIZE];
            vocal_carrier::render_midi_hop(
                &mut oscillators,
                &frequencies,
                &gains,
                Waveform::Saw,
                &mut carrier,
            );
            let position = after.hops * HOP_SIZE;
            let count = HOP_SIZE.min(total - position);
            carrier_audio[position..position + count].copy_from_slice(&carrier[..count]);
            before.finish_block_with_kernel(
                &carrier,
                None,
                |input, carrier, input_phases, output_phases, envelope, inv_envelope, _| {
                    baseline_vocal_dsp::process_vocal_effects_1024_stream(
                        input,
                        Some(carrier),
                        input_phases,
                        output_phases,
                        envelope,
                        inv_envelope,
                        &mut baseline_state,
                        &baseline_config,
                        &baseline_settings,
                    )
                },
            );
            after.finish_block(&carrier, &config, &settings, None);
        }
    }
    old.drain(..window_start);
    new.drain(..window_start);
    carrier_audio.drain(..window_start);
    // These blends combine the two effect revisions, with no added dry voice.
    let blends: Vec<_> = [0.8_f32, 0.9]
        .into_iter()
        .map(|mix| {
            old.iter()
                .zip(&new)
                .map(|(&a, &b)| output_mix::finish((a * (1.0 - mix) + b * mix) * 0.75, 1.0))
                .collect::<Vec<_>>()
        })
        .collect();
    for samples in [&mut old, &mut new] {
        for sample in samples {
            *sample = output_mix::finish(*sample * 0.75, 1.0);
        }
    }
    let mut versions = vec![("original", old), ("updated", new)];
    versions.extend(["new80-old20", "new90-old10"].into_iter().zip(blends));
    let start = (preview_start * rate as f64).round() as usize;
    let end = (start + rate as usize * 10).min(duration);
    let mut common_rms = 0.1_f64;
    for (_, audio) in &versions {
        let m = metrics(&audio[start..end]);
        if m.non_finite != 0 || m.rms < 1e-7 {
            return Err("inaudible/non-finite preview; choose a different MIDI interval".into());
        }
        common_rms = common_rms.min(0.9 * m.rms / m.peak as f64);
    }
    let mut report = format!("midi={}\nvocal={}\n{}sample_rate={rate}\npreview_start={preview_start}\npreview_end={}\ncommon_playback_rms={common_rms}\n", args[0], args[1], midi.report, end as f64 / rate as f64);
    report.push_str(&format!(
        "mode={:?}\nconfig={config:?}\nmidi_window_start={}\nwindow_duration={}\nlegato_audition={}\n",
        settings.mode,
        window_start as f64 / rate as f64,
        duration as f64 / rate as f64,
        options.legato
    ));
    if options.legato {
        report.push_str("Audition modification: note releases/rests are held through the next attack; no SMF or device policy change.\n");
    }
    report.push_str("Quiet-gap check: 10ms RMS windows below 0.001 (-60 dBFS), excluding 2048 startup samples and the DSP tail. Legato auditions reject gaps over 0.2 seconds. This measures audio coverage, not semantic singing recognition.\n");
    report.push_str(&format!(
        "input_longest_quiet_gap_seconds={}\n",
        longest_quiet_gap(&source.samples[..duration], rate).0
    ));
    for prefix in ["synthphone-e-vocal-dsp =", "baseline-vocal-dsp ="] {
        let dependency = include_str!("../Cargo.toml")
            .lines()
            .find(|line| line.starts_with(prefix))
            .ok_or("missing DSP pin")?;
        report.push_str(&format!("dependency: {dependency}\n"));
    }
    report.push_str("Original and updated kernels use identical MIDI targets/carriers, input, configuration and buffering.\nNo added dry-voice blend; Harmony uses MIDI voices only.\n80/20 and 90/10 blend NEW/OLD effect output.\nThe source vocal is independent of the MIDI; this does not reproduce the MIDI song's singer or lyrics.\nHost DSP replay, not a physical-board capture or a verified melody transcription.\n");
    let difference_rms = (versions[0]
        .1
        .iter()
        .zip(&versions[1].1)
        .map(|(&a, &b)| (f64::from(a) - f64::from(b)).powi(2))
        .sum::<f64>()
        / versions[0].1.len() as f64)
        .sqrt();
    report.push_str(&format!("before_after_difference_rms={difference_rms}\n"));
    let mut gains = Vec::new();
    for (name, audio) in &versions {
        let m = metrics(&audio[start..end]);
        let gain = common_rms / m.rms;
        gains.push(gain);
        wav::write(&out.join(format!("{name}-unmatched.wav")), audio, rate)?;
        pcm16(
            &out.join(format!("{name}-preview.wav")),
            audio[start..end].iter().copied(),
            rate,
            gain,
        )?;
        let steady = &audio[(FFT_SIZE * 2).min(duration)..duration];
        let (gap, gap_start) = longest_quiet_gap(steady, rate);
        report.push_str(&format!("{name}: preview={m:?}; playback_gain={gain}; full={:?}; longest_quiet_gap_seconds={gap}\n", metrics(audio)));
        if options.legato && gap > 0.2 {
            return Err(format!(
                "{name}: continuous audition has a {gap:.3}s quiet gap at {:.3}s",
                gap_start + FFT_SIZE as f64 * 2.0 / rate as f64
            )
            .into());
        }
    }
    // Full-song listening files share one constant gain to preserve dynamics.
    let full_gain = 0.8
        / versions[..2]
            .iter()
            .map(|(_, audio)| metrics(audio).peak as f64)
            .fold(0.0, f64::max)
            .max(1e-9);
    report.push_str(&format!("common_full_playback_gain={full_gain}\n"));
    for (name, audio) in &versions[..2] {
        pcm16(
            &out.join(format!("{name}-full.wav")),
            audio.iter().copied(),
            rate,
            full_gain,
        )?;
    }
    let a = versions[0].1[start..end]
        .iter()
        .map(|&x| x * gains[0] as f32);
    let b = versions[1].1[start..end]
        .iter()
        .map(|&x| x * gains[1] as f32);
    pcm16(
        &out.join("before-after-preview.wav"),
        a.chain(std::iter::repeat_n(0.0, rate as usize * 3 / 5))
            .chain(b),
        rate,
        1.0,
    )?;
    let m = metrics(&carrier_audio[start..end]);
    pcm16(
        &out.join("midi-carrier-preview.wav"),
        carrier_audio[start..end].iter().copied(),
        rate,
        (0.1 / m.rms).min(0.9 / m.peak as f64),
    )?;
    fs::write(out.join("provenance.txt"), &report)?;
    println!(
        "{}\nWrote MIDI vocal-effects audition and matched previews to {}",
        midi.report,
        out.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use midly::{Header, TrackEvent};
    fn fixture(channel: u8) -> Vec<u8> {
        let midi = Smf {
            header: Header::new(Format::Parallel, Timing::Metrical(480.into())),
            tracks: vec![
                vec![
                    TrackEvent {
                        delta: 0.into(),
                        kind: TrackEventKind::Meta(MetaMessage::Tempo(500000.into())),
                    },
                    TrackEvent {
                        delta: 480.into(),
                        kind: TrackEventKind::Meta(MetaMessage::Tempo(250000.into())),
                    },
                ],
                vec![
                    TrackEvent {
                        delta: 240.into(),
                        kind: TrackEventKind::Midi {
                            channel: channel.into(),
                            message: MidiMessage::NoteOn {
                                key: 60.into(),
                                vel: 100.into(),
                            },
                        },
                    },
                    TrackEvent {
                        delta: 480.into(),
                        kind: TrackEventKind::Midi {
                            channel: channel.into(),
                            message: MidiMessage::NoteOff {
                                key: 60.into(),
                                vel: 0.into(),
                            },
                        },
                    },
                ],
            ],
        };
        let mut data = Vec::new();
        midi.write(&mut data).unwrap();
        data
    }
    #[test]
    fn tempo_changes_on_other_tracks_control_selected_note_timing() {
        let midi = schedule(&fixture(0), 1, 44100).unwrap();
        assert_eq!(midi.events[0].sample, 11025);
        assert_eq!(midi.events[1].sample, 27563);
        assert_eq!(midi.samples, 27563);
    }
    #[test]
    fn percussion_cannot_be_selected_as_the_vocal_carrier() {
        assert!(schedule(&fixture(9), 1, 44100).is_err());
    }

    #[test]
    fn legato_holds_releases_and_replaces_the_entire_next_chord() {
        let event = |key, on| Event {
            sample: 0,
            message: if on {
                MidiMessage::NoteOn {
                    key: midly::num::u7::new(key),
                    vel: 100.into(),
                }
            } else {
                MidiMessage::NoteOff {
                    key: midly::num::u7::new(key),
                    vel: 0.into(),
                }
            },
        };
        let mut manager = VoiceManager::<8>::new(44100.0);
        apply_group(&mut manager, &[event(60, true)], true);
        let held = manager.get_cached_frequencies();
        apply_group(&mut manager, &[event(60, false)], true);
        assert_eq!(manager.get_cached_frequencies(), held);
        apply_group(&mut manager, &[event(64, true), event(67, true)], true);
        let frequencies = manager.get_cached_frequencies();
        assert_eq!(frequencies.iter().filter(|&&f| f > 0.0).count(), 2);
        assert!(!frequencies.contains(&held[0]));
        apply_group(&mut manager, &[event(64, false), event(67, false)], false);
        assert!(manager.get_cached_frequencies().iter().all(|&f| f == 0.0));
    }

    #[test]
    fn quiet_gap_measurement_detects_a_long_rest_without_counting_zero_crossings() {
        let mut audio: Vec<_> = (0..44100).map(|i| (i as f32 * 0.1).sin() * 0.1).collect();
        assert_eq!(longest_quiet_gap(&audio, 44100).0, 0.0);
        audio[8820..22050].fill(0.0);
        assert!((longest_quiet_gap(&audio, 44100).0 - 0.3).abs() < 0.011);
    }
}
