//! Independent ARM guest and public-result grading for documented QSPI faults.
use super::{checked, grade, Result};
use std::{fs, path::Path, process::Command};
pub fn prepare(directory: &Path) -> Result<()> {
    let source = directory.join("bus-probe.rs");
    let linker = directory.join("bus-probe.ld");
    fs::write(&source, include_str!("../guests/bus.rs"))?;
    fs::write(&linker, include_str!("../guests/cpu.ld"))?;
    checked(
        Command::new("rustc")
            .args([
                "--edition=2021",
                "--target",
                "thumbv7em-none-eabihf",
                "-C",
                "target-cpu=cortex-m7",
                "-C",
                "opt-level=s",
                "-C",
                "panic=abort",
                "-C",
            ])
            .arg(format!(
                "link-arg=-T{}",
                virtual_daisy::host::tool_path(&linker)?
            ))
            .args(["--crate-name", "daisy_bus_probe"])
            .arg(source)
            .arg("-o")
            .arg(directory.join("bus-probe.elf")),
    )
}
fn equal(actual: u32, expected: u32, label: &str) -> Result<()> {
    if actual != expected {
        return Err(format!("ARM bus {label}: expected {expected:#x}, got {actual:#x}").into());
    }
    Ok(())
}
pub fn check(prefix: &Path, valid_dma: bool, tx_address: u32) -> Result<()> {
    grade::execution_evidence(prefix)?;
    let report = grade::artifact(prefix, ".fault.txt");
    if report.exists() {
        fs::remove_file(&report)?;
    }
    let data = fs::read(grade::artifact(prefix, ".fault.bin"))?;
    if data.len() != 256 {
        return Err("Truncated ARM bus fault capture".into());
    }
    let words: Vec<u32> = data
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&b| u32::from_le_bytes(b))
        .collect();
    for (phase, status, address) in [
        (1, 0x8200, 0x90000000),
        (2, 0x8200, 0x90000000),
        (3, 0x8200, 0x90800000),
        (4, 0x100, 0x90000000),
        (5, 0x100, 0x90800000),
        (6, 0x8200, 0x90000000),
        (7, 0x8200, 0x52002004),
        (8, 0x8200, 0x52002004),
    ] {
        let base = 4 + (phase - 1) * 4;
        equal(
            words[base],
            if phase == 6 { 3 } else { 5 },
            &format!("phase {phase} exception"),
        )?;
        equal(words[base + 1], status, &format!("phase {phase} CFSR"))?;
        equal(
            words[40 + phase - 1],
            if phase == 6 { 1 << 30 } else { 0 },
            &format!("phase {phase} HFSR"),
        )?;
        if matches!(phase, 4 | 5) {
            equal(
                words[base + 3],
                address,
                &format!("phase {phase} instruction PC"),
            )?;
        } else {
            equal(words[base + 2], address, &format!("phase {phase} BFAR"))?;
        }
    }
    equal(words[0], 0xab, "completion")?;
    equal(words[1], 8, "fault count")?;
    equal(words[36], 1, "flash locked after wrong key")?;
    equal(words[37], 1, "flash remains locked after correct key")?;
    equal(words[48], if valid_dma { 0x30 } else { 8 }, "DMA TCIF/TEIF")?;
    equal(words[49] & 1, 0, "DMA EN after response")?;
    equal(words[50], u32::from(!valid_dma), "DMA NDTR after response")?;
    equal(words[51], 1, "DMA IRQ count")?;
    equal(words[52], tx_address, "board-selected DMA transmit address")?;
    if grade::counter(&grade::artifact(prefix, ".dma.txt"), "transfers")? != u64::from(valid_dma) {
        return Err("ARM bus DMA transfer was committed incorrectly".into());
    }
    equal(words[63], 0, "unexpected handler")?;
    let sites = fs::read(grade::artifact(prefix, ".fault-sites.bin"))?;
    if sites.len() != 24 {
        return Err("Truncated ARM data fault PC capture".into());
    }
    for (index, &site) in sites.as_chunks::<4>().0.iter().enumerate() {
        equal(
            u32::from_le_bytes(site) & 0xffff,
            if index < 4 { 0x6808 } else { 0x6008 },
            "precise LDR/STR fault PC",
        )?;
    }
    fs::write(report,format!("qspi_disabled_nonmapped_and_fsize_data_busfault=passed\nqspi_disabled_and_fsize_block_entry_instruction_busfault=passed\nprecise_data_pc_bfar_and_ibuserr_no_bfarvalid=passed\ndisabled_busfault_hardfault_escalation=passed\nflash_key_bus_error_and_persistent_lock=passed\ndma_qspi_response={}\ndma_request_boundary=SINGLE_SYNTHETIC_DREQ\nmidblock_instruction_faults=UNSUPPORTED\nstore_buffer_imprecise_faults_and_board_timing=UNVERIFIED\n",if valid_dma {"COMPLETED_AND_IRQ"} else {"TEIF_DISABLE_NDTR_PRESERVED_AND_IRQ"}))?;
    Ok(())
}
