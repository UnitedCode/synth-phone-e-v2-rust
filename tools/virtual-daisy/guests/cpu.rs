//! Independent no-std guest for architectural faults and lazy floating-point frames.
#![no_std]
#![no_main]
use core::{
    arch::{asm, global_asm},
    ptr::{read_volatile, write_volatile},
};
const RESULTS: *mut u32 = 0x20018000 as *mut u32;
unsafe fn read(address: u32) -> u32 {
    read_volatile(address as *const u32)
}
unsafe fn write(address: u32, value: u32) {
    write_volatile(address as *mut u32, value);
}
#[inline(always)]
unsafe fn record(word: usize, value: u32) {
    write_volatile(RESULTS.add(word), value);
}
unsafe fn phase(value: u32) {
    record(0, value);
}
unsafe fn barriers() {
    asm!("dsb", "isb", options(nostack));
}

global_asm!(
    r#"
    .syntax unified
    .thumb
    .fpu fpv5-d16
    .section .vectors,"a",%progbits
    .word 0x2001f000, probe_main, probe_unexpected, probe_fault_entry
    .word probe_fault_entry, probe_fault_entry, probe_fault_entry, 0
    .word 0, 0, 0, probe_svc_entry
    .word probe_unexpected, 0, probe_unexpected, probe_unexpected
    .section .text.probe_entries,"ax",%progbits
    .global probe_fault_entry
    .thumb_func
probe_fault_entry:
    mov r1, lr
    tst lr, #4
    ite eq
    mrseq r0, msp
    mrsne r0, psp
    push {{r4, lr}}
    bl probe_fault
    pop {{r4, lr}}
    bx lr
    .global probe_svc_entry
    .thumb_func
probe_svc_entry:
    mov r1, lr
    mrs r0, msp
    push {{r4, lr}}
    bl probe_svc
    pop {{r4, lr}}
    bx lr
    .global probe_try_xn
    .thumb_func
probe_try_xn:
    push {{r4, lr}}
    ldr r0, =0x24040001
    blx r0
    .global probe_xn_resume
    .thumb_func
probe_xn_resume:
    pop {{r4, pc}}
"#
);
extern "C" {
    fn probe_try_xn();
    fn probe_xn_resume();
}

#[no_mangle]
unsafe extern "C" fn probe_fault(frame: *mut u32, _return_code: u32) {
    let current = read_volatile(RESULTS);
    let count = read_volatile(RESULTS.add(1));
    record(1, count + 1);
    if count >= 32 {
        probe_unexpected();
    }
    let exception: u32;
    asm!("mrs {}, ipsr",out(reg) exception,options(nostack));
    if (1..=5).contains(&current) || (7..=17).contains(&current) {
        let base = if current <= 5 {
            4 + (current as usize - 1) * 4
        } else if current <= 10 {
            24 + (current as usize - 7) * 4
        } else {
            64 + (current as usize - 11) * 4
        };
        if read_volatile(RESULTS.add(base)) == 0 {
            record(base, exception);
            record(base + 1, read(0xe000ed28));
            record(base + 2, read(0xe000ed34));
            record(base + 3, read_volatile(frame.add(6)));
        }
    } else {
        record(62, read(0xe000ed28));
    }
    let pc = read_volatile(frame.add(6));
    write_volatile(
        frame.add(6),
        if current == 4 {
            probe_xn_resume as *const () as u32 & !1
        } else {
            pc + if matches!(current, 2 | 3 | 7 | 8 | 15) {
                2
            } else {
                4
            }
        },
    );
    write(0xe000ed28, u32::MAX);
}
#[no_mangle]
unsafe extern "C" fn probe_svc(frame: *mut u32, return_code: u32) {
    record(44, return_code);
    record(45, read(0xe000ef34) & 1);
    record(47, read(0xe000ef38));
    record(48, frame as u32);
    if read(0x20018200) & 2 == 0 {
        asm!("vmov.f32 s0, #3.0",out("s0") _,options(nostack));
    }
    record(46, read(0xe000ef34) & 1);
    record(49, read_volatile(frame.add(8)));
}
#[no_mangle]
unsafe extern "C" fn probe_main() -> ! {
    for word in 0..96 {
        record(word, 0);
    }
    record(2, read(0xe000ed90));
    for address in [0xe000ef40, 0xe000ef44, 0xe000ef48] {
        write(address, u32::MAX);
    }
    record(3, read(0xe000ef40));
    write(0xe000ed24, 7 << 16); // Enable MemManage, BusFault and UsageFault.
    write(
        0xe000ed14,
        0x200 | 8 | if read(0x20018200) & 1 == 0 { 16 } else { 0 },
    );
    barriers();
    phase(1);
    asm!("movs r1, #1","movs r2, #0","udiv r0, r1, r2",out("r0") _,out("r1") _,out("r2") _);
    phase(2);
    asm!(".inst.n 0x6808",in("r1") 0x24040001_u32,out("r0") _); // Unaligned LDR.
    phase(3);
    write(0xe000ed98, 15);
    write(0xe000ed9c, 0x24040000);
    write(0xe000eda0, 9);
    write(0xe000ed94, 5);
    barriers();
    asm!(".inst.n 0x6808",in("r1") 0x24040000_u32,out("r0") _); // Denied data access.
    phase(4);
    write(0xe000eda0, (1 << 28) | (3 << 24) | (1 << 19) | 9);
    barriers();
    probe_try_xn();
    write(0xe000ed94, 0);
    barriers();
    phase(5);
    asm!("vmov.f32 s0, #1.0",out("s0") _,options(nostack)); // CPACR still disabled: NOCP.
    write(0xe000ed88, 0xf << 20);
    barriers();
    phase(6);
    record(58, read(0xe000ef44));
    record(59, read(0xe000ef48));
    record(52, 0);
    record(53, 0x3ff80000); // 1.5 f64.
    record(56, 0);
    record(57, 0x40020000); // 2.25 f64.
    asm!("vldr d0, [{a}]","vldr d1, [{b}]","vadd.f64 d0, d0, d1","vstr d0, [{sum}]",
        a=in(reg) RESULTS.add(52),b=in(reg) RESULTS.add(56),sum=in(reg) RESULTS.add(54),out("d0") _,out("d1") _);
    record(50, 0x3fc00000); // Single-precision state preserved across the SVC.
    write(
        0xe000ef34,
        if read(0x20018200) & 4 != 0 {
            1 << 31
        } else {
            3 << 30
        },
    );
    asm!("vldr s0, [{value}]","vmsr fpscr, {fpscr}",value=in(reg) RESULTS.add(50),fpscr=in(reg) 0x00400000_u32,out("s0") _);
    let sp: u32;
    asm!("mrs {}, msp",out(reg) sp,options(nostack));
    record(40, sp);
    asm!("svc #0");
    let sp: u32;
    asm!("mrs {}, msp",out(reg) sp,options(nostack));
    record(41, sp);
    asm!("vstr s0, [{value}]",value=in(reg) RESULTS.add(42));
    let fpscr: u32;
    asm!("vmrs {}, fpscr",out(reg) fpscr,options(nostack));
    record(43, fpscr);
    // CCR.UNALIGN_TRP=0 permits ordinary Normal-memory loads, but never
    // LDM/STM/LDRD/STRD at non-word-aligned addresses (PM0253).
    write(0xe000ed14, 0x200);
    barriers();
    write(0x20017000, 0x44332211);
    write(0x20017004, 0x88776655);
    write(0x20017008, 0xccbbaa99);
    let unaligned: u32;
    asm!("ldr {value}, [r1]",value=out(reg) unaligned,in("r1") 0x20017001_u32);
    record(60, unaligned);
    phase(7);
    asm!("ldm r1!, {{r0,r2}}",inout("r1") 0x20017001_u32=>_,out("r0") _,out("r2") _);
    phase(8);
    asm!("stm r1!, {{r0,r2}}",inout("r1") 0x20017001_u32=>_,in("r0") 0xaabbccdd_u32,in("r2") 0_u32);
    phase(9);
    asm!("ldrd r0,r2,[r1]",in("r1") 0x20017001_u32,out("r0") _,out("r2") _);
    phase(10);
    asm!("strd r0,r2,[r1]",in("r1") 0x20017001_u32,in("r0") 0xaabbccdd_u32,in("r2") 0_u32);
    phase(11);
    asm!("vldr s1,[r1]",in("r1") 0x20017001_u32,out("s1") _);
    phase(12);
    asm!("vstr s0,[r1]",in("r1") 0x20017001_u32);
    phase(13);
    asm!("vldm r1!,{{s2,s3}}",inout("r1") 0x20017001_u32=>_,out("s2") _,out("s3") _);
    phase(14);
    asm!("vstm r1!,{{s0,s1}}",inout("r1") 0x20017001_u32=>_);
    record(61, read(0x20017000));
    phase(15);
    write(0xe000ed98, 15);
    write(0xe000ed9c, 0x24040000);
    write(0xe000eda0, 0x03000009); // Strongly-ordered RAM, full access.
    write(0xe000ed94, 5);
    barriers();
    asm!(".inst.n 0x6808",in("r1") 0x24040001_u32,out("r0") _);
    write(0xe000ed94, 0);
    barriers();
    phase(16);
    asm!("ldrex r0,[r1]",in("r1") 0x20017001_u32,out("r0") _);
    phase(17);
    asm!("strex r2,r0,[r1]",in("r1") 0x20017001_u32,in("r0") 0xaabbccdd_u32,out("r2") _);
    record(92, read(0x20017000));
    // Doubleword FP transfers require word alignment, not 8-byte alignment.
    asm!("vldr d2,[r1]","vstr d2,[{value}]",in("r1") 0x20017004_u32,value=in(reg) RESULTS.add(93),out("d2") _);
    phase(0xab);
    loop {
        asm!("wfi", options(nostack));
    }
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
