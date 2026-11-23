# Isolated candidate: keypad diodes

The original plain-contact harness model connects opposite push-pull GPIO columns
when two keys in one row are held. Its existing 33 Ω driver / 10 Ω contact fixture
predicts a row near 1.65 V and roughly 38 mA from the HIGH driver. These are model
results, not measured currents or proof of the fitted keypad's internal wiring.

`keypad-harness.kicad_sch` replaces that external harness with twelve individual
switch/diode branches. Each BAT54H **anode (pin 2) faces its row**; its **cathode
(pin 1, marked bar) faces the switch and column**. The series switch is rotated
so pin 1 joins the diode. `candidate.net` joins this subcircuit to an exact copy
of `synthphone-v2.net`. The main board, unknown preamp and firmware are unchanged.

This needs access to each key contact or a replacement keypad. Adding one diode
to each shared row/column wire cannot provide this isolation. No PCB layout or
fabrication output is supplied; the builder owns the physical PCB design.

## Comparison

- 4,096 contact sets × three scan columns: 12,288 cases, no phantom/missing keys.
- The original same-row short remains an expected failure; reversing a candidate
  diode is rejected by the wiring gate.
- Twelve DC corners: 3.0/3.3/3.6 V, 30/50 kΩ pull-ups, 33/100 Ω drivers.
  Worst LOW is about 0.366 V; minimum conservative logic margin is 0.542 V.
- KiCad-exported connectivity independently matches every candidate branch.
- Actual ARM firmware captures are linked in the device evidence directory.

The analog model bounds forward drop at 0.32 V and reverse leakage at 2 µA per
diode at **25°C**, with four rows sharing a driver. It is a constant-drop bound,
not a vendor transistor model. Forward currents remain below 1 mA. HIGH-state
leakage uses a conservative three-diode sinking bound. Cable capacitance, bounce,
temperature extremes and the actual assembled harness remain unverified.

Sources: [Nexperia BAT54H, characteristics table](https://assets.nexperia.com/documents/data-sheet/BAT54H.pdf),
[ST STM32H750, I/O characteristics](https://www.st.com/resource/en/datasheet/stm32h750xb.pdf).
The 0.3 VDD / 0.7 VDD logic limits are conservative CMOS thresholds; the supplied
fixture's 33 Ω driver is an assumption, with a separate 100 Ω corner.

## Reproduce

From this repository, with native ngspice installed:

```sh
export CARGO_TARGET_DIR="$PWD/target"
export CARGO_INCREMENTAL=0
cargo run --release --locked --manifest-path tools/circuit-lab/Cargo.toml \
  --target aarch64-apple-darwin --features device -- compare synthphone-key-diodes \
  circuits/baselines/synthphone-v2.net circuits/experiments/key-diodes/candidate.net \
  circuits/experiments/key-diodes/assumptions.fixture circuits/results/key-diodes-new
```

Use the normal device command with profile `synthphone-key-diodes` and this
candidate/fixture. Run `chords` for a three-key rectangle, then `baseline` for the
original all-contact/encoder/audio sequence. Use fresh output directories.
On Linux use the host target from `rustc -vV` instead of the macOS target above.

To regenerate a new copy, `candidate synthphone-key-diodes PARENT FIXTURE NEW_DIR`
creates the graph, copied assumptions and BOM delta; `schematic synthphone-key-diodes
KICAD_SYMBOL_DIRECTORY NEW_FILE` creates the reviewable KiCad harness. Export with
KiCad's `sch export netlist --format kicadxml`, then use `check-schematic NAME
CANDIDATE XML SCHEMATIC NEW_REPORT` to verify the independent drawing.
