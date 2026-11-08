# Voice/note balance on updated `clankers`

The volume captures below record `clankers` at `cd4bb42`, including main
`a2119fd` and vocal DSP `59a1c1a`. The later [autotune speed update](autotune-speed.md)
pins DSP `cddeddc`; these retained volume captures remain tied to their original
images. The new
Tuned mode, separate synth octave, mode switches, drum/ringer/DTMF sounds, saved
settings, background flash task and emulator LDRD retry fix are retained.
These volume changes stay on `clankers` for hardware validation. Keep CI off;
use local verification and `[skip ci]` commits, without merging this experiment
into main.

## Routing and gains

Main's 6x melody makeup gain does increase raw MIDI loudness. It does not decouple
keypad tone from Voice Volume, because that tone was already inside the processed
output being scaled. This branch applies Voice Level to microphone samples before
the vocal DSP, and Synth Level independently to the Dry/Tuned keypad carrier and
raw melodic MIDI. The calibrated synth gain replaces the 6x makeup gain; the two
are not stacked. Vocoder excitation is unchanged.

MIDI channel 1 remains vocal control only, channel 2 sound only, channel 3 switches
between raw sound and vocal control by profile, and channel 10 is percussion.
Drum Volume, held-sound release and muted-voice polyphony behavior are preserved.
Percussion crush still affects all mixed sources; the common master and final
0.8 sample ceiling remain in the output path. A ceiling can distort loud sums;
it is not an automatic loudness normalizer.

| Setting | Meaning |
| --- | --- |
| Voice Level | Linear: 0 mute, 10 unity, 20 2x; MIDI CC44 |
| Synth Level | 0 mute, 10 unity, 20 2x; MIDI CC45; finer attenuation below 10 |
| Drum Volume | Main's existing independent percussion control |

For Synth Level `L` from 1..10, gain is `(L/10)^2`; above 10 it is `L/10`.
Examples: 1=0.01x, 3=0.09x, 7=0.49x, 10=1x, 20=2x. This gives 46.02 dB of
nonzero adjustment. Voice gain is still linear and can boost up to 6.02 dB.
MIDI maps each source CC with `floor(value*20/127)`; the test driver uses the first
CC value mapping exactly to the requested level.

The menu order and saved-settings record layout are unchanged from new main.
Existing saved Synth values 1..9 now produce more attenuation; re-adjust those
presets if necessary. Values 10..20 retain their gain anchors. Source changes
from the menu or MIDI use main's background persistence mechanism.
The save debounce now measures elapsed DWT core cycles using the configured
core clock. It does not count serviced UI interrupts: the first dense-vocal
capture still had 645 countdown ticks remaining after three seconds when UI
ticks coalesced. Wrap-safe elapsed-time checks preserve the two-second quiet
interval under that load while keeping flash work in the background task.

## Why the low end changed

The first actual-firmware quiet test failed: at 0.05 input peak, Voice=20 and
the old lowest nonzero Synth=1, the generated note was **3.50 dB above** the vocal.
That linear fader could not turn the note below that real quiet vocal without
muting it. The retained failure capture and a red/green PCM regression motivated
the additional attenuation. Defaults and the maximum gain did not change.

Nominal calibration still uses the actual pinned DSP with a 0.2-peak reference
vocal and velocity 127: unit Synth Level 10 matches processed vocal RMS within
1 dB for all four waveforms. This does not claim equal perceived loudness at
every pitch, input level or velocity. Very weak/noisy microphones can still
need appropriate analog gain; digital trim cannot recover preamp/ADC clipping.

## Actual-firmware hot/quiet test

The user-authorized source is the separated Clone Hero vocal stem
`chart-engine/.runtime/head-on-a-plate-stems/vocals.wav`, original seconds 30..40,
left channel only, resampled from 44100 to 48014 Hz. The native test uses its
first 3.1 seconds. There is no post-capture normalization or source replacement.

Preamp fixtures multiply the recording to 0.95 peak (hot) and 0.05 peak (quiet).
An extra 2.0-peak overload fixture is explicitly saturated before the serial ADC
boundary. It demonstrates unrecoverable input clipping, not a clean-audio pass.
The gain/saturation fixture is specified test input, not a measured analog model.

Build/save main's and this branch's firmware ELFs, then prepare the Rev5/WM8731
serial-PCM emulator with the documented `startup` command and
`VIRTUAL_DAISY_ELF`. Reuse the normal native and .NET caches. Run:

```text
cargo run --release --locked --manifest-path tools/virtual-daisy/Cargo.toml --target HOST_TARGET -- levels RENODE_RUNTIME PREPARED_DIRECTORY MAIN.elf CLANKERS.elf VOCAL_48014.wav OUTPUT_DIRECTORY
```

`HOST_TARGET` is the installed host triple. Set `VIRTUAL_DAISY_DOTNET` if needed.
Native paths, including spaces, are passed as structured arguments on Windows,
Linux and macOS. This fixture explicitly selects Rev5/WM8731; other codec profiles
retain their separate existing model checks.

The 21-case plan boots actual ARM firmware for every capture. Settings are sent
through USART1 and matrix keys through GPIO connections. No guest instructions,
RAM settings or DSP output samples are patched. The plan checks:

- hot voice at unity/half input trim and quiet voice with input boost;
- MIDI and Dry keypad minimum/maximum levels with microphone mute, in both regimes;
- Tuned keypad levels against Dry calibration;
- source range spanning below and above the real vocal in both regimes;
- unchanged synth output when the muted microphone's input level changes;
- zero DMA FIFO underruns/overruns, finite matching stereo and bounded output;
- saved source controls and no output-ceiling hits in the balanced demonstrations.

Short captures run one virtual second. Four listening demonstrations run three
seconds each: vocal alone, then vocal+keypad, then vocal+MIDI. Main is the A/B
baseline. Updated-branch presets are Voice=5/Synth=7 for hot input and
Voice=20/Synth=3 for quiet input; these are examples, not automatic matching.
Both use triangle A2, MIDI channel 2 and velocity 127.

The driver runs at most three captures concurrently, bounded to 600 wall-clock
seconds each. Timeouts and nonzero exits fail. `.completed.txt` files record
successful child exit and firmware hash; final hashes must remain unchanged.
WAV rates are checked against derived SAI-clock metadata, not relabeled 48014 Hz.
`levels.csv` measures seconds 0.65..0.95 and reports whole-capture peaks/ceiling
counts. `capture-plan.csv`, `preamp.csv`, stimuli, RTT and codec/clock/DMA logs
retain the input and execution evidence. Stale passes are removed before a run.

`levels-analyze OUTPUT_DIRECTORY` rechecks saved captures and their exit evidence.
The auxiliary `audio-lab levels-recording` remains a shared-DSP diagnostic; its
frozen older baseline variants are not a substitute for these updated-main ARM
captures. Historical results and linear-control examples belong to `17621ac`.

Physical gain/clipping, flash retention through power loss and calibrated silicon
timing still require a real board. This establishes the functional digital path.

## Updated-main results (2026-11-02)

All 21 actual-firmware captures passed. Main's hot-vocal reference measured
0.105079 RMS; keypad tone was 9.64 dB below it, while MIDI was 11.93 dB above it.
This confirms that main's MIDI boost works, while identifying the remaining
keypad routing and quiet-input adjustment problems separately.

On updated `clankers`, hot-input trim reduced the vocal by 6.34 dB. Both keypad
and MIDI span 46.02 dB and straddle the real vocal in both input regimes. The
quiet minimum MIDI level is 14.99 dB below the boosted vocal. The hot A/B
demonstration hit the 0.8 output ceiling 1170 times on main and zero times with
the selected branch settings; the quiet demonstration also has zero ceiling
hits on the branch. Both branch demonstrations saved their source settings.
All captures have zero DMA underruns/overruns and finite matching stereo.

The baseline ELF SHA-256 is
`94dffca92aa7c8abfa3cc61b8c9f145c01c722026b9bc98f3fa08906dc8b532c`;
the tested branch ELF is
`8c136ae6abd352d717ab009c0a35232a0b1549f1d514b50865ae56b42b8baf9b`.
Reanalysis verifies every completion marker against the expected image hash.
Replacing a high-level MIDI capture with the low-level PCM fails the range gate;
substituting another image hash also fails. Both remove an old PASS marker.

Local checks passed: 79 audio/source tests on macOS and Linux, 210 emulator/tool
tests on macOS, the analyzer CLI regression on Linux, and Windows GNU target
compilation of all tool targets. Windows runtime/audio-device behavior was not
executed on this Mac. No CI was used.

Listening pairs retain the captured gain and sample rate. They remove the same
initial 0.6 seconds from each three-second demonstration, put main first and
`clankers` second with a 0.4-second silent gap, and use PCM16 WAV for playback.
Untrimmed float captures and input fixtures are retained with the evidence.
