//! Grade independent processor-written SAI MMIO and IRQ87 observations.
use super::{grade, Result};
use std::{fs, path::Path};
pub fn check(prefix: &Path, rx: u32) -> Result<()> {
    grade::execution_evidence(prefix)?;
    let data = fs::read(grade::artifact(prefix, ".sai.bin"))?;
    if data.len() != 128 {
        return Err("Truncated ARM SAI result".into());
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
        (2, 2, "MUTEDET"),
        (3, 103, "IRQ87 exception number"),
        (4, 0, "CMUTEDET"),
        (5, 0, "write-only CLRFR"),
        (6, 0x40, "ACR1 reset"),
        (7, 0x40, "BCR1 reset"),
        (8, 1, "TX quarter level"),
        (10, rx, "physical RX block"),
        (31, 0, "unexpected exception"),
    ] {
        if words[word] != expected {
            return Err(format!(
                "ARM SAI {label}: expected {expected:#x}, got {:#x}",
                words[word]
            )
            .into());
        }
    }
    fs::write(grade::artifact(prefix,".sai-probe.txt"),"arm_sai_reset_fifo_level_and_mute_irq87=passed\nbit_edge_and_mid_frame_timing=UNVERIFIED\n")?;
    Ok(())
}
