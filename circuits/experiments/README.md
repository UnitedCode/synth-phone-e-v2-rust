# Experiments

Create a named subdirectory, copy a baseline and a fixture, and edit the copies.
Keep the parent baseline unchanged. Record the question, changed parameters,
expected result and comparison to the baseline in that directory's README.

Run `verify` with the copied files and a new result directory. The output includes
the exact baseline and assumptions used, source fingerprints, executed SPICE
decks, vectors, simulator logs, per-component coverage and a report.

`MODEL_REGRESSION=PASS` means the fixture and negative controls ran as expected.
Read `DESIGN_ACCEPTANCE`, the individual measurements and unknowns separately.
For changed topology outside the existing profile contract, add a new profile
and independent expectations; don't weaken baseline assertions to obtain a pass.

Production schematics and firmware stay in their normal locations. Promote a
design only after reviewing the comparison and its remaining evidence gaps.

## First improvements

- [Keypad diodes](key-diodes/README.md): isolated external-harness schematic,
  exhaustive contact tests and bounded analog margins.
- [Issue ranking](../issue-triage.md): reproduced faults, unmodeled paths and
  evidence required for later changes.

`candidate` creates a new copy; `compare` validates the exact allowed delta before
running the baseline and candidate checks. `COMPARISON=PASS` means that comparison
executed successfully. Read the separate design acceptance and measured gates.

- [Receiver load / power study](rx-load-2k2/README.md): separate R5 schematic,
  bounded thermal cases and [independent verification](../verify-power-thermal.md).
