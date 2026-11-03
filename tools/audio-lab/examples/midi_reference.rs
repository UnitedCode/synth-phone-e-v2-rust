//! Inspect an external SMF without storing the music in the repository.
use midly::{MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};
use std::{collections::BTreeMap, error::Error};

/// Merge vocal notes, preserving overlapping unisons across tracks/channels.
/// Instrument programs, controllers and bends are deliberately excluded: their
/// original multi-channel meanings cannot be applied to one vocal channel.
fn vocal_midi(
    events: &[(u64, usize, TrackEventKind<'_>)],
    timing: Timing,
) -> Result<Vec<u8>, Box<dyn Error>> {
    use midly::{Format, Header, TrackEvent};
    let mut held = BTreeMap::<(usize, u8, u8), ()>::new();
    let mut track = Vec::new();
    let mut last = 0_u64;
    for &(tick, part, kind) in events {
        let kind = match kind {
            TrackEventKind::Meta(
                MetaMessage::Tempo(_)
                | MetaMessage::TimeSignature(..)
                | MetaMessage::KeySignature(..),
            ) => Some(kind),
            TrackEventKind::Midi { channel, message } => {
                let ch = channel.as_int();
                let (key, velocity, on) = match message {
                    MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                        (key, vel.as_int(), true)
                    }
                    MidiMessage::NoteOn { key, .. } | MidiMessage::NoteOff { key, .. } => {
                        (key, 0, false)
                    }
                    _ => continue,
                };
                let k = key.as_int();
                let was_held = held.keys().any(|&(_, _, note)| note == k);
                if on {
                    held.insert((part, ch, k), ());
                } else {
                    held.remove(&(part, ch, k));
                }
                let is_held = held.keys().any(|&(_, _, note)| note == k);
                if was_held == is_held {
                    None
                } else {
                    Some(TrackEventKind::Midi {
                        channel: 0.into(),
                        message: if is_held {
                            MidiMessage::NoteOn {
                                key,
                                vel: velocity.into(),
                            }
                        } else {
                            MidiMessage::NoteOff { key, vel: 0.into() }
                        },
                    })
                }
            }
            _ => None,
        };
        if let Some(kind) = kind {
            let delta = u32::try_from(tick - last).map_err(|_| "MIDI delta too large")?;
            if delta > 0x0fff_ffff {
                return Err("MIDI delta exceeds SMF limit".into());
            }
            track.push(TrackEvent {
                delta: delta.into(),
                kind,
            });
            last = tick;
        }
    }
    if !held.is_empty() {
        return Err("input has notes without NoteOff; refusing a stuck-note export".into());
    }
    let end = events.last().map_or(last, |e| e.0);
    let delta = u32::try_from(end - last).map_err(|_| "MIDI end delta too large")?;
    if delta > 0x0fff_ffff {
        return Err("MIDI end delta exceeds SMF limit".into());
    }
    track.push(TrackEvent {
        delta: delta.into(),
        kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
    });
    let merged = Smf {
        header: Header::new(Format::SingleTrack, timing),
        tracks: vec![track],
    };
    let mut bytes = Vec::new();
    merged
        .write(&mut bytes)
        .map_err(|e| format!("MIDI write failed: {e:?}"))?;
    Ok(bytes)
}

/// Diagnostic reduction, deliberately separate from the device's MIDI policy.
/// Recompute after simultaneous events so selecting the highest held notes does
/// not emit transient notes from intermediate states within the same tick.
fn select_vocal_midi(
    bytes: &[u8],
    low: u8,
    high: u8,
    maximum: usize,
    skip_highest: usize,
) -> Result<Vec<u8>, Box<dyn Error>> {
    use midly::{Format, Header, TrackEvent};
    if low > high || high > 127 || !(1..=8).contains(&maximum) {
        return Err("require MIDI range 0..127 and voice limit 1..8".into());
    }
    let smf = Smf::parse(bytes).map_err(|e| format!("invalid merged MIDI: {e:?}"))?;
    let mut timed = Vec::new();
    let mut tick = 0_u64;
    for event in &smf.tracks[0] {
        tick += u64::from(event.delta.as_int());
        timed.push((tick, event.kind));
    }
    let (mut active, mut selected) = (BTreeMap::<u8, u8>::new(), BTreeMap::<u8, u8>::new());
    let (mut track, mut last, mut position) = (Vec::new(), 0_u64, 0);
    while position < timed.len() {
        let tick = timed[position].0;
        let mut emit = |kind| -> Result<(), Box<dyn Error>> {
            let delta = u32::try_from(tick - last)?;
            if delta > 0x0fff_ffff {
                return Err("MIDI delta exceeds SMF limit".into());
            }
            track.push(TrackEvent {
                delta: delta.into(),
                kind,
            });
            last = tick;
            Ok(())
        };
        while position < timed.len() && timed[position].0 == tick {
            match timed[position].1 {
                TrackEventKind::Midi {
                    message: MidiMessage::NoteOn { key, vel },
                    ..
                } if vel.as_int() > 0 => {
                    active.insert(key.as_int(), vel.as_int());
                }
                TrackEventKind::Midi {
                    message: MidiMessage::NoteOn { key, .. } | MidiMessage::NoteOff { key, .. },
                    ..
                } => {
                    active.remove(&key.as_int());
                }
                TrackEventKind::Meta(MetaMessage::EndOfTrack) => (),
                kind => emit(kind)?,
            }
            position += 1;
        }
        if skip_highest > 0 && active.len() > 8 {
            return Err("split preview supports up to eight simultaneous notes; refusing to lose backing notes".into());
        }
        let next: BTreeMap<_, _> = active
            .range(low..=high)
            .rev()
            .skip(skip_highest)
            .take(maximum)
            .map(|(&key, &vel)| (key, vel))
            .collect();
        for &key in selected.keys().filter(|key| !next.contains_key(key)) {
            emit(TrackEventKind::Midi {
                channel: 0.into(),
                message: MidiMessage::NoteOff {
                    key: key.into(),
                    vel: 0.into(),
                },
            })?;
        }
        for (&key, &vel) in next.iter().filter(|(key, _)| !selected.contains_key(key)) {
            emit(TrackEventKind::Midi {
                channel: 0.into(),
                message: MidiMessage::NoteOn {
                    key: key.into(),
                    vel: vel.into(),
                },
            })?;
        }
        selected = next;
    }
    let end_delta = u32::try_from(timed.last().map_or(last, |event| event.0) - last)?;
    if end_delta > 0x0fff_ffff {
        return Err("MIDI end delta exceeds SMF limit".into());
    }
    track.push(TrackEvent {
        delta: end_delta.into(),
        kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
    });
    let reduced = Smf {
        header: Header::new(Format::SingleTrack, smf.header.timing),
        tracks: vec![track],
    };
    let mut result = Vec::new();
    reduced
        .write(&mut result)
        .map_err(|e| format!("MIDI write failed: {e:?}"))?;
    Ok(result)
}

fn render_reference(
    events: &[(f64, u8, MidiMessage)],
    song_seconds: f64,
    args: &[String],
) -> Result<(), Box<dyn Error>> {
    use audio_lab::{
        constants::{BLOCK_SIZE, HOP_SIZE, SAMPLE_RATE},
        replay::Replay,
        vocal_frame,
        voice_generator::VoiceManager,
    };
    use synthphone_e_vocal_dsp::{dsp::normalize_sample, MusicalSettings, ProcessingMode};
    let [source, output, start, duration] = args else {
        return Err(
            "optional rendering: VOCAL.wav OUTPUT.wav START_SECONDS DURATION_SECONDS".into(),
        );
    };
    let start: f64 = start.parse()?;
    let duration: f64 = if duration == "full" {
        song_seconds - start
    } else {
        duration.parse()?
    };
    if !start.is_finite()
        || start < 0.0
        || !duration.is_finite()
        || !(0.1..=600.0).contains(&duration)
    {
        return Err("invalid clip interval (duration 0.1..600 seconds, or full)".into());
    }
    let reader = hound::WavReader::open(source)?;
    let spec = reader.spec();
    if spec.channels != 1
        || spec.sample_rate != SAMPLE_RATE as u32
        || spec.sample_format != hound::SampleFormat::Float
    {
        return Err("use mono 48014 Hz float vocals".into());
    }
    let source = reader
        .into_samples::<f32>()
        .collect::<Result<Vec<_>, _>>()?;
    if source.len() < 1024 || source.iter().any(|s| !s.is_finite()) {
        return Err("invalid vocal recording".into());
    }
    let mut writer = hound::WavWriter::create(output, spec)?;
    let mut manager = VoiceManager::<8>::new(SAMPLE_RATE);
    let mut replay = Replay::new();
    let mut config = vocal_frame::default_config();
    config.harmony_lead_gain = 0.0;
    let mut settings = MusicalSettings {
        mode: ProcessingMode::Harmony,
        ..Default::default()
    };
    let mut next = 0;
    let n = (duration * SAMPLE_RATE as f64) as usize;
    for block in (0..n).step_by(BLOCK_SIZE) {
        for i in block..(block + BLOCK_SIZE).min(n) {
            let time = start + i as f64 / SAMPLE_RATE as f64;
            while next < events.len() && events[next].0 <= time {
                let (_, channel, message) = events[next];
                match message {
                    MidiMessage::NoteOn { key, vel } => {
                        manager.note_on(key.as_int(), vel.as_int(), channel)
                    }
                    MidiMessage::NoteOff { key, .. } => manager.note_off(key.as_int(), channel),
                    MidiMessage::Controller { controller, value } => match controller.as_int() {
                        64 => manager.set_sustain(channel, value.as_int() >= 64),
                        120 => manager.all_notes_off(Some(channel)),
                        123 => manager.release_channel_notes(channel),
                        _ => (),
                    },
                    MidiMessage::PitchBend { bend } => {
                        manager.apply_channel_pitch_bend(channel, bend.as_f32())
                    }
                    _ => (),
                }
                next += 1;
            }
            // A deliberately repeated vowel with ten-ms edge fades, not a
            // recording of the song's singer or an aligned lyric performance.
            let j = i % source.len();
            let fade = (j.min(source.len() - 1 - j) as f32 / (SAMPLE_RATE * 0.01)).min(1.0);
            let sample = source[j] * fade * 0.35;
            writer.write_sample(normalize_sample(replay.sample(sample), 0.8))?;
        }
        settings.midi_frequencies = manager.get_cached_frequencies();
        replay.finish_block(&[0.0; HOP_SIZE], &config, &settings, None);
    }
    writer.finalize()?;
    eprintln!("host DSP preview: source vowel looped; MIDI timing preserved; clip {start}..{} seconds; not a production ARM capture",start+duration);
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let path = args.first().ok_or("usage: midi_reference FILE.mid")?;
    let data = std::fs::read(path)?;
    let smf = Smf::parse(&data).map_err(|e| format!("invalid MIDI: {e:?}"))?;
    let Timing::Metrical(ticks) = smf.header.timing else {
        return Err("SMPTE timing unsupported".into());
    };
    if ticks.as_int() == 0 {
        return Err("MIDI ticks per beat must be positive".into());
    }
    let mut events = Vec::new();
    let mut tracks = Vec::new();
    for (track, sequence) in smf.tracks.iter().enumerate() {
        let mut tick = 0_u64;
        let mut count = 0;
        for event in sequence {
            tick += u64::from(event.delta.as_int());
            if let TrackEventKind::Meta(MetaMessage::TrackName(name)) = event.kind {
                eprintln!("track {track}: {}", String::from_utf8_lossy(name));
            }
            if matches!(event.kind, TrackEventKind::Midi {message: MidiMessage::NoteOn {vel, ..}, ..} if vel.as_int() > 0)
            {
                count += 1;
            }
            events.push((tick, track, event.kind));
        }
        tracks.push(count);
    }
    events.sort_by_key(|e| e.0);
    if args.get(1).is_some_and(|arg| arg == "--split-upper-voice") {
        if args.len() != 4 {
            return Err(
                "usage: midi_reference INPUT.mid --split-upper-voice UPPER.mid BACKING.mid".into(),
            );
        }
        if args[2] == args[3]
            || std::path::Path::new(&args[2]).exists()
            || std::path::Path::new(&args[3]).exists()
        {
            return Err("choose two distinct new output files".into());
        }
        let merged = vocal_midi(&events, smf.header.timing)?;
        let upper = select_vocal_midi(&merged, 0, 127, 1, 0)?;
        let backing = select_vocal_midi(&merged, 0, 127, 8, 1)?;
        std::fs::write(&args[2], upper)?;
        std::fs::write(&args[3], backing)?;
        eprintln!("split highest held note from its complementary backing; this does not identify a verified melody");
        return Ok(());
    }
    if args.get(1).is_some_and(|arg| arg == "--select-vocal-midi") {
        if args.len() != 6 {
            return Err("usage: midi_reference INPUT.mid --select-vocal-midi OUTPUT.mid LOW_KEY HIGH_KEY MAX_VOICES".into());
        }
        if std::path::Path::new(&args[2]).exists() {
            return Err("output already exists".into());
        }
        let merged = vocal_midi(&events, smf.header.timing)?;
        let selected = select_vocal_midi(
            &merged,
            args[3].parse()?,
            args[4].parse()?,
            args[5].parse()?,
            0,
        )?;
        std::fs::write(&args[2], selected)?;
        eprintln!("exported diagnostic note selection; highest held notes take priority");
        return Ok(());
    }
    let export = args.get(1).is_some_and(|arg| arg == "--merge-vocal-midi");
    if export {
        if args.len() != 3 {
            return Err("usage: midi_reference INPUT.mid --merge-vocal-midi OUTPUT.mid".into());
        }
        if std::path::Path::new(&args[2]).exists() {
            return Err("output already exists; choose a new file".into());
        }
        std::fs::write(&args[2], vocal_midi(&events, smf.header.timing)?)?;
        eprintln!(
            "exported notes and tempo to MIDI channel 1; excluded programs, controllers and bends"
        );
    }
    let (mut last, mut micros, mut tempo) = (0, 0_f64, 500_000_f64);
    let mut held = BTreeMap::new();
    let mut maximum = 0;
    let mut channels = BTreeMap::new();
    let mut timed_midi = Vec::new();
    println!("seconds,channel,event,key,value,held_notes");
    for (tick, _, kind) in events {
        micros += (tick - last) as f64 * tempo / f64::from(ticks.as_int());
        last = tick;
        match kind {
            TrackEventKind::Meta(MetaMessage::Tempo(t)) => tempo = f64::from(t.as_int()),
            TrackEventKind::Midi { channel, message } => {
                let ch = channel.as_int();
                timed_midi.push((micros / 1e6, ch, message));
                let (event, key, value) = match message {
                    MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                        held.insert((ch, key.as_int()), ());
                        *channels.entry(ch + 1).or_insert(0_usize) += 1;
                        ("on", key.as_int(), vel.as_int())
                    }
                    MidiMessage::NoteOff { key, .. } | MidiMessage::NoteOn { key, .. } => {
                        held.remove(&(ch, key.as_int()));
                        ("off", key.as_int(), 0)
                    }
                    MidiMessage::Controller { controller, value } => {
                        ("cc", controller.as_int(), value.as_int())
                    }
                    _ => continue,
                };
                maximum = maximum.max(held.len());
                println!(
                    "{:.6},{},{event},{key},{value},{}",
                    micros / 1e6,
                    ch + 1,
                    held.len()
                );
            }
            _ => (),
        }
    }
    eprintln!("tracks={} note_counts={tracks:?} channels={channels:?} maximum_held={maximum} duration_seconds={:.3} unreleased={}", smf.tracks.len(), micros/1e6, held.len());
    if args.len() > 1 && !export {
        render_reference(&timed_midi, micros / 1e6, &args[1..])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn upper_and_backing_are_disjoint_and_preserve_all_held_notes() {
        use std::collections::BTreeSet;
        let note = |key: u8, on| TrackEventKind::Midi {
            channel: 0.into(),
            message: if on {
                MidiMessage::NoteOn {
                    key: key.into(),
                    vel: 90.into(),
                }
            } else {
                MidiMessage::NoteOff {
                    key: key.into(),
                    vel: 0.into(),
                }
            },
        };
        let events = vec![
            (0, 0, note(48, true)),
            (0, 0, note(60, true)),
            (240, 0, note(64, true)),
            (480, 0, note(64, false)),
            (720, 0, note(60, false)),
            (960, 0, note(48, false)),
            (1200, 0, TrackEventKind::Meta(MetaMessage::EndOfTrack)),
        ];
        let original = vocal_midi(&events, Timing::Metrical(480.into())).unwrap();
        let upper = select_vocal_midi(&original, 0, 127, 1, 0).unwrap();
        let backing = select_vocal_midi(&original, 0, 127, 8, 1).unwrap();
        let snapshot = |bytes: &[u8], until: u64| {
            let midi = Smf::parse(bytes).unwrap();
            let (mut tick, mut held) = (0_u64, BTreeSet::new());
            for event in &midi.tracks[0] {
                tick += u64::from(event.delta.as_int());
                if tick > until {
                    break;
                }
                match event.kind {
                    TrackEventKind::Midi {
                        message: MidiMessage::NoteOn { key, vel },
                        ..
                    } if vel.as_int() > 0 => {
                        held.insert(key.as_int());
                    }
                    TrackEventKind::Midi {
                        message: MidiMessage::NoteOff { key, .. } | MidiMessage::NoteOn { key, .. },
                        ..
                    } => {
                        held.remove(&key.as_int());
                    }
                    _ => (),
                }
            }
            held
        };
        for tick in [0, 240, 480, 720, 960, 1200] {
            let all = snapshot(&original, tick);
            let lead = snapshot(&upper, tick);
            let back = snapshot(&backing, tick);
            assert!(lead.is_disjoint(&back));
            assert_eq!(lead.union(&back).copied().collect::<BTreeSet<_>>(), all);
            assert_eq!(lead.last(), all.last());
            assert!(lead.len() <= 1);
        }
        for bytes in [&upper, &backing] {
            assert_eq!(
                Smf::parse(bytes).unwrap().tracks[0]
                    .iter()
                    .map(|e| e.delta.as_int())
                    .sum::<u32>(),
                1200
            );
        }
        let mut crowded=Vec::new();
        for key in 48..57 {crowded.push((0,0,note(key,true)));}
        for key in 48..57 {crowded.push((480,0,note(key,false)));}
        let merged=vocal_midi(&crowded,Timing::Metrical(480.into())).unwrap();
        assert!(select_vocal_midi(&merged,0,127,8,1).is_err(),"never silently drop a backing note in a split preview");
    }
    #[test]
    fn diagnostic_selection_preserves_timing_and_restores_held_lower_notes() {
        let note = |key: u8, on| TrackEventKind::Midi {
            channel: 0.into(),
            message: if on {
                MidiMessage::NoteOn {
                    key: key.into(),
                    vel: 100.into(),
                }
            } else {
                MidiMessage::NoteOff {
                    key: key.into(),
                    vel: 0.into(),
                }
            },
        };
        let events = vec![
            (
                0,
                0,
                TrackEventKind::Meta(MetaMessage::Tempo(500_000.into())),
            ),
            (0, 0, note(36, true)),
            (0, 0, note(57, true)),
            (0, 0, note(60, true)),
            (0, 0, note(64, true)),
            (240, 0, note(64, false)),
            (480, 0, note(36, false)),
            (480, 0, note(57, false)),
            (480, 0, note(60, false)),
            (960, 0, TrackEventKind::Meta(MetaMessage::EndOfTrack)),
        ];
        let merged = vocal_midi(&events, Timing::Metrical(480.into())).unwrap();
        let bytes = select_vocal_midi(&merged, 48, 72, 2, 0).unwrap();
        let selected = Smf::parse(&bytes).unwrap();
        let mut tick = 0;
        let mut notes = Vec::new();
        for event in &selected.tracks[0] {
            tick += event.delta.as_int();
            if let TrackEventKind::Midi { channel, message } = event.kind {
                assert_eq!(channel.as_int(), 0);
                match message {
                    MidiMessage::NoteOn { key, .. } => notes.push((tick, key.as_int(), true)),
                    MidiMessage::NoteOff { key, .. } => notes.push((tick, key.as_int(), false)),
                    other => panic!("unexpected selected event: {other:?}"),
                }
            }
        }
        assert_eq!(
            notes,
            [
                (0, 60, true),
                (0, 64, true),
                (240, 64, false),
                (240, 57, true),
                (480, 57, false),
                (480, 60, false)
            ]
        );
        assert_eq!(tick, 960, "retain the original final silence");
        let range_only = select_vocal_midi(&merged, 48, 72, 8, 0).unwrap();
        let smf = Smf::parse(&range_only).unwrap();
        let keys: Vec<_> = smf.tracks[0]
            .iter()
            .filter_map(|e| match e.kind {
                TrackEventKind::Midi {
                    message: MidiMessage::NoteOn { key, .. },
                    ..
                } => Some(key.as_int()),
                _ => None,
            })
            .collect();
        assert_eq!(keys, [57, 60, 64]);
    }
    #[test]
    fn merged_unison_keeps_sounding_until_all_vocal_parts_release() {
        let note = |channel, on| TrackEventKind::Midi {
            channel: midly::num::u4::new(channel),
            message: if on {
                MidiMessage::NoteOn {
                    key: 60.into(),
                    vel: 80.into(),
                }
            } else {
                MidiMessage::NoteOff {
                    key: 60.into(),
                    vel: 0.into(),
                }
            },
        };
        let events = vec![
            (
                0,
                0,
                TrackEventKind::Meta(MetaMessage::Tempo(500_000.into())),
            ),
            (0, 0, note(0, true)),
            (
                0,
                0,
                TrackEventKind::Midi {
                    channel: 0.into(),
                    message: MidiMessage::Controller {
                        controller: 7.into(),
                        value: 127.into(),
                    },
                },
            ),
            (240, 1, note(1, true)),
            (480, 0, note(0, false)),
            (720, 1, note(1, false)),
        ];
        let bytes = vocal_midi(&events, Timing::Metrical(480.into())).unwrap();
        let smf = Smf::parse(&bytes).unwrap();
        assert_eq!(smf.tracks.len(), 1);
        let mut tick = 0;
        let mut notes = Vec::new();
        for event in &smf.tracks[0] {
            tick += event.delta.as_int();
            if let TrackEventKind::Midi { channel, message } = event.kind {
                assert_eq!(channel.as_int(), 0);
                match message {
                    MidiMessage::NoteOn { .. } => notes.push((tick, true)),
                    MidiMessage::NoteOff { .. } => notes.push((tick, false)),
                    _ => panic!("unexpected instrument setup in vocal export"),
                }
            }
        }
        assert_eq!(notes, vec![(0, true), (720, false)]);
        assert!(smf.tracks[0].iter().any(
            |e| matches!(e.kind,TrackEventKind::Meta(MetaMessage::Tempo(t)) if t.as_int()==500_000)
        ));
    }

    #[test]
    fn repeated_note_on_in_one_part_retriggers_instead_of_creating_a_stuck_note() {
        let message = |on| TrackEventKind::Midi {
            channel: 0.into(),
            message: if on {
                MidiMessage::NoteOn {
                    key: 60.into(),
                    vel: 80.into(),
                }
            } else {
                MidiMessage::NoteOff {
                    key: 60.into(),
                    vel: 0.into(),
                }
            },
        };
        let events = [
            (0, 0, message(true)),
            (240, 0, message(true)),
            (480, 0, message(false)),
        ];
        let bytes = vocal_midi(&events, Timing::Metrical(480.into()))
            .expect("retrigger has a matching release");
        let smf = Smf::parse(&bytes).unwrap();
        let notes: Vec<_> = smf.tracks[0]
            .iter()
            .filter(|e| matches!(e.kind, TrackEventKind::Midi { .. }))
            .collect();
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[1].delta.as_int(), 480);
    }
}
