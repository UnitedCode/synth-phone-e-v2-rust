//! PM0253 exception transitions: late arrival and tail-chaining reuse one frame.
#![no_std]
#![no_main]
use core::{
    arch::{asm, global_asm},
    ptr::{read_volatile, write_volatile},
};
const RESULTS: *mut u32 = 0x20019600 as *mut u32;
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
    .word probe_low,probe_high
    .section .text.probe_entries,"ax",%progbits
    .thumb_func
probe_low:
    movs r0,#16
    mov r1,lr
    mov r2,sp
    b probe_irq
    .thumb_func
probe_high:
    movs r0,#17
    mov r1,lr
    mov r2,sp
    ldr r3,=0x20019708
    str lr,[r3]
    bl probe_irq
    ldr r3,=0x20019700
    ldr r0,[r3]
    tst r0,#4
    beq probe_high_return
    ldr r3,=0x20019618
    movs r0,#2
    str r0,[r3]
    ldr r3,=0x20019704
probe_high_wait:
    ldr r0,[r3]
    cmp r0,#0
    beq probe_high_wait
probe_high_return:
    ldr r3,=0x20019708
    ldr lr,[r3]
    bx lr
"#
);
#[no_mangle]
unsafe extern "C" fn probe_irq(number: u32, link: u32, frame: u32) {
    let count = read_volatile(RESULTS.add(1));
    if count >= 2 {
        probe_unexpected();
    }
    let base = 8 + count as usize * 8;
    record(base, number);
    record(base + 1, link);
    record(base + 2, frame);
    let exception: u32;
    asm!("mrs {},ipsr", out(reg) exception, options(nostack));
    record(base + 3, exception);
    record(base + 4, read(0xe000ed04));
    record(base + 5, read(0xe000e300));
    record(base + 6, read(frame + 24));
    record(base + 7, read(frame + 28));
    if read(0x20019700) & 8 != 0 {
        asm!("vmov s0, {value}", "vmsr fpscr, {fpscr}",
            value=in(reg) if number==17 {0x40000000_u32} else {0x40400000_u32},
            fpscr=in(reg) 0x00800000_u32, out("s0") _);
    }
    record(1, count + 1);
    record(2, read_volatile(RESULTS.add(2)) | (1 << (number - 16)));
    if number == 16 && read(0x20019700) & 2 != 0 {
        let sp: u32;
        asm!("mov {},sp", out(reg) sp, options(nostack));
        record(7, sp);
        record(6, 1);
        while read_volatile(RESULTS.add(2)) & 2 == 0 {
            asm!("nop", options(nostack));
        }
    }
}
#[no_mangle]
unsafe extern "C" fn probe_main() -> ! {
    for word in 0..32 {
        record(word, 0);
    }
    write(0xe000ed14, 0x200);
    write(
        0xe000e400,
        if read(0x20019700) & 1 != 0 {
            0x00008080
        } else {
            0x00001080
        },
    );
    write(0xe000e100, 3);
    asm!("dsb", "isb", options(nostack));
    if read(0x20019700) & 8 != 0 {
        write(0xe000ed88, 0x00f00000);
        write(0xe000ef34, 0xc0000000);
        asm!("dsb", "isb", "vmov s0, {value}", "vmsr fpscr, {fpscr}",
            value=in(reg) 0x3fc00000_u32, fpscr=in(reg) 0x00400000_u32, out("s0") _);
    }
    let sp: u32;
    asm!("mov {},sp", out(reg) sp, options(nostack));
    record(3, sp);
    record(0, 0x42);
    loop {
        if read_volatile(RESULTS.add(2)) == 3 {
            let sp: u32;
            asm!("mov {},sp", out(reg) sp, options(nostack));
            record(4, sp);
            record(5, read(0xe000e300));
            if read(0x20019700) & 8 != 0 {
                let value: u32;
                let fpscr: u32;
                asm!("vmov {value}, s0", "vmrs {fpscr}, fpscr",value=out(reg) value,fpscr=out(reg) fpscr,options(nostack));
                record(24, value);
                record(25, fpscr);
                record(26, read(0xe000ef34));
            }
            record(0, 0xab);
        }
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
