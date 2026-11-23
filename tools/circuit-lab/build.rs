fn main() {
    let files = [
        "Cargo.toml",
        "build.rs",
        "src/lib.rs",
        "src/main.rs",
        "src/netlist.rs",
        "src/profiles.rs",
        "src/fixture.rs",
        "src/improvements.rs",
        "src/schematics.rs",
        "src/carrier.rs",
        "src/spice.rs",
        "src/electrical.rs",
        "src/oled.rs",
        "src/device.rs",
    ];
    let mut hashes = Vec::new();
    for file in files {
        println!("cargo:rerun-if-changed={file}");
        let output = std::process::Command::new("shasum")
            .args(["-a", "256", file])
            .output()
            .expect("shasum is required to fingerprint the compiled model");
        assert!(output.status.success(), "cannot hash circuit model source");
        let output = String::from_utf8(output.stdout).unwrap();
        hashes.push(format!(
            "{}={}",
            file,
            output.split_whitespace().next().unwrap()
        ));
    }
    println!(
        "cargo:rustc-env=CIRCUIT_WORKBENCH_SOURCES={}",
        hashes.join(";")
    );
    println!("cargo:rerun-if-env-changed=NGSPICE_LIB_DIR");
    if std::env::var_os("CARGO_FEATURE_SPICE").is_some() {
        if let Some(path) = std::env::var_os("NGSPICE_LIB_DIR") {
            println!("cargo:rustc-link-search=native={}", path.to_string_lossy());
        } else if std::path::Path::new("/opt/homebrew/lib/libngspice.dylib").exists() {
            println!("cargo:rustc-link-search=native=/opt/homebrew/lib");
        }
    }
}
