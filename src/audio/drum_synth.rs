// Full GM1 percussion synthesis — notes 35–81, tuned for 90s 808/909 character.

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
}

// phase_inc = freq_hz * 89_443  (2^32 / 48_014.312 Hz ≈ 89_443)
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
                self.phase_inc = 314_839_360; // 3520 Hz (A7)
                self.target_phase_inc = 0;
                self.pitch_decay = 0;
                self.env_decay = 65_200;
                self.noise_mix = 0;
                self.phase2_inc = 0;
            }
            DrumType::OpenTriangle => {
                self.engine = SynthEngine::PureTone;
                self.max_samples = 28_800; // 600 ms ring
                self.phase_inc = 314_839_360; // 3520 Hz (A7)
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
        self.env = ((self.env as u32 * self.env_decay as u32) >> 16) as u16;

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
                    || (sc >= 192 && sc < 288)
                    || (sc >= 384 && sc < 480)
                    || (sc >= 576 && sc < 672);
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
        _ => None,
    }
}
