# Complete virtual-device baseline, 2026-11-22

The unchanged Synthphone main firmware was built from source revision
`a7870470019bb5827da88ba2a79b30c72ea0a5d2`. Its immutable ELF SHA256 is
`2926a632bf93eb1991f5c91d21cb72309cd0890501d659540f673a3e2517e766`.
The carrier is the existing `synthphone-v2` netlist with `reference.fixture`;
the board is Rev4/AK4556. Virtual Daisy is pinned to
`f7892c73b36960d729e6c3bf8bbd0ce3e65498a0`.

| Executed test | Result | Evidence |
| --- | --- | --- |
| 820 ms controls/audio run | PASS | `controls-audio/`: all 12 keypad positions, encoder single/double press, rotation, route switch, voice-control and drum MIDI, live OLED and audio |
| Six-mode sweep, 650 ms | PASS | `profiles/`: actual OLED pixel matches for all six names; sustained audio; no Cortex-M or SAI faults; correct GPIO/AF/open-drain configuration |
| Audible MIDI on channel 2, silent jack | PASS | `midi/`: isolated raw synth output; channel 1 is intentionally silent vocal control |
| Disconnected MIDI/key/jack and opposing keypad columns | Expected failures detected | `negative-controls/`: each individual device fails its relevant gate without an unrelated CPU fault |
| Disconnected OLED | Expected failure detected | Original error is `display physically disconnected`; observable-only recheck corrects an overly generic batch marker |

The original attachment stopped at a UART byte read. That failed capture is
retained in `failed-first-attachment/`; [Virtual Daisy PR #12](https://github.com/UnitedCode/virtual-daisy/pull/12)
adds the actual ARM/MMIO paths required by the unchanged firmware. Independent
instruction/serial regressions and the broader runtime/CPU suites passed.

Two early fixture assumptions were corrected without changing the firmware or
carrier: startup already opens Effects, and audible raw synthesis uses MIDI
channel 2. The earlier failed captures remain in the complete local archive.
Each retained execution report includes its own compiled runner-source hashes.

The full captures include the immutable ELF, ADC/GPIO/MIDI and RTT traces,
display RAM, source hashes, PCM and settings image. Small reports and observed
OLED renders are committed here. The SVGs are drawn from actual captured RAM;
they are not UI mockups.

Twelve all-feature Rust checks and eight without optional native/device support
passed, including RC/ngspice comparison, time partition invariance, OLED
addressing, MIDI observation and carrier contracts. Formatting and clippy passed.

These are **functional virtual-device results** under the declared electrical
fixtures. Preamp internals/R2, optocoupler analog behavior, actual sample/hold,
supply/thermal effects and physical calibration remain unknown. Reset, settings
retention across power loss, extended soak and human listening were not run.
The modeled clock was approximately 48,187.487 Hz; that value is retained rather
than represented as a new measurement of the fitted Seed.
