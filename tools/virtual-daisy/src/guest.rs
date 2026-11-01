//! Shared cross-platform compilation of standalone Rust ARM conformance guests.
use super::{checked, Result};
use std::{fs, path::Path, process::Command};
pub fn prepare(directory: &Path, name: &str, content: &str) -> Result<()> {
    let source = directory.join(format!("{name}-probe.rs"));
    let linker = directory.join(format!("{name}-probe.ld"));
    fs::write(&source, content)?;
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
            .arg("--crate-name")
            .arg(format!("daisy_{name}_probe"))
            .arg(source)
            .arg("-o")
            .arg(directory.join(format!("{name}-probe.elf"))),
    )
}
