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

4. **Install cargo-embed:**

   ```sh
   cargo install cargo-embed
   ```

   `cargo-embed` is used for flashing your program to the microcontroller and provides a basic debugger.

## Flashing Your Program

To flash your program to a microcontroller, navigate to your project directory in the terminal and run:

```sh
cargo embed
```

This command reads your `Embed.toml` configuration file (if present) and flashes the program accordingly.

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
