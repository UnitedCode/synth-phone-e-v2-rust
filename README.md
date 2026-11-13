# Synthphone E

Rust firmware for a **Daisy Seed synth and vocal effects processor**
(STM32H750 Cortex-M7).

- **Voice:** pitch correction, formant effects, vocoder and MIDI-controlled harmony.
- **Instruments:** polyphonic MIDI synth and digitally synthesized drums.
- **Controls:** phone keypad, rotary encoder and 128×32 OLED.

## Build

Install [Rust](https://www.rust-lang.org/tools/install), then run these commands
from the repository root:

```sh
rustup target add thumbv7em-none-eabihf
cargo build --release --locked
```

**Output:** `target/thumbv7em-none-eabihf/release/app`. Building does not flash the device.

## Flash to a Daisy Seed

Install [probe-rs](https://probe.rs/docs/getting-started/installation/) and connect
the Seed through an **SWD debug probe**, then run:

```sh
cargo run --release --locked
```

The chip and runner are configured in [.cargo/config.toml](.cargo/config.toml).
For RTT logging and a GDB server on `localhost:1337`, use `cargo embed --release`
with [Embed.toml](Embed.toml).

## Modes

| On-screen name | What it does |
| --- | --- |
| Pitch Ctrl | Pitch correction and vocal pitch/formant effects |
| Vocode | Vocoder with note-controlled carrier |
| Synth+Vox | Synth with uncorrected vocals |
| Synth+Tune | Synth with pitch-corrected vocals |
| Harmony | MIDI-controlled vocal harmonies |
| Percussion | Synthesized drums |

For Harmony, start with chords on **MIDI channel 1**. See the
[MIDI harmony guide](docs/harmony-midi.md) for lead/backing channels and controls.

In Vocode, adjust **Vocode Amount** in the encoder menu: **0** is your natural
voice, **5** is a 50/50 blend (default), and **10** is the full effect.
Double-press the encoder to open the menu. The setting saves automatically;
MIDI **CC49** also controls it (0–127 maps to 0–10).

For Harmony, Pitch Ctrl and Synth+Tune, **Voice Range** selects the tracker’s
lowest source pitch: **All** = 50 Hz (default), **Mid** = 100 Hz, **Hi** = 150 Hz.
Try **Hi** for the tested vocal; use **All** for low voices. It saves automatically.
MIDI **CC50** values **0 / 64 / 127** select All / Mid / Hi.

## Check changes without hardware

These commands use a POSIX shell (macOS/Linux). The explicit host target is
required because this repo defaults to ARM.

```sh
LAB_HOST=$(rustc -vV | sed -n 's/^host: //p')
cargo fmt --all -- --check
cargo test --locked --lib --target "$LAB_HOST"
cargo test --release --locked --manifest-path tools/audio-lab/Cargo.toml --target "$LAB_HOST"
```

The audio lab checks vocal DSP and MIDI behavior. For WAV replay, pitch sweeps
and measurements, see [audio testing](docs/audio-testing.md).

To boot the production firmware in an emulator, follow the
[Virtual Daisy guide](docs/virtual-daisy.md). Initialize its pinned submodule first:

```sh
git submodule update --init tools/virtual-daisy
```

**Hardware validation is still needed** for real-time deadlines, analog audio
and musical feel. See the [conformance gaps](docs/daisy-conformance.md).

## Find your way around

| Location | Purpose |
| --- | --- |
| [src/main.rs](src/main.rs) | RTIC tasks, hardware setup and audio routing |
| [src/state_machine/](src/state_machine/) | Modes, menus and settings |
| [src/audio/](src/audio/) · [src/midi/](src/midi/) | Audio processing, synth voices and MIDI |
| [src/input/](src/input/) · [src/display/](src/display/) | Buttons, encoder and OLED |
| [tools/audio-lab/](tools/audio-lab/) | Host DSP tests and WAV tools |
| [tools/virtual-daisy/](tools/virtual-daisy/) | Pinned firmware emulator |

FFT vocal effects come from the pinned
[`synthphone-e-vocal-dsp` dependency](https://github.com/nathansbradshaw/synthphone_e_vocal_dsp).
For deeper checks, see [ARM DSP emulation](docs/arm-emulation.md) and the
[pitch audit](docs/pitch-audit.md).
