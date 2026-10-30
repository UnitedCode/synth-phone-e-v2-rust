# Booting production firmware on a virtual Daisy

The virtual Daisy runs the **actual release `app` ELF**, including reset startup,
STM32 HAL initialization, RTIC tasks, interrupt handlers, UI startup and vocal
processing. It feeds PCM through the selected codec/SAI/DMA path and records the
firmware's stereo output to WAV. No board, debug probe, audio interface, flashed
test image, or host replacement for the firmware DSP is required.

This is a functional STM32H750 model with explicit **Rev4/AK4556, Rev5/WM8731 and
Rev7/PCM3060** profiles, built on
[Renode 1.17.0](https://github.com/renode/renode/releases/tag/v1.17.0).
It is a substantial step toward a full virtual board, with explicit limits below.
It does **not** establish real-hardware timing or analog fidelity.
The [conformance ledger](daisy-conformance.md) records sources, corrected model
defects and the evidence still required. Every run reports hardware equivalence
as **UNVERIFIED**; a functional pass cannot certify it.

## Run

Install Rust's `thumbv7em-none-eabihf` target, a .NET 8 SDK, and the official
prebuilt Renode 1.17.0 release for Linux x86-64 or macOS ARM64. CMake and a C
compiler are also required for the pinned native instruction-fetch adapter.
The runner downloads SHA256-verified Renode infrastructure and tlib source
archives and builds a private ARM-M core. The adapter tags instruction
fetches separately from data loads and calls Rust before each translated block
executes. Stale decoded code is invalidated at the engine's existing safe restart
point. Register/cache behavior and probe generation/
grading are Rust; the existing engine's required C interface and managed Renode
adapters provide dispatch. No Python build/helper is used. The installed
Renode runtime is unchanged. Do not launch
Renode's normal CLI for these tests. The Rust runner calls a small managed host
directly; it never constructs Renode's Python-backed Monitor.

From this repository:

```sh
rustup target add thumbv7em-none-eabihf
LAB_HOST=$(rustc -vV | sed -n 's/^host: //p')
cargo run --release --locked --manifest-path tools/virtual-daisy/Cargo.toml \
  --target "$LAB_HOST" -- run /absolute/path/to/renode-runtime audio-lab-results/daisy --board rev5
```

The runtime directory contains `Infrastructure.dll`, `platform-lib/` and
`platforms/cpus/stm32h7.repl`. In the macOS disk image this is
`Renode.app/Contents/MacOS`; on Linux use the extracted portable archive directory.
CI pins and verifies the Linux archive SHA256. The command builds the native
Rust peripherals, the unchanged production firmware with its existing lockfile,
and the mandatory C# Renode API bridge. `dotnet` must resolve to an SDK, not just
a runtime. Set `VIRTUAL_DAISY_DOTNET` to an SDK executable if needed.

`--board` is required. `rev4`, `rev5`, and `rev7` select physical wiring,
revision GPIO straps and codec control together. An optional `--codec ak4556`,
`--codec wm8731` or `--codec pcm3060` asserts the expected part and rejects a
mismatch. `virtual-daisy profiles` lists the combinations. Seed2 DFM and Seed3
are separate boards and currently fail explicitly. Rev7's PD5 strap is distinct
from Seed2 DFM's PD4 strap; the pinned older HAL follows its compatible default
audio initialization path for Rev7.

`--require-hardware-match` refuses to run while equivalence is unverified. It is
a strict evidence requirement, not a way to enable additional emulation. There
is currently no independent hardware reference dataset, calibrated timing model
or validated analog/filter implementation capable of satisfying it.

To boot an existing firmware build, set `VIRTUAL_DAISY_ELF` to its absolute path.
The runner copies it byte-for-byte, records its SHA256, and verifies the original
and copy are unchanged after all runs. It does not patch instructions, bypass
initialization functions, replace interrupt handlers, or modify firmware settings.
The CPU starts with the SP/PC from the flash vector table, as the chip does.
The ELF header's entry point is currently zero because of a pre-existing linker
entry-symbol warning; it is not used as the reset vector.

Use `probe` in place of `run` to execute the generated ARM cache, flash,
writeback/exception and interrupt-during-wait programs with their negative
controls. `timing` runs only the latter two programs and their controls for
faster feedback. `probe-summary.txt` identifies the omitted checks; neither
command tests production audio. Set `VIRTUAL_DAISY_FLASH_MAXIMUM=1` to select
the datasheet maximum-duration flash scenario; the default uses typical values.
These are timing scenarios, not measured or guaranteed board latencies.

An optional fourth argument replays the first second of a user WAV after running
the standard regression tests:

```sh
cargo run --release --locked --manifest-path tools/virtual-daisy/Cargo.toml \
  --target "$LAB_HOST" -- run /absolute/path/to/renode-runtime audio-lab-results/daisy --board rev5 /absolute/path/to/voice.wav
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
| Capture size | 95–100% of one second at the derived SAI rate; WAV length equals model counter |
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

The same ELF is booted with five deliberate faults. Silent input must fail the RMS criterion.
Suppressing RX DMA requests must fail RMS **and** produce SAI overrun evidence.
Making the DMA MPU region cacheable must corrupt audio and record cacheable DMA
traffic. Redirecting RX into DTCM must corrupt audio, set TEIF, and clear stream EN.
Holding its SRAM target busy with synthetic competing traffic must delay DMA
completion and disrupt audio with FIFO under/overrun evidence.
Faults are MMIO writes after startup; the ELF is never patched. Every fault run
must still complete startup and produce a valid capture. Unit
tests also check that the grader catches bypass, clipping, NaNs and missing audio.
Rev5 also boots through the WM8731 ideal line stage and checks its audio output.
Each run also executes a separate generated-code probe on the emulated M7 CPU.
The ARM program writes a new instruction through the data cache, records stale
return values when either clean or invalidate is omitted, and records the new
value after DCCMVAU/ICIMVAU plus DSB/ISB. A second phase overwrites code through
the DMA/physical memory path and checks stale instructions until ICIALLU. The
production ELF is unchanged; the probe has its own process and results buffer.

A separate ARM flash probe runs from ITCM. It unlocks the controller, erases
sector zero, waits through a flash data read, programs a 256-bit word using
STRD/STMIA stores, and executes the new function only after I-cache maintenance.
It checks BSY/QW, EOP and the absence of accidental duplicate strobes on retried
instructions. Bypassing CPU data waits must return the old word before erase
completion and fail the result gate. Exceptions and timeouts cannot pass it.

With the derived clock, the macOS production fixture measures about **220.90Hz,
7.05 cents error**, RMS 0.1585, matching left/right output, no silent steady hops,
and no FIFO underruns/overruns. Earlier 220.11Hz results used the fixed timer and
are superseded. Treat the generated report as evidence for a particular build;
these numbers are not golden output samples.

## Model coverage and limits

Renode executes the Cortex-M7 instructions, floating-point code, NVIC, timers,
bus accesses and much of the STM32 peripheral initialization. Rust implements
the additional behavior needed by this firmware:

- Request-paced DMA1, 32-bit transfers, circular reload, double buffering,
  separate half/full/error interrupt flags and write-one-to-clear behavior.
  DMA1 cannot access ITCM/DTCM or RAM holes; illegal transfers set TEIF and clear EN.
  Configuration writes are protected while enabled; double-buffer inactive
  pointers remain writable. Other DMA widths, bursts, FIFO packing, memory-to-memory
  and non-audio request paths are not implemented.
- Revision-specific SAI receive/transmit FIFOs with signed 24-bit stereo samples, DMA request
  connections through DMAMUX and explicit under/overrun counters. TX prefill works
  before channel enable, matching the firmware's startup wait.
- GPIO input/output, pulls, BSRR and selected alternate-function signals.
  External board-version straps and idle controls survive peripheral reset.
- WM8731 register writes, documented reset values, both-channel loads and separate
  ADC/DAC/channel mute gating; AK4556 GPIO reset; PCM3060 hardware control;
  digital-format/direction validation; SSD1306 write acceptance. The OLED has no
  rendered framebuffer yet.
- PWR readiness, FMC SDRAM configuration, missing SYSCFG/ADC2 startup registers,
  and IS25LP064 QSPI startup commands: JEDEC ID, status, write-enable, register
  writes, QPI entry/exit and instruction lane validation.
- Shared physical RAM with a separate CPU data-cache view: 16KiB, four ways,
  32-byte lines, MPU memory attributes/priority/subregions, write-through and
  write-back policies, MVA and set/way clean/invalidate operations. DMA bypasses
  both the CPU cache and CPU MPU permissions. CMSIS can enumerate all sets/ways.
- A separate 16KiB, two-way, 32-byte instruction-cache view shared by user flash
  and executable system RAM. Flash literal loads use the separate data cache.
  Fetches never forward dirty data-cache bytes. ICIALLU/ICIMVAU update that
  view and invalidate Renode translations so the CPU actually observes the change.
  Shared/noncacheable memory bypasses caching; execute-never memory is checked.
  CSSELR's level bits are read-only zero and CCSIDR reports the documented encoding.
- Runtime block-entry fetching, including reused translations and flash/RAM
  conflict eviction. Decoding peeks at bytes without allocating cache lines;
  execution fetches them. A changed cache view forces retranslation before the
  stale body can run. This is a functional block-granular model.
- Simultaneous single-beat DMA requests are collected through DMAMUX, then
  arbitrated by software priority and stream number. FIFO request levels are
  checked again after each word; receive traffic no longer gets unconditional
  priority merely because the host serviced it first.
- Daisy's 16MHz HSE, rational PLL/SAI clock calculation, fractional shadow-register
  latching, PLL write protection, SAI divider/oversampling selection, and clock
  gates. APB timer kernels and SysTick follow their prescalers. DWT CYCCNT follows
  the core clock with 32-bit wrap, trace enable, software lock and writable count.

The default input boundary remains **serial PCM**, after ADC processing. Raw input
is timestamped at 48014Hz and linearly interpolated at the derived SAI clock.
WAV output uses the rounded derived rate; `.wav.clock.txt` retains its exact ratio.
The production PLL/SAI register sequence currently derives **48187.487053Hz**:
16MHz / 3 × (178 + 827/8192) / 77 / 256. This is a register-derived expectation,
not a physical measurement; firmware sample-rate constants deserve a separate
audit. The runner no longer hides this difference behind a fixed 48014Hz timer.
Captures that change rate after audio starts fail instead of mislabeling a WAV.

For **Rev5 only**, add `--audio-boundary ideal-line` to place normalized input
before the WM8731 nominal line gain and digital DC-removal stage. This implements
1.5dB gain steps (-34.5..+12dB), individual line mutes, and the published
H(z)=(1-z^-1)/(1-0.9995z^-1), including ADCHPD/HPOR offset retention. It is an
idealized floating-point stage, not a complete ADC. Normal-mode Type1's nominal
group delay is represented by separate 20-frame ADC and 18-frame DAC passband
delay lines. These are delay surrogates, not reconstructed FIR filters: gain
tolerance, internal rounding, complete decimation/interpolation responses and
analog circuitry remain unmodeled. `*.wav.codec.txt` records the boundary,
nominal delays and whether the surrogate was applied. The serial PCM boundary
does not add converter latency. Rev4/Rev7 reject ideal-line input. Unsupported active WM8731 sample-clock
ratios, de-emphasis, microphone and analog-bypass paths fail explicitly.

The memory map constrains internal flash to 128 KiB and SDRAM to 64 MiB; it
retains the actual SRAM/DTCM regions. Generic H743 extra flash and huge unused FMC
windows are removed. The unused second flash-controller backing object is not
mapped onto the bus. Unloaded user flash reads as erased ones and survives CPU
reset. CPU and DMA programming stores use the same flash controller. It models
the key sequence/lock, PG, the 256-bit write buffer, duplicate/inconsistent-write
flags, forced partial-word programming, two queued operations, sector/bank erase,
busy flags, EOP, interrupt enables and W1C clearing. Physical changes occur at
completion; programming never invents a 0-to-1 transition. Reprogramming an ECC
word without erase, option/security commands and flash CRC fail explicitly.
QSPI memory-mapped reads,
Seed2 DFM/Seed3 codecs, USB audio, MIDI input injection, full ADC simulation,
and complete alternate-function electrical behavior are not implemented. Unknown
Rust-model registers/commands throw instead of returning invented success.

**Instruction timing is not cycle accurate.** The CPU retains Renode's 480 MIPS
pacing. The SAI phase accumulator preserves fractional clock rate over time,
but events have 1-microsecond resolution. CYCCNT counts core-clock periods of
virtual elapsed time; it cannot measure true M7 instruction costs. PLL lock,
oscillator tolerance, the complete CPU store buffer and SDRAM refresh contention
remain unmodeled. Dirty cache lines now use the shared fabric as described below.
Data-cache replacement uses deterministic
round-robin, not the silicon's replacement sequence, and the M7 dynamic
read-allocation optimization is not reproduced. Flash and RAM are fetched through Rust when translated
blocks execute, including reuse. Lines for a linear block are fetched at entry;
this does not reproduce individual pipeline fetch cycles, speculative paths or
the exact victim sequence of silicon. Fetch statistics describe block-entry
opportunities, not a retired-instruction trace. Pipeline behavior when barriers
are missing, speculative fetches, full MPU fault semantics and general peripheral kernel-clock muxes are
not validated. Passing cannot
certify deadline compliance, CPU headroom or complete cache correctness.

`*.sdram.txt` decodes the FMC kernel mux, SDCLK divider, bus width, CAS, RPIPE,
TRCD/TRP/TRC/TMRD and refresh COUNT. It reports analytical lower bounds for an
open/precharged/conflicting-row read, a 32-byte line, refresh occupancy and mode
register quiet time. Field encodings include the specified plus-one timing
conversion; refresh periods use COUNT directly, without adding the software's
20-cycle safety margin. Live SDRAM reads use the open-row lower bound as a
declared service scenario; writes use transfer clocks. FMC queues, row state,
refresh scheduling and SDRAM retention still need implementation.

`*.flash.txt` and `*.memory.txt` report completed flash operations, the selected
timing scenario and coupled CPU/DMA wait counters. DS12556's typical/maximum
program times depend on PSIZE; x64 uses 100/200µs and sector erase uses 1/2s.
Bank erase uses the published mass-erase timing scenario (x64 5/10s); exact
H750 bank latency is uncalibrated. Voltage, wear and temperature are not modeled.
Both the separately tested pipeline and live shared fabric enforce one active
plus two waiting flash reads. Live CPU and DMA requests reuse the 256-bit bank
buffer on consecutive reads to the same flashword. Lookup occurs at grant;
completion replaces the buffer. A hit avoids the array wait but retains the
declared bus transfer cost. Programming/erase invalidates the buffer. Request
arrival, interface turnaround and speculative overlap are not cycle accurate.

The AXI GPV's six initiators expose read/write QoS and issuing overrides. The
live arbitration model implements higher QoS first, least-recently-served
equal-priority requests per target/channel, and Table4's outstanding limits.
CPU cache misses/uncached transfers and DMA source/destination ports submit
transactions to a shared event-driven fabric. Target costs use HCLK/PCLK/FMC
registers and explicit lower-bound service assumptions. Busy targets and issuing
limits delay requests. CPU wait debt consumes virtual time without retiring
instructions; stalled multi-access instructions replay accepted accesses so
they cannot write flash twice. DMA preserves NDTR and IRQ state until the
destination completes, including deferred EN clear. Reset ties use a deterministic
port order because the initial hardware ranking is not specified.
`*.dma.txt` separately records live DMA grants and simultaneous-request contests.
DMA service is polled on the 1µs event grid. Separate AXI read/write service lanes,
AHB round-robin service and request arrival times are explicit model choices;
pipeline/target overlap remains incomplete. Coupled stalls are functional
scenarios, not cycle-accurate hardware deadline estimates.

Dirty eviction and clean maintenance snapshot a 32-byte line and enqueue a CPU
write on the shared target. Physical memory changes only at completion, so DMA
cannot observe an early clean. Later cached stores dirty the line again without
changing an earlier snapshot. DSB waits for pending modeled writebacks; DMB
orders subsequent modeled RAM accesses. The internal maintenance queue is not a
model of silicon buffer depth. Same-line refill waits conservatively for pending
writeback; forwarding, write merging, critical-word-first refill and full MMIO/
TCM ordering remain incomplete.

The native exception adapter checkpoints entry after acknowledging the IRQ and
allocating its frame, then resumes vector/stack service without stacking twice.
Vector fetch and stack stores use separate I/D requests. Vectors use the default
memory map and instruction-cache view, bypassing MPU attributes. Busy flash
vectors wait unless served from I-cache. External unstack reads are prepared and
latched before restoring CPU state. A pending enabled IRQ can abandon a normal
memory load's unconsumed wait budget and restart that load after the handler;
Device/Strongly-ordered loads retain their wait. This does not reproduce late
arrival, complete tail-chaining timing, precise fault delivery or the M7 pipeline.
The ARM fixture verifies a basic SRAM frame and return; external floating-point
frames and snapshots/reset during in-flight operations still need independent
coverage. No fixed exception-entry cycle count is claimed.

`tools/virtual-daisy/known-warnings.txt` contains exact observed warnings for these
I2C timing fields, ADC calibration tags, UART fields and flash latency.
The obsolete PLL/cache/DWT warnings have been removed as those paths gained
models. It also allows the exact observed two-wakeup diagnostic
from Renode's [host-thread condition loop](https://github.com/renode/renode-infrastructure/blob/master/src/Emulator/Main/Utilities/MonitorCondition.cs),
which rechecks its wait predicate after a redundant wakeup; it does not indicate
a dropped virtual interrupt. The grader permits only those exact messages, without
timestamps. New unsupported accesses fail; do not expand the list to conceal a
functional failure. Native models reject unsupported accesses immediately.

The runner makes a **local copy** of Renode's managed infrastructure assembly and
replaces its Python-engine initializer with an unconditional managed exception.
Every boot verifies that initializer and that initialization is blocked. Platform
executable sections are rejected, and Python PWR/FMC models are replaced with
Rust. Some packaged assembly metadata references IronPython; its interpreter is
never initialized or executed. The installed emulator is unchanged. No Python
test runner, helper script or fallback is used.

The same private assembly changes the initial skipped-instruction budget check
to dispatch its existing virtual getter, allowing the CPU adapter to cancel an
interruptible memory wait on its CPU thread. Its two subsequent arithmetic reads
are unchanged. The pinned IL shape is checked before patching and attested at
each boot; an incompatible emulator fails explicitly.

## Results and next steps

`summary.txt` contains measurements and negative-control results. Other artifacts
include the exact `firmware.elf` and `firmware.sha256`, generated `daisy.repl`,
`conformance.txt` with model/firmware/platform hashes and explicit evidence gaps,
`baseline.wav`, fault WAVs, `.wav.stats`, host/peripheral logs, RTT logs and DTCM
captures. `.memory.txt` reports cache fills/writebacks and cacheable DMA traffic;
`.bus.txt` records DMA status/control/count, and `.wav.clock.txt` records clock
rates and the programmed PLL/SAI fields. CI uploads these on success and failure.
`cpu-core.txt` fingerprints the native core and its pinned source revisions;
`architecture-probe.bin`, `*.architecture.bin` and `*.architecture.txt` preserve
the ARM test program, processor-written outcomes and Rust grade. CPU builds are
cached under the output's `cpu-core/`; `VIRTUAL_DAISY_CORE_CACHE` can select a
shared cache directory. Archive hashes are verified on every run.
The ARM probe executes flash and RAM functions in the same cache set. Its
conflict-victim expectation tests the declared replacement approximation.
`runtime-cache-fault.architecture-negative.txt` must confirm that disabling the
runtime hook produces the wrong ARM return value. Exceptions and timeouts cannot
count as that negative control passing.
`flash-probe.bin`, `*.flash.bin`, `*.flash-probe.txt` and `*.flash-negative.txt`
preserve the flash program, CPU-written results and wait-bypass rejection.
`timing-probe.bin`, `*.timing*.bin` and `*.timing*.txt` preserve the DSB, busy
vector and stack/return tests. Disabling DSB must expose an early marker;
bypassing entry waits must expose an early handler timestamp. `interrupt-probe.bin`
and `*.interrupt*.bin`/`*.interrupt*.txt` verify IRQ preemption and load restart;
disabling preemption must miss the early IRQ marker. Each negative control must
fail its particular result gate after successful execution and guard attestation.
Replay the WAVs to judge
timbre; objective pitch gates are only part of listening quality.

Next useful additions are event-driven virtual controls/MIDI, longer annotated
vocal fixtures with settings sweeps, full codec/flash/display behavior, and
calibration against a board's digital audio and cycle traces. These can build on
the same production ELF boot rather than requiring a separate DSP-only image.
