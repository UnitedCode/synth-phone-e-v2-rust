# Test the complete virtual Synthphone-E

Run the unchanged ARM firmware with the frozen carrier circuit, keypad, encoder,
SSD1306 memory, MIDI receiver, codec, DMA and vocal/synth DSP connected.

From a checkout containing this lab:

```sh
LAB_HOST=$(rustc -vV | sed -n 's/^host: //p')
export CARGO_TARGET_DIR="$PWD/target"
export CARGO_INCREMENTAL=0
cargo build --release --locked
mkdir -p circuits/results
cargo run --release --locked --manifest-path tools/circuit-lab/Cargo.toml \
  --features device --target "$LAB_HOST" -- device \
  synthphone-v2 circuits/baselines/synthphone-v2.net circuits/fixtures/reference.fixture \
  target/thumbv7em-none-eabihf/release/app rev4 circuits/results/device-first baseline
```

Choose a **fresh** result directory every time. Native libngspice is required.
This builds and executes an ELF; it never flashes hardware. The baseline uses
repository-main firmware. Active experiment branches are separate inputs and
must be named and hashed as such if tested later.

## What is connected

- The ARM ELF executes in the pinned Rust Virtual Daisy runtime. Interrupts,
  firmware tasks, state transitions, display drawing and DSP run on that CPU.
- Keypad contacts connect the GPIO columns driven by the firmware to its row
  inputs. Opposing push-pull columns are a circuit fault, not an arbitrary low.
- Encoder push/quadrature and hangup contacts drive the actual GPIO inputs.
- The OLED interprets firmware I2C commands and stores its real 128×32 GDDRAM.
  Profile tests recognize visible labels from pixels, not private app state.
- The known direct jack route supplies the codec's right input. The microphone
  preamp route is a declared silent fixture because R2 and module internals are
  unknown; no invented amplifier gain is presented as the existing circuit.
- MIDI bytes enter the modeled UART at 31,250 baud after an ideal receiver
  boundary. Optocoupler CTR/delay and analog MIDI noise remain unmodeled.
- Stereo audio is captured after firmware DSP and codec output. GPIO modes,
  UART/I2C alternate functions, I2C open-drain setup, Cortex-M faults, SAI flags
  and sustained audio-clock output are checked.

The baseline presses every keypad contact, exercises encoder single/double
presses and rotation, switches the audio route, and sends melody/drum MIDI.

## Additional cases

Replace `device` with `device-batch` and use these final two arguments:

```text
circuits/results/device-cases profiles,midi,open-midi,open-key,open-jack,contention,open-display
```

The earlier profile/baseline/fixture/ELF/board arguments stay the same.

`profiles` cycles through all six named modes and checks the OLED pixels for
Pitch Ctrl, Vocode, Synth+Vox, Synth+Tune, Harmony and Percussion while firmware
continues processing audio. It checks functional operation, not subjective vocal
quality, musical tracking accuracy or physical listening acceptance.

`midi` isolates MIDI-generated audio on channel 2 with a silent jack. Channel 1
is intentionally a silent vocal-control channel in this firmware. `open-midi` removes
that source; `open-key` disconnects one contact; `open-jack` removes the audio
source. `contention` closes two keys against opposing drivers. `open-display`
disconnects the OLED. These individual device runs must fail their relevant
gate. The batch marks a negative control PASS only when its specific failure is
observed without an unrelated Cortex-M fault.

`boot` is a shorter I/O-configuration check. A `reset` scenario is available for
explicit later testing; do not treat its existence as an executed reset or
settings-retention result.

## Evidence and limits

Each capture retains its exact ELF, source fingerprints, baseline, assumptions,
stimulus, RTT, GPIO/ADC/MIDI traces, OLED frames/pixels, stereo WAV, nonvolatile
image, progress metrics and acceptance report. `analyze RESULT_DIRECTORY`
summarizes saved MIDI note balance, audio values and recognized profile labels
without rerunning the CPU. It does not replace or overwrite acceptance results.
`recheck RESULT_DIRECTORY` can assess a specific saved negative control or the
historical keyboard pot-polarity correction. It writes a separate hashed verdict
and preserves the original report; it cannot substitute for missing execution.

RTT consumption updates only the debugger read pointer. No application variables,
timers, PC, QSPI settings or idle-loop execution are patched to accelerate a run.
Virtual time and wall time are reported separately. Running the full firmware is
substantially slower than real time on the interpreter.

The runtime pin includes independently tested packed-saturation and narrow-UART
paths exposed by these devices. Neither production firmware nor schematics were
changed to accommodate the tests. Functional device results are separate from
physical calibration: supply/thermal behavior, preamp internals, actual ADC
sample/hold and optocoupler behavior are still declared gaps. Clock rates in the
capture are derived by the model and retained explicitly; they are not a new
measurement of a fitted Seed.

See [the retained device evidence](evidence/2026-11-22-devices/README.md) and
[Keyboard Keyboard's paired lab](https://github.com/UnitedCode/keyboard-keyboard/blob/main/circuits/virtual-devices.md).
