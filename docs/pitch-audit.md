# Pitch, notes and WAV audit

The pitch audit started at firmware `main` revision `6403420`.
Firmware, host audio lab and ARM guest pin DSP `main` revision
`4cf708acb2c1744707a316c9fc37bf15b46d8f3e`, merged through
[DSP PR #11](https://github.com/nathansbradshaw/synthphone_e_vocal_dsp/pull/11).
It includes all streaming, harmony, formant and pitch-audit changes. Its DSP
implementation matches audit revision `5fb637c`; the only subsequent source
changes make two documentation links compatible with strict rustdoc.
Virtual Daisy stays at v0.1.1; its models are unchanged.

## Corrections

- Select the nearest scale note by musical distance (a frequency ratio), rather
  than linear Hz distance. Test inputs one cent either side of each geometric
  midpoint across all 24 keys now select the correct neighbor.
- Use precise equal-tempered base frequencies. Validate key, degree and
  transposition inputs before indexing note tables.
- Track periodic input with the existing streaming pitch history and move
  coherent harmonic lobes together. Independent FFT-bin phases previously
  weakened or displaced the intended pitch. Spectral refinement is accepted
  only near the period estimate, with octave checks for high source notes.
- Bound formant envelope gains and preserve the fundamental. This prevents
  amplified leakage from dominating corrected bright tones.
- Derive the firmware sample rate from the programmed PLL3/SAI clock.
  The pinned HAL requests `48,000 × 257` Hz for PLL3P and SAI divides by 256;
  HSE=16 MHz, M=3, N=178, FRACN=827 and P=77 yield 48,187.487 Hz.
  The previous 48,014.312 Hz synthesis constant was about 6.23 cents low relative
  to that clock, and DSP previously defaulted to 48,000 Hz. A register-based
  regression check and complete-production-ELF capture verify this correction
  in the model. Physical frequency/clock measurement remains unverified.
  Ringer melody increments, DTMF tones and the pitched triangle presets now
  derive their tuning from the same clock; melody tuning covers all 128 notes.
  For file replay, use the WAV's declared clock for DSP, oscillators, analysis,
  timing and output headers. Mono PCM/float files from 8–192 kHz are accepted;
  there is no silent resampling or stereo downmix. Tested rates are listed below.
- Correct the test oracle's last six minor-key roots to D, G, C, F, Bb and Eb,
  matching the device's key order. Compare interpolated autocorrelation peak
  heights so strong upper harmonics and fractional periods do not cause false
  octave errors. The estimator never receives the expected target note.

The old 192-case failure list mixed DSP problems with incorrect expected targets
and measurement errors. Its entries were removed only after every case passed
on both supported host platforms. No acceptance tolerance was widened and no
new failure allowance was added. The previous results remain in Git history.

## Local verification, 2026-11-08

| Check | Result |
| --- | --- |
| Steady pitch sweep, macOS and Linux | 720/720 on each, within 20 cents |
| Speed and correction-strength probes | Both respond |
| Audio/MIDI/tool tests, macOS and Linux | 124 tests on each |
| Firmware library tests, Linux | 52 pass |
| All 128 MIDI frequencies | Within 0.001 cent of independent A440 math |
| Keypad degrees and transpositions | 24 keys × 12 degrees × 5 shifts × 2 registers |
| Actual synthesized WAV notes | 128 notes × 4 rates; within 0.5 cent |
| File replay rates | 44,100 / 48,000 / 48,014 / 96,000 Hz |
| Dry file pitch and duration | Preserved; only the documented FFT tail is added |
| Fixed A4 target from 110/220/659/880 Hz | Correct output octave |
| QEMU Cortex-M7 guest | 14/14 PCM cases match host within 0.0001 peak error |
| Injected lost frame | Detected |
| Production ELF, native Rev5 serial capture | 48,099 stereo frames; 2.993 cents from 220 Hz; no FIFO errors or silent hops |
| Embedded release build | Fits the existing 128 KiB flash region; about 224 bytes remain after alignment |

Strict audio-lab Clippy, DSP library Clippy with all features, formatting and
the DSP's own tests also pass. Builds reuse the existing shared target directory.
All checks run locally with CI skipped.

The final native vocal capture boots ELF SHA256
`6da7f79324573f02bba4af0ea1cc3919ea56d1067d8faa4890f7a3136b32a800`.
Its channels match and RTT contains no warnings/errors. Native acceptance here
covers this targeted production capture; a fresh complete model/fault-suite run
is not claimed. QEMU's lost-frame control is listed separately above.

## Verification against merged DSP main, 2026-11-08

The firmware merge uses DSP `4cf708a` in all three manifests and lockfiles.
It also retains the independently completed keypad-release fix `c590ee2` from
the shared audit branch, including its seven regression tests.

- DSP: 50 unit tests, 6 integration tests and 12 doctests pass. The separate
  embedded-only 50-test suite, strict Clippy (all targets, all-feature library
  and embedded-only library), formatting, strict rustdoc and no-std Cortex-M7
  release build pass.
- Firmware library: 52 tests pass in the existing offline Linux ARM64 image.
  The macOS host build of this suite is unsupported by the STM32 dependency's
  ELF vector-table section; it is not counted as a passing macOS suite.
- Audio/MIDI/tools: 131 tests pass on each of macOS and Linux ARM64. The strict
  720-case pitch sweep passes on both platforms, with both correction controls
  responding. Strict host audio-lab and ARM-guest Clippy and formatting pass.
- QEMU: all 14 Cortex-M7 audio cases match host PCM within 0.0001 peak sample
  error. SysTick executes during processing, and the injected dropped frame is
  detected with 0.250108 peak sample error.
- Production release firmware builds in the unchanged 128 KiB flash region,
  with 288 bytes remaining after alignment. Existing firmware compiler warnings
  remain; a warning-free full firmware build is not claimed.
- A fresh Rev5 serial-boundary capture boots ELF SHA256
  `20161bf02eac174620b4e0fc6a83017c3baf9087524596b8b01dd3da27a7f996`.
  Both channels contain 48,099 frames, measure 220.382 Hz (3.003 cents from
  220 Hz), and have no silent hops or SAI FIFO underruns/overruns. The capture
  and independent audio grade both pass. This is a targeted production-audio
  check, not a fresh complete peripheral/fault-suite run.

These checks run locally, with GitHub CI skipped to conserve minutes. They do
not establish physical-board timing, analog behavior or listening acceptance;
no hardware was flashed.

## Existing audio file

`5_notes.wav` declares 96,000 Hz, mono float, 960,000 frames (10 seconds).
Despite its name, measured window pitches rise smoothly from roughly 440 to
560 Hz. It has no independent intended-note annotations.

The audit renders dry, tuned, male-formant and female-formant outputs at 96 kHz,
with a measured 1,025-frame pipeline delay used to align analysis windows.
In C major, all 54 corrected windows whose early/late input select the same
target are within 0.924 cent. Dry pitch differs by less than 0.044 cent.
Two windows per corrected mode cross scale-selection boundaries; their raw
errors remain in the CSV (approximately −42 and −27 cents). They are marked
`target_stable=false`, rather than graded as steady notes. That marker is an
inference from two half-windows, not an intended-note annotation or a guarantee
that the intervening audio is steady.

Reproduce from the firmware root:

```sh
LAB_HOST=$(rustc -vV | sed -n 's/^host: //p')
export CARGO_TARGET_DIR="$PWD/target"
export CARGO_INCREMENTAL=0
cargo test --release --locked --manifest-path tools/audio-lab/Cargo.toml --target "$LAB_HOST"
cargo run --release --locked --manifest-path tools/audio-lab/Cargo.toml --target "$LAB_HOST" -- sweep audio-lab-results/pitch-audit --strict
cargo run --release --locked --manifest-path tools/audio-lab/Cargo.toml --target "$LAB_HOST" --example audit_file -- 5_notes.wav audio-lab-results/5_notes 0
```

Audio and generated CSVs stay outside Git under `audio-lab-results/`. The example
requires an explicit key index and retains all windows, including transitions
and missing estimates. It does not automatically grade arbitrary recordings.

## Remaining scope

The 720-case sweep measures steady synthetic monophonic tones at the fast
default. It does not establish general singing accuracy, natural timbre,
expressive transitions, consonant handling or all speed/formant combinations.
Negative controls still reject genuine 100-cent and two-octave pitch errors.
MIDI pitch tables, keypad selection and generated tones are checked separately
from the pitch estimator. Physical Daisy timing, analog input/output, actual
interrupt deadlines and listening acceptance require device and listening tests.
No board was flashed.
