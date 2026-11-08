# Virtual Daisy

Virtual Daisy is maintained in [UnitedCode/virtual-daisy](https://github.com/UnitedCode/virtual-daisy).
This project pins it as a Git submodule at `tools/virtual-daisy`.

```sh
git submodule update --init tools/virtual-daisy
cargo build --release --locked
export VIRTUAL_DAISY_ELF="$PWD/target/thumbv7em-none-eabihf/release/app"
LAB_HOST=$(rustc -vV | sed -n 's/^host: //p')
cargo run --release --locked --manifest-path tools/virtual-daisy/Cargo.toml \
  --target "$LAB_HOST" -- run /path/to/renode-runtime audio-lab-results/daisy --board rev5
```

Use `startup` for the Synthphone startup check or `capture` for an ungraded firmware
capture. The runner loads the supplied ELF; firmware builds remain in this project.

See the [standalone README](../tools/virtual-daisy/README.md),
[Synthphone regression guide](../tools/virtual-daisy/docs/synthphone.md), and
[conformance ledger](../tools/virtual-daisy/docs/conformance.md) for runtime setup,
board profiles, ARM probes, audio checks and remaining hardware gaps.
