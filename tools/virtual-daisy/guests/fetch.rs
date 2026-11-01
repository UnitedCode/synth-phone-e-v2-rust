//! Execute a valid mapped prefix and fault a 32-bit fetch across the FSIZE end.
#![no_std]
#![no_main]
use core::{
    arch::{asm, global_asm},
    ptr::{read_volatile, write_volatile},
};
const RESULTS: *mut u32 = 0x20019400 as *mut u32;
unsafe fn read(address: u32) -> u32 {
    read_volatile(address as *const u32)
}
unsafe fn write(address: u32, value: u32) {
    write_volatile(address as *mut u32, value);
}
unsafe fn record(word: usize, value: u32) {
    write_volatile(RESULTS.add(word), value);
}
unsafe fn barriers() {
    asm!("dsb", "isb", options(nostack));
}
global_asm!(
    r#"
    .syntax unified
    .thumb
    .section .vectors,"a",%progbits
    .word 0x2001f000,probe_main,probe_unexpected,probe_fault_entry
    .word probe_fault_entry,probe_fault_entry,probe_fault_entry,0
    .word 0,0,0,probe_unexpected,probe_unexpected,0,probe_unexpected,probe_unexpected
    .section .text.probe_entries,"ax",%progbits
    .global probe_fault_entry
    .thumb_func
probe_fault_entry:
    mrs r0,msp
    push {{r4,lr}}
    bl probe_fault
    pop {{r4,lr}}
    bx lr
    .global probe_fetch
    .thumb_func
probe_fetch:
    push {{r4,lr}}
    blx r0
    .global probe_fetch_resume
    .thumb_func
probe_fetch_resume:
    pop {{r4,pc}}
    .balign 4
    .global probe_t32_store
    .thumb_func
probe_t32_store:
    str.w r2,[r1]
    bx lr
"#
);
extern "C" {
    fn probe_fetch(address: u32, marker: *mut u32, value: u32);
    fn probe_fetch_resume();
    fn probe_t32_store();
}
#[no_mangle]
unsafe extern "C" fn probe_fault(frame: *mut u32) {
    let phase = read_volatile(RESULTS);
    let count = read_volatile(RESULTS.add(1));
    if !(1..=3).contains(&phase) || count >= 4 {
        probe_unexpected();
    }
    record(1, count + 1);
    let base = 4 + (phase as usize - 1) * 4;
    let exception: u32;
    asm!("mrs {},ipsr",out(reg) exception,options(nostack));
    record(base, exception);
    record(base + 1, read(0xe000ed28));
    record(base + 2, read_volatile(frame.add(6)));
    record(base + 3, read(0xe000ed2c));
    write_volatile(frame.add(6), probe_fetch_resume as *const () as u32 & !1);
    write(0xe000ed28, u32::MAX);
    write(0xe000ed2c, read(0xe000ed2c) & 0x40000002);
}
unsafe fn program(address: u32, words: &[u32]) {
    write(0x52005000, 3 | (3 << 24)); // Abort mapping; normal SPI at HSI64/4.
    write(0x52005004, 22 << 16); // Entire physical chip for indirect programming.
    write(0x52005014, 0x106);
    write(0x52005010, words.len() as u32 * 4 - 1);
    write(0x52005014, 0x01002502);
    write(0x52005018, address);
    for &word in words {
        write(0x52005020, word);
    }
    loop {
        write(0x52005010, 0);
        write(0x52005014, 0x05000105);
        if read(0x52005020) & 1 == 0 {
            break;
        }
    }
}
unsafe fn mapped(size: u32) {
    write(0x52005004, size << 16);
    write(0x52005014, 0x0d002503);
    barriers();
}
#[no_mangle]
unsafe extern "C" fn probe_main() -> ! {
    for word in 0..32 {
        record(word, 0);
    }
    write(0xe000ed24, 7 << 16);
    write(0xe000ed14, 0x200 | (1 << 17)); // Real Rust I-cache model, bypassing native translation cache.
    write(0x580244d4, 1 << 14);
    barriers();
    let marker_instruction = if read(0x20019500) & 1 == 0 {
        0x6008205a
    } else {
        0x60082000
    };
    program(0x38, &[marker_instruction, 0xbf00bf00]);
    mapped(5); // 64-byte logical window; valid prefix ends at its boundary.
    record(0, 1);
    probe_fetch(0x90000039, RESULTS.add(16), 0xbadbad);
    record(17, read_volatile(RESULTS.add(16)));
    let instruction = read(probe_t32_store as *const () as u32 & !1);
    record(19, instruction);
    program(0x7e, &[instruction]);
    mapped(6); // The second halfword of STR.W is outside the 128-byte window.
    record(0, 2);
    probe_fetch(0x9000007f, RESULTS.add(16), 0xbadbad);
    record(18, read_volatile(RESULTS.add(16)));
    write(0x52005000, 0); // Mapping off; first line must still execute from I-cache.
    barriers();
    record(16, 0);
    record(0, 3);
    probe_fetch(0x90000039, RESULTS.add(16), 0xbadbad);
    record(0, 0xab);
    loop {
        asm!("wfi", options(nostack));
    }
}
#[no_mangle]
unsafe extern "C" fn probe_unexpected() -> ! {
    record(31, 0xdead);
    loop {
        asm!("wfi", options(nostack));
    }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    unsafe { probe_unexpected() }
}
