# Getting Started with Embedded Rust

This guide will help you set up your environment for embedded development using Rust. We'll cover the installation of necessary tools and libraries, and show you how to flash your program to a microcontroller and debug it.

## Prerequisites

Ensure you have Rust installed on your system. If not, you can install it from the [official Rust website](https://www.rust-lang.org/tools/install).

## Required Libraries and Tools

1. **Add the Rust Target for ARM Cortex-M microcontrollers:**

   ```sh
   rustup target add thumbv7em-none-eabihf
   ```

2. **Add LLVM Tools:**

   ```sh
   rustup component add llvm-tools-preview
   ```

3. **Install cargo-binutils:**

   ```sh
   cargo install cargo-binutils
   ```

4. **Install cargo-embed (actually probe-rs):**

   https://probe.rs/docs/getting-started/installation/

   `cargo-embed` is used for flashing your program to the microcontroller and provides a basic debugger.


## Flashing Your Program

To flash your program to a microcontroller, navigate to your project directory in the terminal and run:

```sh
cargo xtask embed
```

This command reads your `Embed.toml` configuration file (if present) and flashes the program accordingly.

## Automated vocal audio tests

The [Rust audio lab](docs/audio-testing.md) runs the firmware's pinned vocal DSP on
your computer, replays WAV recordings, and sweeps 720 pitch/key/formant settings.
It measures pitch error and audio defects without flashing the Daisy.

```sh
LAB_HOST=$(rustc -vV | sed -n 's/^host: //p')
cargo test --release --locked --manifest-path tools/audio-lab/Cargo.toml --target "$LAB_HOST"
cargo run --release --locked --manifest-path tools/audio-lab/Cargo.toml --target "$LAB_HOST" -- sweep audio-lab-results
# Execute the ARM DSP in QEMU (requires qemu-system-arm):
cargo run --release --locked --manifest-path tools/audio-lab/Cargo.toml --target "$LAB_HOST" -- emulate audio-lab-results/arm
```

The [virtual Daisy](docs/virtual-daisy.md) now boots the **actual production ELF**
under Renode, feeds audio through modeled codec/SAI/DMA peripherals and records
stereo WAV output. Its regression command checks tuning, boot completion and
audio continuity, then verifies detection of silent input and lost RX DMA.

```sh
cargo run --release --locked --manifest-path tools/virtual-daisy/Cargo.toml --target "$LAB_HOST" -- run /absolute/path/to/renode-runtime audio-lab-results/daisy
```

The [ARM emulation guide](docs/arm-emulation.md) covers the complementary QEMU
DSP parity tests. Physical hardware is still needed to calibrate performance and
analog codec behavior. The audio-testing guide documents known quality failures
and how CI distinguishes regressions from unresolved issues.

## Setting Up the Debugger

### Linux

- Install `gdb-multiarch` to debug a wide range of architectures:

  ```sh
  sudo apt-get install gdb-multiarch
  ```

### Windows

- Download and install the [Arm GNU Toolchain](https://developer.arm.com/downloads/-/arm-gnu-toolchain-downloads). This package includes `arm-none-eabi-gdb`, which is the debugger for ARM microcontrollers.

### macOS

- Use Homebrew to install the ARM version of GDB:

  ```sh
  brew install arm-none-eabi-gdb
  ```

## Debugging Your Program

To start a debug session, you need a GDB server running for your specific microcontroller. This might be provided by `cargo embed`, OpenOCD, or JLinkGDBServer, depending on your setup.

1. **Start the GDB Server** (if not automatically started by `cargo embed`):

   This step varies depending on your hardware and the software you're using as a GDB server. Refer to your hardware's documentation or the documentation of the GDB server software.

2. **Connect to the GDB Server:**

   - For Linux and macOS, you can use `gdb-multiarch` or `arm-none-eabi-gdb` respectively.
   - For Windows, use the `arm-none-eabi-gdb` from the GNU Arm Toolchain.

   Open a terminal and run:

   ```sh
   gdb-multiarch target/thumbv7em-none-eabihf/debug/synth-phone-e-v2-rust
   ```

   Replace `gdb-multiarch` with `arm-none-eabi-gdb` if you're on Windows or macOS


3. **Inside GDB**, connect to the local GDB server:

   ```gdb
   target remote :1337
   ```

   The port `1337` is an example; use the port number on which your GDB server is running.

You're now ready to debug your program using GDB commands!

## Further Learning

- The [Embedded Rust Book](https://docs.rust-embedded.org/book/) is an excellent resource for learning more about embedded development with Rust.
- Check out [The Discovery Book](https://docs.rust-embedded.org/discovery/) for hands-on projects aimed at beginners.
- How the project is setup [Ferrous Systems Test Embedded App](https://ferrous-systems.com/blog/test-embedded-app/)


# File Structure
```
├── .cargo
│   └── config.toml # <- contains no build target, just command alias
├── .github
│   ├── workflows
│   │   ├── nightly.yml
│   │   ├── rustfmt.yml # <- linter
│   │   └── test.yml
├── .vscode
│   └── settings.json
├── Cargo.toml # <- outer Cargo workspace
├── autotune # <- auto tune library, can be run on host
│   ├── Cargo.toml
│   └── src
├── cross # <- Embedded workspace cannot be ran on host
│   ├── Cargo.toml # <- inner Cargo workspace
│   ├── memory.x
│   ├── .cargo
│   │   └── config.toml # <- build target for daisy seed
│   ├── app
│   ├── board
│   ├── Embed.toml
│   ├── self-tests
│   └── target
├── docs
│   ├── help.md
│   └── random_notes.md
├── host-target-tests 
│   ├── Cargo.toml
│   ├── src
│   └── tests
├── README.md
├── target
│   ├── CACHEDIR.TAG
│   └── debug
└── xtask
    ├── Cargo.toml
    └── src

```
