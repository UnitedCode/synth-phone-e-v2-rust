//! Build the pinned Renode ARM-M core with a fetch/data transaction tag.
//! This is mandatory C API glue in the existing engine; cache behavior is Rust.
use super::{checked, Result};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};
const INFRA: &str = "066a7f13c052215632d469c995c89aea37c573b1";
const TLIB: &str = "167decf9129758829762e582939128c0694e90d6";
const INFRA_SHA: &str = "8353496d3489e3801a45fd79b9281040e9d6b00f0c1615a5e77e9959b44e5b85";
const TLIB_SHA: &str = "8db4d1fe0a316eec5dcae83bd4d2757b5172332493d6fdecfd9d4b81cb7a77a3";
const TAG: &str =
    "    cpustate |= (READ_ACCESS_TYPE == ACCESS_INST_FETCH) ? (UINT64_C(1) << 63) : 0;";
const MARKER:&str="\nuint32_t tlib_daisy_fetch_tag_version(void) { return 3; }\nEXC_INT_0(uint32_t, tlib_daisy_fetch_tag_version)\nvoid *daisy_fetch_state;\nuint32_t (*daisy_fetch_runtime)(void *, uint32_t, uint32_t);\nvoid tlib_daisy_set_runtime_fetch(void *state, void *callback) { daisy_fetch_state = state; daisy_fetch_runtime = callback; }\nEXC_VOID_2(tlib_daisy_set_runtime_fetch, void *, state, void *, callback)\n";
const RUNTIME:&str="    if(cpu->tb_restart_request == 0 && daisy_fetch_runtime) {\n        uint32_t result = daisy_fetch_runtime(daisy_fetch_state, cpu->current_tb->pc, cpu->current_tb->size);\n        if(result > 1) { tlib_abort(\"Invalid Daisy runtime instruction fetch\"); }\n        if(result == 1) { tb_phys_invalidate(cpu->current_tb, -1); cpu->tb_restart_request = 1; }\n    }\n";

fn archive(cache: &Path, name: &str, url: &str, sha: &str) -> Result<PathBuf> {
    let path = cache.join(name);
    if !path.exists() {
        checked(
            Command::new("curl")
                .args(["-fL", "--retry", "3", url, "-o"])
                .arg(&path),
        )?;
    }
    if format!("{:x}", Sha256::digest(fs::read(&path)?)) != sha {
        return Err(format!("Pinned CPU source hash mismatch: {}", path.display()).into());
    }
    Ok(path)
}

pub fn prepare(renode: &Path, directory: &Path, managed: &Path) -> Result<()> {
    let rid = match (env::consts::OS, env::consts::ARCH) {
        ("macos", "aarch64") => "osx-arm64",
        ("linux", "x86_64") => "linux-x64",
        _ => return Err("Fetch-tag core supports macOS ARM64 and Linux x86-64".into()),
    };
    let cache = env::var_os("VIRTUAL_DAISY_CORE_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| directory.join("cpu-core"));
    fs::create_dir_all(&cache)?;
    let infra = archive(
        &cache,
        "infrastructure.tar.gz",
        &format!("https://codeload.github.com/renode/renode-infrastructure/tar.gz/{INFRA}"),
        INFRA_SHA,
    )?;
    let tlib = archive(
        &cache,
        "tlib.tar.gz",
        &format!("https://codeload.github.com/antmicro/tlib/tar.gz/{TLIB}"),
        TLIB_SHA,
    )?;
    let source = cache.join("source");
    let source_stamp = format!(
        "{:x}",
        Sha256::digest(format!("{INFRA}{TLIB}{TAG}{MARKER}{RUNTIME}"))
    );
    if fs::read_to_string(source.join(".fetch-tag-v1"))
        .ok()
        .as_deref()
        != Some(&source_stamp)
    {
        if source.exists() {
            fs::remove_dir_all(&source)?;
        }
        fs::create_dir_all(&source)?;
        checked(
            Command::new("tar")
                .arg("-xzf")
                .arg(infra)
                .arg("-C")
                .arg(&source)
                .arg("--strip-components=1"),
        )?;
        let tlib_dir = source.join("src/Emulator/Cores/tlib");
        fs::create_dir_all(&tlib_dir)?;
        checked(
            Command::new("tar")
                .arg("-xzf")
                .arg(tlib)
                .arg("-C")
                .arg(&tlib_dir)
                .arg("--strip-components=1"),
        )?;
        let header = tlib_dir.join("include/softmmu_template.h");
        let original = fs::read_to_string(&header)?;
        let needle="    uint64_t cpustate = cpu_get_state_for_memory_transaction(env, physaddr, READ_ACCESS_TYPE);";
        if original.matches(needle).count() != 1 {
            return Err("Pinned CPU fetch hook changed".into());
        }
        fs::write(
            header,
            original.replace(needle, &format!("{needle}\n{TAG}")),
        )?;
        let exports = tlib_dir.join("exports.c");
        let mut code = fs::read_to_string(&exports)?;
        code.push_str(MARKER);
        fs::write(exports, code)?;
        let helper = tlib_dir.join("helper.c");
        let original = fs::read_to_string(&helper)?;
        let needle = "    return cpu->tb_restart_request;";
        if original.matches(needle).count() != 1 {
            return Err("Pinned runtime fetch hook changed".into());
        }
        fs::write(helper,format!("extern void *daisy_fetch_state;\nextern unsigned int (*daisy_fetch_runtime)(void *, unsigned int, unsigned int);\n{}",original.replace(needle,&format!("{RUNTIME}{needle}"))))?;
        fs::write(source.join(".fetch-tag-v1"), &source_stamp)?;
    }
    let build = cache.join("build");
    checked(
        Command::new("cmake")
            .arg("-S")
            .arg(source.join("src/Emulator/Cores"))
            .arg("-B")
            .arg(&build)
            .args(["-DTARGET_ARCH=arm-m", "-DCMAKE_BUILD_TYPE=Release"])
            .arg(format!("-DTLIB_COMMIT_SHA={TLIB}")),
    )?;
    checked(
        Command::new("cmake")
            .arg("--build")
            .arg(&build)
            .args(["-j", "4"]),
    )?;
    let core = build.join("tlib/translate-arm-m-le.so");
    let platform = managed.join("platform-lib");
    // Private override, never mutate the installed Renode runtime or a symlink target.
    if platform.symlink_metadata().is_ok() {
        if platform.is_symlink() {
            fs::remove_file(&platform)?;
        } else {
            fs::remove_dir_all(&platform)?;
        }
    }
    fs::create_dir_all(&platform)?;
    for entry in fs::read_dir(renode.join("platform-lib"))? {
        let entry = entry?;
        let destination = platform.join(entry.file_name());
        if entry.file_name() == rid {
            fs::create_dir_all(&destination)?;
            for file in fs::read_dir(entry.path())? {
                let file = file?;
                let target = destination.join(file.file_name());
                if file.file_name() == "translate-arm-m-le.so" {
                    fs::copy(&core, target)?;
                } else {
                    #[cfg(unix)]
                    std::os::unix::fs::symlink(file.path(), target)?;
                }
            }
        } else {
            #[cfg(unix)]
            std::os::unix::fs::symlink(entry.path(), destination)?;
        }
    }
    if !platform.join(rid).join("translate-arm-m-le.so").is_file() {
        return Err("Renode native platform directory missing".into());
    }
    fs::write(directory.join("cpu-core.txt"),format!("infrastructure_commit={INFRA}\ntlib_commit={TLIB}\nfetch_tag_version=3\ncore_sha256={:x}\n",Sha256::digest(fs::read(core)?)))?;
    Ok(())
}
