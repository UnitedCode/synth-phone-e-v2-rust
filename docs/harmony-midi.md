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

- 97 audio/MIDI/tool tests and 210 emulator-model tests pass on macOS and Linux.
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
