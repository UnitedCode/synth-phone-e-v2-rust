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

CI runs the unmodified production firmware on all three profiles. Each must
complete startup, produce valid stereo PCM with correct tuning and continuity,
and detect both silence and lost receive-DMA faults. The existing desktop sweep
and ARM DSP parity tests remain complementary checks. The source references
and tests are not substitutes for comparing with a board.

## What blocks a hardware-equivalence claim

| Area | Current evidence | Required next evidence/work |
| --- | --- | --- |
| Firmware boot and selected audio path | Executed in simulation | Same ELF on an identified physical revision |
| Serial data, reset and control behavior | Source-backed cases | Captured control transactions and SAI words/edges, including reset and faults |
| Interrupt/buffer ownership | Functional DMA and IRQ tests | Hardware DMA HT/TC, NVIC and task traces with timestamps and simultaneous UI/MIDI load |
| Performance and deadlines | Nominal instruction pacing only | Calibrated M7 instruction timing, caches, flash/SDRAM latency and bus contention |
| Clock rates and drift | Fixed 48014 Hz virtual SAI clock | RCC/PLL-to-SAI derivation, clock measurements and oscillator tolerance |
| Audio latency/filtering | PCM injected/captured at serial boundary | ADC/DAC filter and startup delays, impulse and frequency responses per codec |
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
