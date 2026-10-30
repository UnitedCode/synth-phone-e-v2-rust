# Booting production firmware on a virtual Daisy

The virtual Daisy runs the **actual release `app` ELF**, including reset startup,
STM32 HAL initialization, RTIC tasks, interrupt handlers, UI startup and vocal
processing. It feeds PCM through a modeled WM8731/SAI/DMA path and records the
firmware's stereo output to WAV. No board, debug probe, audio interface, flashed
test image, or host replacement for the firmware DSP is required.

This is a functional Daisy Seed **1.1 / STM32H750** model, built on
[Renode 1.17.0](https://github.com/renode/renode/releases/tag/v1.17.0).
It is a substantial step toward a full virtual board, with explicit limits below.
It does **not** establish real-hardware timing or analog fidelity.

## Run

Install Rust's `thumbv7em-none-eabihf` target, a .NET 8 SDK, and the official
prebuilt Renode 1.17.0 release for Linux x86-64 or macOS ARM64. Do not launch
Renode's normal CLI for these tests. The Rust runner calls a small managed host
directly; it never constructs Renode's Python-backed Monitor.

From this repository:

```sh
rustup target add thumbv7em-none-eabihf
LAB_HOST=$(rustc -vV | sed -n 's/^host: //p')
cargo run --release --locked --manifest-path tools/virtual-daisy/Cargo.toml \
  --target "$LAB_HOST" -- run /absolute/path/to/renode-runtime audio-lab-results/daisy
```

The runtime directory contains `Infrastructure.dll`, `platform-lib/` and
`platforms/cpus/stm32h7.repl`. In the macOS disk image this is
`Renode.app/Contents/MacOS`; on Linux use the extracted portable archive directory.
CI pins and verifies the Linux archive SHA256. The command builds the native
Rust peripherals, the unchanged production firmware with its existing lockfile,
and the mandatory C# Renode API bridge. `dotnet` must resolve to an SDK, not just
a runtime. Set `VIRTUAL_DAISY_DOTNET` to an SDK executable if needed.

To boot an existing firmware build, set `VIRTUAL_DAISY_ELF` to its absolute path.
The runner copies it byte-for-byte, records its SHA256, and verifies the original
and copy are unchanged after all runs. It does not patch instructions, bypass
initialization functions, replace interrupt handlers, or modify firmware settings.
The CPU starts with the SP/PC from the flash vector table, as the chip does.
The ELF header's entry point is currently zero because of a pre-existing linker
entry-symbol warning; it is not used as the reset vector.

An optional fourth argument replays the first second of a user WAV after running
the standard regression tests:

```sh
cargo run --release --locked --manifest-path tools/virtual-daisy/Cargo.toml \
  --target "$LAB_HOST" -- run /absolute/path/to/renode-runtime audio-lab-results/daisy /absolute/path/to/voice.wav
```

Input must be mono/stereo PCM or float WAV at **48014 Hz**, at least one second
long, finite, and within full scale. The tool deliberately does not silently
resample. Output is `user.wav`; the A3 test's pitch/level gates are not applied to
arbitrary voice recordings. Boot checks still apply. Longer recordings, scripted
settings, MIDI events and virtual controls are future work.

## What must pass

Each boot runs for one second of virtual time, drains firmware RTT logs, and
requires both normal startup and the transition away from the splash screen.
Unexpected idle keypad presses, firmware faults, missing/truncated captures,
emulator exceptions and unknown warnings fail the run. Each emulator process has
a 180-second wall-clock watchdog. A crash or timeout never counts as successful
fault detection.

The standard input is a duplicated mono sine at **224.492958 Hz**, 35 cents above
A3, amplitude 0.2. On the unchanged default autotune settings both recorded
channels must satisfy:

| Measurement | Gate |
| --- | --- |
| Capture size | 46,000–48,100 stereo frames; WAV length equals model counter |
| Steady RMS, after 200 ms | 0.05–0.4 |
| Pitch, independent autocorrelation | Within 15 cents of 220 Hz |
| Peak / absolute DC / largest adjacent step | At most 0.4 / 0.02 / 0.05 |
| Non-finite, clipped samples, silent 256-sample hops | Zero |
| Minimum/maximum sustained-hop RMS ratio | At least 0.7 |
| SAI FIFO underruns and overruns | Zero |
| Left/right difference for duplicated mono fixture | At most 0.000001 |

The pitch gate rejects dry bypass: 224.49 Hz is outside the tolerance. These
thresholds test this steady fixture; they do not establish musical quality for
all singers or settings. Keep using the desktop 720-setting sweep and QEMU DSP
parity tests for that wider coverage.

The same ELF is then booted twice more. Silent input must fail the RMS criterion.
Suppressing RX DMA requests must fail RMS **and** produce SAI overrun evidence.
Both fault runs must still complete startup and produce valid captures. Unit
tests also check that the grader catches bypass, clipping, NaNs and missing audio.

The initial macOS production run measured **220.11 Hz, 0.9 cents error**, RMS
0.1583, 47,863 frames, matching left/right output, no silent steady hops, and no
FIFO underruns/overruns. Treat the generated report as the evidence for any
particular build; these numbers are not golden output samples.

## Model coverage and limits

Renode executes the Cortex-M7 instructions, floating-point code, NVIC, timers,
bus accesses and much of the STM32 peripheral initialization. Rust implements
the additional behavior needed by this firmware:

- Request-paced DMA1, 32-bit transfers, circular reload, double buffering,
  separate half/full interrupt flags and write-one-to-clear behavior.
- SAI A receive / B transmit FIFOs with signed 24-bit stereo samples, DMA request
  connections through DMAMUX and explicit under/overrun counters. TX prefill works
  before channel enable, matching the firmware's startup wait.
- GPIO input/output, pulls, BSRR and selected alternate-function signals.
  External board-version straps and idle controls survive peripheral reset.
- WM8731 register writes/reset, digital-format validation and active/power/mute
  gating; SSD1306 write acceptance. The OLED has no rendered framebuffer yet.
- PWR readiness, FMC SDRAM configuration, missing SYSCFG/ADC2 startup registers,
  and IS25LP064 QSPI startup commands: JEDEC ID, status, write-enable, register
  writes, QPI entry/exit and instruction lane validation.

The memory map constrains internal flash to 128 KiB and SDRAM to 64 MiB; it
retains the actual SRAM/DTCM regions. Generic H743 extra flash and huge unused FMC
windows are removed. The unused second flash-controller backing object is not
mapped onto the bus. QSPI memory-mapped reads, general flash read/program/erase,
other codecs/Seed revisions, USB audio, MIDI input injection, full ADC simulation,
and complete alternate-function electrical behavior are not implemented. Unknown
Rust-model registers/commands throw instead of returning invented success.

**Timing is nominal, not cycle accurate.** The CPU uses Renode's 480 MIPS setting,
SAI advances at a nominal 48014 stereo frames/second, and the scheduler quantum
is 1 microsecond. PLL fractional routing, flash wait states, cache coherence,
DMA/bus contention, SDRAM refresh and DWT cycle timing are not calibrated. Passing
this test cannot prove deadline compliance, performance gains, or freedom from
cache-related hardware bugs. Analog codec gain/filtering/noise and physical
signal integrity are also outside this model. A future hardware comparison is
needed to calibrate those behaviors.

`tools/virtual-daisy/known-warnings.txt` contains exact observed warnings for these
specific clock/cache/I2C timing fields, ADC calibration tags, UART fields, flash
latency and DWT unlock. The grader permits only those exact messages, without
timestamps. New unsupported accesses fail; do not expand the list to conceal a
functional failure. Native models reject unsupported accesses immediately.

The runner makes a **local copy** of Renode's managed infrastructure assembly and
replaces its Python-engine initializer with an unconditional managed exception.
Every boot verifies that initializer and that initialization is blocked. Platform
executable sections are rejected, and Python PWR/FMC models are replaced with
Rust. Some packaged assembly metadata references IronPython; its interpreter is
never initialized or executed. The installed emulator is unchanged. No Python
test runner, helper script or fallback is used.

## Results and next steps

`summary.txt` contains measurements and negative-control results. Other artifacts
include the exact `firmware.elf` and `firmware.sha256`, generated `daisy.repl`,
`baseline.wav`, fault WAVs, `.wav.stats`, host/peripheral logs, RTT logs and DTCM
captures. CI uploads these on both success and failure. Replay the WAVs to judge
timbre; objective pitch gates are only part of listening quality.

Next useful additions are event-driven virtual controls/MIDI, longer annotated
vocal fixtures with settings sweeps, full codec/flash/display behavior, and
calibration against a board's digital audio and cycle traces. These can build on
the same production ELF boot rather than requiring a separate DSP-only image.
