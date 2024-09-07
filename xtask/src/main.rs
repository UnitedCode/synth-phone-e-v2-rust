#![allow(dead_code)]
#![deny(unused_must_use)]

use std::{env, path::PathBuf};
use xshell::{cmd, Shell};

fn main() -> Result<(), anyhow::Error> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let args = args.iter().map(|s| &**s).collect::<Vec<_>>();

    match &args[..] {
        ["embed"] => flash(),
        ["build"] => build(),
        ["test", "all"] => test_all(),
        ["test", "host"] => test_host(),
        ["test", "host-target"] => test_host_target(),
        ["test", "target"] => test_target(),
        _ => {
            println!("USAGE cargo xtask test [all|host|host-target|target] or cargo xtask embed");
            Ok(())
        }
    }
}

fn test_all() -> Result<(), anyhow::Error> {
    test_host()?;
    test_target()?;
    test_host_target()
}

fn test_host() -> Result<(), anyhow::Error> {
    let sh = Shell::new()?;
    let _p = sh.push_dir(root_dir());
    cmd!(sh, "cargo test --workspace --exclude host-target-tests").run()?;
    Ok(())
}

fn test_host_target() -> Result<(), anyhow::Error> {
    flash()?;

    let sh = Shell::new()?;
    let _p = sh.push_dir(root_dir());
    cmd!(sh, "cargo test -p host-target-tests").run()?;

    Ok(())
}

fn test_target() -> Result<(), anyhow::Error> {
    let sh = Shell::new()?;
    let _p = sh.push_dir(root_dir().join("cross"));
    cmd!(sh, "cargo test -p self-tests").run()?;
    Ok(())
}

fn flash() -> Result<(), anyhow::Error> {
    let sh = Shell::new()?;
    let _p = sh.push_dir(root_dir().join("cross"));
    cmd!(sh, "cargo embed --chip stm32h750v --release").run()?;
    Ok(())
}

fn build() -> Result<(), anyhow::Error> {
    let sh = Shell::new()?;
    let _p = sh.push_dir(root_dir().join("cross"));
    cmd!(sh, "cargo build --verbose --release --target thumbv7em-none-eabihf").run()?;
    Ok(())
}

fn root_dir() -> PathBuf {
    let mut xtask_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    xtask_dir.pop();
    xtask_dir
}
