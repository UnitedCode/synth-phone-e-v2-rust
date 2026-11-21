# Firmware flash size

Measured on 2026-11-09 against `clankers-pitch-audit` baseline `f7a3b32`, using
Rust 1.97.1, the locked dependencies, release LTO and `opt-level = "s"`.
Both builds use the same existing 128 KiB flash region.

| Flash component | Before (bytes) | After (bytes) |
| --- | ---: | ---: |
| Vector table | 664 | 664 |
| Code (`.text`) | 89,804 | 86,532 |
| Constants (`.rodata`) | 40,168 | 37,992 |
| Initialized data | 8 | 8 |
| Occupied span, including linker alignment | 130,656 | 125,216 |
| Free flash | 416 | 5,856 |

The saving is **5,440 bytes (4.16%)**, giving about 5.72 KiB of free flash.
Debug symbols remain available in the ELF and do not occupy device flash.
The CPU-only vocal state, DMA buffers and other RAM allocations retain their
previous sizes. No flash region was enlarged or moved into external storage.

## Changes

- Compile explicitly for `cortex-m7`. The
  [STM32H750's double-precision FPU](https://www.st.com/en/microcontrollers-microprocessors/stm32h750vb.html)
  handles arithmetic that previously pulled in software double-precision
  helpers. The resulting ELF identifies Cortex-M7 and no longer links
  `__adddf3`, `__muldf3` or the related software float-conversion helpers.
- Replace the full 2,400-byte large ASCII font with a 280-byte atlas containing
  the eleven glyphs needed for note names, space and `?`. Character metrics and
  pixels are preserved. Other fonts and the sprite atlas are unchanged. See
  [font provenance and regeneration](../assets/note-font-README.md).
- Draw static key/mode/note labels directly from their existing string slices,
  avoiding format-and-copy operations and their impossible buffer-error paths.

The existing size-optimization level, DSP algorithms, controls, diagnostic
warnings and musical features are retained.

## Verification

- 150 audio/MIDI/UI tests pass on both macOS and Linux; 52 firmware library tests
  also pass on Linux. Strict audio-lab Clippy and formatting pass.
- Pixel comparisons cover every keypad note in all 24 keys, both text polarities,
  accidentals, space and the replacement glyph. Font regeneration is byte-identical.
- All 14 QEMU Cortex-M7 DSP/replay cases match host PCM within 0.0001 peak sample
  error; the injected lost frame is detected.
- The optimized production ELF boots in native Virtual Daisy Rev5 and produces
  48,099 matching stereo frames. Its tone measures 2.992 cents from the 220 Hz
  target, with no FIFO errors or silent hops.

Optimized ELF SHA256:
`aaea1e8f2cddc0a4408b3226f0e89b101ff4d17beb97534323b6c7f978feb347`.
Section reports, ELFs, test logs and native audio captures are retained outside
Git under `audio-lab-results/binary-size-2026-11-09/`.

These are functional model checks, not a physical-board deadline or listening
measurement. The complete emulator fault suite was not rerun. No board was flashed.

## Build and inspect

```sh
cargo build --release --locked
cargo size --release -- -A
```

`cargo size` comes from `cargo-binutils` and requires `llvm-tools-preview`.
Inspect the flash sections and their aligned load addresses when comparing
device footprint; the ELF file also contains debug and RAM metadata.
