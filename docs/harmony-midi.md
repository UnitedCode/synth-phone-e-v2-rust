# MIDI vocal harmony on clankers

“Hide and Seek” needs keyboard-controlled vocal harmonization, beyond snapping
one singing voice to a scale. Heap describes shifting her voice to the notes she
played over MIDI in this [artist interview](https://aurgasm.us/2005/10/aurgasm-interview-imogen-heap/).
This implementation supplies that performance workflow; it does not reproduce
the original commercial processor or guarantee the record's timbre.

## Playing

1. Select **Harmony** on the phone.
2. Send chords on **MIDI channel 1** for vocal control without raw synth audio.
   Channel 3 also controls Harmony; channel 2 remains independent synth audio.
3. Send **CC47 = 0** for MIDI-only harmonies. **CC47 = 127** restores the original
   voice alongside the harmonies; intermediate values blend it in. Default is
   127. This is a transient performance control, reset on reboot, and does not
   change the saved-settings format. Voice Level still controls the vocal output.
4. Use **CC64** for sustain (64..127 down, 0..63 up). Releasing the pedal stops
   released keys and keeps keys still held. **CC123** releases keys and respects
   sustain. **CC120** stops sound immediately. **CC121** releases sustain and
   centers pitch bend. These controller numbers follow the
   [MIDI controller table](https://midi.org/midi-1-0-control-change-messages).

Pitch bend affects only its message channel, including notes played while bent.
The implemented wheel range is fixed at two semitones either side of center;
RPN bend-sensitivity negotiation is not implemented. Sound-only notes and drums
do not enter the vocal pitch cache after a bend or voice steal. Repeated NoteOn
messages reuse their note slot, including when an earlier slot is free.

## DSP and memory

The old Harmony path mixed magnitudes from every note into one shared frequency
and phase per FFT bin. Close notes could overwrite one another. An independent
sinusoidal projection test found the requested 220 Hz note almost absent in a
four-note chord (amplitude 0.00078 for a 0.2-peak source).

The 1024-point streaming API retains one phase offset per MIDI voice and moves
the bins of each vocal harmonic together, preserving their complex phase
relationships. It sums the voice spectra before one shared inverse FFT. The
fractional-bin interpolation accounts for the centre of the symmetric Hann
window. This adapts the frequency-domain harmonizing method of
[Laroche and Dolson](https://www.ee.columbia.edu/~dpwe/papers/LaroD99-pvoc.pdf)
to monophonic vocal harmonics. A fresh envelope is calculated every hop. YIN
selects the source harmonic and inter-frame phase refines its tuning; the old
forced octave folding is absent. Near-unison wet and dry remain coherent.

The first independent-bin implementation was a quality regression despite
passing steady-tone chord tests. Reconstructing phase from the nearest input
bin made that reference change as the sung pitch moved. A 200 Hz source with
five-Hz, 50-cent vibrato, corrected to a held 220 Hz MIDI note, put 55.88% of
output energy outside that note. The harmonic-region repair reduces this to
about 0.01% in the same controlled test. This is a regression diagnostic, not
a claim of universal perceptual quality or a percentage measured on a song.

Independent voices initially caused 26 dropped FFT jobs in the four-note
production-firmware fixture. A quarter-wave phase table with linear interpolation
replaces expensive per-bin trigonometry. Its tested absolute sine/cosine error is
below 0.000005. The optimized eight-note firmware fixture has zero dropped FFT
jobs, clear Cortex-M fault registers, and zero DMA underruns/overruns. These are
model results, not physical deadline measurements.

The state is initialized once in a dedicated, CPU-only AXI SRAM section.
DMA buffers remain together in their original D2 SRAM region. Initialization
explicitly writes NOLOAD storage and returns one exclusive reference to RTIC.
The initializer and DSP dispatch boundaries remain out of line. The earlier
per-bin state could inflate RTIC's initialization frame and cause an emulated
Cortex-M fault. The harmonic-coherent state occupies a 96-byte aligned SRAM
section instead of 16,448 bytes. Builds and production captures, including
Cortex-M fault-status checks, cover the memory layout.

Legacy DSP entry points remain available. Firmware, host replay and ARM audio
guest use the same pinned DSP revision; streaming pitch-correction state and
the previous source-volume controls are retained.

## Reproducing the local checks

Run the audio tests and the 720-case pitch sweep with your host target:

```text
cargo test --release --locked --manifest-path tools/audio-lab/Cargo.toml --target HOST_TARGET
cargo run --release --locked --manifest-path tools/audio-lab/Cargo.toml --target HOST_TARGET -- sweep OUTPUT
```

The harmony tests independently probe requested frequencies, cover eight-note
order independence, unison after silence, changing-pitch sidebands, and MIDI-only silence without keys.
MIDI tests cover retrigger/release, channel isolation, pitch bend, sustain and
panic. The established pitch sweep still has 192 known quality failures; this
change introduces no new failures at the default fast setting.

Local verification of DSP revision `d7148ad9dc6f285876bf11d31ddc4e4f268d646f`:

- 100 audio/MIDI/tool tests and 210 emulator-model tests pass on macOS and Linux.
- Windows GNU cross-target checks pass for all audio/emulator Rust targets;
  this does not substitute for running on Windows.
- DSP unit/integration tests pass, including the independent phase-table check.
- Ten Cortex-M7 DSP guest cases, including changing-pitch harmony, match host PCM within 0.0001 peak error;
  SysTick interrupts run and deliberate loss of an FFT frame is detected.
- Four real production-ELF scenarios pass on virtual Rev5: matched old/new
  four-note audio, MIDI-only sustain/release, and eight-note harmony. They have
  clear CFSR/HFSR, matching stereo, expected SAI rate, zero FIFO errors, zero
  dropped FFT/MIDI jobs and no unexpected emulator warnings.

These firmware examples use an unshifted natural vowel from an authorized local
separated vocal recording, not a singer performing “Hide and Seek.” The optional
external-MIDI preview is explicitly a host-DSP rendering of a repeated vowel.

Prepare a short external real-vocal vowel and physical-input scripts:

```text
cargo run --release --locked --manifest-path tools/audio-lab/Cargo.toml --target HOST_TARGET --example harmony_capture -- prepare VOCAL.wav OUTPUT
```

Input must be mono/stereo float WAV at 48014 Hz. The tool applies 0.35 gain,
starts the recording after 750 ms, and pads to 2.1 seconds. It generates generic
four- and eight-note tests, mode selection over GPIO, and actual MIDI bytes over
USART1. Each stimulus index is a ten-millisecond bridge step. The scripts compare
old/new firmware holding the same complete chord at the same lead setting, then exercise MIDI-only harmony with
key-up under sustain and pedal-up while the vocal continues.

Run each script through the prepared virtual Daisy with
`VIRTUAL_DAISY_AUDIO_STEPS=200`. Preserve the exact ELF hashes and successful
process exit in `firmware.txt` and each `NAME.completed.txt`, following the
existing capture format. Use the `harmony_capture inspect OUTPUT` example to
require startup completion, CPU guards, clear CFSR/HFSR, finite audible PCM,
zero DMA errors, and the expected sustain/pedal-release audio. The bridge emits
read-only CPU fault-status/register diagnostics as `NAME.cpu-faults.txt`.

## External song references

Keep downloaded MIDI, extracted note lists and song-derived audio outside this
repository. No transcription is needed for the generic regression tests. The
`midi_reference` example reads an external Standard MIDI File, reports its tracks,
tempo-aware note times, raw held-key count and unreleased keys:

```text
cargo run --release --locked --manifest-path tools/audio-lab/Cargo.toml --target HOST_TARGET --example midi_reference -- EXTERNAL.mid
```

Optional arguments `VOCAL.wav OUTPUT.wav START_SECONDS DURATION_SECONDS` render
up to 600 seconds through the shared host DSP. `full` instead of a numeric
duration renders to the end of the MIDI arrangement. This repeats the supplied vowel
with short edge fades and preserves MIDI timing and pre-existing held notes.
It is a host preview, not a production ARM capture or a singer performing lyrics.
Use mono float 48014 Hz vocals. SMPTE-timed MIDI is explicitly rejected.

`midi_reference INPUT.mid --merge-vocal-midi OUTPUT.mid` creates an external,
single-track channel-1 copy of a vocal arrangement. It preserves tempo, meter,
key metadata and note timing, and keeps shared pitches held until all source
voices release them. It deliberately strips programs, controllers and bends;
General MIDI CC7 setup messages would otherwise change the phone's master
volume, and per-channel pedal/bend meanings cannot be retained on one channel.
Repeated NoteOn within the same part is a retrigger, matching the phone's note
policy; independent parts holding the same pitch still release independently.
Use this only for vocal note arrangements; it does not identify or remove
instrumental parts. Input with missing NoteOff messages is rejected.

For listening, `harmony_capture listen INPUT.wav OUTPUT.wav` applies only a
constant playback gain, targeting 0.1 RMS with peak at most 0.9. It reports that
gain and preserves the raw capture. Level-matched listening copies help separate
pitch-content changes from loudness differences. Independent complex voices can
be quieter at the same firmware gain than the old shared-bin sum; compare raw
levels separately and use Voice Level on the device.

`harmony_capture match A.wav B.wav OUT_A.wav OUT_B.wav` chooses a common RMS
target that both recordings can reach with a constant gain and peak at most 0.9.
It requires matching rates and durations. Use this for A/B comparisons: separate
`listen` calls can reach different RMS levels when one file hits the peak limit.
Full-arrangement demonstrations use the same neutral vowel and external MIDI
through both DSP revisions; they are host-DSP renders. Short native captures
verify the production firmware and scheduling separately. These files do not
contain another song's lyrics or imply a performance by the original singer.

A public file's title does not establish that it is the vocal arrangement.
Inspect its register, track labels and chords before using it for rehearsal;
accompaniment bass notes may need a different arrangement or register.

## Isolating sound quality from the arrangement

The moving-sine regression above does **not** establish acceptable vocal quality.
The user rejected the repaired full-arrangement sound too. Independent held-note
diagnostics find remaining source-pitch and complex-voice problems; do not treat
successful firmware execution or note-presence tests as musical acceptance.

```text
cargo run --release --locked --manifest-path tools/audio-lab/Cargo.toml --target HOST_TARGET --example harmony_isolation -- VOWEL.wav NEW_OUTPUT_DIRECTORY 288
```

This uses the same vowel repetition, ten-ms edge fades, input gain, default DSP
configuration and overlap-add as `midi_reference`. It emits the exact dry input,
firmware Dry, G3 unison control (195.998 Hz), held A3 (220 Hz), and held A3/E4
(220/329.628 Hz) for the entire duration. All Harmony versions have the dry lead
muted. These are stationary diagnostic notes, **not** a song transcription.
The generated channel-1 MIDI files contain only the specified held keys and
their final releases. A regression test parses them to verify that contract.

`levels.csv` reports actual rendered levels, nonfinite values and limiter hits.
`input-pitch.csv` compares the production 1024-sample YIN estimate with an
independent 8192-sample autocorrelation estimate over one repeated vowel cycle.
The windows have different durations; inspect stable interior passages before
interpreting discrepancies. The supplied local vowel shows frequent half-pitch
estimates in the production detector while the independent estimate stays near
196 Hz. The G3 control is approximately its source pitch, not a zero-shift bypass:
it still depends on that detector and includes the singer's natural variation.

Synthetic probes additionally cover 90/130/200/300 Hz sources with a sine,
ten harmonics, ten harmonics plus vibrato, and moving harmonic amplitudes.
Their source and DSP clocks match, avoiding a clock-mismatch false positive.
The CSV measures energy outside exact target harmonics, plus independently
estimated pitch and RMS. This metric is sensitive to small tuning errors and
intentional amplitude modulation, so it must not be described as a general
distortion percentage or perceptual quality score. Broader voiced fixtures
expose defects that the single-sine vibrato regression does not catch.

These full-duration controls are host DSP renders with no deadline loss. Their
artifacts therefore cannot be attributed to physical CPU overload or a codec.
They establish that software/source issues remain; they do not prove either
that this algorithm can achieve the desired sound or that the Daisy cannot.
Keep the experimental DSP on clankers pending a better vocal-quality result.

The user subsequently judged the held A3/E4 real-vowel output to sound good.
The octave-estimate discrepancies are present with that same input too, so they
do not by themselves explain the rejected full-arrangement sound. Do not discard
that listening result or equate a diagnostic pitch statistic with bad sound.

### Separating polyphony, register and changing notes

A waveform regression now renders each voice separately and compares their
average with simultaneous 2/3/4/6/8-voice output. With the tested harmonic/vibrato
input, the largest difference is below 0.00000003. This tests held-voice summing,
not new-note/release transitions, perceptual quality or physical CPU deadlines.

`harmony_isolation VOWEL.wav NEW_OUTPUT_DIRECTORY 288 --polyphony` instead emits
the accepted A3/E4 reference, a four-note A-minor-seventh chord, and eight notes
containing that chord's pitch classes between C3 and A4. The two-note raw output
matches the earlier baseline byte for byte. These controls deliberately avoid
the full arrangement's extreme low register; their generated MIDI is external.

The full external arrangement previously used spans MIDI 26..76 (36.7..659.3
Hz), with 204 of 787 note attacks below C3 and 57 below C2. It has up to seven
held notes and 342 changes in count across successive positive-duration event
intervals. Driving all of these pitches as vocal parts was an inadequate test
of an unverified vocal arrangement, particularly for a source near 196 Hz.

To isolate this, generate external diagnostic reductions before rendering:

```text
midi_reference INPUT.mid --select-vocal-midi RANGE.mid 48 127 8
midi_reference INPUT.mid --select-vocal-midi TWO.mid 0 127 2
midi_reference INPUT.mid --select-vocal-midi RANGE_TWO.mid 48 127 2
```

This selection retains tempo and the original duration, filters the given note
range, and takes the highest held pitches up to the limit. It processes equal-tick
events together and restores a lower still-held note when a selected upper note
releases. Tests cover simultaneous notes, restoration, filtering and final silence.
The supplied file's range-only reduction has up to five held notes. This is a
counterfactual listening tool: it does not identify the song's true vocal parts,
rewrite device MIDI routing, transpose notes or impose a firmware voice limit.
The moving two-note reduction changes the selected voicing; the range reduction
changes both register and possible voice count. Compare them with held chords
before attributing a difference to polyphony alone.

`harmony_capture levels INPUT.wav` reports raw levels for matching controls.
The local polyphony listening pack uses one constant gain per complete file for
common RMS 0.05, no new dynamics processing, and identical MP3 encoding. Its
full-duration files use the shared host DSP; this audit changes no firmware DSP.

## Remaining acceptance gates

There are at most eight distinct harmony pitches. Source pitch detection still
uses YIN's existing nominal 80..600 Hz vocal range. Unvoiced consonants pass
through while a chord is held; with CC47 zero and no MIDI notes the voice is
muted. Large shifts, breaths, noisy or clipped microphones and wide vibrato can
still sound artificial. This change retains natural-envelope preservation;
Harmony's male/female formant selectors are not implemented by the new path.
The output remains mono duplicated to stereo, without a new reverb or spatial
choir engine. Analog preamp clipping cannot be recovered after the ADC.

The functional emulator checks firmware execution and modeled PCM, cache, bus,
codec and DMA behavior. It does not establish physical Cortex-M7 deadline margin,
analog behavior, or a commercial-processor match. Perform the same chord,
sustain, quiet/hot mic and mode-switch checks on a real board and listen before
merging this experimental firmware. Windows Rust cross-compilation is a build
check; native Windows execution remains a separate gate. CI stays off.
