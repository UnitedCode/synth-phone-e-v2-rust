//! RM0433: FMC refresh error is IRQ48, independent of the audio DMA path.
#![no_std]
#![no_main]
use core::{
    arch::{asm, global_asm},
    ptr::{read_volatile, write_volatile},
};
const RESULTS: *mut u32 = 0x20019c00 as *mut u32;
extern "C" {
    fn probe_bad_load(dummy: u32, address: u32) -> u32;
}
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
    .rept 3
    .word probe_unexpected
    .endr
    .word probe_bus_fault_entry
    .rept 58
    .word probe_unexpected
    .endr
    .word probe_fmc
    .section .text.probe_entries,"ax",%progbits
    .global probe_bus_fault_entry
    .thumb_func
probe_bus_fault_entry:
    mrs r0,msp
    b probe_bus_fault
    .global probe_bad_load
    .thumb_func
probe_bad_load:
    ldr r0,[r1]
    bx lr
"#
);
#[no_mangle]
pub unsafe extern "C" fn probe_bus_fault(frame: *mut u32) {
    record(12, read(0xe000ed28));
    record(13, read(0xe000ed38));
    let ipsr: u32;
    asm!("mrs {},ipsr",out(reg) ipsr);
    record(14, ipsr);
    write(0xe000ed28, read(0xe000ed28));
    write_volatile(frame.add(6), read_volatile(frame.add(6)) + 2);
    record(15, read_volatile(RESULTS.add(15)) + 1);
}
#[no_mangle]
pub unsafe extern "C" fn probe_fmc() {
    let ipsr: u32;
    asm!("mrs {},ipsr",out(reg) ipsr);
    record(2, read(0x52004158));
    record(3, ipsr);
    write(0x52004154, (41 << 1) | 1);
    record(4, read(0x52004158));
    record(5, read(0x52004154));
    record(1, read_volatile(RESULTS.add(1)) + 1);
}
#[no_mangle]
pub unsafe extern "C" fn probe_main() -> ! {
    for word in 0..32 {
        record(word, 0);
    }
    write(0x580244d4, read(0x580244d4) | (1 << 12));
    write(0x52004140, 0);
    write(0x52004148, 0);
    write(0x5802447c, 1 << 12);
    write(0x5802447c, 0);
    record(10, read(0x52004140));
    record(11, read(0x52004148));
    write(
        0x52004140,
        (2 << 4) | (3 << 7) | (2 << 10) | (1 << 13) | (1 << 12),
    );
    write(0x52004148, 0x02126471);
    for (index, address) in [0xc0000000, 0xc0000400, 0xc0200000].into_iter().enumerate() {
        write(address, 0x12340000 + index as u32);
    }
    for (index, address) in [0xc0000000, 0xc0000400, 0xc0200000].into_iter().enumerate() {
        record(6 + index, read(address));
        if index == 0 {
            record(9, read(address + 4));
        }
    }
    write(0xe000ed24, 1 << 17);
    let _ = probe_bad_load(0, 0xc0400000);
    write(0xe000e430, 0x20);
    write(0xe000e104, 1 << 16);
    write(0x52004154, (41 << 1) | (1 << 14));
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
