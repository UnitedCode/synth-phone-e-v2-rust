use std::{fs, path::PathBuf};
use virtual_daisy::host::{runtime_id, stage_runtime, windows_path};
#[path = "../src/platform.rs"]
mod platform;
#[test]
fn supported_native_hosts_select_the_matching_official_runtime() {
    for (os, arch, rid) in [
        ("windows", "x86_64", "win-x64"),
        ("linux", "x86_64", "linux-x64"),
        ("linux", "aarch64", "linux-arm64"),
        ("macos", "aarch64", "osx-arm64"),
        ("macos", "x86_64", "osx-x64"),
    ] {
        assert_eq!(runtime_id(os, arch).unwrap(), rid);
    }
    assert!(runtime_id("windows", "aarch64").is_err());
}
#[test]
fn windows_extended_drive_and_unc_paths_become_usable_repl_and_cmake_paths() {
    assert_eq!(
        windows_path(r"\\?\C:\My phone\café\virtual_daisy.dll"),
        "C:/My phone/café/virtual_daisy.dll"
    );
    assert_eq!(
        windows_path(r"\\?\UNC\server\phone files\virtual_daisy.dll"),
        "//server/phone files/virtual_daisy.dll"
    );
    assert_eq!(windows_path(r"C:\phone\model.dll"), "C:/phone/model.dll");
}
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "daisy native staging café {}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn private_native_runtime_is_copied_without_symlink_privileges_or_installed_mutations() {
    let temporary = Directory::new();
    let root = &temporary.0;
    let original = root.join("installed/win-x64");
    fs::create_dir_all(&original).unwrap();
    fs::write(original.join("translate-arm-m-le.so"), b"installed core").unwrap();
    fs::write(original.join("libhelper.dll"), b"native dependency").unwrap();
    let core = root.join("custom.so");
    fs::write(&core, b"tagged private core").unwrap();
    let private = root.join("output/platform-lib");
    fs::create_dir_all(&private).unwrap();
    fs::write(private.join("stale-library"), b"old").unwrap();
    stage_runtime(&root.join("installed"), &private, "win-x64", &core).unwrap();
    assert_eq!(
        fs::read(private.join("win-x64/translate-arm-m-le.so")).unwrap(),
        b"tagged private core"
    );
    assert_eq!(
        fs::read(private.join("win-x64/libhelper.dll")).unwrap(),
        b"native dependency"
    );
    assert_eq!(
        fs::read(original.join("translate-arm-m-le.so")).unwrap(),
        b"installed core"
    );
    assert!(!private.join("stale-library").exists());
    assert!(!private.join("win-x64/libhelper.dll").is_symlink());
    assert!(stage_runtime(
        &root.join("installed"),
        &root.join("absent"),
        "osx-arm64",
        &core
    )
    .is_err());
}

#[test]
fn windows_line_endings_do_not_silently_preserve_the_generic_h743_flash_mapping() {
    let temporary = Directory::new();
    let root = &temporary.0;
    fs::create_dir_all(root.join("runtime/platforms/cpus")).unwrap();
    let template="cpu: CPU.CortexM @ sysbus\n    cpuType: \"cortex-m7\"\nflashBank1: Memory.MappedMemory @ sysbus 0x08000000\n    size: 0x100000\npwr: Python.PythonPeripheral @ sysbus 0x58024800\n    size: 0x400\nusb1: USB.STM_USB @ sysbus 0x40040000\n    IRQ -> nvic@77\n";
    fs::write(
        root.join("runtime/platforms/cpus/stm32h7.repl"),
        template.replace('\n', "\r\n"),
    )
    .unwrap();
    platform::prepare(
        &root.join("runtime"),
        &root.join("capture"),
        &root.join("native/model.dll"),
        virtual_daisy::board::Board::Rev5,
    )
    .unwrap();
    let generated = fs::read_to_string(root.join("capture/daisy.repl")).unwrap();
    assert!(generated
        .contains("userFlash: Miscellaneous.DaisyMemory @ sysbus 0x08000000\n    size: 0x20000"));
    assert!(!generated.contains("flashBank1: Memory.MappedMemory"));
}
