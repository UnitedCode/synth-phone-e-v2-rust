# Automated vocal testing

For booting the complete production firmware with modeled audio IO, see the
[virtual Daisy guide](virtual-daisy.md). This page covers the faster desktop DSP
lab and its wider parameter sweeps.

The Rust audio lab runs the firmware's actual pinned vocal DSP on a desktop. It
shares `src/vocal_frame.rs` with the firmware, imports the device's audio constants,
and checks that the DSP revision and its locked dependencies match. It needs no
Daisy, audio interface, Python, or flashing step.

It replays the input history, phase/envelope state, ring wraparound, hop scheduling
and overlap-add of the current vocal path. This is **audio simulation**, not a
cycle-accurate STM32H750 or complete firmware emulator. The simulation assumes each
FFT task completes before the next hop and runs after the current DMA block. RTIC
preemption, DMA/cache coherence, codec conversion, UI/MIDI load and analog circuitry
are outside its scope. The synth/drum mix and non-default output effects are also
outside this first version.

## Run it

Run from the repository root. An explicit host target overrides the firmware's
default ARM target on macOS, Linux and Windows shells with equivalent syntax:

```sh
LAB_HOST=$(rustc -vV | sed -n 's/^host: //p')
cargo test --release --locked --manifest-path tools/audio-lab/Cargo.toml --target "$LAB_HOST"
cargo run --release --locked --manifest-path tools/audio-lab/Cargo.toml --target "$LAB_HOST" -- sweep audio-lab-results
```

The sweep generates:

- `pitch.csv`: 720 cases across 24 keys, five vocal pitches, two harmonic shapes,
  and three formant settings. Inputs are 35 cents sharp. The target comes from an
  independent semitone/scale oracle; output pitch comes from normalized
  autocorrelation, rather than the DSP's own HPS/YIN estimate.
- `controls.csv`: whether changing correction strength and transition speed
  actually changes rendered audio.
- `summary.txt`: failures, newly failing cases, and improvements.
- Eight short WAVs: sine/bright inputs and outputs with each formant setting.

A pitch case passes when the sustained output has a measurable pitch within
20 cents of its independently calculated target, nonzero RMS, no non-finite samples
and no fully silent hops. The analysis omits startup and tail. The threshold is an
initial engineering criterion, not a guarantee of natural-sounding vocals.

`sweep` exits unsuccessfully if a previously passing case fails. Existing failures
are listed by case ID in `tools/audio-lab/known-pitch-failures.csv`; they remain
visible as failures in every report. Do not add entries to hide regressions. Remove
an entry when its fix is verified on both supported host platforms. To require
**every** case and control-response check to pass, add `--strict`:

```sh
cargo run --release --locked --manifest-path tools/audio-lab/Cargo.toml --target "$LAB_HOST" -- sweep audio-lab-results --strict
```

The November pitch audit reaches strict acceptance on macOS and Linux. See
[pitch and WAV audit](pitch-audit.md) for the fixes, measurement corrections and
remaining limits. The historical baseline below used an older estimator and an
incorrect target mapping for six minor keys; its failure count is not a count of
independently confirmed DSP defects.

## Reproduce vocal issue evidence

The original 2026-11-13 audit on `main` (`6b715db`) reproduced four defects.
On **`clankers-experiments`**, those four checks now run in the normal suite,
alongside four fixture controls and four carrier/formant/phrase regressions. They
assert the desired behavior with the original acceptance thresholds.

```sh
LAB_HOST=$(rustc -vV | sed -n 's/^host: //p')
export CARGO_TARGET_DIR="$PWD/target"
export CARGO_INCREMENTAL=0
cargo test --offline --release --locked --manifest-path tools/audio-lab/Cargo.toml --target "$LAB_HOST" --test vocal_issues
cargo run --offline --release --locked --manifest-path tools/audio-lab/Cargo.toml --target "$LAB_HOST" --example vocal_evidence -- audio-lab-results/vocal-evidence-NEW
```

The second command requires a new output directory and writes WAVs,
`measurements.csv` and `provenance.txt`. It uses the selected firmware DSP pin through the same
streaming entry point and overlap-add replay. All fixtures use exactly 48 kHz
for both processing and WAV headers, independent of the modeled device clock.

Each `*-ab.wav` plays A, a one-third-second gap, then B. Each segment is about
1.24 seconds including the DSP tail. A/B share one gain; neither segment is
independently normalized. These are synthetic vowels and broadband noise,
not human speech or physical-board captures. Append the original evidence
directory as an optional second argument to also produce `*-before-after-ab.wav`
files (archived original output first, experimental output second).

| File prefix | A | B | Expected behavior / original audit observation |
| --- | --- | --- | --- |
| `vocoder-grid` | Current vocoder: 200 Hz vowel, 440 Hz carrier | Known 440 Hz vowel reference with the same formant model | At least 90% of steady energy should follow carrier harmonics below 10 kHz; 5/12 source/target pairs fail, worst 66.87% retained |
| `unvoiced` | Pitch correction strength 0 | Current default strength 0.99 | Rejected unvoiced input should change by less than 1% relative RMS; measured 118.13% |
| `vocoder-formant` | Formant off | Male formant, ratio 0.5 | A meaningful formant adjustment should change output; measured exactly zero difference |
| `vocoder-backing` | MIDI lead and backing enabled | Backing group muted in the supplied MIDI gains | Group mute should change output; measured exactly zero difference |
| `pitch-formant-positive-control` | Pitch mode, formant off | Pitch mode, male formant | The same comparison detects a working formant control |
| `harmony-backing-positive-control` | Harmony, both groups enabled | Harmony, backing group muted | The same comparison detects a working group fader |

The carrier-grid measurement is a sinusoidal projection, not the DSP pitch
detector. Known carrier-pitch fixtures retain over 98% energy and a wrong-pitch
negative control is rejected. A 0.8-second measurement window contains integer
cycles for every tested carrier (110/220/330/440 Hz). The noise fixture is
independently checked with the public period detector: over 99% of windows
are rejected, while a known vowel is accepted. The 1%/5%/90% thresholds are
engineering acceptance criteria, not a claim about perceptual thresholds.

Backing tests now supply real `VoiceManager` frequencies and gain arrays through
the same carrier builder as the device, then into DSP; they do not emulate MIDI
interrupts. Gains must be applied before individual carrier notes are mixed,
because the DSP cannot recover those separate notes from one combined signal.
The original DSP-only backing test demonstrated the missing response but did
not enforce this ownership boundary. The updated test failed with unweighted
carrier generation before the gain multiplication was added.
The pitch reference demonstrates the
requested harmonic grid, not the exact timbre a repaired vocoder must produce.
This evidence does not establish real-vocal intelligibility or hardware timing.

The report also records scalar carrier attenuation. Its cancellation is
**diagnostic only**: carrier-level normalization can be intentional in a
vocoder, and is not counted as an independent defect. This experimental branch
uses fixed envelope calibration so that carrier faders remain linear; the
regression verifies 1/8 gain produces 1/8 RMS, and zero gain produces silence.

The experiment passes rejected unvoiced frames without pitch or formant
shifting and resets correction between phrases. Vocoding uses two spectral
moving averages to transfer the broad vocal envelope onto the original carrier,
with male/female settings warping that envelope. No bin-by-bin carrier
normalization is applied. Firmware, lab and ARM guest pin experimental DSP
`c29ac158fc04d19395d8cb43e0ea92d644fabaee`. Neither repository's `main` is changed.
The experimental render passes all 12 carrier cases with at least **99.94%**
of steady energy on the carrier grid. Rejected noise has zero difference
between correction strength 0 and 0.99; formant and backing controls now
produce different audio. Both formant modes retain the carrier pitch, and
a noise gap releases the previous phrase's correction.

Local verification on 2026-11-13: the host audio-lab suite and strict Clippy
pass; all 720 steady pitch cases pass the unchanged 20-cent limit. All 14
QEMU Cortex-M7 PCM cases match host output within 0.0001 peak error, and the
injected lost frame is detected. The production ARM release builds within
128 KiB flash (under 1 KiB remains after alignment); existing firmware compiler
warnings remain. QEMU executes the DSP test guest, not the complete device.
No board was flashed; real vocals, listening acceptance and physical timing
remain unverified.

### Adjustable vocoder amount on the experimental branch

**Vocode Amount** ranges from 0 (original voice) to 10 (full vocoder), default 5.
It is a linear dry/wet blend with identical analysis/synthesis windows and
overlap-add delay on both paths. The natural component remains audible without
held notes below 10; the fully wet setting still needs a carrier. The setting
only affects Vocode. MIDI CC49 maps 0–127 to 0–10 and requests a display refresh.
It is included in automatic settings saves.

The item is appended to the persisted menu order. Existing checksummed v2
records with 21 values restore all original settings and use 5 for the new
amount; current records store 22 values. The QSPI record codec is shared with
host tests, including a frozen legacy record and a corruption check.

DSP defaults to fully wet for API compatibility and existing full-effect
tests. The device's menu and `config_for_controls` supply the user's amount.
The ARM guest adds dry/half/full vocoder cases to verify mixing on Cortex-M7.
The initial amount-control DSP pin was `6cfec92ec95e92119654184949e7b9433acb750e`.
Validation: 146 host tests pass, including the new control/persistence checks;
DSP mix tests cover the dry endpoint and aligned midpoint. All 17 Cortex-M7
guest cases match host PCM, including dry/half/full amount, and the embedded
firmware fits its original 128 KiB flash region. Hardware flashing and physical
listening remain unverified.

Real-vocal examples at amounts 2, 5 and 10 are retained under
`audio-lab-results/vocoder-amount-2026-11-13/`. Listening copies use constant
whole-file gain to a common RMS of 0.1. The unmatched amount-10 WAV is
byte-identical to the preceding fully wet `c29ac15` vocal render.

## Before/after with real vocals

`vocal_recording` renders an existing **mono** WAV through the selected DSP
revision, producing pitch correction, a 220 Hz saw-carrier vocoder and male
formant vocoder files. It uses the file's clock and records settings and PCM
statistics in `render.txt`; it does not generate or replace the vocal.

```sh
cargo run --offline --release --locked --manifest-path tools/audio-lab/Cargo.toml --target "$LAB_HOST" --example vocal_recording -- INPUT_MONO.wav NEW_OUTPUT_DIRECTORY 5
```

Settings are identical across revisions: fast correction at 99% strength in
C major, no octave shift, and the device's final vocal trim/ceiling. The optional
last argument sets vocoder amount 0–10 (default 5); use 10 for the original
fully wet comparison. The key
is a test setting, not an automatic estimate of the recording's intended key.
To compare an older DSP, run the same example with a Cargo `patch` override
pointing to that revision's existing checkout, then restore the normal lockfile
before running the experimental revision. Record both exact DSP commit IDs.
Baselines predating `vocoder_mix` need the compatible frozen replay tool at
firmware `393ceeaa54589ce929e2675a3236527e336ae03a`; the original A/B below used it.
Keep unmatched outputs; use the existing `harmony_capture match` command for
constant whole-file playback gains, with no compression or extra effects.

The local 2026-11-13 comparison uses the previously authorized separated
`head-on-a-plate-stems/vocals.wav`, source seconds 30..40, left channel only,
with 0.35 input gain. Its 44,100 Hz clock is preserved. Baseline `4cf708a`
and experimental `c29ac15` use the same input and carrier. The final A/B files
play baseline, 0.6 seconds of silence, then experimental, with loudness matched
for listening; raw levels and provenance remain alongside them under the
ignored `audio-lab-results/real-vocal-ab-2026-11-13/` directory. Audio and source
recordings stay out of Git. These are host renders, not physical-board captures
or an automatic musical-quality grade.

## MIDI-driven vocoder scenario

`midi_vocoder` plays one selected SMF track through the device's VoiceManager
and saw-carrier builder, then applies the same real microphone recording to
the original and current vocoder kernels. It preserves note/rest timing,
sustain and pitch bend, and honors the global tempo map. Percussion is excluded;
select one melodic track with up to eight simultaneous held notes.

```sh
cargo run --offline --release --locked --manifest-path tools/audio-lab/Cargo.toml --target "$LAB_HOST" --example midi_vocoder -- SONG.mid VOCAL_MONO.wav NEW_OUTPUT_DIRECTORY TRACK_INDEX PREVIEW_START_SECONDS
```

Track indices are zero-based; preview start defaults to zero. The real mono
WAV must cover the complete MIDI duration (up to 600 seconds); it is not
automatically looped, stretched or replaced with synthetic vocals. The file's
sample rate configures both DSPs and carrier oscillators. Unmatched float WAVs,
matched ten-second previews, complete original/updated performances, MIDI-only
carrier preview and a provenance report are written to the chosen directory.
The before/after preview plays original, 0.6 seconds of silence, then updated.

The same renderer supports `--mode pitch` and `--mode harmony` (default:
`vocoder`). Both revisions use their complete streaming entry points with
independent state and identical settings, MIDI targets, replay timing and input.
Pitch uses 99% correction and speed 10; Harmony renders MIDI voices only with
`harmony_lead_gain = 0` by default; `--harmony-lead 0..1` includes the original
voice identically in both revisions. The report records the mode/config and the RMS of the
sample-by-sample before/after difference, including exact equality when zero.

For the local Hide and Seek audition, the source SMF is
`Downloads/other/hide-and-seek-imogen-heap.mid`. The existing `midi_reference`
notes-only merge retains all distinct held pitches (maximum eight); its
`--select-vocal-midi ... 0 127 1` reduction supplies the highest held pitch for
the autotune target. Both use MIDI seconds 0..30 with audition-only legato,
paired with Si Veo a Tu Mamá vocal seconds 100..130, left channel, gain 0.35,
native 44,100 Hz. This is a different-song vocal performance, not aligned
Hide and Seek lyrics. No vocal looping, splicing or resampling is used.
The initial `4cf708a`/`6cfec92` Harmony WAVs are byte-identical because that engine
was unchanged at that point. Autotune differs; neither difference size
nor equality establishes subjective musical quality. Four 96 kbps mono MP3s
are exported with common pre-encode RMS 0.07 under
`audio-lab-results/hide-seek-pitch-harmony-2026-11-13/` for sharing.

### Harmony pitch audit and regression proof

The first Harmony-audit DSP pin, shared by firmware, host lab and ARM guest, was
`67205e8a64103018a7972a5c485a1dc91a905761` on `clankers-experiments`.
Two public streaming audio tests first reproduced defects on the previous pin:

- A continuous 90 Hz singer targeting 220 Hz generated an unwanted 350 Hz
  partial at about 15% of the target's amplitude. FFT bin centres split one
  low vocal harmonic into separately shifted groups. Harmony now uses bounded
  phase-derived frequencies for harmonic ownership. The regression steps the
  singer through 90, 130, 200, 300, 440 and 590 Hz while holding 220 Hz, requiring
  tuning within 20 cents. The low-note output measures 219.99966 Hz, with the
  unwanted 350 Hz amplitude reduced to 0.00357% of the target (limit 3%).
- Unvoiced normalization's unity-gain shortcut omitted the original lead. With
  lead gain 0.5 and one harmony, the old output had gain 1 instead of the
  independent-voice power scale 0.74536. The shortcut now applies only when
  the lead is absent; otherwise its power and normalization both participate.
  Tests cover lead gains 0, 0.5 and 1, with one, two and four harmony voices.

The identical Hide and Seek MIDI window and Si Veo a Tu Mamá vocal passage
above were rendered again, using `--mode harmony`, both MIDI-only and with
`--harmony-lead 1`. The frozen `4cf708a` Harmony is also the pre-fix `6cfec92`
Harmony; its MIDI-only output compares byte-identical to the prior audition.
New output differs with RMS difference 0.002993 (MIDI-only) / 0.004908 (with
lead), and neither has quiet gaps in the measured steady region. These numbers
establish a change, not subjective improvement. Unmatched WAVs and provenance
are under `audio-lab-results/harmony-audit-2026-11-13-hide-seek*/`; 96 kbps mono
MP3s use matched whole-file RMS 0.07, about 361 KB per 30-second performance.
The optional lead A/B plays 30 seconds before, one second pause, then after.

Validation: 59 DSP tests and 153 host-lab tests pass, with strict host Clippy.
All 19 Cortex-M7 guest cases match host PCM exactly, including the new low
90 Hz voice and noise-with-lead cases; the injected missing frame is detected.
The production ARM release links: its last flash load segment ends at
`0x0801FEDC`, leaving only **292 bytes** of the 128 KiB region. No physical
flashing, listening acceptance or real-time deadline measurement is claimed.

For a continuous singing audition, add `--window MIDI_START DURATION --legato`.
The supplied vocal begins at the selected MIDI window and must cover that
duration. Legato holds the last note/chord through releases until the next
attack, keeping original attack timing and replacing the whole next chord.
It seeds the first chord for leading rests. This is an explicitly modified
audition, not unchanged SMF playback; the source MIDI and device's normal
NoteOff behavior stay intact. Both algorithms use the same legato carrier.

The supplied **Si Veo a Tu Mamá** stem provides an uninterrupted one-minute
passage at source seconds **100..160** (left channel, 0.35 input gain, native
44,100 Hz). The continuous audition uses HBFS MIDI seconds **43.2..103.2**,
track 2, with no vocal looping, splicing, resampling or added dry voice.
In 10 ms RMS windows below -60 dBFS, its input's longest quiet gap is 0.04s;
old/new vocoder output has at most 0.06s/0.14s, excluding pipeline startup/tail.
Legato renders reject gaps over 0.2s. This is an audio coverage check, not an
automatic claim about lyrical content or natural singing quality. The new
60-second listening files are matched to a common whole-file RMS. Evidence
is under `audio-lab-results/si-veo-midi-vocoder-2026-11-13/`.

The original DSP is a frozen **host-only dev dependency** at `4cf708a`, with an
exact exception in the dependency-parity test. Firmware and the ARM guest
retain only the current DSP. Both kernels use the same replay buffers and hop
timing; an independent constant-frame test checks the first job and overlap
positions, and the 17 ARM parity cases still pass.

The 2026-11-13 local scenario uses the supplied “Harder, Better, Faster,
Stronger” MIDI's **track 2, Pop Synth**, 351 notes with up to three held at once.
The selected melody is not a verified vocal transcription. Its full timeline
is 218.91 seconds; previews use MIDI seconds 19.2..29.2. The previously used
separated Head On A Plate vocal starts at its source second 10.8, left channel,
0.35 gain and native 44,100 Hz clock. Consequently the preview's voice is the
same original 30..40-second passage used earlier. No voice looping is needed.
This is not Daft Punk's voice, lyrics or a full General MIDI backing arrangement.

All comparisons here are fully vocoded: **no natural voice is mixed in**. The
additional `new80-old20` and `new90-old10` WAVs blend the *two algorithms* before
the final output trim, as listening previews. They do not change the device's
existing dry/wet Vocode Amount control. Preview versions are matched by constant
whole-file gain; full original/updated files share a common gain. The source
MIDI, recordings, generated audio and detailed provenance remain local under
the ignored `audio-lab-results/hbfs-midi-vocoder-2026-11-13/` directory.

### Vowel-dependent warbling and a strictly MIDI-gated audition

The current experimental DSP pin is
`47cd58cfe7bb3c910c3996b7989399b6b671cd8c`. A new public-stream fixture
keeps the singer at 150 Hz while changing odd/even harmonic balance at 2 Hz,
with a held 220 Hz MIDI target. The previous tracker alternated registers;
82.03% of output energy fell outside the target's harmonic bands, allowing
the fixture's legitimate amplitude sidebands. The new output has about 0.01%
outside those bands. A positive control verifies that the source fixture's
own harmonic bands capture its energy. This is a controlled reproduction,
not a perceptual score for the song recording.

Period selection now compares interpolated NSDF peak heights with a 99%
relative gate instead of accepting an early peak at 93%. Streaming Harmony
also stops overriding the period's register from even/odd spectral strength.
The shared period detector feeds streaming PitchControl too; its existing
regressions pass. The pitch-detector sweep now covers every semitone from
MIDI 40 through 74 at three phases, alongside the earlier off-grid pitches.
Held-target tests also cover true 150/300/150 Hz octave changes and wide vibrato.

`harmony_pitch_trace INPUT_MONO.wav` now uses the file's native sample clock,
chronological windows without looping, and a longer past-window reference
every eight hops. The first 30 seconds of the selected real vocal produce
5,136 analysis windows: no period in 1,495 before / 1,374 after, and 44 before /
68 after near-octave steps. Those counts do **not** establish that every jump
is false or that perceived warbling is resolved; real sung changes, unvoiced
phonemes and the reference's longer window complicate this recording.
`source-pitch-trace-final.csv` preserves that diagnostic evidence.

For the requested listening aid, `midi_vocoder --gate-midi` preserves normal
MIDI releases and forces output to zero whenever no selected MIDI note is
active, including queued overlap-add tails. Two-ms fades stay entirely inside
active-note intervals. It rejects combination with `--legato`. This is a host
audition option, not a change to the phone's default Harmony behavior. Its
stream test first demonstrates audible tails without the gate, then requires
exact zero during every rest and audible output during active notes.

The final pair uses the same Hide and Seek MIDI seconds 0..30 and Si Veo a Tu
Mamá vocal seconds 100..130 as before, with no vocal loops/splices/resampling.
The previous `67205e8` DSP was rebuilt with the same gate renderer, then all
manifests and locks were restored to `47cd58c`. Both gate-interval CSVs compare
identical. About 7.48 seconds of the 30.046-second files are MIDI-rest silence,
including the drained tail. Matched-level 192 kbps mono MP3s are 722,904 bytes
each under `audio-lab-results/harmony-warble-2026-11-13/`; unmatched WAVs and
exact source pins remain in `gated-previous/` and `gated-new/`.

Validation: 59 DSP tests, 156 host tests, strict host Clippy and 20 Cortex-M7
guest parity cases pass; the injected missing frame is detected. The initial
`760f06c` firmware exceeded flash by 64 bytes. Algebraically simplifying the
peak-height calculation in `47cd58c` restores a successful pinned release
build, with its final flash load ending at `0x0801FF94` (**108 bytes spare**).
The phone has not been flashed; physical timing and listening acceptance
remain unverified.

## Replay recordings

```sh
cargo run --release --locked --manifest-path tools/audio-lab/Cargo.toml --target "$LAB_HOST" -- render voice.wav processed.wav pitch 0
```

The last two arguments select `pitch`, `dry`, `harmony`, or `vocode`, and key 0–23
(the same ordering as the DSP library). Input must be mono PCM or float WAV at
8,000 through 192,000 Hz. The declared file rate configures the DSP, generated
carriers and output header; no resampling or channel mixing occurs. Output is
mono float WAV at the original rate, with a drained processing tail and the
firmware's final 0.8 peak limiter. File replay tests cover 44,100, 48,000, 48,014
and 96,000 Hz. The embedded path uses 48,187.487 Hz, derived from the pinned
HAL's programmed PLL3/SAI dividers; physical clock measurement remains pending.

For reproducibility the vocoder uses a fixed 220 Hz harmonic carrier; harmony uses
fixed MIDI 57/64 targets (220/329.62756 Hz). This does not reproduce the MIDI parser or live note
changes. Arbitrary recordings get audio/level statistics, **not** an automatic
accuracy grade: grading sung recordings needs intended note/timing annotations.

The regression tests also exercise silence in all four modes, dry-pitch preservation,
buffer wraparound and tail clearing, and detuned-input correction. Negative controls
include an incorrect pitch, NaN, overload, silence, and deliberately omitting one
synthesis frame. The dropped-frame test checks that the hop RMS metric detects the
resulting amplitude dip. These protect against plausible defects rather than just
comparing function return values to themselves.

## Initial measurements, 2026-10-30

DSP revision `ac8cc3582d3f0c0ee53ab0ad9af840e9d80ce6bc`, with the firmware's locked
dependencies and existing default configuration, measured on Apple Silicon:

| Formant setting | Passing | Failing |
| --- | ---: | ---: |
| Off | 202 | 38 |
| Male | 180 | 60 |
| Female | 146 | 94 |
| Total | 528 | 192 |

Some failures are large pitch errors; some have no reliable output-pitch estimate.
Formant reshaping can weaken the fundamental, so an estimator failure is a case to
investigate/listen to, not proof of a specific perceived note. Synthetic tones are
controlled probes, not a representative singing dataset or a percentage score for
the product's musical quality.

Two configuration fields have no observable effect in this probe:
`transition_speed` (default to 1.0) and `pitch_correction_strength` (default to 0.0).
Source review agrees: `calculate_pitch_shift` uses a hardcoded smoothing factor and
does not consume either field. The firmware passes a previous pitch ratio of 1.0
without updating it. Fixing this requires persistent correction state and tests for
note changes/settling time, not more manual adjustment of those configuration fields.

That describes the original baseline. The `clankers` stateful correction update
now connects speed and retains the ratio; both control probes respond. The fresh
default preserves the old fast sound, while lower settings glide. See
[autotune speed verification](autotune-speed.md) for its regression scope and
the remaining formant/estimator limitations.

Another mismatch was preserved for the original baseline: the DSP's default sample
rate is 48,000 Hz while the device constant is 48,014.312 Hz. This is only about half
a cent and cannot explain the large failures. The November audit removes that
mismatch in the shared firmware configuration.

## Performance and hardware testing

CSV host p95/max timings measure a DSP hop, including its buffer copies and
overlap-add bookkeeping.
They can help compare versions on the same host/build configuration. They cannot
predict Cortex-M7 cycles, interrupt deadlines or physical round-trip latency.

The configured two-sample DMA block arrives every approximately 41.50 microseconds;
a 256-sample FFT hop arrives every approximately 5.31 milliseconds. The FFT task
must finish with room for higher-priority audio interrupts. An average faster than
real time is insufficient; worst-case duration, scheduling delay and missed hops
matter. The next on-device instrument should record DWT cycle high-water marks,
deadline misses, failed task spawns and I/O errors, reporting counters outside the
audio interrupt. Use the actual configured CPU clock to convert cycles to time.

Hardware-in-the-loop can automate the remaining feedback: an audio interface plays
the same fixture into the Daisy and records its output; a MIDI connection supplies
repeatable note/control bursts. Correlating a marker estimates round-trip latency;
the same pitch, level and dropout metrics grade the recording. This still needs
the physical board and a known wiring/interface setup, but it can run hundreds of
settings unattended once connected. No device was flashed as part of this change.

Full board emulation is not a shortcut to those measurements. QEMU's documented
[STM32 models](https://www.qemu.org/docs/master/system/arm/stm32.html) do not include
the H750 and list DMA and I2S among missing devices. A CPU emulator can exercise
some ARM code, but a Daisy model would additionally need its clocks, SAI/DMA,
codec, external memory and timing behavior. Even functional peripheral emulation
would need validation before making performance claims.

There is now an [ARM execution layer](arm-emulation.md): a Rust command builds a
`no_std` Cortex-M7 guest, executes it in QEMU, compares its output samples against
the host, and verifies that an injected lost frame is caught. It requires no
physical board. It does not yet boot the complete production firmware or model
the STM32's audio peripherals. The guide describes the remaining virtual-Daisy work.

The next useful work is to add annotated real singing, vibrato/slides, consonants,
noise and note transitions; fix the exposed failures one at a time; then optimize
against both these quality checks and on-device deadline measurements. Keep short
listening comparisons for timbre and musical feel, which these numeric criteria
cannot fully assess.
