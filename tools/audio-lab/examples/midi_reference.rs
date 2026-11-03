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
