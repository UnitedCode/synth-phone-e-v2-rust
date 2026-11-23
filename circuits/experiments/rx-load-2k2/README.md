# Isolated Synthphone receiver load / thermal candidate

Only R5 changes: **220 Ω → 2.2 kΩ** in a separate full KiCad schematic.
The parent remains `synthphone-v2.net`; neither the keypad-diode candidate nor any
other change is stacked here. The original schematic and firmware stay intact.

At 3.3 V with a declared ideal 0.2 V sink, R5 dissipation falls from **43.68 mW
to 4.37 mW**, a 90% reduction for this resistor. That removes only about **42 mW
of receiver rail load**, before conversion losses. It cannot explain or solve a
large whole-device thermal problem by itself.

`study/receiver-thermal.csv` sweeps rail tolerance, ambient temperature and
assumed 200/400/600 K/W resistor thermal resistance. `device-envelope.csv` adds
explicit unknown Seed/auxiliary current, conversion efficiency, source budget,
wiring resistance and enclosure/local thermal scenarios. These are sensitivity
cases, not measurements or fitted Seed regulator ratings. The 85°C threshold is
a project budget, not a component maximum rating.

**Decision: receiver acceptance remains unverified.** The output sink model is
not a 6N138 CTR/storage-delay model. A larger pull-up also changes receiver edge
behavior. The 3.3 V optocoupler supply, actual vendor, input harness, capacitance,
preamp and other loads still need qualification. No firmware UART run can validate
this analog receiver while bytes enter after an ideal optocoupler boundary.
Do not infer a complete thermal fix or MIDI reliability from the power-only pass.

The separate schematic exports to exactly the candidate connectivity. The edit
has an independent test proving every other schematic byte is unchanged.
No PCB layout or manufacturing output is generated.

[Independent verification commands](../../verify-power-thermal.md) reproduce the
power calculation, compare candidate isolation and run deliberate invalid-input
controls. Exact scenarios and runner/source fingerprints accompany each report.
