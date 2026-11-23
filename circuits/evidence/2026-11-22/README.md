# Baseline run, 2026-11-22

Executed on macOS/aarch64 with ngspice 47. Both repository schematic paths
export the same 23-component, 58-net circuit; their source and baseline hashes
are identical. The v2 directory holds the shared measurements; the original
directory retains its independently executed report.

Inputs: `../../baselines/*.net` and `../../fixtures/reference.fixture`.
The compiled engine source hashes are in `synthphone-v2/runner-sources.sha256`.
The reports retain baseline and fixture hashes. Executed SPICE decks and the
simulator banner are retained for the main contention/load findings. Full
regenerable vectors can be recreated using the commands in the circuit README.

Validation: seven Rust tests with all features (including SDK adapter contracts),
six without ngspice/SDK, format and all-feature clippy checks. The open-audio,
open-MIDI and open-contact negative controls behaved as expected. Production
firmware was not changed, built or flashed for this circuit-only task.

**Model regression passed; design acceptance remains unverified.** The declared
keypad fixture predicts same-row contention. R2 and module/pot internals remain
unknown. There is no claimed full ARM/SPICE coupled run or physical measurement.
