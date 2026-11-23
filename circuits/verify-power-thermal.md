# Independently verify the power / thermal study

Use a clean checkout of this report's revision. Your active experiment branch
can stay untouched: fetch the repository and create a separate Git worktree
under `~/Documents/GitHub` at the commit linked by the report. Check `df -h` before
builds; retain the project's 500 GB free-space target. Reuse the existing target
cache rather than creating another large build cache.

Requires Rust and native ngspice (`brew install ngspice` on macOS; on Debian/Ubuntu,
`libngspice0-dev` and `libdigest-sha-perl`). The commands below use Apple Silicon;
replace `aarch64-apple-darwin` with the host target shown by `rustc -vV` elsewhere.
Nothing here flashes hardware. Every result directory must be new.

From the Synthphone repository:

```sh
export CARGO_TARGET_DIR="$HOME/Documents/GitHub/synth-phone-e-v2-rust/target"
export CARGO_INCREMENTAL=0
mkdir -p circuits/results
cargo test --locked --manifest-path tools/circuit-lab/Cargo.toml \
  --target aarch64-apple-darwin --all-features
cargo run --release --locked --manifest-path tools/circuit-lab/Cargo.toml \
  --target aarch64-apple-darwin --features device -- power-study synthphone-rx-2k2 \
  circuits/baselines/synthphone-v2.net circuits/experiments/rx-load-2k2/candidate.net \
  circuits/fixtures/power-scenarios.fixture circuits/results/verify-rx-1
```

Expected: `ISOLATION=PASS`, R5 power **0.0436818 W → 0.00436818 W**, and
`DESIGN_ACCEPTANCE=UNVERIFIED_RECEIVER`. A passing study means its checks executed;
it does not certify the unmodeled receiver or a physical operating temperature.
The file hashes identify the exact inputs; `runner-sources.sha256` identifies the
compiled model sources and lockfile.

Hand-check the load: `I=(3.3−0.2)/R`, `P=I²R`. For a constant-power source plus
series wiring resistance, the high-voltage solution is
`Vload=(Vin+sqrt(Vin²−4 Rpath Pin))/2`. An invalid root or insufficient source
current is a failure, not a regulator upgrade. Thermal cases use
`Tsteady=Tamb+P θ` and `T(t)=Tamb+P θ(1−exp(−t/(θ C)))`.
They exclude foldback/shutdown and thermal coupling detail; above-budget cells
are rejected assumptions, not predictions that a real board reaches that value.

Prove the isolation gate catches a mistake: repeat the command with
`circuits/baselines/synthphone-v2.net` in place of the candidate and a fresh output
folder such as `circuits/results/verify-rx-invalid-1`. It must exit unsuccessfully
and record `STUDY_EXECUTION=FAIL`. Never edit the frozen parent to get a pass.

For an independent drawing check, export the candidate with KiCad:

```sh
kicad-cli sch export netlist --format kicadxml -o circuits/results/verify-rx.xml \
  circuits/experiments/rx-load-2k2/synthphone-rx-2k2.kicad_sch
cargo run --release --locked --manifest-path tools/circuit-lab/Cargo.toml \
  --target aarch64-apple-darwin --features device -- check-schematic synthphone-rx-2k2 \
  circuits/experiments/rx-load-2k2/candidate.net circuits/results/verify-rx.xml \
  circuits/experiments/rx-load-2k2/synthphone-rx-2k2.kicad_sch \
  circuits/results/verify-rx-drawing.txt
```

On macOS, the native CLI may be at
`/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli`. The expected result is
`SCHEMATIC_CONNECTIVITY=PASS`; this is not a manufacturing ERC/DRC approval.

The [keyboard verification guide](https://github.com/UnitedCode/keyboard-keyboard/blob/main/circuits/verify-power-noise.md)
reproduces the filter sweep, native firmware-kernel power-loss fault and separate
carrier power allocation. Physical confirmation needs the actual Seed/Hall part,
5 V input current/voltage, distant 3.3 V rail, temperature rise and noise captures.
A USB power meter, voltage meter and thermocouples/thermal camera can establish
those values on the existing device while repeating the same key/pot sequence.
Do not treat hypothetical fixture loads or thermal resistances as measurements.
