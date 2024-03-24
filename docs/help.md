`cargo size -- -Ax` 
`cargo `

# GDB in linux
`gdb-multiarch target/thumbv7em-none-eabihf/debug/synth-phone-e-v2-rust`
- `target remote :1337` port should be whatever is specified by the `cargo embed` command
- `info registers`
- `set print asm-demangle on`
- `disassemble`
- `stepi`
- `break main.rs:16` this is where we increment x
- `continue`
- `info locals`
- `print x`
- `print &x` see address
- `set var x=6` update variable
- `info break` check breakpoints
     - `delete 1`
- `monitor reset` reset micro controller