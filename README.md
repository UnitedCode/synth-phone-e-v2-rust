## Required Libraries
`rustup target add thumbv7em-none-eabihf`

`rustup component add llvm-tools`

`cargo install cargo-binuntils`

`cargo install cargo-embed`

## FLASHING

<!-- TODO check this one -->
`cargo embed --chip Cortex-M7`

## DEBUGGER
Linux
`sudo apt-get install gdb-multiarch`
Window
[Arm GNU Toolchain download](https://developer.arm.com/downloads/-/arm-gnu-toolchain-downloads)
Mac
`brew install arm-none-eabi-gdb`

