// Full GM1 percussion synthesis — notes 35–81, tuned for 90s 808/909 character.
// Notes 82–105 continue past the GM map with held ringers and DTMF dial tones.

use crate::percussion::{DTMF_FIRST_NOTE, KEYPAD_KEYS, RINGER_FIRST_NOTE};

/// Bhaskara I sine approximation — max error 0.165 %, no lookup table.
/// phase: 0..u32::MAX = 0..2π  →  output: −32767..32767
#[inline(always)]
fn bhaskara_sine(phase: u32) -> i32 {
    let (positive, p) = if phase < 0x8000_0000 {
        (true, phase)
    } else {
        (false, phase.wrapping_sub(0x8000_0000))
    };
    let t = p as f32 * (1.0 / 2_147_483_648.0); // 0.0 ..= 1.0 over the half period
    let s = t * (1.0 - t); // 0.0 ..= 0.25
    let v = (16.0 * s / (5.0 - 4.0 * s) * 32767.0) as i32;
    if positive {
        v
    } else {
        -v
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum DrumType {
    AcousticBassDrum, // 35
    BassDrum1,        // 36
    SideStick,        // 37
    AcousticSnare,    // 38
    HandClap,         // 39
    ElectricSnare,    // 40
    LowFloorTom,      // 41
    ClosedHiHat,      // 42
    HighFloorTom,     // 43
    PedalHiHat,       // 44
    LowTom,           // 45
    OpenHiHat,        // 46
    LowMidTom,        // 47
    HiMidTom,         // 48
    CrashCymbal1,     // 49
    HighTom,          // 50
    RideCymbal1,      // 51
    ChineseCymbal,    // 52
    RideBell,         // 53
    Tambourine,       // 54
    SplashCymbal,     // 55
    Cowbell,          // 56
    CrashCymbal2,     // 57
    Vibraslap,        // 58
    RideCymbal2,      // 59
    HiBongo,          // 60
    LowBongo,         // 61
    MuteHiConga,      // 62
    OpenHiConga,      // 63
    LowConga,         // 64
    HighTimbale,      // 65
    LowTimbale,       // 66
    HighAgogo,        // 67
    LowAgogo,         // 68
    Cabasa,           // 69
    Maracas,          // 70
    ShortWhistle,     // 71
    LongWhistle,      // 72
    ShortGuiro,       // 73
    LongGuiro,        // 74
    Claves,           // 75
    HiWoodBlock,      // 76
    LowWoodBlock,     // 77
    MuteCuica,        // 78
    OpenCuica,        // 79
    MuteTriangle,     // 80
    OpenTriangle,     // 81
    /// Ringer (notes 82–93); index 0–11 into `RINGERS`.
    Ringer(u8),
    /// DTMF digit tone (notes 94–105); index 0–11 in keypad order 1–9, *, 0, #.
    Dtmf(u8),
}

/// Envelope multiplier for releasing a held sound: 0xFFFF falls below the
/// `is_finished` threshold (100) in ~240 samples ≈ 5 ms — short enough for tight
/// rests, long enough not to click.
const RELEASE_DECAY: u16 = 63_790;

/// DTMF row (low) and column (high) frequencies in Hz, per ITU-T Q.23.
const DTMF_ROW_HZ: [u32; 4] = [697, 770, 852, 941];
const DTMF_COL_HZ: [u32; 3] = [1209, 1336, 1477];

/// Integer phase advance uses the same clock as synthesis and vocal analysis.
const fn hz_to_phase_inc(hz: u32) -> u32 {
    hz * (4_294_967_296.0 / crate::constants::SAMPLE_RATE + 0.5) as u32
}

const SAMPLE_RATE_HZ: u32 = crate::constants::SAMPLE_RATE as u32;
const SAMPLES_PER_MS: u32 = 48;
/// Fade at every ring and note edge (~2 ms) so gating doesn't click.
const EDGE_RAMP: u32 = 96;
const FULL_GAIN: i32 = 32_767;

/// Phase increment for MIDI notes 0–11 (C-1 … B-1); each octave up is one
/// left shift, so melodies need no float math per sample.
const SEMITONE_PHASE_INC: [u32; 12] = {
    let mut increments = [0; 12];
    let mut note = 0;
    while note < 12 {
        increments[note] = (synthphone_e_vocal_dsp::audio::frequencies::BASE_FREQUENCIES[note]
            * 0.5
            * (4_294_967_296.0 / crate::constants::SAMPLE_RATE)) as u32;
        note += 1;
    }
    increments
};

#[inline(always)]
fn note_phase_inc(note: u8) -> u32 {
    SEMITONE_PHASE_INC[(note % 12) as usize] << (note / 12)
}

/// One ringer sound (notes 82–93). Each loops with its real-world cadence,
/// pauses included, while the key is held — like a phone left ringing.
/// Letting go of the key cuts it off at any point.
#[derive(Clone, Copy)]
enum Ringer {
    /// Tone ringer: `f1` and `f2` (Hz) summed, or alternated `warble_hz` times a
    /// second. `cadence_ms` is on, off, on, off; both offs 0 means a steady ring.
    Pattern {
        f1: u16,
        f2: u16,
        warble_hz: u8,
        square: bool,
        cadence_ms: [u16; 4],
    },
    /// Electromechanical bell: a clapper swinging at `strike_hz` hits two gongs
    /// alternately. Each gong is a fundamental plus an inharmonic overtone (Hz) —
    /// the non-integer ratio is what makes it sound metallic instead of buzzy.
    Bell {
        gongs: [[u16; 2]; 2],
        strike_hz: u8,
        cadence_ms: [u16; 4],
    },
    /// Square-wave tune: (MIDI note, length in steps); note 0 is a rest.
    /// `pause_ms` of silence follows each pass, as a ringtone loops.
    Melody {
        notes: &'static [(u8, u8)],
        step_ms: u16,
        pause_ms: u16,
    },
}

/// Silence between passes of a melody ringtone.
const MELODY_PAUSE_MS: u16 = 1500;

/// Keypad key 1–12 → ringer. Tune by ear on hardware.
///
/// Cadences for 1, 4 and 9 follow the network standards (US 2 s on / 4 s off,
/// UK 0.4-0.2-0.4 s then 2 s). The rest are device sounds with no standard;
/// their timings are typical values.
const RINGERS: [Ringer; 12] = [
    // 1: Mechanical bell — 20 Hz ringing current swings the clapper between two
    //    gongs, US cadence. Gong frequencies are starting values; tune by ear.
    Ringer::Bell {
        gongs: [[1050, 2690], [1290, 3410]],
        strike_hz: 20,
        cadence_ms: [2000, 4000, 0, 0],
    },
    // 2: 90s desk-phone burble — low, buzzy square trill, US cadence.
    Ringer::Pattern {
        f1: 600,
        f2: 750,
        warble_hz: 20,
        square: true,
        cadence_ms: [2000, 4000, 0, 0],
    },
    // 3: Early cell-phone "ring ring" — fast two-tone square trill, double burst.
    Ringer::Pattern {
        f1: 1300,
        f2: 1700,
        warble_hz: 30,
        square: true,
        cadence_ms: [500, 250, 500, 1500],
    },
    // 4: Electronic warble — 80s desk phone, US cadence.
    Ringer::Pattern {
        f1: 1000,
        f2: 1250,
        warble_hz: 16,
        square: false,
        cadence_ms: [2000, 4000, 0, 0],
    },
    // 5: Double warble, UK-style double cadence.
    Ringer::Pattern {
        f1: 800,
        f2: 1000,
        warble_hz: 12,
        square: false,
        cadence_ms: [400, 200, 400, 2000],
    },
    // 6: 90s cordless-phone trill — high, very fast square trill in short bursts.
    Ringer::Pattern {
        f1: 1800,
        f2: 2400,
        warble_hz: 40,
        square: true,
        cadence_ms: [800, 200, 800, 2000],
    },
    // 7: Pager beep.
    Ringer::Pattern {
        f1: 2000,
        f2: 0,
        warble_hz: 0,
        square: true,
        cadence_ms: [150, 100, 150, 2000],
    },
    // 8: Office chirp — two quick beeps.
    Ringer::Pattern {
        f1: 1200,
        f2: 0,
        warble_hz: 0,
        square: true,
        cadence_ms: [60, 40, 60, 2000],
    },
    // 9: UK double ring — 400 + 450 Hz.
    Ringer::Pattern {
        f1: 400,
        f2: 450,
        warble_hz: 0,
        square: false,
        cadence_ms: [400, 200, 400, 2000],
    },
    // 10: Gran Vals excerpt (Tárrega, 1902) — the "Nokia tune". Trial for a
    //     personal build only; see GRAN_VALS before any release.
    //     To go back: notes: MOUNTAIN_KING, step_ms: 130.
    Ringer::Melody {
        notes: GRAN_VALS,
        step_ms: 140,
        pause_ms: MELODY_PAUSE_MS,
    },
    // 11: Badinerie (Bach, Orchestral Suite No. 2) — opening phrase.
    Ringer::Melody {
        notes: BADINERIE,
        step_ms: 90,
        pause_ms: MELODY_PAUSE_MS,
    },
    // 12: Flight of the Bumblebee (Rimsky-Korsakov, 1900) — chromatic opening.
    Ringer::Melody {
        notes: BUMBLEBEE,
        step_ms: 60,
        pause_ms: MELODY_PAUSE_MS,
    },
];

// Public-domain melodies, transcribed from the original scores (not from any
// phone maker's ringtone). (MIDI note, steps); 0 = rest. 60 = C4, 72 = C5, 84 = C6.

// TRADEMARK: the composition is public domain, but Nokia has registered this
// excerpt as a sound trademark. Fine for a personal build; swap slot 10 back to
// MOUNTAIN_KING before selling or distributing the firmware.
#[rustfmt::skip]
const GRAN_VALS: &[(u8, u8)] = &[
    // E6 D6 F#5 G#5 · C#6 B5 D5 E5 · B5 A5 C#5 E5 · A5
    (88, 1), (86, 1), (78, 2), (80, 2),
    (85, 1), (83, 1), (74, 2), (76, 2),
    (83, 1), (81, 1), (73, 2), (76, 2),
    (81, 6),
];

// Unused while slot 10 trials GRAN_VALS.
#[allow(dead_code)]
#[rustfmt::skip]
const MOUNTAIN_KING: &[(u8, u8)] = &[
    // B5 C#6 D6 E6 F#6 D6 F#6 · F6 C#6 F6 · E6 C6 E6
    (83, 1), (85, 1), (86, 1), (88, 1), (90, 1), (86, 1), (90, 2),
    (89, 1), (85, 1), (89, 2), (88, 1), (84, 1), (88, 2),
    // B5 C#6 D6 E6 F#6 D6 F#6 B6 · A6 F#6 D6 F#6 A6
    (83, 1), (85, 1), (86, 1), (88, 1), (90, 1), (86, 1), (90, 1), (95, 1),
    (93, 1), (90, 1), (86, 1), (90, 1), (93, 4),
];

#[rustfmt::skip]
const BADINERIE: &[(u8, u8)] = &[
    // B5 D6 B5 F#5 B5 F#5 D5 F#5 D5 B4 — falling B-minor figure
    (83, 2), (86, 1), (83, 1), (78, 2), (83, 1), (78, 1), (74, 2), (78, 1), (74, 1), (71, 4),
];

#[rustfmt::skip]
const BUMBLEBEE: &[(u8, u8)] = &[
    // E6 D#6 D6 C#6 C6 F6 E6 D#6 · E6 D#6 D6 C#6 C6 C#6 D6 D#6 — twice
    (88, 1), (87, 1), (86, 1), (85, 1), (84, 1), (89, 1), (88, 1), (87, 1),
    (88, 1), (87, 1), (86, 1), (85, 1), (84, 1), (85, 1), (86, 1), (87, 1),
    (88, 1), (87, 1), (86, 1), (85, 1), (84, 1), (89, 1), (88, 1), (87, 1),
    (88, 1), (87, 1), (86, 1), (85, 1), (84, 1), (85, 1), (86, 1), (87, 1),
];

/// 0..=FULL_GAIN at `pos` within a segment `len` samples long, fading over
/// `EDGE_RAMP` samples at both ends.
#[inline(always)]
fn edge_ramp(pos: u32, len: u32) -> i32 {
    let distance = pos.min(len - 1 - pos);
    if distance >= EDGE_RAMP {
        FULL_GAIN
    } else {
        (distance * FULL_GAIN as u32 / EDGE_RAMP) as i32
    }
}

#[inline(always)]
fn ringer_wave(phase: u32, square: bool) -> i32 {
    if square {
        if phase < 0x8000_0000 {
            32000
        } else {
            -32000
        }
    } else {
        bhaskara_sine(phase)
    }
}

/// Gong level `pos` samples into a clapper cycle `cycle` samples long: a sharp
/// hit that rings down toward a sustained level, because the next hit lands
/// before the gong dies out.
#[inline(always)]
fn gong_ring(pos: u32, cycle: u32) -> i32 {
    let left = ((cycle - pos) * 256 / cycle) as i32; // 256 → 0 across the cycle
    let fall = (left * left) >> 1; // quadratic, 32_768 → 0
    let level = 13_107 + ((fall * 19_660) >> 15); // 40 % sustain + 60 % hit
    level.min(FULL_GAIN)
}

/// Gain of a looping on/off cadence at sample `sc`.
fn cadence_gain(sc: u32, cadence_ms: &[u16; 4]) -> i32 {
    if cadence_ms[1] == 0 && cadence_ms[3] == 0 {
        return FULL_GAIN;
    }
    let period: u32 = cadence_ms
        .iter()
        .map(|&ms| ms as u32 * SAMPLES_PER_MS)
        .sum();
    let mut pos = sc % period;
    for (i, &ms) in cadence_ms.iter().enumerate() {
        let len = ms as u32 * SAMPLES_PER_MS;
        if pos < len {
            return if i % 2 == 0 { edge_ramp(pos, len) } else { 0 };
        }
        pos -= len;
    }
    0
}

/// 13 synthesis engines — assigned during trigger(), dispatched in next_value().
#[derive(Copy, Clone, PartialEq)]
enum SynthEngine {
    /// Bhaskara sine + exponential downward pitch sweep + optional click transient.
    Kick,
    /// Bhaskara sine body + white-noise mix + initial crack burst.
    Snare,
    /// Four rapid noise bursts then decaying noise tail (808-style clap).
    Clap,
    /// HP-filtered LFSR noise (metallic character for cymbals/hats).
    MetallicNoise,
    /// Raw LFSR noise (short transient sounds: maracas, tambourine).
    PureNoise,
    /// Bhaskara sine at tuned pitch with slight downward sweep.
    Tom,
    /// Bhaskara sine + noise mix (timbales, congas with bite).
    TomNoise,
    /// Bhaskara sine only (agogos, whistles, triangles, ride bell).
    PureTone,
    /// Short square-wave burst (sticks, wood blocks, claves).
    Click,
    /// Two detuned square oscillators — classic 808 cowbell.
    Cowbell,
    /// Click transient → gated rattle noise (vibraslap).
    Vibraslap,
    /// Periodically gated noise (guiro scrape).
    Guiro,
    /// AM-modulated noise (cabasa rattle).
    Cabasa,
    /// Bhaskara sine with upward pitch sweep (cuica).
    Cuica,
    /// Two summed Bhaskara sines at phase_inc and phase2_inc (telephone DTMF).
    Dtmf,
    /// Table-driven ringer patterns and melodies; see `RINGERS`.
    Ringer,
}

pub struct DrumSampler {
    drum_type: DrumType,
    engine: SynthEngine,
    sample_count: u32,
    max_samples: u32,
    phase: u32,
    phase_inc: u32,
    target_phase_inc: u32,
    pitch_decay: u16, // exponential pitch-sweep multiplier  (Kick, Tom)
    phase2: u32,      // second oscillator / LFO
    phase2_inc: u32,  // second osc freq, LFO rate, or cuica linear step
    env: u16,
    env_decay: u16,
    noise: u16,    // 15-bit LFSR state
    noise_hp: i16, // previous noise sample — used for HP difference filter
    noise_mix: u8, // 0 = all tone, 16 = all noise
    /// Held sounds (dial tones, ringers) sustain at full level until `release()`.
    held: bool,
}

impl DrumSampler {
    pub fn new() -> Self {
        Self {
            drum_type: DrumType::BassDrum1,
            engine: SynthEngine::Kick,
            sample_count: 0,
            max_samples: 0,
            phase: 0,
            phase_inc: 0,
            target_phase_inc: 0,
            pitch_decay: 65_490,
            phase2: 0,
            phase2_inc: 0,
            env: 0,
            env_decay: 65_500,
            noise: 0xACE1,
            noise_hp: 0,
            noise_mix: 0,
            held: false,
        }
    }

    pub fn set_drum_type(&mut self, drum_type: DrumType) {
        self.drum_type = drum_type;
    }

    pub fn trigger(&mut self) {
        self.sample_count = 0;
        self.phase = 0;
        self.phase2 = 0;
        self.noise = 0xACE1;
        self.noise_hp = 0;
        self.env = 0xFFFF;
        self.held = false;

        match self.drum_type {
            // ── KICKS ────────────────────────────────────────────────────────────
            // 35: 808-style – deep sub-bass bloom, long tail
            DrumType::AcousticBassDrum => {
                self.engine = SynthEngine::Kick;
                self.max_samples = 19_200; // 400 ms safety cap; env fades first
                self.phase_inc = 8_944_300; // 100 Hz start
                self.target_phase_inc = 2_504_404; // 28 Hz sub-bass end
                self.pitch_decay = 65_470;
                self.env_decay = 65_524; // very slow – the 808 "bloom"
                self.noise_mix = 0;
                self.phase2_inc = 0;
            }
            // 36: 909-style – higher start, faster sweep, punch + thump
            DrumType::BassDrum1 => {
                self.engine = SynthEngine::Kick;
                self.max_samples = 9_600; // 200 ms
                self.phase_inc = 13_416_450; // 150 Hz start
                self.target_phase_inc = 3_130_505; // 35 Hz end
                self.pitch_decay = 65_430; // fast sweep – characteristic 909
                self.env_decay = 65_509;
                self.noise_mix = 0;
                self.phase2_inc = 0;
            }

            // ── SNARES / STICKS ──────────────────────────────────────────────────
            // 37: Side stick – sharp high-freq click
            DrumType::SideStick => {
                self.engine = SynthEngine::Click;
                self.max_samples = 480; // 10 ms
                self.phase_inc = 134_164_500; // 1500 Hz
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_000;
                self.noise_mix = 4; // 75 % square + 25 % noise
                self.phase2_inc = 0;
            }
            // 38: 808 snare – tonal crack + noise tail
            DrumType::AcousticSnare => {
                self.engine = SynthEngine::Snare;
                self.max_samples = 4_320; // 90 ms
                self.phase_inc = 17_888_600; // 200 Hz body
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_494;
                self.noise_mix = 11; // 69 % noise
                self.phase2_inc = 0;
            }
            // 39: 808 clap – four rapid bursts then decaying tail
            DrumType::HandClap => {
                self.engine = SynthEngine::Clap;
                self.max_samples = 4_800; // 100 ms
                self.phase_inc = 0;
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_480;
                self.noise_mix = 0;
                self.phase2_inc = 0;
            }
            // 40: 909 snare – tight, mostly noise, very snappy
            DrumType::ElectricSnare => {
                self.engine = SynthEngine::Snare;
                self.max_samples = 2_400; // 50 ms
                self.phase_inc = 22_360_750; // 250 Hz body
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_435;
                self.noise_mix = 13; // 81 % noise
                self.phase2_inc = 0;
            }

            // ── TOMS ─────────────────────────────────────────────────────────────
            DrumType::LowFloorTom => {
                self.engine = SynthEngine::Tom;
                self.max_samples = 8_640;
                self.phase_inc = 7_155_440; // 80 Hz
                self.target_phase_inc = 6_500_000;
                self.pitch_decay = 65_530;
                self.env_decay = 65_513;
                self.noise_mix = 0;
                self.phase2_inc = 0;
            }
            DrumType::HighFloorTom => {
                self.engine = SynthEngine::Tom;
                self.max_samples = 8_640;
                self.phase_inc = 8_497_085; // 95 Hz
                self.target_phase_inc = 7_800_000;
                self.pitch_decay = 65_530;
                self.env_decay = 65_513;
                self.noise_mix = 0;
                self.phase2_inc = 0;
            }
            DrumType::LowTom => {
                self.engine = SynthEngine::Tom;
                self.max_samples = 9_600;
                self.phase_inc = 9_838_730; // 110 Hz
                self.target_phase_inc = 9_000_000;
                self.pitch_decay = 65_530;
                self.env_decay = 65_515;
                self.noise_mix = 0;
                self.phase2_inc = 0;
            }
            DrumType::LowMidTom => {
                self.engine = SynthEngine::Tom;
                self.max_samples = 9_600;
                self.phase_inc = 11_627_590; // 130 Hz
                self.target_phase_inc = 10_600_000;
                self.pitch_decay = 65_530;
                self.env_decay = 65_515;
                self.noise_mix = 0;
                self.phase2_inc = 0;
            }
            DrumType::HiMidTom => {
                self.engine = SynthEngine::Tom;
                self.max_samples = 8_160;
                self.phase_inc = 13_863_665; // 155 Hz
                self.target_phase_inc = 12_600_000;
                self.pitch_decay = 65_530;
                self.env_decay = 65_511;
                self.noise_mix = 0;
                self.phase2_inc = 0;
            }
            DrumType::HighTom => {
                self.engine = SynthEngine::Tom;
                self.max_samples = 7_200;
                self.phase_inc = 16_546_955; // 185 Hz
                self.target_phase_inc = 15_000_000;
                self.pitch_decay = 65_530;
                self.env_decay = 65_507;
                self.noise_mix = 0;
                self.phase2_inc = 0;
            }

            // ── HI-HATS (HP-filtered metallic noise) ─────────────────────────────
            DrumType::ClosedHiHat => {
                self.engine = SynthEngine::MetallicNoise;
                self.max_samples = 768; // 16 ms – very tight 909-style
                self.phase_inc = 0;
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 64_256; // extremely fast snap
                self.noise_mix = 16;
                self.phase2_inc = 0;
            }
            DrumType::PedalHiHat => {
                self.engine = SynthEngine::MetallicNoise;
                self.max_samples = 1_152; // 24 ms
                self.phase_inc = 0;
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 64_768;
                self.noise_mix = 16;
                self.phase2_inc = 0;
            }
            DrumType::OpenHiHat => {
                self.engine = SynthEngine::MetallicNoise;
                self.max_samples = 10_560; // 220 ms
                self.phase_inc = 0;
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_512;
                self.noise_mix = 16;
                self.phase2_inc = 0;
            }

            // ── CYMBALS (HP-filtered noise, long decays) ──────────────────────────
            DrumType::CrashCymbal1 => {
                self.engine = SynthEngine::MetallicNoise;
                self.max_samples = 28_800; // 600 ms
                self.phase_inc = 0;
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_530;
                self.noise_mix = 16;
                self.phase2_inc = 0;
            }
            DrumType::RideCymbal1 => {
                self.engine = SynthEngine::MetallicNoise;
                self.max_samples = 14_400; // 300 ms
                self.phase_inc = 0;
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_523;
                self.noise_mix = 16;
                self.phase2_inc = 0;
            }
            DrumType::ChineseCymbal => {
                self.engine = SynthEngine::MetallicNoise;
                self.max_samples = 12_000; // 250 ms
                self.phase_inc = 0;
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_521;
                self.noise_mix = 16;
                self.phase2_inc = 0;
            }
            DrumType::RideBell => {
                self.engine = SynthEngine::PureTone;
                self.max_samples = 7_200; // 150 ms
                self.phase_inc = 62_610_100; // 700 Hz
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_507;
                self.noise_mix = 0;
                self.phase2_inc = 0;
            }
            DrumType::Tambourine => {
                self.engine = SynthEngine::MetallicNoise;
                self.max_samples = 2_400; // 50 ms
                self.phase_inc = 0;
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_360;
                self.noise_mix = 16;
                self.phase2_inc = 0;
            }
            DrumType::SplashCymbal => {
                self.engine = SynthEngine::MetallicNoise;
                self.max_samples = 6_000; // 125 ms
                self.phase_inc = 0;
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_505;
                self.noise_mix = 16;
                self.phase2_inc = 0;
            }
            // ── 808 COWBELL ───────────────────────────────────────────────────────
            DrumType::Cowbell => {
                self.engine = SynthEngine::Cowbell;
                self.max_samples = 2_880; // 60 ms – snappy 808 bonk
                self.phase_inc = 50_267_006; // 562 Hz osc 1
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_350;
                self.noise_mix = 0;
                self.phase2_inc = 75_579_335; // 845 Hz osc 2
            }
            DrumType::CrashCymbal2 => {
                self.engine = SynthEngine::MetallicNoise;
                self.max_samples = 24_000; // 500 ms
                self.phase_inc = 0;
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_528;
                self.noise_mix = 16;
                self.phase2_inc = 0;
                self.noise = 0x5A3C;
            }
            DrumType::Vibraslap => {
                self.engine = SynthEngine::Vibraslap;
                self.max_samples = 8_000;
                self.phase_inc = 134_164_500; // 1500 Hz click
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_510;
                self.noise_mix = 0;
                self.phase2_inc = 4_472_150; // ~50 Hz rattle gate
            }
            DrumType::RideCymbal2 => {
                self.engine = SynthEngine::MetallicNoise;
                self.max_samples = 12_000;
                self.phase_inc = 0;
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_521;
                self.noise_mix = 16;
                self.phase2_inc = 0;
                self.noise = 0x7B2E;
            }

            // ── BONGOS / CONGAS / TIMBALES ────────────────────────────────────────
            DrumType::HiBongo => {
                self.engine = SynthEngine::Tom;
                self.max_samples = 2_400;
                self.phase_inc = 35_777_200; // 400 Hz
                self.target_phase_inc = 32_000_000;
                self.pitch_decay = 65_520;
                self.env_decay = 65_360;
                self.noise_mix = 3;
                self.phase2_inc = 0;
            }
            DrumType::LowBongo => {
                self.engine = SynthEngine::Tom;
                self.max_samples = 3_600;
                self.phase_inc = 23_255_180; // 260 Hz
                self.target_phase_inc = 20_800_000;
                self.pitch_decay = 65_522;
                self.env_decay = 65_475;
                self.noise_mix = 3;
                self.phase2_inc = 0;
            }
            DrumType::MuteHiConga => {
                self.engine = SynthEngine::Tom;
                self.max_samples = 1_200;
                self.phase_inc = 28_174_545; // 315 Hz
                self.target_phase_inc = 26_000_000;
                self.pitch_decay = 65_510;
                self.env_decay = 65_200;
                self.noise_mix = 2;
                self.phase2_inc = 0;
            }
            DrumType::OpenHiConga => {
                self.engine = SynthEngine::Tom;
                self.max_samples = 2_976;
                self.phase_inc = 28_174_545; // 315 Hz
                self.target_phase_inc = 26_000_000;
                self.pitch_decay = 65_520;
                self.env_decay = 65_450;
                self.noise_mix = 2;
                self.phase2_inc = 0;
            }
            DrumType::LowConga => {
                self.engine = SynthEngine::Tom;
                self.max_samples = 3_600;
                self.phase_inc = 17_888_600; // 200 Hz
                self.target_phase_inc = 16_000_000;
                self.pitch_decay = 65_522;
                self.env_decay = 65_475;
                self.noise_mix = 2;
                self.phase2_inc = 0;
            }
            DrumType::HighTimbale => {
                self.engine = SynthEngine::TomNoise;
                self.max_samples = 2_400;
                self.phase_inc = 50_088_080; // 560 Hz
                self.target_phase_inc = 44_000_000;
                self.pitch_decay = 65_510;
                self.env_decay = 65_360;
                self.noise_mix = 6;
                self.phase2_inc = 0;
            }
            DrumType::LowTimbale => {
                self.engine = SynthEngine::TomNoise;
                self.max_samples = 3_000;
                self.phase_inc = 33_093_910; // 370 Hz
                self.target_phase_inc = 29_000_000;
                self.pitch_decay = 65_515;
                self.env_decay = 65_448;
                self.noise_mix = 6;
                self.phase2_inc = 0;
            }

            // ── PITCHED TONES ─────────────────────────────────────────────────────
            DrumType::HighAgogo => {
                self.engine = SynthEngine::PureTone;
                self.max_samples = 2_400;
                self.phase_inc = 78_709_840; // 880 Hz
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_360;
                self.noise_mix = 0;
                self.phase2_inc = 0;
            }
            DrumType::LowAgogo => {
                self.engine = SynthEngine::PureTone;
                self.max_samples = 3_000;
                self.phase_inc = 59_032_380; // 660 Hz
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_448;
                self.noise_mix = 0;
                self.phase2_inc = 0;
            }
            DrumType::ShortWhistle => {
                self.engine = SynthEngine::PureTone;
                self.max_samples = 4_800;
                self.phase_inc = 187_184_099; // 2093 Hz (C7)
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_500;
                self.noise_mix = 0;
                self.phase2_inc = 0;
            }
            DrumType::LongWhistle => {
                self.engine = SynthEngine::PureTone;
                self.max_samples = 12_000;
                self.phase_inc = 187_184_099; // 2093 Hz
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_521;
                self.noise_mix = 0;
                self.phase2_inc = 0;
            }
            DrumType::MuteTriangle => {
                self.engine = SynthEngine::PureTone;
                self.max_samples = 1_200;
                self.phase_inc = hz_to_phase_inc(3520); // A7
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_200;
                self.noise_mix = 0;
                self.phase2_inc = 0;
            }
            DrumType::OpenTriangle => {
                self.engine = SynthEngine::PureTone;
                self.max_samples = 28_800; // 600 ms ring
                self.phase_inc = hz_to_phase_inc(3520); // A7
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_530;
                self.noise_mix = 0;
                self.phase2_inc = 0;
            }

            // ── SHORT NOISE TRANSIENTS ────────────────────────────────────────────
            DrumType::Maracas => {
                self.engine = SynthEngine::PureNoise;
                self.max_samples = 960;
                self.phase_inc = 0;
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_100;
                self.noise_mix = 16;
                self.phase2_inc = 0;
            }

            // ── CLICKS / STICKS ───────────────────────────────────────────────────
            DrumType::Claves => {
                self.engine = SynthEngine::Click;
                self.max_samples = 240;
                self.phase_inc = 178_886_000; // 2000 Hz
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 64_800;
                self.noise_mix = 0;
                self.phase2_inc = 0;
            }
            DrumType::HiWoodBlock => {
                self.engine = SynthEngine::Click;
                self.max_samples = 1_200;
                self.phase_inc = 67_082_250; // 750 Hz
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_200;
                self.noise_mix = 0;
                self.phase2_inc = 0;
            }
            DrumType::LowWoodBlock => {
                self.engine = SynthEngine::Click;
                self.max_samples = 1_200;
                self.phase_inc = 53_665_800; // 600 Hz
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_200;
                self.noise_mix = 0;
                self.phase2_inc = 0;
            }

            // ── GUIRO ─────────────────────────────────────────────────────────────
            DrumType::ShortGuiro => {
                self.engine = SynthEngine::Guiro;
                self.max_samples = 2_400;
                self.phase_inc = 0;
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_450;
                self.noise_mix = 0;
                self.phase2_inc = 14_311_040; // 160 Hz gate → ~8 scrapes
            }
            DrumType::LongGuiro => {
                self.engine = SynthEngine::Guiro;
                self.max_samples = 6_000;
                self.phase_inc = 0;
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_505;
                self.noise_mix = 0;
                self.phase2_inc = 7_155_440; // 80 Hz gate → ~10 scrapes
            }

            // ── CABASA ────────────────────────────────────────────────────────────
            DrumType::Cabasa => {
                self.engine = SynthEngine::Cabasa;
                self.max_samples = 4_800;
                self.phase_inc = 0;
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_500;
                self.noise_mix = 0;
                self.phase2_inc = 1_788_860; // ~20 Hz AM triangle LFO
            }

            // ── CUICA ─────────────────────────────────────────────────────────────
            DrumType::MuteCuica => {
                self.engine = SynthEngine::Cuica;
                self.max_samples = 2_400;
                self.phase_inc = 26_832_900; // 300 Hz start
                self.target_phase_inc = 62_610_100; // 700 Hz end
                self.pitch_decay = 0;
                self.env_decay = 65_360;
                self.noise_mix = 0;
                // linear step: (62_610_100 − 26_832_900) / 2400 ≈ 14_907
                self.phase2_inc = 14_907;
            }
            DrumType::OpenCuica => {
                self.engine = SynthEngine::Cuica;
                self.max_samples = 6_000;
                self.phase_inc = 26_832_900; // 300 Hz start
                self.target_phase_inc = 62_610_100; // 700 Hz end
                self.pitch_decay = 0;
                self.env_decay = 65_505;
                self.noise_mix = 0;
                // linear step: 35_777_200 / 6000 ≈ 5_963
                self.phase2_inc = 5_963;
            }

            // ── RINGERS ──────────────────────────────────────────────────────────
            // The ringer engine advances its own phases from `RINGERS`.
            DrumType::Ringer(_) => {
                self.engine = SynthEngine::Ringer;
                self.max_samples = u32::MAX;
                self.phase_inc = 0;
                self.phase2_inc = 0;
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = RELEASE_DECAY;
                self.noise_mix = 0;
                self.held = true;
            }

            // ── DIAL TONES ───────────────────────────────────────────────────────
            // Row tone + column tone, held until release.
            DrumType::Dtmf(index) => {
                let index = (index as usize).min(11);
                self.engine = SynthEngine::Dtmf;
                self.max_samples = u32::MAX;
                self.phase_inc = hz_to_phase_inc(DTMF_ROW_HZ[index / 3]);
                self.phase2_inc = hz_to_phase_inc(DTMF_COL_HZ[index % 3]);
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = RELEASE_DECAY;
                self.noise_mix = 0;
                self.held = true;
            }
        }
    }

    /// Next raw sample of ringer `index` at sample `sc` since the key was pressed.
    fn ringer_sample(&mut self, sc: u32, index: u8, noise: i32) -> i32 {
        match RINGERS[(index as usize).min(RINGERS.len() - 1)] {
            Ringer::Bell {
                gongs,
                strike_hz,
                cadence_ms,
            } => {
                // Each gong is hit once per clapper cycle, half a cycle apart.
                let cycle = SAMPLE_RATE_HZ / strike_hz as u32;
                let mut sum = 0i32;
                for (g, [fundamental_hz, overtone_hz]) in gongs.iter().enumerate() {
                    let since_strike = (sc + g as u32 * cycle / 2) % cycle;
                    // Phase straight from the sample count, so the four partials
                    // need no stored oscillator state.
                    let fundamental =
                        bhaskara_sine(sc.wrapping_mul(hz_to_phase_inc(*fundamental_hz as u32)));
                    let overtone =
                        bhaskara_sine(sc.wrapping_mul(hz_to_phase_inc(*overtone_hz as u32)));
                    let gong = (fundamental * 2 + overtone) / 3;
                    sum += (gong * gong_ring(since_strike, cycle)) >> 15;
                }
                // Clapper impact: a ~1 ms metallic tick on every hit.
                let tick = if sc % (cycle / 2) < 48 { noise >> 2 } else { 0 };
                (((sum >> 1) + tick) * cadence_gain(sc, &cadence_ms)) >> 15
            }
            Ringer::Pattern {
                f1,
                f2,
                warble_hz,
                square,
                cadence_ms,
            } => {
                let tone = if warble_hz > 0 {
                    // One phase with switching increments, so the warble never clicks.
                    let half_period = SAMPLE_RATE_HZ / (2 * warble_hz as u32);
                    let hz = if (sc / half_period).is_multiple_of(2) {
                        f1
                    } else {
                        f2
                    };
                    self.phase = self.phase.wrapping_add(hz_to_phase_inc(hz as u32));
                    ringer_wave(self.phase, square)
                } else {
                    self.phase = self.phase.wrapping_add(hz_to_phase_inc(f1 as u32));
                    if f2 == 0 {
                        ringer_wave(self.phase, square)
                    } else {
                        self.phase2 = self.phase2.wrapping_add(hz_to_phase_inc(f2 as u32));
                        (ringer_wave(self.phase, square) + ringer_wave(self.phase2, square)) >> 1
                    }
                };
                (tone * cadence_gain(sc, &cadence_ms)) >> 15
            }
            Ringer::Melody {
                notes,
                step_ms,
                pause_ms,
            } => {
                let step = step_ms as u32 * SAMPLES_PER_MS;
                // Past the last note, the loop's tail is the pause: silence.
                let total: u32 = notes.iter().map(|&(_, len)| len as u32 * step).sum::<u32>()
                    + pause_ms as u32 * SAMPLES_PER_MS;
                let mut pos = sc % total;
                for &(note, len) in notes {
                    let len = len as u32 * step;
                    if pos < len {
                        if note == 0 {
                            return 0;
                        }
                        self.phase = self.phase.wrapping_add(note_phase_inc(note));
                        return (ringer_wave(self.phase, true) * edge_ramp(pos, len)) >> 15;
                    }
                    pos -= len;
                }
                0
            }
        }
    }

    /// Starts the fade-out of a held sound. One-shot drums ignore it and play
    /// out their own envelope, as the GM spec expects.
    pub fn release(&mut self) {
        if self.held {
            self.held = false;
            self.env_decay = RELEASE_DECAY;
        }
    }

    #[inline(always)]
    pub fn is_finished(&self) -> bool {
        self.max_samples == 0 || self.sample_count >= self.max_samples || self.env < 100
    }

    #[inline(always)]
    pub fn next_value(&mut self) -> f32 {
        if self.sample_count >= self.max_samples || self.env < 100 {
            return 0.0;
        }
        let sc = self.sample_count;
        self.sample_count += 1;

        // ── LFSR noise ───────────────────────────────────────────────────────────
        let lsb = self.noise & 1;
        self.noise >>= 1;
        if lsb == 1 {
            self.noise ^= 0xB400;
        }
        let noise_i = ((self.noise & 0x7FFF) as i16).wrapping_sub(16384) << 1;

        // ── Primary oscillator ───────────────────────────────────────────────────
        self.phase = self.phase.wrapping_add(self.phase_inc);
        let square: i32 = if self.phase < 0x8000_0000 {
            32000
        } else {
            -32000
        };

        // ── Envelope ─────────────────────────────────────────────────────────────
        if !self.held {
            self.env = ((self.env as u32 * self.env_decay as u32) >> 16) as u16;
        }

        // ── Engine dispatch ───────────────────────────────────────────────────────
        let raw: i32 = match self.engine {
            SynthEngine::Kick => {
                // Exponential pitch sweep downward
                if self.phase_inc > self.target_phase_inc {
                    let swept = ((self.phase_inc as u64 * self.pitch_decay as u64) >> 16) as u32;
                    self.phase_inc = swept.max(self.target_phase_inc);
                }
                // Sine body – clean, punchy, no triangle buzz
                let body = bhaskara_sine(self.phase);
                // Click transient for attack punch (~1.7 ms)
                let click = if sc < 80 {
                    (noise_i as i32 * (80 - sc as i32)) >> 7
                } else {
                    0
                };
                body + click
            }
            SynthEngine::Snare => {
                // Initial crack: pure noise burst for first ~1 ms
                if sc < 48 {
                    noise_i as i32
                } else {
                    let mix = self.noise_mix as i32;
                    let body = bhaskara_sine(self.phase);
                    (body * (16 - mix) + noise_i as i32 * mix) >> 4
                }
            }
            SynthEngine::Clap => {
                // Four tight 2 ms noise bursts (96 samples each), then decaying tail
                let in_burst = sc < 96
                    || (192..288).contains(&sc)
                    || (384..480).contains(&sc)
                    || (576..672).contains(&sc);
                if in_burst || sc >= 672 {
                    noise_i as i32
                } else {
                    0
                }
            }
            SynthEngine::MetallicNoise => {
                // First-order HP filter: removes low-end, leaves airy metallic sheen
                let hp = noise_i as i32 - self.noise_hp as i32;
                self.noise_hp = noise_i;
                hp
            }
            SynthEngine::PureNoise => noise_i as i32,
            SynthEngine::Tom => {
                if self.phase_inc > self.target_phase_inc {
                    let swept = ((self.phase_inc as u64 * self.pitch_decay as u64) >> 16) as u32;
                    self.phase_inc = swept.max(self.target_phase_inc);
                }
                let tone = bhaskara_sine(self.phase);
                let mix = self.noise_mix as i32;
                if mix == 0 {
                    tone
                } else {
                    (tone * (16 - mix) + noise_i as i32 * mix) >> 4
                }
            }
            SynthEngine::TomNoise => {
                if self.phase_inc > self.target_phase_inc {
                    let swept = ((self.phase_inc as u64 * self.pitch_decay as u64) >> 16) as u32;
                    self.phase_inc = swept.max(self.target_phase_inc);
                }
                let mix = self.noise_mix as i32;
                (bhaskara_sine(self.phase) * (16 - mix) + noise_i as i32 * mix) >> 4
            }
            SynthEngine::PureTone => bhaskara_sine(self.phase),
            SynthEngine::Click => {
                let mix = self.noise_mix as i32;
                (square * (16 - mix) + noise_i as i32 * mix) >> 4
            }
            SynthEngine::Cowbell => {
                self.phase2 = self.phase2.wrapping_add(self.phase2_inc);
                let sq2: i32 = if self.phase2 < 0x8000_0000 {
                    16000
                } else {
                    -16000
                };
                square / 2 + sq2
            }
            SynthEngine::Vibraslap => {
                self.phase2 = self.phase2.wrapping_add(self.phase2_inc);
                if sc < 200 {
                    square
                } else if self.phase2 < 0x8000_0000 {
                    noise_i as i32
                } else {
                    0
                }
            }
            SynthEngine::Guiro => {
                self.phase2 = self.phase2.wrapping_add(self.phase2_inc);
                if self.phase2 < 0x8000_0000 {
                    noise_i as i32
                } else {
                    0
                }
            }
            SynthEngine::Cabasa => {
                self.phase2 = self.phase2.wrapping_add(self.phase2_inc);
                let am: i32 = if self.phase2 < 0x8000_0000 {
                    (self.phase2 >> 17) as i32
                } else {
                    32767 - ((self.phase2 - 0x8000_0000) >> 17) as i32
                };
                (noise_i as i32 * am) >> 15
            }
            SynthEngine::Cuica => {
                if self.phase_inc < self.target_phase_inc {
                    self.phase_inc = (self.phase_inc + self.phase2_inc).min(self.target_phase_inc);
                }
                bhaskara_sine(self.phase)
            }
            SynthEngine::Dtmf => {
                self.phase2 = self.phase2.wrapping_add(self.phase2_inc);
                (bhaskara_sine(self.phase) + bhaskara_sine(self.phase2)) >> 1
            }
            SynthEngine::Ringer => match self.drum_type {
                DrumType::Ringer(index) => self.ringer_sample(sc, index, noise_i as i32),
                _ => 0,
            },
        };

        // i64 multiply avoids overflow when raw + click exceeds i16 range
        let out = ((raw as i64 * self.env as i64) >> 16) as i32;
        out as f32 / 32768.0
    }
}

impl Default for DrumSampler {
    fn default() -> Self {
        Self::new()
    }
}

pub fn midi_note_to_drum_type(note: u8) -> Option<DrumType> {
    match note {
        35 => Some(DrumType::AcousticBassDrum),
        36 => Some(DrumType::BassDrum1),
        37 => Some(DrumType::SideStick),
        38 => Some(DrumType::AcousticSnare),
        39 => Some(DrumType::HandClap),
        40 => Some(DrumType::ElectricSnare),
        41 => Some(DrumType::LowFloorTom),
        42 => Some(DrumType::ClosedHiHat),
        43 => Some(DrumType::HighFloorTom),
        44 => Some(DrumType::PedalHiHat),
        45 => Some(DrumType::LowTom),
        46 => Some(DrumType::OpenHiHat),
        47 => Some(DrumType::LowMidTom),
        48 => Some(DrumType::HiMidTom),
        49 => Some(DrumType::CrashCymbal1),
        50 => Some(DrumType::HighTom),
        51 => Some(DrumType::RideCymbal1),
        52 => Some(DrumType::ChineseCymbal),
        53 => Some(DrumType::RideBell),
        54 => Some(DrumType::Tambourine),
        55 => Some(DrumType::SplashCymbal),
        56 => Some(DrumType::Cowbell),
        57 => Some(DrumType::CrashCymbal2),
        58 => Some(DrumType::Vibraslap),
        59 => Some(DrumType::RideCymbal2),
        60 => Some(DrumType::HiBongo),
        61 => Some(DrumType::LowBongo),
        62 => Some(DrumType::MuteHiConga),
        63 => Some(DrumType::OpenHiConga),
        64 => Some(DrumType::LowConga),
        65 => Some(DrumType::HighTimbale),
        66 => Some(DrumType::LowTimbale),
        67 => Some(DrumType::HighAgogo),
        68 => Some(DrumType::LowAgogo),
        69 => Some(DrumType::Cabasa),
        70 => Some(DrumType::Maracas),
        71 => Some(DrumType::ShortWhistle),
        72 => Some(DrumType::LongWhistle),
        73 => Some(DrumType::ShortGuiro),
        74 => Some(DrumType::LongGuiro),
        75 => Some(DrumType::Claves),
        76 => Some(DrumType::HiWoodBlock),
        77 => Some(DrumType::LowWoodBlock),
        78 => Some(DrumType::MuteCuica),
        79 => Some(DrumType::OpenCuica),
        80 => Some(DrumType::MuteTriangle),
        81 => Some(DrumType::OpenTriangle),
        n if (RINGER_FIRST_NOTE..RINGER_FIRST_NOTE + KEYPAD_KEYS as u8).contains(&n) => {
            Some(DrumType::Ringer(n - RINGER_FIRST_NOTE))
        }
        n if (DTMF_FIRST_NOTE..DTMF_FIRST_NOTE + KEYPAD_KEYS as u8).contains(&n) => {
            Some(DrumType::Dtmf(n - DTMF_FIRST_NOTE))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(drum: &mut DrumSampler, samples: usize) {
        for _ in 0..samples {
            drum.next_value();
        }
    }

    fn triggered(note: u8) -> DrumSampler {
        let mut drum = DrumSampler::new();
        drum.set_drum_type(midi_note_to_drum_type(note).unwrap());
        drum.trigger();
        drum
    }

    #[test]
    fn dtmf_notes_follow_keypad_order() {
        assert_eq!(midi_note_to_drum_type(94), Some(DrumType::Dtmf(0)));
        assert_eq!(midi_note_to_drum_type(105), Some(DrumType::Dtmf(11)));
        assert_eq!(midi_note_to_drum_type(106), None);
    }

    #[test]
    fn dtmf_keys_use_row_and_column_tones() {
        // Key "5" (index 4): 770 Hz + 1336 Hz
        let drum = triggered(98);
        assert_eq!(drum.phase_inc, hz_to_phase_inc(770));
        assert_eq!(drum.phase2_inc, hz_to_phase_inc(1336));
        // Key "#" (index 11): 941 Hz + 1477 Hz
        let drum = triggered(105);
        assert_eq!(drum.phase_inc, hz_to_phase_inc(941));
        assert_eq!(drum.phase2_inc, hz_to_phase_inc(1477));
    }

    #[test]
    fn held_dtmf_sustains_until_released() {
        let mut drum = triggered(94);
        play(&mut drum, 48_000 * 10); // 10 s
        assert!(!drum.is_finished());
        assert_eq!(drum.env, 0xFFFF);

        drum.release();
        play(&mut drum, 480); // 10 ms
        assert!(drum.is_finished());
    }

    #[test]
    fn release_does_not_shorten_one_shot_drums() {
        let mut released = triggered(36);
        let mut untouched = triggered(36);
        released.release();
        for _ in 0..2_000 {
            assert_eq!(released.next_value(), untouched.next_value());
        }
    }

    #[test]
    fn ringer_notes_follow_keypad_order() {
        assert_eq!(midi_note_to_drum_type(82), Some(DrumType::Ringer(0)));
        assert_eq!(midi_note_to_drum_type(93), Some(DrumType::Ringer(11)));
    }

    #[test]
    fn held_ringer_sustains_until_released() {
        let mut drum = triggered(85);
        play(&mut drum, 48_000 * 10);
        assert!(!drum.is_finished());

        drum.release();
        play(&mut drum, 480);
        assert!(drum.is_finished());
    }

    /// Over `samples` of a held sound: the longest run of exact silence, and
    /// the index of the last sample that made sound.
    fn silence_profile(drum: &mut DrumSampler, samples: usize) -> (usize, usize) {
        let (mut run, mut longest, mut last_sound) = (0, 0, 0);
        for i in 0..samples {
            if drum.next_value() == 0.0 {
                run += 1;
                longest = longest.max(run);
            } else {
                run = 0;
                last_sound = i;
            }
        }
        (longest, last_sound)
    }

    #[test]
    fn every_ringer_rings_again_after_its_pause() {
        // The longest real pause is the US 4 s; every ringer must come back
        // after it and keep looping while held.
        for note in 82..=93 {
            let mut drum = triggered(note);
            let (silence, last_sound) = silence_profile(&mut drum, 48_000 * 14);
            assert!(
                silence < 48 * 4_100,
                "ringer note {note} was silent for {} ms",
                silence / 48
            );
            assert!(
                last_sound > 48_000 * 7,
                "ringer note {note} stopped looping at {} ms",
                last_sound / 48
            );
        }
    }

    #[test]
    fn bell_follows_the_us_cadence() {
        // 2 s ringing, 4 s silent, then ringing again.
        let mut drum = triggered(82);
        play(&mut drum, 48 * 2_010);
        for _ in 0..48 * 3_980 {
            assert_eq!(drum.next_value(), 0.0);
        }
        play(&mut drum, 48 * 20);
        let rings_again = (0..48 * 50).any(|_| drum.next_value() != 0.0);
        assert!(rings_again);
    }

    #[test]
    fn melody_notes_hit_their_pitch() {
        for note in 0..=127 {
            let hz = note_phase_inc(note) as f32 * crate::constants::SAMPLE_RATE / 4_294_967_296.0;
            let expected = 440.0 * libm::powf(2.0, (note as f32 - 69.0) / 12.0);
            let cents = 1200.0 * libm::log2f(hz / expected);
            assert!(
                cents.abs() < 0.01,
                "MIDI {note}: got {hz} Hz ({cents} cents)"
            );
        }
        for hz in DTMF_ROW_HZ.into_iter().chain(DTMF_COL_HZ) {
            let actual =
                hz_to_phase_inc(hz) as f32 * crate::constants::SAMPLE_RATE / 4_294_967_296.0;
            assert!((1200.0 * libm::log2f(actual / hz as f32)).abs() < 0.01);
        }
    }

    #[test]
    fn retriggering_a_drum_clears_held_state() {
        let mut drum = triggered(94);
        drum.set_drum_type(DrumType::BassDrum1);
        drum.trigger();
        play(&mut drum, 9_600);
        assert!(drum.is_finished());
    }
}
