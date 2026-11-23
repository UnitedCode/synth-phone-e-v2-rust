# Unchanged firmware against the diode candidate

Both captures passed their actual ARM device gates with Virtual Daisy revision
`f7892c73b36960d729e6c3bf8bbd0ce3e65498a0`, Rev4 / AK4556 model. The immutable
production ELF is the same as the original device baseline:
`2926a632bf93eb1991f5c91d21cb72309cd0890501d659540f673a3e2517e766`.

- `chords/`: 260 ms of real virtual time. Physical keys 1, 2 and 4 close together
  at 80 ms and release at 140 ms. Exactly those three presses appear in the
  firmware log; there is no phantom key 5. Audio and pin/clock/fault gates pass.
- `complete/`: 820 ms, the original all-twelve-contact sequence, encoder
  single/double/rotation, route switch, MIDI stimulus, display and audio gates.
  No firmware, timer, instruction pointer or private application state is patched.

The original plain-contact carrier's same-row fault is retained in prior
baseline negative-control evidence and the new exhaustive comparison. The
candidate drawing's later rotation/grid corrections were independently exported
and checked against the **same unchanged candidate graph** before publication.
The carrier's subsequent nested-if cleanup is logically equivalent; capture
fingerprints record hashes of the exact compiled model sources for each run.

Reports, RTT logs, stimuli, pin configuration and runner hashes are committed
here. Full raw captures (ELF, GPIO, PCM/WAV, OLED RAM and flash snapshot) are
preserved locally under `audio-lab-results/circuit-improvements-2026-11-22/synthphone/`.
These are simulation results; physical harness, thermal and listening acceptance
remain open. The six-mode profile suite from the original baseline was not
repeated for this candidate; the complete sequence exercises its usual modes.
