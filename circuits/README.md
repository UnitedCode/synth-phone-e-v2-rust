# Synthphone-E circuit lab

Test the existing circuit before redesigning it. Start here:

```sh
LAB_HOST=$(rustc -vV | sed -n 's/^host: //p')
export CARGO_TARGET_DIR="$PWD/target"
export CARGO_INCREMENTAL=0
mkdir -p circuits/results
cargo run --locked --manifest-path tools/circuit-lab/Cargo.toml --target "$LAB_HOST" -- \
  verify synthphone-v2 circuits/baselines/synthphone-v2.net \
  circuits/fixtures/reference.fixture circuits/results/v2-first
```

Use a **new** output directory for every run. A failed run is retained too.
The native ngspice shared library must be installed; `NGSPICE_LIB_DIR` selects
a custom library directory. The runner and build fingerprinting require `shasum`.
No hardware is flashed and no firmware dependency or build setting changes.

| Directory | Purpose |
| --- | --- |
| `baselines/` | Frozen component values and every net/pin, with schematic SHA256 |
| `fixtures/` | Explicit provisional impedances and loading; edit copies for experiments |
| `experiments/` | Isolated proposals with their own question and comparison |
| `results/` | Ignored, regenerable decks, vectors, logs, inputs and coverage |
| `evidence/` | Small retained reports and test inputs from reviewed runs |
| `../tools/circuit-lab/` | Rust runner, generic connectivity engine and boundary adapters |

## Existing designs

`synthphone-original` comes from `kicad/synth-phone-e-v2-hardware/`;
`synthphone-v2` comes from `kicad/synth-phone-e-v2-hardware-v2/`.
At the captured revision both schematics are byte-identical: these names record
their repository locations, not two electrically different designs. Their exact
match to the assembled unit has not been established. Neither is silently repaired.

The tests check keypad physical-pad mapping, encoder connector routing, OLED
routing, optocoupler/output pullup wiring and both audio input/output routes.
ngspice tests twelve declared external keypad contacts, a disconnected contact,
two opposing columns connected by held keys, and the load imposed by the actual
220 ohm MIDI output pullup on an ideal receiving sink. This is **not** an
optoisolator CTR, speed or supply-compliance model.

The keypad harness is external to the schematic. A provisional contact model is
paired with the firmware's low/high/high column policy in `src/input/buttons.rs`.
Two same-row contacts predict opposing-output contention. The reference fixture
gives about 1.65 V at the row and 38 mA from the high driver; those numbers are
fixture predictions, not measured MCU current or a verified description of a
diode-equipped external keypad.

R2 is explicitly `?`; the SSM2167 board's internal schematic and both pot values
are absent. Preamp gain, compression, noise, headroom and codec analog behavior
remain unmodeled. Every run writes `coverage.csv` so inventory does not get
mistaken for electrical coverage. These unknowns do not block the models above.

## Run checks and refresh a baseline

```sh
cargo test --locked --manifest-path tools/circuit-lab/Cargo.toml --target "$LAB_HOST"
cargo clippy --locked --manifest-path tools/circuit-lab/Cargo.toml --target "$LAB_HOST" --all-targets --all-features -- -D warnings
cargo fmt --manifest-path tools/circuit-lab/Cargo.toml -- --check
```

`--no-default-features` runs connectivity and host boundary tests without ngspice.
Reports explicitly mark analog tests `NOT_RUN` in that build.

To check source drift, run `check-source BASELINE SCHEMATIC` through the same
runner. To create a **new** baseline, first export with KiCad 9:

```sh
kicad-cli sch export netlist --format kicadxml -o /tmp/new-synthphone.xml path/to/design.kicad_sch
# Runner arguments after `--`:
# import /tmp/new-synthphone.xml path/to/design.kicad_sch circuits/experiments/my-design/design.net
```

The importer accepts KiCad's line-oriented XML export and produces sorted TSV;
export timestamps, machine paths and XML ordering do not affect connectivity.
Never regenerate the original baseline in place to bless a redesign.

## Connect to Virtual Daisy

Enable `daisy` for adapters implementing `virtual_daisy::machine::Carrier` at
the pinned SDK revision. `carrier::SynthphoneCarrier::new(&graph)` attaches
keypad contacts through `contact(row, column, closed)`. `row_contacts` also works
without the SDK. The adapter records a contention fault rather than declaring an
ambiguous row a valid digital low; callers must retain access to and check
`fault` after execution. OLED writes are acknowledged/counted at I2C1/0x3c;
this is a recording sink, not an SSD1306 image simulation. Encoder, hangup and
channel controls are not yet actively stimulated by this adapter.

`carrier::shared(carrier)` returns `(handle, attachment)` so the machine can own
`attachment` while the caller changes contacts and inspects faults through
`handle.borrow_mut()` / `handle.borrow()`. Change inputs between bounded machine
runs; do not borrow the handle while executing the machine.

SPICE uses explicit scan stimuli, and the adapter supplies digital contact
levels. They do **not** run a synchronized CPU/analog feedback loop. Full
production ARM execution with these adapters is `NOT_RUN` for this baseline.
The existing [firmware emulator](../docs/virtual-daisy.md) and
[audio lab](../docs/audio-testing.md) remain independently usable.

## Interpret results

`MODEL_REGRESSION=PASS` means the declared models and negative controls behaved
as expected. `DESIGN_ACCEPTANCE` remains unverified; known faults are findings,
not waived checks. `ANALOG_SIMULATED`, `ARM_COUPLED`, and `PHYSICAL_MEASURED` are
separate evidence levels. This lab establishes a repeatable first circuit model,
not complete analog fidelity or physical hardware acceptance.

Keyboard Keyboard keeps its own [baselines and pinned consumer](https://github.com/UnitedCode/keyboard-keyboard/tree/main/circuits).
The organization follows AnsaLoop's independent controller/carrier/circuit layers;
no AnsaLoop file or shared Virtual Daisy checkout is modified by these tests.
