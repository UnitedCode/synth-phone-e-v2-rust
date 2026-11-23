# Virtual Daisy

Virtual Daisy is maintained in [UnitedCode/virtual-daisy](https://github.com/UnitedCode/virtual-daisy).
This project pins it as a Git submodule at `tools/virtual-daisy`.

## Check the Rust host

No board, Renode or .NET is required. Build the firmware, then run the finite
audio check on Rev4, Rev5 and Rev7:

```sh
git submodule update --init tools/virtual-daisy
cargo build --release --locked
export VIRTUAL_DAISY_ELF="$PWD/target/thumbv7em-none-eabihf/release/app"
LAB_HOST=$(rustc -vV | sed -n 's/^host: //p')
VIRTUAL_DAISY_FIRMWARE="$VIRTUAL_DAISY_ELF" \
VIRTUAL_DAISY_AUDIO_OUT="$PWD/audio-lab-results/daisy-rust" \
cargo test --release --locked --manifest-path tools/virtual-daisy/Cargo.toml \
  --target "$LAB_HOST" --test firmware_audio -- --ignored --nocapture
```

The output folder must be new. It saves the unchanged ELF/hash, input/output
WAVs and board reports. The test supplies a deterministic input recording and
an explicit I²C sink at the display address. It checks both audio channels,
CPU fault flags and SAI FIFO errors; it does not inspect display pixels or play
audio. Set `VIRTUAL_DAISY_AUDIO_BOARD=rev5` to check one board.

## Compare with Renode

The optional previous backend needs the runtime described in the
[reference setup](../tools/virtual-daisy/docs/reference-runner.md):

```sh
cargo run --release --locked --manifest-path tools/virtual-daisy/Cargo.toml \
  --target "$LAB_HOST" -- run /path/to/renode-runtime audio-lab-results/daisy --board rev5
```

Use `startup` for the Synthphone startup check or `capture` for an ungraded firmware
capture. The runner loads the supplied ELF; firmware builds remain in this project.

The [parity record](../tools/virtual-daisy/docs/rust-parity.md) lists comparisons,
measured waveform differences and the remaining timing limits. Passing these
checks does not establish physical-device equivalence.

See the [standalone README](../tools/virtual-daisy/README.md),
[Synthphone regression guide](../tools/virtual-daisy/docs/synthphone.md), and
[conformance ledger](../tools/virtual-daisy/docs/conformance.md) for runtime setup,
board profiles, ARM probes, audio checks and remaining hardware gaps.
