# ARM execution tests without a board

The complementary [virtual Daisy](virtual-daisy.md) now boots the production ELF
with STM32 peripherals and modeled audio IO. This page describes the smaller
QEMU guest used for sample-by-sample DSP parity checks.

The audio lab now builds and runs a real `no_std` ARM executable under QEMU's
`mps2-an500` Cortex-M7 model. The guest uses the Daisy firmware's
`thumbv7em-none-eabihf` target, DSP revision, locked dependencies and shared
`src/vocal_frame.rs` entry point. Its replay code also runs in the desktop lab,
so the two executions can be compared sample by sample.

This is the first hardware-emulation layer. It executes ARM floating-point code,
atomics, reset/vector startup and SysTick interrupts rather than recompiling all
DSP code for the desktop CPU. **It is not yet a virtual Daisy Seed.** The guest is
a test executable, not the production firmware ELF with the STM32 HAL and RTIC
task graph. Audio DMA and peripheral initialization are not being tested here.

## Run

Install a prebuilt emulator and Rust target:

```sh
# macOS; --force-bottle avoids QEMU's source-build tooling
brew install --force-bottle qemu
rustup target add thumbv7em-none-eabihf
```

On Debian/Ubuntu, the emulator package is `qemu-system-arm`. Then, from the repo:

```sh
LAB_HOST=$(rustc -vV | sed -n 's/^host: //p')
cargo run --release --locked --manifest-path tools/audio-lab/Cargo.toml --target "$LAB_HOST" -- emulate audio-lab-results/arm
```

The Rust runner builds the guest with `target-cpu=cortex-m7`, runs QEMU, captures
binary samples and checks the results. Set `AUDIO_LAB_QEMU` to an executable path
if `qemu-system-arm` is not on PATH. No board, audio interface, physical playback,
flashing, Python scripts or Python test framework is involved.

## What it checks

Nine fixtures cover silence, correction at three vocal pitches, dry mode,
harmony, vocoding, and two existing formant/pitch defects. Each fixture processes
24,000 input samples followed by a drained tail. The guest retains phase/envelope
state and exercises ring wraparound and the two-sample block/256-sample hop cadence.
It must also receive SysTick interrupts during the workload.

Every output sample is compared with the host execution of the same inputs and
settings. The acceptance threshold is a maximum absolute sample difference of
`0.0001` (full scale is 1.0). The initial Apple Silicon/QEMU 11.1.3 run was
**bit-for-bit identical for all nine cases**. The report also includes measured
pitch, RMS and silent-hop counts. Passing this comparison means the two executions
agree; the known formant/pitch defects remain defects in both executions.

The command then rebuilds the guest with `omit-frame`, intentionally drops one
dry-mode synthesis frame, and requires the comparison to detect it. The initial
fault produced a peak difference of about `0.243264`, well above tolerance. This
negative control must produce a complete valid capture; a guest crash does not
count as a successful fault-detection test.

Nonzero emulator exit, panic, timeout, malformed/truncated capture, non-finite
samples, missing interrupt evidence and mismatched audio all fail the command.
Each QEMU execution has a 30-second watchdog. The guest image is never flashed to
hardware. CI runs this command on Linux alongside the desktop quality sweep.

The results directory contains `arm-comparison.csv`, nine rendered WAVs, the QEMU
version, binary normal/fault captures, emulator logs and `emulator-summary.txt`.
Semihosting transports samples to the runner; it is not an emulated codec. Do not
print the binary capture as text. Listen to the WAVs when reviewing timbre.

## What is still needed for a full virtual Daisy

QEMU documents [Cortex-M7 support on MPS2 AN500](https://www.qemu.org/docs/master/system/arm/mps2.html),
which has different peripheral addresses from the STM32H750. The guest linker
limits flash and RAM capacities to 128 KB each at MPS2 addresses. This checks link
capacity, not the Daisy memory map, runtime stack high-water mark, or memory
access/cache timing. SysTick delivery tests interrupt execution, not production
scheduling deadlines. Host duration and emulator interrupt counts cannot be
converted into trustworthy Daisy processing time.

The [virtual Daisy guide](virtual-daisy.md) describes the implemented Renode
platform: the production ELF boots through HAL and RTIC startup and processes
modeled PCM through SAI/DMAMUX/DMA and the codec. Rust replaces the Python PWR/FMC
stubs, and the direct managed host blocks Python initialization. The model uses
H750/Daisy memory capacities and checks dropped RX DMA and silent-input faults.
MIDI bursts, scripted UI activity, broader peripheral coverage and full electrical
behavior remain future work.

Accurate performance prediction is a separate requirement: M7 pipeline, cache,
flash/external-memory wait states and bus contention would need calibration against
the physical board. Functional emulation can remove much of the manual debugging
loop without making those unvalidated timing claims.
