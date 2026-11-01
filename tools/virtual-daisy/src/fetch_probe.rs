//! Grade precise, demand-time QSPI instruction faults after a valid prefix.
use super::{grade, Result};
use std::{fs, path::Path};
pub fn check(prefix: &Path) -> Result<()> {
    grade::execution_evidence(prefix)?;
    let report = grade::artifact(prefix, ".fetch.txt");
    if report.exists() {
        fs::remove_file(&report)?;
    }
    let data = fs::read(grade::artifact(prefix, ".fetch.bin"))?;
    if data.len() != 128 {
        return Err("Truncated ARM fetch capture".into());
    }
    let words: Vec<u32> = data
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&b| u32::from_le_bytes(b))
        .collect();
    for (phase, pc) in [(1, 0x90000040), (2, 0x9000007e), (3, 0x90000040)] {
        let base = 4 + (phase - 1) * 4;
        for (actual, expected, label) in [
            (words[base], 5, "exception"),
            (words[base + 1], 0x100, "CFSR"),
            (words[base + 2], pc, "PC"),
            (words[base + 3], 0, "HFSR"),
        ] {
            if actual != expected {
                return Err(format!(
                    "ARM fetch phase {phase} {label}: expected {expected:#x}, got {actual:#x}"
                )
                .into());
            }
        }
    }
    if words[17] != 0x5a {
        return Err(format!(
            "ARM fetch valid prefix marker: expected 0x5a, got {:#x}",
            words[17]
        )
        .into());
    }
    if words[18] != 0x5a || words[16] != 0x5a {
        return Err("ARM fetch performed the faulting store or lost cached execution".into());
    }
    if words[0] != 0xab || words[1] != 3 || words[31] != 0 {
        return Err("ARM fetch guest did not finish exactly three faults".into());
    }
    if words[19] != 0x2000f8c1 {
        return Err("ARM fetch straddling instruction is not STR.W r2,[r1]".into());
    }
    fs::write(report,"valid_qspi_prefix_executes_before_ibuserr=passed\nstraddling_thumb32_fetch_fault_pc_and_no_store=passed\ncached_qspi_prefix_executes_after_controller_disable=passed\nqspi_fetch_scope=DEMAND_INSTRUCTION_BOUNDARIES\ncycle_accuracy_and_prefetch_speculation=UNVERIFIED\n")?;
    Ok(())
}
