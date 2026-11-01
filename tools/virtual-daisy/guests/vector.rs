//! PM0253 VECTTBL: a failed IRQ vector uses HardFault and the existing frame.
#![no_std]
#![no_main]
use core::{
    arch::{asm, global_asm},
    ptr::{read_volatile, write_volatile},
};
const RESULTS: *mut u32 = 0x20019800 as *mut u32;
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
    .rept 14
    .word probe_unexpected
    .endr
    .section .text.probe_entries,"ax",%progbits
    .global probe_fault_entry
    .thumb_func
probe_fault_entry:
    mov r0,sp
    mov r1,lr
    b probe_fault
    .global probe_wait
    .thumb_func
probe_wait:
    wfi
    .global probe_wait_return
    .thumb_func
probe_wait_return:
    bx lr
    .global probe_warm
    .thumb_func
probe_warm:
    udf #0
    .global probe_warm_return
    .thumb_func
probe_warm_return:
    bx lr
"#
);
extern "C" {
    fn probe_fault_entry();
    fn probe_wait();
    fn probe_wait_return();
    fn probe_warm();
    fn probe_warm_return();
}
#[no_mangle]
unsafe extern "C" fn probe_fault(frame: *mut u32, link: u32) {
    let exception: u32;
    asm!("mrs {},ipsr", out(reg) exception, options(nostack));
    if read_volatile(RESULTS) == 0 && read(0x20019900) & 1 != 0 {
        record(18, read(0xe000ed2c));
        record(19, read(0xe000ed28));
        record(20, exception);
        record(21, link);
        record(2, 1);
        write_volatile(frame.add(6), probe_warm_return as *const () as u32 & !1);
        write(0xe000ed2c, 0xc0000002);
        write(0xe000ed28, u32::MAX);
        return;
    }
    if read_volatile(RESULTS) != 0x42 || read_volatile(RESULTS.add(1)) != 0 {
        probe_unexpected();
    }
    record(1, 1);
    record(5, exception);
    record(6, read(0xe000ed2c));
    record(7, read(0xe000ed28));
    record(8, link);
    record(9, frame as u32);
    record(10, read_volatile(frame.add(6)));
    record(11, read_volatile(frame.add(7)));
    record(12, read(0xe000ed04));
    record(13, read(0xe000e300));
    record(14, read(0xe000e200));
    write(0xe000e180, 1); // The original IRQ remains pending, but cannot repeat.
}
unsafe fn program(address: u32, words: &[u32]) {
    write(0x52005000, 3 | (3 << 24));
    write(0x52005004, 22 << 16);
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
#[no_mangle]
unsafe extern "C" fn probe_main() -> ! {
    for word in 0..32 {
        record(word, 0);
    }
    let control = read(0x20019900);
    write(
        0xe000ed14,
        0x200 | if control & 1 != 0 { 1 << 17 } else { 0 },
    );
    write(0xe000ed24, if control & 4 != 0 { 7 << 16 } else { 0 });
    record(23, read(0xe000ed24) & (7 << 16));
    write(0x580244d4, 1 << 14);
    asm!("dsb", "isb", options(nostack));
    program(
        0,
        &[
            0x3001f000,
            0,
            0,
            probe_fault_entry as *const () as u32 | 1,
            probe_unexpected as *const () as u32 | 1,
            probe_unexpected as *const () as u32 | 1,
            probe_unexpected as *const () as u32 | 1,
            0,
        ],
    );
    program(0x40, &[probe_no_fault as *const () as u32 | 1]);
    write(0x52005004, if control & 2 != 0 { 6 << 16 } else { 5 << 16 });
    write(0x52005014, 0x0d002503);
    write(0xe000ed08, 0x90000000);
    asm!("dsb", "isb", options(nostack));
    if control & 1 != 0 {
        probe_warm(); // A real HardFault fetch fills the first vector line.
        write(0x52005000, 0);
    }
    record(22, read(0x52005000) & 1);
    record(15, probe_wait_return as *const () as u32 & !1);
    write(0xe000e100, 1);
    let sp: u32;
    asm!("mov {},sp",out(reg) sp,options(nostack));
    record(3, sp);
    record(0, 0x42);
    probe_wait();
    let sp: u32;
    asm!("mov {},sp",out(reg) sp,options(nostack));
    record(4, sp);
    record(16, read(0xe000e300));
    record(17, read(0xe000e100));
    record(0, 0xab);
    loop {
        asm!("wfi", options(nostack));
    }
}
#[no_mangle]
unsafe extern "C" fn probe_no_fault() -> ! {
    let exception: u32;
    asm!("mrs {},ipsr",out(reg) exception,options(nostack));
    record(5, exception);
    record(6, read(0xe000ed2c));
    record(7, read(0xe000ed28));
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
