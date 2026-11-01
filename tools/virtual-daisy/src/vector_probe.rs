//! Grade demanded vector bus errors using PM0253 HFSR and frame semantics.
use super::{grade, Result};
use std::{fs, path::Path};
fn equal(actual: u32, expected: u32, label: &str) -> Result<()> {
    if actual != expected {
        return Err(format!("ARM vector {label}: expected {expected:#x}, got {actual:#x}").into());
    }
    Ok(())
}
pub fn check(prefix: &Path, cached: bool, enabled: bool) -> Result<()> {
    grade::execution_evidence(prefix)?;
    let report = grade::artifact(prefix, ".vector.txt");
    if report.exists() {
        fs::remove_file(&report)?;
    }
    let data = fs::read(grade::artifact(prefix, ".vector.bin"))?;
    if data.len() != 128 {
        return Err("Truncated ARM vector capture".into());
    }
    let words: Vec<u32> = data
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&b| u32::from_le_bytes(b))
        .collect();
    equal(words[6], 2, "HFSR.VECTTBL without FORCED")?;
    equal(words[5], 3, "HardFault exception")?;
    equal(words[7], 0, "CFSR without normal-data fault bits")?;
    equal(words[1], 1, "fault count")?;
    equal(words[0], 0xab, "completion")?;
    equal(words[31], 0, "unexpected exception")?;
    equal(words[2], u32::from(cached), "warm HardFault count")?;
    if !(0x3001e000..=0x3001f000).contains(&words[3]) {
        return Err("ARM vector used an invalid SRAM stack".into());
    }
    equal(words[4], words[3], "restored SP")?;
    equal(words[8], 0xfffffff9, "EXC_RETURN")?;
    equal(words[9], words[3] - 32, "reused frame SP")?;
    equal(words[10], words[15], "preempted wait return PC")?;
    equal(words[11] & 0x1ff, 0, "stacked thread IPSR")?;
    equal((words[11] >> 24) & 1, 1, "stacked Thumb state")?;
    equal(words[12] & 0x9ff, 0x803, "active HardFault and RETTOBASE")?;
    equal(words[13] & 1, 0, "original IRQ inactive")?;
    equal(words[14] & 1, 1, "original IRQ restored pending")?;
    equal(words[16] & 1, 0, "no active IRQ after return")?;
    equal(words[17] & 1, 0, "original IRQ disabled")?;
    equal(words[22], u32::from(!cached), "QSPI enable state")?;
    equal(
        words[23],
        if enabled { 7 << 16 } else { 0 },
        "configurable fault enables",
    )?;
    if cached {
        equal(words[18], 0x40000000, "warm forced HardFault")?;
        equal(words[19], 0x10000, "warm undefined-instruction cause")?;
        equal(words[20], 3, "warm HardFault exception")?;
        equal(words[21], 0xfffffff9, "warm EXC_RETURN")?;
    }
    let stats = grade::artifact(prefix, ".memory.txt");
    if grade::counter(&stats, "exception_stack_words")? != if cached { 16 } else { 8 }
        || grade::counter(&stats, "exception_entry_wait_yields")? == 0
    {
        return Err("ARM vector fault did not retain the original shared-SRAM frame".into());
    }
    fs::write(report, format!("vector_bus_error_hardfault_vecttbl_and_original_pc=passed\noriginal_frame_and_pending_irq_retained=passed\ncached_hardfault_vector_after_qspi_disable={}\nvector_bus_cycle_timing=UNVERIFIED\n", if cached {"passed"} else {"NOT_RUN"}))?;
    Ok(())
}
