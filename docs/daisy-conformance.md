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
| WM8731 DAC channel swap | R7.LRSWAP swaps DAC samples in captured serial/ideal-line WAVs, preserving ADC order and nominal converter delays |
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

Local validation runs the unmodified production firmware on all three profiles.
CI is kept off for this work; no OS matrix or workflow changes are added. Each must
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
to its runtime period. These standalone analytical bounds do not predict CPU
stalls. The subsequent shared-fabric and live SDRAM scenarios below add queueing,
row state and refresh interference without claiming calibrated silicon timing.

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
waiting reads and 256-bit read buffer. Both a standalone earliest-service model
and the live CPU/DMA fabric test queue-full backpressure, ordered completion and
consecutive-word reuse. Live buffer lookup occurs at grant, after earlier reads
can replace it; completion updates the buffer. A hit retains transfer beats but
does not charge another array wait. Programming/erase invalidates the bank buffer.
FLASH_ACR LATENCY plus one interface cycle supplies its nonvolatile-read service
bound; per-run reports retain the actual register and clock. It does not model
AXI handshakes, ECC or pipeline overlap. Live CPU instruction/data requests are
serialized functional transactions; the standalone read-pipeline test does not
prove that the CPU issues overlapping flash commands correctly.
The live fabric enforces the three-command capacity across masters. The 25ns
cold/5ns hit test at HCLK200MHz and LATENCY4 verifies this declared service
scenario, not an independently measured silicon latency.

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
Dirty eviction and maintenance now snapshot 32-byte lines, use CPU issuing
credits and share target write service with DMA. Physical bytes change on
completion. An independent DMA observer verifies both delayed visibility and
the immutability of an earlier clean when a later cached store changes the line.
Same-line refill serializes behind a pending writeback. Buffer depth, forwarding,
write merging, critical-word-first refill and complete store-buffer behavior
remain implementation gaps.

[PM0253 Rev5 section4.8.7](https://www.st.com/resource/en/programming_manual/dm00237416-stm32f7-series-and-stm32h7-series-cortexm7-processor-programming-manual-stmicroelectronics.pdf)
requires DSB completion around cache maintenance. Native DSB now waits for all
pending modeled writebacks. DMB orders subsequent modeled RAM accesses, without
waiting at the barrier itself. This does not establish complete ordering for
MMIO/TCM or the ordinary CPU store buffer. The generated ARM test competes with
a 100µs SRAM write, cleans a line and executes DSB; its early marker must remain
unset. Disabling the barrier callback must expose that marker early.

[Arm Cortex-M7 TRM section2.7](https://documentation-service.arm.com/static/5e906b038259fe2368e2a7bb?token=)
specifies parallel vector fetch/state saving and restart of abandoned normal
memory operations after an IRQ. The native adapter now checkpoints entry after
one acknowledgement/frame allocation, services vector and stack requests on I/D
ports, then resumes before handler execution. Unstack reads are prepared before
native state restoration. Vector loads use I-cache and the default memory map,
ignoring MPU attributes, as specified by `MPUCheck` in the
[Armv7-M ARM](https://www.pjrc.com/teensy/DDI0403Ee_arm_v7m_ref_manual.pdf).
The ARM fixture programs flash while taking SVC through a cold flash vector,
then checks a single SRAM frame, handler timestamp, restored SP/LR and return PC.
Bypassing entry waits must produce an early handler timestamp. A warm vector
unit test verifies that entry does not drain unrelated dirty writeback traffic.

A second ARM fixture waits on a normal flash load while an IRQ with a TCM
vector/stack becomes pending. The unconsumed skip budget is cancelled on the
CPU thread; the handler must run before programming completes and the load must
restart successfully afterward. Disabling preemption must miss the early IRQ
marker. Device/Strongly-ordered load waits remain non-interruptible. The private
adapter also detaches abandoned CPU request slots at entry. Already-issued
transactions retain their fabric occupancy/credits until response, but cannot
serialize handler accesses to an unrelated target. A third ARM fixture holds
the AXI SRAM read target busy while an IRQ stacks into D2 SRAM; the handler must
still run early and the abandoned load must restart after return.
The private managed assembly dispatches only its initial budget check through the existing
virtual getter; both arithmetic field reads remain intact. Each boot attests
the pinned IL shapes, Python guard and native adapter version12.

[PM0253 section2.4.7](https://www.st.com/resource/en/programming_manual/pm0253-stm32f7-series-and-stm32h7-series-cortexm7-processor-programming-manual-stmicroelectronics.pdf)
requires late arrival and tail-chaining to reuse the interrupted frame. The
adapter now resumes arbitration while entry is waiting, lets the existing NVIC
select a higher-priority replacement, and re-pends the original exception.
Completed stack stores are retained. Redirected vector reads release their CPU
request slot while preserving already-issued fabric occupancy; a refill that
already covers the replacement vector is reused.

Canonical H750 exception returns deactivate once, then checkpoint before
unstacking. Pending eligible exceptions reuse that frame without a pop/push
round trip. A resumed return can select an IRQ before native state restoration.
The pinned NVIC's RETTOBASE callback previously counted only system exceptions;
the private assembly now counts its complete active stack. Every boot attests
the exact patched IL, without changing the source Renode installation.

A Rust ARM guest checks late arrival, ordinary tail-chaining, equal-priority
deferral, genuine nesting, and an IRQ during modeled unstack service. It records
handler order, EXC_RETURN, both SPs, IPSR, IABR/RETTOBASE, saved PC/xPSR and final
thread state. Reuse cases require eight counted basic-frame stores; real nesting requires
sixteen basic-frame stores. A lazy-FP tail-chain case also requires the shared
104-byte extended frame and restoration of the thread's S0/FPSCR after both
handlers overwrite them. Disabled-late and disabled-tail controls must exit normally and fail
their frame gates. Priority-mask/fault combinations, physical late-arrival
cutoffs and exact pipeline/stack-pop timing remain unverified. These tests
establish functional transitions, not a silicon cycle count.
Basic and lazy-FP tail-chain cases also run with PSP thread stacks and verify
both MSP and PSP restoration, covering F9/FD/E9/ED thread return selections.

[PM0253 sections2.5.1 and4.3.11](https://www.st.com/resource/en/programming_manual/pm0253-stm32f7-series-and-stm32h7-series-cortexm7-processor-programming-manual-stmicroelectronics.pdf)
requires a vector-table bus error to enter HardFault with HFSR.VECTTBL. A known
QSPI demand error now lets the original stack frame finish before the native
vector read receives that error in its existing VECTOR_READ phase. The engine
handles the derived HardFault using the same frame and restores the original
IRQ's pending state. This avoids aborting the host or fabricating a data fault.
Stopped clocks, unknown protocols and prefetch-only failures outside the demanded
vector word remain unsupported.

A further ARM guest programs a NOR vector table with an IRQ outside FSIZE and a
valid HardFault vector. It checks VECTTBL without FORCED or data-fault bits,
original PC/xPSR, a single shared-SRAM frame, and the inactive/pending original
IRQ. A second case warms the HardFault vector through an actual exception,
disables QSPI, then requires that cached vector to work after the cold IRQ vector
faults. A configurable-fault-enabled case still requires HardFault. Expanding
FSIZE to make the IRQ vector valid must fail the VECTTBL gate. Physical fault
response timing and complete bus/MPU/fault permutations remain unverified.

A separate no-std Rust ARM guest checks enabled divide/alignment faults,
16 MPU regions, denied data and execute-never accesses, and disabled-coprocessor
faults through the actual exception vectors. Stacked PCs are checked against the
faulting instruction encodings. Ordinary Normal-memory accesses remain unaligned
when permitted; integer multiword, exclusive and floating-point transfers always
require their architectural alignment. MPU Device/Strongly-ordered mappings are
also tested with CCR.UNALIGN_TRP clear. Faulting stores must preserve memory.
These requirements come from [ST PM0253, address alignment, CCR and fault status](https://www.st.com/resource/en/programming_manual/pm0253-stm32f7-series-and-stm32h7-series-cortexm7-processor-programming-manual-stmicroelectronics.pdf).

The Cortex-M7 FPv5-D16 profile exposes read-only MVFR0/1/2 through the PPB,
using [Arm DDI0489B, section8.3 and table8-2](https://documentation-service.arm.com/static/5e906b038259fe2368e2a7bb).
The guest checks feature values, double-precision addition, automatic and lazy frame
reservation/completion, EXC_RETURN, FPCAR, S0/FPSCR and SP restoration. Both TCM
and D2 SRAM stacks run in both modes; SRAM lazy stores use the existing Rust transaction replay
after capturing the TCG helper's precise unwind PC. Completed stores are replayed
without duplicate bus service when a later store stalls. This corrects the prior
unmodeled-context abort. Separate disabled divide/alignment/lazy-save controls
must exit normally and fail the intended result gate. `cpu` runs this focused
suite; `probe` and `run` also include it. No CI workflow or OS matrix was added.

The Rust bus bridge now distinguishes documented QSPI/FLASH error responses
from missing or unsupported behavior. [RM0433 sections23.3.14 and4.5.1/4.7.12](https://www.st.com/resource/en/reference_manual/rm0433-stm32h742-stm32h743-753-and-stm32h750-value-line-advanced-armbased-32bit-mcus-stmicroelectronics.pdf)
require bus errors for disabled/unmapped/out-of-range QSPI reads and incorrect
FLASH unlock writes. A separate Rust ARM guest verifies CFSR, BFAR, precise data
PCs, instruction IBUSERR without BFARVALID, disabled-BusFault escalation, and
persistent FLASH locking after a bad key. Register stores use an MPU
Strongly-ordered region; posted-store timing remains outside this test.
The suppressed-data-error control must finish and fail its exception gate.

QSPI uses one instruction per native translation block, selected by a pure Rust
translation-policy query. Internal flash/RAM retain their existing block sizes.
The existing decoder and exception machinery remain authoritative. Valid code
executes up to a failing demand fetch; both halves of Thumb32 belong to that
instruction. A separate ARM guest programs actual NOR code, writes a marker
before the configured window ends, and verifies the subsequent fault PC. A second
case straddles the window with STR.W: it must report the instruction's starting
PC without performing the store. A third case executes the cached prefix after
disabling QSPI, then faults on the uncached line. Corrupting the prefix must fail its marker gate.
Refill service now uses the aligned line span, matching the data subsequently
filled into the cache; an interior PC no longer creates a falsely out-of-range
linear request. Tests also retain cached execution after controller disable.
Defensive multi-instruction callers and external prefetch-only errors outside
the demanded instruction remain explicitly unsupported. Burst ordering,
critical-word-first arrival and pipeline timing are not certified. Unknown
protocols and stopped-clock behavior still fail as unsupported.

Repeated validation of an unchanged, fully resident instruction block reuses
its previous comparison. Every instruction fill/eviction, decode, cache
maintenance, MPU/CCR write and reset invalidates that host-side memo. Physical
DMA/data writes deliberately do not: cached instructions remain stale until
maintenance or eviction. Tests cover both stale DMA code and a long validation
that evicts its own first line. Fetch counters and modeled service are preserved;
this optimization changes host work, not virtual CPU speed or audio tolerances.

DMA can now read mapped QSPI. A modeled read bus error sets TEIF, clears EN,
preserves NDTR and raises the enabled transfer-error IRQ. A successful read
commits one transfer and raises completion. The ARM guest programs the stream
and acknowledges its real NVIC interrupt; the host supplies one synthetic DREQ
after the guest sleeps. The guest selects its transmit register from the physical
board straps. Valid, disabled and out-of-range sources are checked. Additional
cases disable the controller or request ABORT after a live read ticket is
admitted but before its response. A documented bus error from that response
now reaches the same Rust DMA completion path as an admission-time error,
without committing a transfer. Unsupported reads still terminate as model
errors. All error cases require unchanged NDTR, EN clear, TEIF and exactly one
guest IRQ. These are functional response tests on the existing 1µs DMA service
grid. Controller state is evaluated at the modeled read response; physical
ABORT latency, partially returned bursts and pin timing are not certified.

These tests verify functional service scenarios. Physical late-arrival cutoffs,
complete tail-chaining timing, full MPU/fault-priority and BusFault coverage, FP numerical corner cases,
snapshot/reset during in-flight operations, bus turnaround, actual target
overlap, exact FMC command scheduling and pipeline fetch arrival times still need
implementation or independent validation. The source-backed tests do not justify
a fixed silicon exception-entry latency or hardware deadline claim.

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
| Performance and deadlines | Functional caches/runtime refetch, shared CPU/DMA stalls, live flash buffer/programming/erase, deferred dirty writes/DSB, resumable exception service, load-wait IRQ preemption, late-arrival/tail-chain frame reuse and FMC row/refresh/read-ahead service | Pipeline/speculation and silicon victim policy, complete store buffers, physical late-arrival cutoff and tail-chain cycles, exact FMC queue/command overlap and calibrated CPU/bus costs |
| Clock rates and drift | RCC/PLL/SAI rational derivation, 1µs event grid, DWT/core and timer prescalers | PLL/oscillator lock and jitter, clock-domain crossing delays, oscillator tolerance, independent frequency measurements |
| Audio latency/filtering | Serial PCM; separate nominal WM8731 gain/DC-removal and passband-delay stage | Full ADC/DAC filters, internal rounding and startup behavior, independent impulse/frequency responses per codec |
| Analog sound | No analog model | Calibrated gain, impedance, headroom, clipping, noise and distortion measurements |
| Other peripherals | QSPI NOR programming/erase, mapped M7 data/execution, polling/timeout IRQ; GPIO lock/analog/open-drain cases | Remaining QSPI modes and serial FIFO timing; full display, MIDI/USB/ADC behavior and error/reset suites |
| Other revisions | Separate Rev4/Rev5/Rev7 boot/audio profiles | Independent captures for each physical revision; Seed2/Seed3 profiles remain unsupported |

## External NOR and QUADSPI

The external chip model follows ISSI's **IS25LP064A**, not the similarly named
non-A device. The manufacturer's
[25LP032-64A-B Rev A, 2015-11-06](https://www.mouser.com/datasheet/3/3722/1/25LP032_64A_B.pdf)
tables6.4–6.10 and sections8.1–8.32 define the modeled command/state behavior.
The pinned libdaisy-rust flash driver identifies this part and uses the same
QPI commands. Storage is independent of the STM32 controller: RCC reset clears
controller registers/FIFO but retains NOR contents, mode and in-flight NVM work.
Chip reset has its own enable sequence, recovery interval and retained NV state.

The Rust tests cover WREN/WEL/WIP, one-to-zero programming, page wrapping,
sector/block/chip erase, BP/SRWD/WP# protection, OTP top/bottom selection, SPI/QPI,
read dummy clocks, deep power-down and suspend/resume. Program/erase changes are
deferred. The timing scenarios use 0.2/0.8ms page program, 70/300ms sector erase,
100/500ms 32KiB erase, 150/1000ms 64KiB erase and 16/45s chip erase from the
typical/maximum table. Status writes use 2/15ms. Function-register writes use
that same register-write scenario; the datasheet does not independently specify
their duration. Suspend uses the 100µs read-ready bound with work continuing
during that latency; this progress distribution is an emulator policy. Reads
inside the suspended target and reset during programming/erase are rejected
rather than inventing intermediate cell state. Deep-power entry/release use
the published 3µs bounds. UID/security contents and SFDP are not fabricated.

[RM0433, QUADSPI chapter](https://www.st.com/resource/en/reference_manual/rm0433-stm32h742-stm32h743753-and-stm32h750-value-line-advanced-armbased-32bit-mcus-stmicroelectronics.pdf)
defines controller triggers, FIFO, sticky flags/W1C, polling mask/match, IRQ92,
FSIZE and the 256MiB read-only mapped window. Register setup without an instruction
does not issue a chip command; writes restricted by BUSY retain their prior value.
SPI/QPI instruction/address/data lanes and the programmed dummy clocks must match
the selected read mode. Alternate-byte clocks count inside the chip's dummy
requirement: production C0=f8/EB needs ten clocks, satisfied by two mode clocks
plus DCYC=8. Rated array-read limits are checked at Daisy's 3.3V scenario.
HSI/HSE/PLL/CKPER selection, enable gates and PRESCALER determine polling and
mapped service rates. Timeout retains unelapsed serial clocks across a gate.

Mapped reads share the NOR bytes, M7 caches and fabric target2, rather than using
a disconnected RAM mirror. A cold mapped access uses one serial transaction per
system-bus request. The timeout scenario adds a 32-byte prefetch fill and LPTR
clocks. Continuous prefetch, nCS edge placement and overlap are not calibrated.
Indirect transfers use immediate functional FIFO shifting. RM0433 states a
32-byte FIFO and refill after four vacant bytes, but its FLEVEL<=11 example
corresponds to a 16-byte FIFO. The model uses the stated 32-byte/four-vacant-byte
rule and does not claim to resolve that contradictory example from silicon.
DDR, SIOO, security/UID/SFDP and unused dual/quad-output SPI opcodes remain gaps.

A separate ITCM Thumb guest programs and erases through MMIO, sleeps in WFI
until actual polling IRQ92 delivery, checks controller-reset persistence, executes
programmed code from 0x90000000, observes stale I/D caches after reprogramming,
then checks fresh results after maintenance. It also waits for the mapped-timeout
interrupt, including an earlier deadline after the kernel clock accelerates.
Masked-NVIC, corrupt-data and stale-scheduled-timer controls must fail specific Rust result
gates after a successful process exit. ARM-written buffers and their grades are
preserved as `*.storage*.bin` and `*.storage*.txt`. The full suite and `probe`
include these checks; `storage` runs them alone.

The conformance report records the requested run scope and hashes the native
model, platform, core manifest, managed host and Python-disabled managed engine.
Its required full-suite list does not claim those checks ran in a focused mode;
the mode's success summary states the actual scope.

The v9 report distinguishes FMC's shared SDRAM service port from independent
AXI read/write channels. RM0433 22.9.3 figures119/120 use the same DATA pins
for reads and writes. Mixed CPU/DMA tests exercise both direction orders and
completion-time stalls; an unrelated target can still progress. A queued
direction is selected by oldest arrival and its initiator by AXI QoS/LRU.
Serializing service windows is a scenario choice: exact FMC
direction arbitration, FIFO acceptance, command overlap and turnaround still
require independent traces. It is not a measured
controller latency or a new hardware deadline guarantee.

WM8731 R7.LRSWAP follows Table15 of the linked Rev4.9 datasheet: the DAC
channels swap while ADC ordering stays unchanged. Public I2C/SAI tests capture
WAV output in serial and ideal-line modes, checking the latter's existing
nominal ADC/DAC delay surrogate. No unpublished converter or de-emphasis
filter coefficients are inferred from those channel-routing checks.

Conformance v10 adds the live row/bank state, recovery constraints, periodic
refresh and the FMC command path. The SDRTR timer uses COUNT+1 memory clocks,
correcting the earlier COUNT-only period. Cold/active/conflicting rows, independent
banks, TRAS/TWR, refresh priority/PALL, delayed-refresh RE/CRE/REIE, IRQ48,
manual NRFS/TMRD, self-refresh/TXSR, power-down refresh, WP and documented reset
values are checked through public interfaces. FMC is registered with RCC so
its gate and reset reach the real managed peripheral rather than unused flags.
An ARM guest independently programs MMIO, writes and reads rows/banks, exercises
read-ahead and receives IRQ48. Masked-IRQ and wrong-target controls must fail its
processor-written completion result after successful guarded execution.
The same guest checks a configured-geometry overflow as a precise BusFault,
including CFSR, BFAR and exception number. DMA admission and response APIs return
the corresponding bus-error category so the existing TEIF path can handle it.

For the Seed 32-bit bus, RBURST uses consumable read-ahead tags with data-ready
times, row limits, oldest eviction and write/precharge/refresh flushing. The
detailed RM0433 read-cycle section specifies six 32-bit FIFO lines, whereas the
feature list says six 64-bit lines. The revision history explicitly corrects it
to six 64-bit entries. The model follows that correction with per-word validity
as a declared narrow-read packing scenario. Exact packing remains unverified.
FIFO acknowledgements use a
one-kernel-clock scenario; exact response latency, write-FIFO acceptance depth,
pin-level command overlap and low-power transition edges remain unverified.
Other bus widths with RBURST and in-flight clock stops fail explicitly.

WM8731 normal/USB sample tables now use rational clock ratios for every listed
SR/BOSR combination, CLKIDIV2 and CLKODIV2. Table32's four nominal ADC/DAC delay
pairs are 12/11, 20/18, 3/5 and 6/5 frames. Live codec rates must agree with the
shared SAI LRC and generated MCLK for powered converters; invalid combinations
fail instead of emitting plausible PCM. This extends the delay surrogate, not
the unpublished complete converter response. Filter-mode changes restart that
surrogate and do not claim to reproduce silicon reconfiguration transients.
The hardware-mode codecs also reject missing MCLK. Necessary ratio checks use
the [AKM AK4556 clock-mode table](https://www.alldatasheet.jp/html-pdf/206730/AKM/AK4556/879/15/AK4556.html)
and [TI PCM3060 system-clock requirements](https://www.ti.com/lit/ds/symlink/pcm3060.pdf).
PCM3060's shared ADC/DAC wiring requires an ADC-compatible ratio;128/192fs is
DAC-only. The AK4556 ratio union does not establish its physical CKS straps,
speed selection, full operating-frequency limits or transition behavior.

Reference data must identify physical revision, codec marking, firmware ELF hash,
settings, clock configuration, input/calibration and measurement method. Digital
comparisons should use exact sample words and event order where deterministic;
analog comparisons need declared tolerances reflecting measured board variation.
Captures generated by this emulator cannot serve as independent ground truth.

Until those gaps are closed and the evidence is available, the correct status
is **functional tests passed; hardware equivalence unverified**. In particular,
do not use this model to certify CPU headroom or analog sound, or to skip eventual
hardware acceptance testing.
