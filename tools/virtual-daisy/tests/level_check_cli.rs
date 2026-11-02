use std::{fs, process::Command};

#[test]
fn incomplete_device_capture_cannot_retain_an_old_pass() {
    for analyzer in ["levels-analyze", "autotune-analyze"] {
        let directory =
            std::env::temp_dir().join(format!("daisy-{analyzer}-gate-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("result.txt"), "PASS: old capture").unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_virtual-daisy"))
            .arg(analyzer)
            .arg(&directory)
            .output()
            .unwrap();
        let success = output.status.success();
        let stale_pass = directory.join("result.txt").exists();
        fs::remove_dir_all(&directory).unwrap();
        assert!(!success, "incomplete captures must fail");
        assert!(
            !stale_pass,
            "old evidence must not masquerade as a new pass"
        );
    }
}
