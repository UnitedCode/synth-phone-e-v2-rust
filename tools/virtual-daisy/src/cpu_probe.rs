//! Build and grade the independent Rust ARM guest through public memory results.
use super::{grade, Result};
use std::{fs, path::Path};
pub fn prepare(directory: &Path) -> Result<()> {
    super::guest::prepare(directory, "cpu", include_str!("../guests/cpu.rs"))
}
fn equal(actual: u32, expected: u32, label: &str) -> Result<()> {
    if actual != expected {
        return Err(format!("ARM CPU {label}: expected {expected:#x}, got {actual:#x}").into());
    }
    Ok(())
}
pub fn check(prefix: &Path, shared_stack: bool, eager: bool) -> Result<()> {
    grade::execution_evidence(prefix)?;
    let report = grade::artifact(prefix, ".cpu.txt");
    if report.exists() {
        fs::remove_file(&report)?;
    }
    let data = fs::read(grade::artifact(prefix, ".cpu.bin"))?;
    if data.len() != 384 {
        return Err("Truncated ARM CPU capture".into());
    }
    let words: Vec<u32> = data
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&word| u32::from_le_bytes(word))
        .collect();
    for (phase, exception, status) in [
        (1, 6, 0x02000000),
        (2, 6, 0x01000000),
        (3, 4, 0x82),
        (4, 4, 1),
        (5, 6, 0x00080000),
    ] {
        let base = 4 + (phase - 1) * 4;
        equal(words[base], exception, &format!("phase {phase} exception"))?;
        equal(
            words[base + 1],
            status,
            &format!("phase {phase} fault status"),
        )?;
    }
    equal(words[14], 0x24040000, "MPU fault address")?;
    equal(words[19], 0x24040000, "execute-never fault PC")?;
    for phase in 7..=10 {
        let base = 24 + (phase - 7) * 4;
        equal(
            words[base],
            6,
            &format!("forced alignment phase {phase} exception"),
        )?;
        equal(
            words[base + 1],
            0x01000000,
            &format!("forced alignment phase {phase} fault status"),
        )?;
    }
    equal(
        words[60],
        0x55443322,
        "permitted unaligned Normal-memory load",
    )?;
    equal(words[61], 0x44332211, "faulting store preserved memory")?;
    equal(
        words[92],
        0x44332211,
        "faulting exclusive store preserved memory",
    )?;
    equal(words[93], 0x88776655, "word-aligned f64 load low")?;
    equal(words[94], 0xccbbaa99, "word-aligned f64 load high")?;
    equal(words[95], 0x88888888, "stalled LDRD with base/destination overlap")?;
    for phase in 11..=17 {
        let base = 64 + (phase - 11) * 4;
        equal(
            words[base],
            6,
            &format!("FP/Device alignment phase {phase} exception"),
        )?;
        equal(
            words[base + 1],
            0x01000000,
            &format!("FP/Device alignment phase {phase} fault status"),
        )?;
    }
    equal(words[1], 16, "fault count")?;
    equal(words[2], 0x1000, "MPU type")?;
    // Independent oracle: DDI0489B table8-2, single+double precision profile.
    equal(words[3], 0x10110221, "MVFR0")?;
    equal(words[58], 0x12000011, "MVFR1")?;
    equal(words[59], 0x00000040, "MVFR2")?;
    equal(
        words[3] & 0xfff,
        0x221,
        "single/double precision feature fields",
    )?;
    equal(words[54], 0, "f64 sum low")?;
    equal(words[55], 0x400e0000, "f64 sum high")?;
    equal(words[44], 0xffffffe9, "extended-frame EXC_RETURN")?;
    equal(words[45], u32::from(!eager), "lazy reservation")?;
    equal(words[46], 0, "lazy completion")?;
    equal(words[49], 0x3fc00000, "lazy saved S0")?;
    equal(words[42], 0x3fc00000, "restored S0")?;
    equal(words[43], 0x00400000, "restored FPSCR")?;
    equal(words[41], words[40], "restored SP")?;
    let stack_top = if shared_stack { 0x3001f000 } else { 0x2001f000 };
    if !(stack_top - 0x1000..stack_top).contains(&words[40]) {
        return Err("ARM CPU used the wrong stack memory".into());
    }
    if shared_stack {
        let stats = grade::artifact(prefix, ".memory.txt");
        if grade::counter(&stats, "exception_stack_words")? != if eager { 352 } else { 136 }
            || grade::counter(&stats, "exception_entry_wait_yields")? == 0
            || grade::counter(&stats, "exception_return_wait_yields")? == 0
        {
            return Err(
                "ARM SRAM stack did not use single frames and resumable bus service".into(),
            );
        }
    }
    equal(words[48], words[40] - 104, "extended frame SP")?;
    if !eager {
        equal(words[47], words[48] + 32, "FP frame address")?;
    }
    equal(words[62], 0, "unexpected floating-point fault")?;
    equal(words[63], 0, "unexpected handler")?;
    equal(words[0], 0xab, "completion")?;
    let sites = fs::read(grade::artifact(prefix, ".cpu-sites.bin"))?;
    if sites.len() != 60 {
        return Err("Truncated ARM fault instruction capture".into());
    }
    for (i, (&bytes, expected)) in sites
        .as_chunks::<4>()
        .0
        .iter()
        .zip([
            0xf0f2fbb1, 0x6808, 0x6808, 0x0a00eeb7, 0xc905, 0xc105, 0x0200e9d1, 0x0200e9c1,
            0x0a00edd1, 0x0a00ed81, 0x1a02ecb1, 0x0a02eca1, 0x6808, 0x0f00e851, 0x0200e841,
        ])
        .enumerate()
    {
        let actual = u32::from_le_bytes(bytes);
        let mask = if matches!(i, 1 | 2 | 4 | 5 | 12) {
            0xffff
        } else {
            u32::MAX
        };
        equal(
            actual & mask,
            expected,
            &format!("precise fault instruction {i}"),
        )?;
    }
    fs::write(report,format!("arm_divide_unaligned_mpu_nocp_faults=passed\nprecise_fault_instruction_and_mpu_address=passed\nmpu_regions=16\nf64_arithmetic_and_word_alignment=passed\nfpv5_d16_read_only_identification=passed\nfp_frame_and_return=passed\nfp_context_save={}\nstack_storage={}\nfp_bus_timing_and_silicon_pipeline=UNVERIFIED\n",if eager {"AUTOMATIC"} else {"LAZY"},if shared_stack {"D2_SRAM"} else {"DTCM"}))?;
    Ok(())
}
