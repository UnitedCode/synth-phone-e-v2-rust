# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is an embedded Rust firmware for a polyphonic synthesizer + vocal effects processor running on the [Daisy Seed](https://electro-smith.com/products/daisy-seed) (STM32H750 ARM Cortex-M7). The device processes microphone input with real-time vocal DSP (pitch shift, harmony, vocoding) while also serving as a polyphonic MIDI synthesizer with onboard drum synthesis.

## Commands

**Build (no flash):**
```sh
cargo build --release
```

**Flash to hardware** (requires probe-rs and connected Daisy Seed via SWD):
```sh
cargo run --release
# or via embed:
cargo xtask embed
```

**Run host tests:**
```sh
cargo test --target aarch64-apple-darwin
```
The `.cargo/config.toml` defaults to the embedded target, so the host target must be
specified explicitly. Unit tests live in `src/state_machine/mod.rs` under `#[cfg(test)]`
and are exposed via the `[lib]` target in `Cargo.toml`.

**Lint / format check:**
```sh
cargo fmt --all -- --check
cargo fmt --all
```

**Install required toolchain:**
```sh
rustup target add thumbv7em-none-eabihf
rustup component add llvm-tools-preview
cargo install cargo-binutils
# Install probe-rs: https://probe.rs/docs/getting-started/installation/
```

**Debug** (after `cargo run` or `cargo embed` starts a GDB server on `:1337`):
```sh
arm-none-eabi-gdb target/thumbv7em-none-eabihf/release/app
# then inside GDB:
# target remote :1337
```

## Architecture

### RTIC Task Model (`src/main.rs`)

The entire application runs under [RTIC](https://rtic.rs) (Real-Time Interrupt-driven Concurrency). Tasks are priority-ordered; higher number = higher priority. There is no OS or heap allocator — all data structures use `heapless` or fixed-size arrays.

| Task | Priority | Trigger | Role |
|------|----------|---------|------|
| `update_handler` | 8 | DMA1_STR1 (audio DMA) | Audio I/O, synthesis, FX output |
| `dma1_stream0_fft_task` | 7 | DMA complete | FFT-based vocal effects (pitch shift, harmony, vocoder) |
| `midi_batch_processing_task` | 3 | Software | Batch MIDI event dispatch to VoiceManager |
| `interface_handler` | 3 | TIM2 | Button/encoder scanning, AppState transitions |
| `display_update_task` | 2 | Flag | SSD1306 screen rendering |
| `usart1_interrupt` | 1 | UART RX | MIDI byte reception (31.25 kbps) |
| `startup_complete_task` | 1 | Monotonic timer | Exit splash screen |

**Shared resources** (protected by RTIC's ceiling-based locking): audio ring buffers, `AppState`, MIDI event queue, `VoiceManager`.

### Audio Pipeline

```
Mic In (I2S/DMA) → ring buffer → audio_handler()
  → sample-rate reduction / bit-crush
  → VoiceManager.process() (synth/drum voices)
  → vocal DSP (FFT pitch/harmony/vocode via synthphone-e-vocal-dsp crate)
  → stereo out ring buffer → I2S/DMA
```

The `synthphone-e-vocal-dsp` crate (external git fork) handles all FFT-based effects. It runs in a separate high-priority task on the second DMA completion.

### State Machine (`src/state_machine/mod.rs`)

`AppState` drives the UI and determines which audio processing path is active:

- `Splash` → startup animation
- `Processing(ProcessingProfile)` → active voice/FX mode, shows level meter
- `EffectsProfile(ProcessingProfile)` → same audio, shows effects parameters
- `Menu(MenuState, ProcessingProfile)` → encoder navigates `MenuItem` values

**ProcessingProfile** (5 modes, cycled via keypad long-press):
`PitchControl` | `Vocode` | `Dry` | `Harmony` | `Percussion`

**AppEvent** drives transitions. Events originate from `interface_handler` (encoder, buttons) and are dispatched to `AppState::transition()`.

### Voice System (`src/midi/voice_generator.rs`)

`VoiceManager<MAX_VOICES=8>` manages polyphonic voices. Each `HybridVoice` contains both an `Oscillator` (synth) and `DrumSampler`, switching between them via `VoiceTypeId`. MIDI channel 9 (0-indexed) routes to drum voices per General MIDI convention; all other channels route to the oscillator synth.

### Drum Synthesis (`src/audio/drum_synth.rs`)

`DrumSampler` synthesizes all drums digitally (no sample playback from QSPI flash). Uses fixed-point phase accumulation, LFSR noise, and per-drum envelopes. Sample counts at 48 kHz: Kick=4800, Snare=3800, HiHat=2400, Tom=4000, Clap=2900, Cymbal=7200.

### Display (`src/display/screens.rs`, `sprites.rs`, `text.rs`)

128×32 OLED (SSD1306 I2C). Screen selection is driven by `AppState`. Rendering uses `embedded-graphics` with a binary sprite atlas in `assets/`. Display updates are flag-gated so they don't block audio tasks.

### Input (`src/input/buttons.rs`)

4×3 button matrix (GPIO row/column scan) + rotary encoder with push button + dedicated hangup button. `scan_button_matrix()` runs in `interface_handler` and emits `AppEvent`s based on current `AppState`.

## Key Constants (`src/constants.rs`)

```rust
SAMPLE_RATE: f32 = 48_014.312   // Actual Daisy Seed rate
FFT_SIZE: usize  = 1024
BUFFER_SIZE: usize = 4096       // 4× FFT_SIZE
HOP_SIZE: usize  = 256
BLOCK_SIZE: usize = 2           // Stereo samples per audio frame
```

## Memory Map (`memory.x`)

| Region | Size | Address |
|--------|------|---------|
| FLASH | 128 KB | 0x08000000 |
| DTCMRAM | 128 KB | 0x20000000 |
| SRAM | 512 KB | 0x24000000 |
| RAM_D2 | 288 KB | 0x30000000 |
| SDRAM (ext.) | 64 MB | 0xC0000000 |
| QSPIFLASH (ext.) | 8 MB | 0x90000000 |

`opt-level = "s"` and `lto = true` are set in both dev and release profiles to fit within flash constraints.

## External Dependencies

- **`libdaisy`** (git fork) — Daisy Seed HAL: audio subsystem init, I2S, DMA, pin definitions
- **`synthphone-e-vocal-dsp`** (git fork) — FFT vocal effects: pitch control, harmony, vocoder, bit-crush, sample-rate reduction
- **`cortex-m-rtic`** — Real-time task scheduler (no OS)
- **`stm32h7xx-hal`** — STM32H7 peripheral drivers

RTT (`rtt-target`) is used for debug logging; output is visible via `probe-rs` during a debug session.
