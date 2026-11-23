# Circuit improvement queue

Original circuit snapshots, fixtures and earlier reports remain frozen. Every
candidate starts from its named parent; none stacks an untested second fix.
The experiment validator rejects any component or net difference beyond its
allowed change. Production schematics and firmware remain unchanged.

| Priority | Issue | Evidence and decision |
|---|---|---|
| 1 | Plain keypad harness ties opposing GPIO outputs | Reproduced electrically and rejected by the original device model. Diode-per-key schematic candidate passes exhaustive scans and bounded 25°C voltage checks; inspect the fitted keypad before physical changes. |
| 2 | MIDI receiver output loading and receiver fidelity | Original R5=220 Ω loads an ideal 0.2 V sink by 14.1 mA. UART tests bypass the optocoupler. Full 6N138 supply/CTR/edge qualification and the physical DIN/JST harness map are still needed before selecting a receiver redesign. |
| 3 | Microphone preamp cannot yet be judged | R2 remains `?`, module internals and trim values unknown. Direct-jack firmware tests do not validate this path. Preserve the original rather than invent component values. |

The shared engine also tests Keyboard Keyboard's independent filter candidates.
Their improvement and noise tradeoff are documented in that repository. The
keyboard's 0.60–0.70 A sensor load estimate alone does not prove a regulator
fault; a hypothetical 0.5 A source limit is not the fitted Seed's rating. A power
redesign must account for the actual supply, rail drop, other loads and power
sequencing, rather than merely increasing a number in a simulation fixture.

No PCB layout, Gerbers, manufacturing package or physical build is produced by
this work. Schematic candidates are inputs for the builder's later PCB work.

## Power / thermal round

The [isolated R5 load candidate](experiments/rx-load-2k2/README.md) reduces a small
known dissipation term in the ideal-sink model; receiver timing and whole-device
thermal acceptance remain open. Parametric device/source/thermal cases are now
reproducible, with a separate [verification guide](verify-power-thermal.md).
The keyboard study also rejects its offload allocation for promotion after
power-loss counterexamples; moving heat alone does not remove enclosure heat.
