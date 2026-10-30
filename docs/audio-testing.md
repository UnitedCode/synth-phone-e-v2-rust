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
- Eight short WAVs: sine/bright inputs and outputs with each formant setting,
  including a reproducible failing case.

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

Strict acceptance currently fails. CI runs the regression tests, checks all 720
cases against the existing-failure list, and uploads the CSV/WAV results. A green
CI run means the protected behavior did not regress; it does not mean the known
pitch defects were fixed.

## Replay recordings

```sh
cargo run --release --locked --manifest-path tools/audio-lab/Cargo.toml --target "$LAB_HOST" -- render voice.wav processed.wav pitch 0
```

The last two arguments select `pitch`, `dry`, `harmony`, or `vocode`, and key 0–23
(the same ordering as the DSP library). Input must be mono PCM or float WAV at
48,000 or 48,014 Hz. Samples are treated as samples arriving at the board's
48,014.312 Hz rate; there is no resampler. A nominal 48 kHz recording therefore plays
about 0.03% faster, approximately half a cent. Output is mono float WAV at rounded
48,014 Hz, with a drained processing tail and the firmware's final 0.8 peak limiter.

For reproducibility the vocoder uses a fixed 220 Hz harmonic carrier; harmony uses
fixed 220/330 Hz MIDI targets. This does not reproduce the MIDI parser or live note
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

Another mismatch is preserved for a faithful baseline: the DSP's default sample
rate is 48,000 Hz while the device constant is 48,014.312 Hz. This is only about half
a cent and cannot explain the large failures. The lab deliberately does not change
the sound while establishing the baseline.

## Performance and hardware testing

CSV host p95/max timings measure a DSP hop, including its buffer copies and
overlap-add bookkeeping.
They can help compare versions on the same host/build configuration. They cannot
predict Cortex-M7 cycles, interrupt deadlines or physical round-trip latency.

The current two-sample DMA block arrives every approximately 41.65 microseconds;
a 256-sample FFT hop arrives every approximately 5.33 milliseconds. The FFT task
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
