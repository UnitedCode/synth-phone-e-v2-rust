# Daisy hardware conformance

The current emulator is **not 100% equivalent to a real Daisy**. Booting an ELF
and producing correctly tuned PCM demonstrates those functional behaviors for
that fixture. It does not validate unmeasured properties of silicon or analog
circuitry. No physical-board captures have been compared with this model yet.
The verification target is accurate behavior for a stated revision, firmware,
input, configuration and measurable tolerance. A blanket perfection claim is
not an acceptance criterion we can honestly satisfy.

The runner requires a physical `--board` selection, rejects incompatible codec
assertions and lists `hardware_equivalence=UNVERIFIED` even after passing tests.
`--require-hardware-match` fails before running because independent evidence and
several required models are absent. No success in the functional suite changes
that status. `conformance.txt` fingerprints the ELF, native model and generated
platform and lists the unverified categories. It is an evidence ledger, not a
certificate or a measured percentage of correctness.

## Physical board identity

The board profile selects the codec and physical wiring together. It never
changes the firmware's detected version by patching its code or settings.

| CLI profile | Codec | Version straps held low | SAI receive / transmit | Control |
| --- | --- | --- | --- | --- |
| `rev4` | AK4556 | None on PD3/PD4/PD5 | B / A | GPIO PB11 reset |
| `rev5` | WM8731 | PD3 | A / B | I²C2 address 0x1a |
| `rev7` | PCM3060 | PD5 | B / A | Hardware pins; no codec I²C device |

Unasserted version straps read high under the firmware's pull-ups. The physical
strap levels survive peripheral resets. Rev7 is the legacy Seed 1.2, not the
separate Seed2 DFM board (PD4). The pinned Rust HAL checks PD3 and PD4, so Rev7
uses its compatible original-Seed audio path. This distinction matters: inventing
a PD4 ground for Rev7 can exercise a different firmware path and pin map.

The manufacturer [Seed datasheet, v1.2.0](https://daisy.nyc3.cdn.digitaloceanspaces.com/products/seed/Daisy_Seed_datasheet.pdf),
audio-codec and hardware-revision pages, establishes the revision names, PD3/PD5
straps, PCM3060 hardware mode and SAI connections. The manufacturer's
[libDaisy board support](https://github.com/daisyaudio/libDaisy/blob/master/src/daisy_seed.cpp)
and the project's pinned `libdaisy-rust` startup code establish the matching
codec initialization and direction choices. These are source-backed expectations;
physical timing and electrical behavior remain unverified.

SAI mode-bit expectations use ST's [RM0433 reference manual](https://www.st.com/resource/en/reference_manual/rm0433-stm32h742-stm32h743-753-and-stm32h750-value-line-advanced-armbased-32bit-mcus-stmicroelectronics.pdf),
SAI_xCR1: mode bit 1 selects slave and bit 0 selects receive. Tests use the
documented literal encodings, including master RX=01 and slave TX=10.

Seed2 DFM and [Seed3](https://docs.daisy.audio/hardware/Seed3/) are **unsupported**,
with explicit CLI errors. Seed3 uses TAC5242; it must not receive an AK4556 or
PCM3060 label merely because older firmware can use a compatible audio mode.
Its detection is separately documented in the manufacturer's
[board enumeration](https://github.com/daisyaudio/libDaisy/blob/master/src/daisy_seed.h).

## Checked behavior and corrected defects

New tests use the public peripheral/CLI interfaces and hardware-specified
expectations. Each new behavior was introduced with a failing check first.

| Behavior | Independent expectation / check |
| --- | --- |
| Board/codec choice | Listed pair and RX/TX directions; mismatches rejected before loading an emulator |
| AK4556 reset | PB11 low disables the digital boundary; high releases it; no I²C control |
| PCM3060 Rev7 | Hardware-controlled codec; no fictitious I²C peripheral |
| WM8731 reset | Power-on and software-reset register defaults |
| WM8731 DAC mute | ADC capture continues while DAC output is muted |
| WM8731 line mute | Only the selected input channel is silenced |
| WM8731 both-channel load | Update the opposite channel's volume/mute fields |
| PCM endpoints | Signed 24-bit extrema +8,388,607 and −8,388,608; no copied firmware limiter |
| SAI wiring | Reversed A/B RX/TX works; enabling directions that disagree with the board fails |
| Revision straps | Ground only the populated strap; other pins follow their configured pull resistors |
| TX startup | Service a newly asserted DMA request without waiting for the next sample timer |
| FIFO levels and request thresholds | Honor programmed quarter/half/full watermarks, including exact quarter boundaries |
| Evidence boundary | A functional pass cannot satisfy a request for hardware equivalence |

WM8731 control expectations follow the manufacturer's
[WM8731/L datasheet, Rev4.9](https://www.mouser.com/datasheet/2/76/WM8731_v4_9-1141834.pdf),
especially detailed register field descriptions in Table30. Its R7 reset fields
specify 24-bit I²S, slave, normal polarity: 0x0a. The summary Table29 has an
inconsistent R7 default; the model uses the detailed field definitions. A reset
restores standby/mute state instead of zeroing the register bank.

The original WM-only model muted ADC input along with DAC output, zeroed reset
registers, and clipped serial PCM to a software limiter's range. Those assumptions
were incorrect and are fixed. The new Rev4 boot also exposed an artificial
startup underrun caused by servicing TX requests only at sample ticks; the
bridge now services enable-time requests too. Correct threshold handling prevents
overfilling the FIFO during that prefill. These fixes improve the model;
they are not evidence that remaining behaviors are correct by implication.

## Datasheet-driven cache, bus, clock and codec checks

ST's [PM0253 Cortex-M7 programming manual](https://www.st.com/resource/en/programming_manual/pm0253-stm32f7-series-and-stm32h7-series-cortexm7-processor-programming-manual-stmicroelectronics.pdf)
and [AN4838 MPU guide](https://www.st.com/content/st_com/en/technical-documents/an4838-introduction-to-memory-protection-unit-management-on-stm32-mcus-stmicroelectronics.html)
define the cache geometry/maintenance and MPU attribute rules. The Rust model
now implements separate CPU-cache/physical-DMA views, 32-byte cache-line effects,
MVA and set/way operations, write-through/write-back policies and MPU priority/
subregion fallback. A dirty neighboring variable can be lost when its shared
line is invalidated: tests deliberately check this failure, as well as stale
RX data and uncleaned TX data. The default firmware's noncacheable DMA region
passes; changing its attributes after boot breaks the audio test.

[RM0433](https://www.st.com/resource/en/reference_manual/rm0433-stm32h742-stm32h743753-and-stm32h750-value-line-advanced-armbased-32bit-mcus-stmicroelectronics.pdf)
defines the DMA interconnect reachability, TEIF/TEIE behavior, register write
protection, PLL and SAI dividers. DMA1 rejects ITCM/DTCM and unpopulated RAM,
preserves NDTR on a failed request, sets TEIF and clears EN. A production boot
redirected into DTCM must fail the audio gate with those register effects.

The manufacturer's [libDaisy clock configuration](https://github.com/electro-smith/libDaisy/blob/master/src/sys/stm32h7xx_hal_conf.h)
specifies HSE=16MHz; the inherited Renode platform used 8MHz. That source is
corrected. PLL fractional values latch on FRACEN's rising edge and remain latched
while FRACEN is cleared for retuning, as described in RM0433's initialization
sequence. PLL N/P/Q/R field widths, protected writes, SAI clock routing, MCKDIV,
OSR and master-clock-disabled division now use the register values. Rational
phase accumulation avoids rounding the clock to integer Hz. SysTick and APB
timer clocks follow core/bus prescalers, including TIMPRE and the /64 jump in
high D1CPRE encodings. Unrelated peripheral kernel muxes remain incomplete.

CYCCNT now follows the derived core frequency with trace enable, software lock,
writable count and wraparound. The software-lock distinction follows Arm's
[Cortex-M7 errata notice, entry 513195](https://documentation-service.arm.com/static/665dff778ad83c4754308908?token=):
software accesses are protected; an external debugger has different access
semantics. No external-debugger model is claimed. CYCCNT still counts virtual
elapsed clock periods, not calibrated instruction execution time.

For WM8731, the separate `ideal-line` input stage implements the published
1.5dB line-gain steps and page58 high-pass transfer function from the linked
Rev4.9 datasheet. Checks cover DC decay, channel gain independence, HPOR/ADCHPD,
mute and reset. The normal serial input path bypasses that pre-ADC stage;
it must not apply gain/filtering twice. De-emphasis and unsupported sample-clock
ratios are rejected. This does not reproduce the full ADC/DAC response.

CI runs the unmodified production firmware on all three profiles. Each must
complete startup, produce valid stereo PCM with correct tuning and continuity,
and detect silence, lost receive DMA, cacheable buffers and inaccessible DTCM
faults. Rev5 additionally boots through the ideal line stage. The existing desktop sweep
and ARM DSP parity tests remain complementary checks. The source references
and tests are not substitutes for comparing with a board.

## What blocks a hardware-equivalence claim

### Further source-verified behavior

The [Arm Cortex-M7 TRM, DDI0489F](https://documentation-service.arm.com/static/5e906dc68259fe2368e2abbe),
sections3.3.4/3.3.5 and5.9, specifies the instruction-cache geometry and maintenance.
The RAM fetch model now separates instruction and data views, handles ICIALLU
and ICIMVAU, and checks generated-code coherency on the emulated ARM CPU.
Dirty CPU code stores, invalidation without cleaning, cleaning without invalidation,
and external DMA writes all have independent stale-instruction checks. The passing
control uses the prescribed clean/invalidate/DSB/ISB sequence. The same register
tables corrected the prior I-cache CCSIDR value to `0xf01fe009` and made CSSELR
LEVEL RAZ/WI. The subsequent flash/runtime pass below extends this coverage.
Instruction prefetch and missing-barrier hazards remain unvalidated.
A translation-cache flush is implementation plumbing, not an
architectural I-cache invalidation: Rust retains stale bytes until maintenance.

The fetch/data distinction is a minimal adapter in the pinned native engine,
not a PC/TB/thread heuristic. Source archives are SHA256 verified, the built
core is fingerprinted, and every boot checks its adapter version. All cache
decisions and the ARM probe's generator/grader remain Rust.

RM0433's SDRAM register descriptions now drive an analytical timing report.
It decodes the actual FMC mux/divider and programmed timings, including CAS,
row/pipeline delays, refresh COUNT and TMRD. Open/precharged/conflicting-row and
cache-line read bounds are tested against worked register cases. Refresh COUNT
uses SDCLK; RPIPE uses the faster FMC kernel clock, unlike CAS and row timings.
The distinction follows [RM0433 section22.9.5's SDCR definitions](https://www.st.com/resource/en/reference_manual/dm00314099-stm32h742-stm32h743753-and-stm32h750-value-line-advanced-armbased-32bit-mcus-stmicroelectronics.pdf). Refresh COUNT
is a counter period; the documented 20-cycle calculation margin is not added
to its runtime period. These are lower bounds, not a model of actual CPU stalls,
bus arbitration, FMC queueing or SDRAM refresh interference.

The WM8731 Rev4.9 Table32 specifies normal Type1 group delays of20/FS for ADC
and18/FS for DAC. The ideal-line path now includes those nominal passband delay
surrogates, with impulse-frame tests. Serial PCM remains after/before converter
processing and adds no surrogate. Per-capture codec reports record this scope.
The [AK4556 manufacturer datasheet, MS0559-E-02](https://static6.arrow.com/aropdfconversion/f0d656c6c3d4125aedb0547a5480444a9b1c9600/9ak4556vt.pdf)
and [PCM3060 datasheet, SLAS533B](https://www.ti.com/lit/ds/symlink/pcm3060.pdf)
provide additional nominal group-delay/response constraints for those profiles;
reports preserve their ADC/DAC delays (18/21 and17.4/20 frames respectively).
They do not silently enable a WM8731 filter model for other hardware.

The public datasheets provide transfer functions for selected stages and
response/group-delay tables for others. They do not publish all interpolation/
decimation coefficients, internal arithmetic and rounding. Full codec responses
remain incomplete; a pure delay is not claimed to reproduce a converter's impulse
or stop-band response. Reconstructing those responses needs more manufacturer
detail or independent measurements, not an arbitrary filter fitted to a diagram.

### Flash, runtime fetching and bus admission

User flash now shares the Rust cache/memory path with RAM, including separate
instruction and data views, erased contents, reset persistence and the H750's
128KiB boundary. CPU/DMA programming and erase now share that physical storage;
the upstream controller cannot silently mutate a disconnected backing object.
Option/security registers and CRC remain unsupported.

The native adapter calls Rust at the engine's pre-execution block check. Translation
only peeks at the current instruction view; allocation and conflict eviction occur
when a block executes. Reused blocks are checked too, and stale decoded bytes
trigger the engine's own invalidate/restart path before execution. The ARM probe
executes conflicting flash and RAM functions and detects stale translations.
Disabling runtime fetching is a required negative control: the grader must observe
return value2 where the functioning model returns3. All cache decisions, comparison
plans and grading remain Rust; shared memory is protected by a Rust mutex across
the native runtime callback and managed DMA accesses.

This still uses block-granular fetches and deterministic round-robin victims.
Fetching a block's lines at entry is not a reconstruction of M7 prefetch,
speculation, interrupted-block fetches or pipeline overlap. The conflict test's
specific victim sequence verifies the approximation, not a silicon guarantee.

[RM0433 Rev8 sections2.2.3/2.2.4 and15.3.6](https://www.st.com/resource/en/reference_manual/rm0433-stm32h742-stm32h743-753-and-stm32h750-value-line-advanced-armbased-32bit-mcus-stmicroelectronics.pdf)
define the admission and arbitration rules now tested in Rust. DMA's direct
single-beat audio requests are batched through the actual DMAMUX wiring, then
granted by PL priority with lower stream number breaking ties. Runtime reports
count contested grants. Source and destination ports now wait separately for
fabric service; NDTR and HT/TC are committed only after destination completion.
Clearing EN waits for an active beat before allowing reconfiguration.

The AXI GPV exposes independent read/write QoS and issuing overrides for six
initiators. The shared CPU/DMA fabric orders concurrent requests by QoS,
then least-recently-served priority independently per target and channel.
Table4's issuing limits apply admission backpressure until completion; setting
an override limits new issues to one without cancelling existing transactions.
The initial equal-age port order is unspecified and remains a deterministic
model choice. RM0433's prose and register tables specify QoS reset0; the pinned
Rust PAC metadata says4. This model follows RM0433 and records the discrepancy
for an eventual reset-register capture, rather than treating the PAC as proof.

RM0433 section4.3.8 and Figure10 describe a flash bank's active command, two
waiting reads and 256-bit read buffer. A standalone earliest-service model now
tests queue-full backpressure, ordered completion and consecutive-word reuse.
FLASH_ACR LATENCY plus one interface cycle supplies its nonvolatile-read service
bound; per-run reports retain the actual register and clock. It does not model
AXI handshakes, ECC or pipeline overlap. Live CPU instruction/data requests are
serialized functional transactions; the standalone read-pipeline test does not
prove that the CPU issues overlapping flash commands correctly.

### Coupled stalls and nonvolatile operations

CPU cache misses/uncached accesses and DMA port requests now enter a common
event-driven Rust fabric. AXI QoS, issuing limits and target occupancy delay
completion; local AHB targets use a declared round-robin service scenario.
CPU wait debt is consumed through Renode's virtual skipped-instruction budget,
without retiring firmware instructions. Flash busy reads and full write queues
use the same mechanism. STM/STRD retries replay completed accesses, keyed by
native instruction metadata and SP, so an accepted flash strobe is not repeated.
The fetch scheduler retains serviced prefixes if Renode shortens a block during
a retry. Unit tests cover that starvation case.

The live service scenario uses FLASH_ACR latency plus transfer beats, 64-bit AXI
SRAM and 32-bit local SRAM beats, two PCLK cycles for the SAI APB port, and the
programmed SDRAM open-row read lower bound. AXI read/write target lanes are
independent in this scenario. These individual assumptions do not establish a
whole-system hardware timing bound. DMA service is polled every microsecond.
Dirty-cache eviction/maintenance traffic, write buffers, bus turnaround, actual
target overlap, FMC row tracking/refresh scheduling, pipeline fetch arrival times
and precise MPU/BusFault exception delivery remain implementation gaps. Native
interrupt-vector reads are identified separately from translated instructions;
their existing functional memory view does not impose bus latency. A vector read
while flash is busy fails explicitly. Exception entry/exit needs a resumable
adapter before external-memory stack/vector stalls can be modeled safely.

[RM0433 Rev8 sections4.3.9–4.3.10, 4.5.1 and4.7](https://www.st.com/resource/en/reference_manual/rm0433-stm32h742-stm32h743753-and-stm32h750-value-line-advanced-armbased-32bit-mcus-stmicroelectronics.pdf)
specify the modeled key sequence and lock, PG/write-buffer behavior, 8/16/32/64-bit
strobes, forced partial programming, queued operations, sector/bank erase,
STRBERR/INCERR/PGSERR, BSY/WBNE/QW, EOP, interrupts and W1C flags. The H750 has
only user sector zero. BER wins over SER/SNB when both are set. Physical bytes
change only at completion, and erase restores ones. Reprogramming nonvirgin
ECC words is explicitly unsupported; an invented ECC algorithm or error flag
would give false confidence. Model errors are surfaced to the host, not yet
delivered as fully modeled Cortex-M bus exceptions.

[DS12556 Rev8, flash memory characteristics](https://www.st.com/resource/en/datasheet/stm32h750vb.pdf)
provides typical/maximum timing scenarios. At x64 the model uses 100/200µs per
256-bit programming operation and 1/2s per 128KiB sector erase. Bank erase uses
the mass-erase table's 5/10s x64 scenario; exact bank latency is not established.
Other PSIZE values use their own table entries. Actual operation time depends
on conditions not simulated here, including voltage, temperature and wear.

An ITCM ARM probe erases and programs flash using scalar, STRD and STMIA accesses,
checks status/interrupt-related flags and stale instruction-cache behavior, then
executes the programmed function after maintenance. Typical and maximum scenarios
have separate runs. Bypassing data stalls must be caught by an old flash word in
the CPU result buffer; a crash/timeout cannot count. A production audio negative
control holds a shared SRAM target busy with synthetic competing traffic and
requires both delayed DMA and FIFO corruption evidence. These checks demonstrate
functional coupling; they do not calibrate CPU headroom.

| Area | Current evidence | Required next evidence/work |
| --- | --- | --- |
| Firmware boot and selected audio path | Executed in simulation | Same ELF on an identified physical revision |
| Serial data, reset and control behavior | Source-backed cases | Captured control transactions and SAI words/edges, including reset and faults |
| Interrupt/buffer ownership | Functional DMA and IRQ tests | Hardware DMA HT/TC, NVIC and task traces with timestamps and simultaneous UI/MIDI load |
| Performance and deadlines | Functional flash/RAM caches, runtime refetch, shared CPU/DMA stall scenarios, flash programming/erase, AXI admission/grants and SDRAM bounds | Pipeline/speculation and silicon victim policy, dirty writeback traffic, FMC row/refresh behavior, actual overlap and calibrated CPU/bus costs |
| Clock rates and drift | RCC/PLL/SAI rational derivation, 1µs event grid, DWT/core and timer prescalers | PLL/oscillator lock and jitter, clock-domain crossing delays, oscillator tolerance, independent frequency measurements |
| Audio latency/filtering | Serial PCM; separate nominal WM8731 gain/DC-removal and passband-delay stage | Full ADC/DAC filters, internal rounding and startup behavior, independent impulse/frequency responses per codec |
| Analog sound | No analog model | Calibrated gain, impedance, headroom, clipping, noise and distortion measurements |
| Other peripherals | Partial STM32/Renode coverage | Full register/error/reset semantics, QSPI storage, display, MIDI/USB/ADC tests |
| Other revisions | Unsupported | Separate sourced profiles and codec conformance suites, then board comparisons |

Reference data must identify physical revision, codec marking, firmware ELF hash,
settings, clock configuration, input/calibration and measurement method. Digital
comparisons should use exact sample words and event order where deterministic;
analog comparisons need declared tolerances reflecting measured board variation.
Captures generated by this emulator cannot serve as independent ground truth.

Until those gaps are closed and the evidence is available, the correct status
is **functional tests passed; hardware equivalence unverified**. In particular,
do not use this model to certify CPU headroom or analog sound, or to skip eventual
hardware acceptance testing.
