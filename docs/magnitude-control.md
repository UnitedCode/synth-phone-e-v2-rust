# Magnitude control

Magnitude controls the vocal DSP's FFT-bin rejection threshold in Pitch Control,
Tuned and generated Harmony voices. Higher values reject weaker spectral bins;
lower values retain them. Voice Level and master volume control gain separately.

| Menu value | FFT magnitude threshold |
| --- | --- |
| 1 | 0.0000000001 |
| 5 (default) | 0.000001 |
| 10 | 0.1 |

Each step changes the threshold tenfold. These are internal FFT amplitude units.
Value 5 preserves the previous DSP default exactly. High values can remove quiet
voiced content, so use lower settings when quiet material should remain audible.
The control does not gate the unshifted Harmony lead, Dry audio, Vocode or drums.

Encoder menu edits, MIDI CC42 and restored saved settings are applied at the next
FFT hop through the shared firmware/audio-lab configuration. CC42 maps 0–127 into
menu values 1–10. The saved-settings format stays compatible.

Regression tests check the full range, default compatibility at every speed,
menu/MIDI/persistence routing, and actual rendered quiet and normal voiced audio
in Pitch Control and Harmony. They verify spectral rejection and retained pitch;
physical microphone sensitivity and listening acceptance require board testing.
