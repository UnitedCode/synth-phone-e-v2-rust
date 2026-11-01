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
const MARKER:&str="\nuint32_t tlib_daisy_fetch_tag_version(void) { return 8; }\nEXC_INT_0(uint32_t, tlib_daisy_fetch_tag_version)\nvoid *daisy_fetch_state;\nuint32_t (*daisy_fetch_runtime)(void *, uint32_t, uint32_t);\nvoid tlib_daisy_set_runtime_fetch(void *state, void *callback) { daisy_fetch_state = state; daisy_fetch_runtime = callback; }\nEXC_VOID_2(tlib_daisy_set_runtime_fetch, void *, state, void *, callback)\nuint64_t tlib_daisy_memory_context(void) { target_ulong data[TARGET_INSN_START_WORDS]; if(cpu->v7m.exception_phase == V7M_EXCEPTION_PHASE_STACKING) { return UINT64_MAX-1; } if(cpu->v7m.exception_phase == V7M_EXCEPTION_PHASE_UNSTACKING) { return UINT64_MAX-2; } if(cpu->v7m.exception_phase == V7M_EXCEPTION_PHASE_VECTOR_READ) { return UINT64_MAX; } if(!cpu->current_tb || !cpu->mem_io_pc) { return 0; } if(cpu_get_data_for_pc(cpu, cpu->current_tb, cpu->mem_io_pc, true, data, false) < 0) { return 0; } return ((uint64_t)data[0] << 32) | cpu->regs[13]; }\nEXC_INT_0(uint64_t, tlib_daisy_memory_context)\n";
const RUNTIME:&str="    if(cpu->tb_restart_request == 0 && daisy_fetch_runtime) {\n        uint32_t result = daisy_fetch_runtime(daisy_fetch_state, cpu->current_tb->pc, cpu->current_tb->size);\n        if(result > 2) { tlib_abort(\"Invalid Daisy runtime instruction fetch\"); }\n        if(result == 1) { tb_phys_invalidate(cpu->current_tb, -1); cpu->tb_restart_request = 1; }\n        if(result == 2) { cpu->exit_request = 1; cpu->tb_restart_request = 1; }\n    }\n";
const BARRIER_EXPORT: &str = r#"
uint32_t (*daisy_barrier_callback)(void *, uint32_t);
void *daisy_barrier_state;
void tlib_daisy_set_barrier(void *state, void *callback) { daisy_barrier_state = state; daisy_barrier_callback = callback; }
EXC_VOID_2(tlib_daisy_set_barrier, void *, state, void *, callback)
"#;
const BARRIER_HELPER: &str = r#"
extern void *daisy_barrier_state;
extern uint32_t (*daisy_barrier_callback)(void *, uint32_t);
void HELPER(daisy_barrier)(CPUState *env, uint32_t kind) {
    if(daisy_barrier_callback) {
        uint32_t result = daisy_barrier_callback(daisy_barrier_state, kind);
        if(result > 1) { tlib_abort("Invalid Daisy memory barrier"); }
        if(result) {
            extern void *global_retaddr;
            global_retaddr = GETPC();
            interrupt_current_translation_block(env, EXCP_WATCHPOINT);
        }
    }
}
"#;

const EXCEPTION_EXPORT: &str = r#"
void *daisy_exception_state;
uint32_t (*daisy_exception_callback)(void *, uint32_t, uint32_t);
extern void daisy_reset_exception(void);
extern int daisy_vector_pending(void);
void tlib_daisy_set_exception(void *state, void *callback) {
    daisy_exception_state=state; daisy_exception_callback=callback; daisy_reset_exception();
}
EXC_VOID_2(tlib_daisy_set_exception, void *, state, void *, callback)
uint32_t tlib_daisy_irq_can_preempt(void) {
    return (cpu->interrupt_request & CPU_INTERRUPT_HARD) && cpu->regs[15] < ARM_M_FNC_RETURN_MIN
        && !daisy_vector_pending() && !cpu->v7m.locked_up;
}
EXC_INT_0(uint32_t, tlib_daisy_irq_can_preempt)
"#;
const EXCEPTION_DECL: &str = r#"
extern void *daisy_exception_state;
extern uint32_t (*daisy_exception_callback)(void *, uint32_t, uint32_t);
static struct { bool pending; uint32_t lr; int active; bool secure_target; bool lockup; } daisy_vector;
static uint32_t daisy_exception_bus(uint32_t address, uint32_t phase) {
    uint32_t result=daisy_exception_callback ? daisy_exception_callback(daisy_exception_state,address,phase):0;
    if(result>1) { tlib_abort("Invalid Daisy exception bus phase"); }
    return result;
}
"#;
const EXCEPTION_RESUME: &str = r#"
    bool lockup_after_exception_taken=false;
    if(daisy_vector.pending) {
        lr=daisy_vector.lr; active_exception=daisy_vector.active;
        secure_target=daisy_vector.secure_target; lockup_after_exception_taken=daisy_vector.lockup;
        daisy_vector.pending=false;
        goto daisy_vector_resume;
    }
"#;
const EXCEPTION_VECTOR: &str = r#"
        if(daisy_exception_bus(env->v7m.vecbase[env->secure]+env->v7m.exception*4,0)) {
            daisy_vector.pending=true; daisy_vector.lr=lr; daisy_vector.active=active_exception;
            daisy_vector.secure_target=secure_target; daisy_vector.lockup=lockup_after_exception_taken;
            env->v7m.exception_phase=V7M_EXCEPTION_PHASE_NONE;
            env->exception_index=EXCP_INTERRUPT;
            env->exit_request=1;
            cpu_loop_exit_without_hook(env);
        }
"#;
const EXCEPTION_RETURN: &str = r#"
    if(!env->v7m.has_trustzone && (type==0xfffffff1 || type==0xfffffff9 || type==0xfffffffd ||
                                 type==0xffffffe1 || type==0xffffffe9 || type==0xffffffed)) {
        uint32_t frame=(type&4) ? *pointer_psp(env):*pointer_msp(env);
        uint32_t bytes=(!(type&16) && !(env->v7m.fpccr[env->secure]&ARM_FPCCR_LSPACT_MASK)) ? 104:32;
        if(daisy_exception_bus(frame,0x100+bytes)) {
            env->exception_index=EXCP_INTERRUPT;
            env->exit_request=1;
            cpu_loop_exit_without_hook(env);
        }
    }
"#;
const EXCEPTION_WRAPPERS: &str = r#"
int daisy_vector_pending(void) { return daisy_vector.pending; }
void daisy_resume_vector(CPUState *env) { do_interrupt_v7m(env); }
void daisy_reset_exception(void) { daisy_vector.pending=false; }
"#;

fn pinned_replace(code: &mut String, needle: &str, replacement: &str) -> Result<()> {
    if code.matches(needle).count() != 1 {
        return Err(format!("Pinned native adapter point changed: {needle}").into());
    }
    *code = code.replace(needle, replacement);
    Ok(())
}

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
        Sha256::digest(format!("{INFRA}{TLIB}{}", include_str!("core.rs")))
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
        code.push_str(BARRIER_EXPORT);
        code.push_str(EXCEPTION_EXPORT);
        let needle = "void tlib_reset()\n{";
        pinned_replace(
            &mut code,
            needle,
            &format!(
                "extern void daisy_reset_exception(void);\n{needle}\n    daisy_reset_exception();"
            ),
        )?;
        fs::write(exports, code)?;
        let helper = tlib_dir.join("helper.c");
        let original = fs::read_to_string(&helper)?;
        let needle = "    return cpu->tb_restart_request;";
        if original.matches(needle).count() != 1 {
            return Err("Pinned runtime fetch hook changed".into());
        }
        fs::write(helper,format!("extern void *daisy_fetch_state;\nextern unsigned int (*daisy_fetch_runtime)(void *, unsigned int, unsigned int);\n{}",original.replace(needle,&format!("{RUNTIME}{needle}"))))?;
        let arm_helper = tlib_dir.join("arch/arm/helper.c");
        let mut code = fs::read_to_string(&arm_helper)?;
        let needle = "void do_v7m_exception_exit(CPUState *env)";
        pinned_replace(&mut code, needle, &format!("{EXCEPTION_DECL}\n{needle}"))?;
        pinned_replace(
            &mut code,
            "    bool lockup_after_exception_taken = false;",
            "    lockup_after_exception_taken = false;",
        )?;
        let needle = "    /* For exceptions we just mark as pending on the NVIC, and let that";
        pinned_replace(&mut code, needle, &format!("{EXCEPTION_RESUME}\n{needle}"))?;
        let needle="    while(true) {\n        env->v7m.exception_phase = V7M_EXCEPTION_PHASE_VECTOR_READ;";
        pinned_replace(
            &mut code,
            needle,
            &format!("daisy_vector_resume:\n{needle}\n{EXCEPTION_VECTOR}"),
        )?;
        let needle = "        int vector_fault = env->v7m.exception_phase_fault;";
        pinned_replace(
            &mut code,
            needle,
            &format!("        daisy_exception_bus(0,1);\n{needle}"),
        )?;
        let needle = "    type = env->regs[15];";
        pinned_replace(&mut code, needle, &format!("{needle}\n{EXCEPTION_RETURN}"))?;
        let needle = "    int unstacking_fault = env->v7m.exception_phase_fault;";
        pinned_replace(
            &mut code,
            needle,
            &format!("    daisy_exception_bus(0,2);\n{needle}"),
        )?;
        fs::write(
            arm_helper,
            format!("{code}\n{BARRIER_HELPER}\n{EXCEPTION_WRAPPERS}"),
        )?;
        let exec = tlib_dir.join("cpu-exec.c");
        let mut code = fs::read_to_string(&exec)?;
        let needle = "int cpu_exec(CPUState *env)";
        pinned_replace(&mut code,needle,&format!("extern int daisy_vector_pending(void);\nextern void daisy_resume_vector(CPUState *env);\n{needle}"))?;
        pinned_replace(
            &mut code,
            "    if(!cpu_has_work(env)) {",
            "    if(!cpu_has_work(env) && !daisy_vector_pending()) {",
        )?;
        let needle = "                interrupt_request = env->interrupt_request;";
        pinned_replace(&mut code,needle,&format!("                if(daisy_vector_pending()) {{ daisy_resume_vector(env); next_tb=0; }}\n{needle}"))?;
        fs::write(exec, code)?;
        let header = tlib_dir.join("arch/arm/helper.h");
        let code = fs::read_to_string(&header)?;
        let needle = "DEF_HELPER_1(clz, i32, i32)";
        if code.matches(needle).count() != 1 {
            return Err("Pinned barrier declaration changed".into());
        }
        fs::write(
            header,
            code.replace(
                needle,
                &format!("DEF_HELPER_2(daisy_barrier, void, env, i32)\n{needle}"),
            ),
        )?;
        let translate = tlib_dir.join("arch/arm/translate.c");
        let mut code = fs::read_to_string(&translate)?;
        for (needle,replacement) in [
            ("case 4: /* dsb */\n                case 5: /* dmb */", "case 4: /* dsb */\n                    gen_helper_daisy_barrier(cpu_env, tcg_const_i32(4));\n                    ARCH(7); gen_dxb(s); return;\n                case 5: /* dmb */\n                    gen_helper_daisy_barrier(cpu_env, tcg_const_i32(5));"),
            ("case 4: /* dsb */\n                                    case 5: /* dmb */", "case 4: /* dsb */\n                                        gen_helper_daisy_barrier(cpu_env, tcg_const_i32(4));\n                                        gen_dxb(s); break;\n                                    case 5: /* dmb */\n                                        gen_helper_daisy_barrier(cpu_env, tcg_const_i32(5));")
        ] {
            if code.matches(needle).count()!=1 {return Err("Pinned DSB/DMB decode changed".into());}
            code=code.replace(needle,replacement);
        }
        fs::write(translate, code)?;
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
    fs::write(directory.join("cpu-core.txt"),format!("infrastructure_commit={INFRA}\ntlib_commit={TLIB}\nfetch_tag_version=8\ncore_sha256={:x}\n",Sha256::digest(fs::read(core)?)))?;
    Ok(())
}
