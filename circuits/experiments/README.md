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
