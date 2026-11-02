//! RM0433 SAI register, mute-detection and shared IRQ87 fixture.
#![no_std]
#![no_main]
use core::{
    arch::{asm, global_asm},
    ptr::{read_volatile, write_volatile},
};
const RESULTS: *mut u32 = 0x20019d00 as *mut u32;
unsafe fn read(address: u32) -> u32 {
    read_volatile(address as *const u32)
}
unsafe fn write(address: u32, value: u32) {
    write_volatile(address as *mut u32, value);
}
unsafe fn record(word: usize, value: u32) {
    write_volatile(RESULTS.add(word), value);
}
global_asm!(
    r#"
    .syntax unified
    .thumb
    .section .vectors,"a",%progbits
    .word 0x3001f000,probe_main
    .rept 101
    .word probe_unexpected
    .endr
    .word probe_sai
"#
);
#[no_mangle]
pub unsafe extern "C" fn probe_sai() {
    let rx = read(0x20019d80);
    let base = 0x40015800 + rx * 32;
    let ipsr: u32;
    asm!("mrs {},ipsr",out(reg) ipsr);
    record(2, read(base + 0x18) & 2);
    record(3, ipsr);
    write(base + 0x14, 0);
    write(base + 0x1c, 2);
    record(4, read(base + 0x18) & 2);
    record(5, read(base + 0x1c));
    record(1, read_volatile(RESULTS.add(1)) + 1);
}
#[no_mangle]
pub unsafe extern "C" fn probe_main() -> ! {
    for word in 0..32 {
        record(word, 0);
    }
    let rx = read(0x20019d80);
    record(10, rx);
    write(0x580244f0, read(0x580244f0) | (1 << 22));
    write(0x58024450, 4); // SAI kernel = HSI64; MCLK16MHz/LRC62.5kHz.
    record(6, read(0x40015804));
    record(7, read(0x40015824));
    for channel in 0..2 {
        let base = 0x40015800 + channel * 32;
        write(base + 0xc, 63 | (31 << 8) | (1 << 16) | (1 << 18));
        write(base + 0x10, 0x30180);
        write(base + 8, if channel == rx { 2 << 7 } else { 0 });
        write(
            base + 4,
            (4 << 20)
                | (6 << 5)
                | (channel << 1)
                | u32::from(channel == rx)
                | if channel == 1 { 1 << 10 } else { 0 },
        );
    }
    let tx = 0x40015800 + (1 - rx) * 32;
    write(tx + 0x20, 1);
    write(tx + 0x20, 2);
    record(8, read(tx + 0x18) >> 16); // two TX words are FLVL=001.
    write(0x40015824, read(0x40015824) | (1 << 16));
    write(0x40015804, read(0x40015804) | (1 << 16));
    write(0xe000e454, 0x20000000); // IRQ87 priority, aligned IPR word.
    record(0, 0x42);
    asm!("cpsie i");
    while read_volatile(RESULTS.add(1)) == 0 {
        asm!("wfi");
    }
    record(0, 0xab);
    loop {
        asm!("wfi");
    }
}
#[no_mangle]
pub unsafe extern "C" fn probe_unexpected() -> ! {
    let ipsr: u32;
    asm!("mrs {},ipsr",out(reg) ipsr);
    record(31, ipsr);
    loop {
        asm!("wfi");
    }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    unsafe { probe_unexpected() }
}
