//! Real ARM bus-fault vectors for RM0433's documented QSPI error responses.
#![no_std]
#![no_main]
use core::{
    arch::{asm, global_asm},
    ptr::{read_volatile, write_volatile},
};
const RESULTS: *mut u32 = 0x20019000 as *mut u32;
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
    .word 0x2001f000, probe_main, probe_unexpected, probe_fault_entry
    .word probe_fault_entry, probe_fault_entry, probe_fault_entry, 0
    .word 0,0,0,probe_unexpected,probe_unexpected,0,probe_unexpected,probe_unexpected
    .rept 11
    .word probe_unexpected
    .endr
    .word probe_dma_irq
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
"#
);
extern "C" {
    fn probe_fetch(address: u32);
    fn probe_fetch_resume();
}
#[no_mangle]
unsafe extern "C" fn probe_fault(frame: *mut u32) {
    let phase = read_volatile(RESULTS);
    let count = read_volatile(RESULTS.add(1));
    if !(1..=8).contains(&phase) || count >= 16 {
        probe_unexpected();
    }
    record(1, count + 1);
    let base = 4 + (phase as usize - 1) * 4;
    let exception: u32;
    asm!("mrs {},ipsr",out(reg) exception,options(nostack));
    record(base, exception);
    record(base + 1, read(0xe000ed28));
    record(base + 2, read(0xe000ed38));
    record(base + 3, read_volatile(frame.add(6)));
    record(40 + phase as usize - 1, read(0xe000ed2c));
    let pc = read_volatile(frame.add(6));
    write_volatile(
        frame.add(6),
        if matches!(phase, 4 | 5) {
            probe_fetch_resume as *const () as u32 & !1
        } else {
            pc + 2
        },
    );
    write(0xe000ed28, u32::MAX);
    write(0xe000ed2c, read(0xe000ed2c) & 0x40000002);
}
#[no_mangle]
unsafe extern "C" fn probe_main() -> ! {
    for word in 0..64 {
        record(word, 0);
    }
    write(0xe000ed24, 7 << 16);
    write(0xe000ed14, 0x200);
    write(0x580244d4, 1 << 14);
    barriers();
    record(0, 1);
    asm!(".inst.n 0x6808",in("r1") 0x90000000_u32,out("r0") _);
    write(0x52005000, 1 | (3 << 24)); // HSI64/4 stays within normal-read frequency.
    write(0x52005004, 22 << 16);
    record(0, 2);
    asm!(".inst.n 0x6808",in("r1") 0x90000000_u32,out("r0") _);
    write(0x52005014, 0x0d002503);
    record(0, 3);
    asm!(".inst.n 0x6808",in("r1") 0x90800000_u32,out("r0") _);
    write(0x52005000, 0);
    barriers();
    record(0, 4);
    probe_fetch(0x90000001);
    write(0x52005000, 1 | (3 << 24));
    write(0x52005014, 0x0d002503);
    barriers();
    record(0, 5);
    probe_fetch(0x90800001);
    write(0x52005000, 0);
    write(0xe000ed24, 1 << 18); // Disabled BusFault must escalate to HardFault.
    barriers();
    record(0, 6);
    asm!(".inst.n 0x6808",in("r1") 0x90000000_u32,out("r0") _);
    write(0xe000ed24, 7 << 16);
    // Unbuffered register accesses keep posted-store timing outside this
    // precise-PC test. Background accesses remain privileged Normal memory.
    write(0xe000ed98, 0);
    write(0xe000ed9c, 0x52002000);
    write(0xe000eda0, 0x03000009);
    write(0xe000ed94, 5);
    barriers();
    record(0, 7);
    asm!(".inst.n 0x6008",in("r1") 0x52002004_u32,in("r0") 0x12345678_u32);
    record(36, read(0x5200200c) & 1);
    record(0, 8);
    asm!(".inst.n 0x6008",in("r1") 0x52002004_u32,in("r0") 0x45670123_u32);
    record(37, read(0x5200200c) & 1);
    write(0x580244d8, 1); // DMA1 clock.
    write(0x40020014, 1); // Stream0 NDTR.
    write(0x580244e0, 1 << 3); // GPIOD clock, physical board straps.
    let pins = (3 << 6) | (3 << 8) | (3 << 10);
    write(0x58020c00, read(0x58020c00) & !pins);
    write(
        0x58020c0c,
        (read(0x58020c0c) & !pins) | (1 << 6) | (1 << 8) | (1 << 10),
    );
    let tx = if read(0x58020c10) & (1 << 3) == 0 {
        0x40015840
    } else {
        0x40015820
    };
    record(52, tx);
    write(0x40020018, tx); // Board's transmit DR, one DREQ after the guest sleeps.
    write(0x4002001c, 0x90000000);
    write(0x40020010, 1 | 4 | 16 | 64 | (2 << 11) | (2 << 13));
    write(0xe000e100, 1 << 11); // DMA1 stream0 IRQ.
    record(0, 0xab);
    loop {
        asm!("wfi", options(nostack));
    }
}
#[no_mangle]
unsafe extern "C" fn probe_dma_irq() {
    record(48, read(0x40020000));
    record(49, read(0x40020010));
    record(50, read(0x40020014));
    record(51, read_volatile(RESULTS.add(51)) + 1);
    write(0x40020008, 0x3d);
}
#[no_mangle]
unsafe extern "C" fn probe_unexpected() -> ! {
    record(63, 0xdead);
    loop {
        asm!("wfi", options(nostack));
    }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    unsafe { probe_unexpected() }
}
