//! Grade processor-written results and guarded host execution evidence.
use super::{grade, Result};
use std::{fs, path::Path};
pub fn check(prefix: &Path) -> Result<()> {
    grade::execution_evidence(prefix)?;
    let data = fs::read(grade::artifact(prefix, ".sdram.bin"))?;
    if data.len() != 128 {
        return Err("Truncated ARM SDRAM result".into());
    }
    let words: Vec<_> = data
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&b| u32::from_le_bytes(b))
        .collect();
    for (word, expected, label) in [
        (0, 0xab, "completion"),
        (1, 1, "IRQ count"),
        (2, 1, "RE flag"),
        (3, 64, "IRQ48 exception number"),
        (4, 0, "CRE status"),
        (5, 82, "COUNT and write-only CRE"),
        (6, 0x12340000, "row0 data"),
        (7, 0x12340001, "row1 data"),
        (8, 0x12340002, "bank1 data"),
        (10, 0x2d0, "RCC FMC control reset"),
        (11, 0x0fffffff, "RCC FMC timing reset"),
        (12, 0x8200, "geometry BusFault CFSR"),
        (13, 0xc0400000, "geometry BFAR"),
        (14, 5, "geometry exception number"),
        (15, 1, "geometry fault count"),
        (31, 0, "unexpected exception"),
    ] {
        if words[word] != expected {
            return Err(format!(
                "ARM SDRAM {label}: expected {expected:#x}, got {:#x}",
                words[word]
            )
            .into());
        }
    }
    for counter in [
        "sdram_activations",
        "sdram_precharges",
        "sdram_refreshes",
        "sdram_fifo_hits",
    ] {
        if grade::counter(&grade::artifact(prefix, ".memory.txt"), counter)? == 0 {
            return Err(format!("ARM SDRAM did not exercise {counter}").into());
        }
    }
    fs::write(grade::artifact(prefix,".sdram-probe.txt"),"arm_sdram_rows_and_banks=passed\nrefresh_error_irq48_and_cre=passed\nphysical_command_overlap_and_retention=UNVERIFIED\n")?;
    Ok(())
}
