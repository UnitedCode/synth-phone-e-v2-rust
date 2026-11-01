//! Host-platform packaging, separate from the emulated board/revision.
use std::{fs, path::Path};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub fn runtime_id(os: &str, arch: &str) -> Result<&'static str> {
    match (os, arch) {
        ("macos", "aarch64") => Ok("osx-arm64"),
        ("linux", "x86_64") => Ok("linux-x64"),
        ("windows", "x86_64") => Ok("win-x64"),
        ("linux", "aarch64") => Ok("linux-arm64"),
        ("macos", "x86_64") => Ok("osx-x64"),
        _ => Err("unsupported emulator host".into()),
    }
}
pub fn windows_path(value: &str) -> String {
    let value = value.replace('\\', "/");
    if let Some(unc) = value.strip_prefix("//?/UNC/") {
        format!("//{unc}")
    } else {
        value.strip_prefix("//?/").unwrap_or(&value).to_owned()
    }
}
pub fn tool_path(path: &Path) -> Result<String> {
    let value = path
        .to_str()
        .ok_or("Emulator paths must be valid Unicode")?;
    Ok(if cfg!(windows) {
        windows_path(value)
    } else {
        value.to_owned()
    })
}
pub fn repl_path(path: &Path) -> Result<String> {
    Ok(tool_path(path)?.replace('\\', "\\\\").replace('"', "\\\""))
}
fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
pub fn stage_runtime(source: &Path, destination: &Path, rid: &str, core: &Path) -> Result<()> {
    let original = source.join(rid);
    if !original.join("translate-arm-m-le.so").is_file() || !core.is_file() {
        return Err(format!(
            "Missing {rid} Renode runtime/core; install the matching native package"
        )
        .into());
    }
    if destination.symlink_metadata().is_ok() {
        if destination.is_symlink() {
            #[cfg(windows)]
            fs::remove_dir(destination)?;
            #[cfg(not(windows))]
            fs::remove_file(destination)?;
        } else {
            fs::remove_dir_all(destination)?;
        }
    }
    let private = destination.join(rid);
    copy_tree(&original, &private)?;
    fs::copy(core, private.join("translate-arm-m-le.so"))?;
    Ok(())
}
