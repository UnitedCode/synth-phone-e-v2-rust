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
const MARKER:&str="\nuint32_t tlib_daisy_fetch_tag_version(void) { return 11; }\nEXC_INT_0(uint32_t, tlib_daisy_fetch_tag_version)\nvoid *daisy_fetch_state;\nuint32_t (*daisy_fetch_runtime)(void *, uint32_t, uint32_t);\nvoid tlib_daisy_set_runtime_fetch(void *state, void *callback) { daisy_fetch_state = state; daisy_fetch_runtime = callback; }\nEXC_VOID_2(tlib_daisy_set_runtime_fetch, void *, state, void *, callback)\nuint64_t tlib_daisy_memory_context(void) { target_ulong data[TARGET_INSN_START_WORDS]; if(cpu->v7m.exception_phase == V7M_EXCEPTION_PHASE_STACKING) { return UINT64_MAX-1; } if(cpu->v7m.exception_phase == V7M_EXCEPTION_PHASE_UNSTACKING) { return UINT64_MAX-2; } if(cpu->v7m.exception_phase == V7M_EXCEPTION_PHASE_VECTOR_READ) { return UINT64_MAX; } if(!cpu->current_tb || !cpu->mem_io_pc) { return 0; } if(cpu_get_data_for_pc(cpu, cpu->current_tb, cpu->mem_io_pc, true, data, false) < 0) { return 0; } return ((uint64_t)data[0] << 32) | cpu->regs[13]; }\nEXC_INT_0(uint64_t, tlib_daisy_memory_context)\n";
const RUNTIME: &str = r#"
    if(cpu->tb_restart_request == 0 && cpu->current_tb->size != 0 && daisy_fetch_runtime) {
        uint32_t result = daisy_fetch_runtime(daisy_fetch_state, cpu->current_tb->pc, cpu->current_tb->size);
        if(result == 3 || result > 4) { tlib_abort("Invalid Daisy runtime instruction fetch"); }
        if(result == 1) { tb_phys_invalidate(cpu->current_tb, -1); cpu->tb_restart_request = 1; }
        if(result == 2) { cpu->exit_request = 1; cpu->tb_restart_request = 1; }
        if(result == 4) {
            cpu->v7m.bus_fault_status |= 1U << BUS_FAULT_OFFSET; /* IBUSERR, no BFAR update */
            cpu->regs[15]=cpu->current_tb->pc;
            cpu->exception_index=EXCP_BUS_FAULT;
            cpu_loop_exit(cpu);
        }
    }
"#;
const BARRIER_EXPORT: &str = r#"
uint32_t (*daisy_barrier_callback)(void *, uint32_t);
void *daisy_barrier_state;
void tlib_daisy_set_barrier(void *state, void *callback) { daisy_barrier_state = state; daisy_barrier_callback = callback; }
EXC_VOID_2(tlib_daisy_set_barrier, void *, state, void *, callback)
"#;
const ALIGNMENT_EXPORT: &str = r#"
void *daisy_alignment_state;
uint32_t (*daisy_alignment_callback)(void *, uint32_t, uint32_t, uint32_t);
void tlib_daisy_set_alignment(void *state, void *callback) { daisy_alignment_state=state; daisy_alignment_callback=callback; }
EXC_VOID_2(tlib_daisy_set_alignment, void *, state, void *, callback)
"#;
const ALIGNMENT_HELPER: &str = r#"
extern void *daisy_alignment_state;
extern uint32_t (*daisy_alignment_callback)(void *, uint32_t, uint32_t, uint32_t);
void daisy_check_alignment(CPUState *env, uint32_t address, uint32_t alignment, uint32_t forced, void *retaddr) {
    if(!(address & (alignment-1)) || !daisy_alignment_callback) { return; }
    uint32_t result=daisy_alignment_callback(daisy_alignment_state,address,alignment,forced);
    if(result>1) { tlib_abort("Invalid Daisy alignment check"); }
    if(result) {
        extern void *global_retaddr;
        global_retaddr=retaddr;
        interrupt_current_translation_block_from_current_instruction(env,EXCP_UNALIGNED);
    }
}
void HELPER(daisy_alignment)(CPUState *env, uint32_t address, uint32_t alignment, uint32_t forced) {
    daisy_check_alignment(env,address,alignment,forced,GETPC());
}
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
    let rid = virtual_daisy::host::runtime_id(env::consts::OS, env::consts::ARCH)?;
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
        let mut original = fs::read_to_string(&header)?;
        let io_pc = "    cpu->mem_io_pc = (uintptr_t)retaddr;";
        if original.matches(io_pc).count() != 2 {
            return Err("Pinned read/write callback entries changed".into());
        }
        original = original.replace(io_pc, &format!("    global_retaddr = retaddr;\n{io_pc}"));
        let declaration = "uint32_t local_ntohl(uint32_t n);";
        pinned_replace(&mut original, declaration, &format!("{declaration}\n#ifdef TARGET_PROTO_ARM_M\nextern void daisy_check_alignment(CPUState *, uint32_t, uint32_t, uint32_t, void *);\n#endif"))?;
        let load = "    bool is_insn_fetch = (env->current_tb == NULL);";
        pinned_replace(&mut original, load, &format!("{load}\n#if defined(TARGET_PROTO_ARM_M) && SHIFT > 0 && !defined(SOFTMMU_CODE_ACCESS)\n    daisy_check_alignment(env,addr,SHIFT > 2 ? 4 : DATA_SIZE,SHIFT > 2,retaddr);\n#endif"))?;
        let store = "    target_phys_addr_t ioaddr;\n    target_ulong tlb_addr;\n    int index;\n    uintptr_t addend;";
        pinned_replace(&mut original, store, &format!("{store}\n#if defined(TARGET_PROTO_ARM_M) && SHIFT > 0\n    daisy_check_alignment(env,addr,SHIFT > 2 ? 4 : DATA_SIZE,SHIFT > 2,retaddr);\n#endif"))?;
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
        code.push_str(ALIGNMENT_EXPORT);
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
        // Physical lazy-FP stores originate in a TCG helper rather than the
        // softmmu accessors. Preserve that helper's unwind PC so the existing
        // Rust instruction replay can resume without repeating completed stores.
        for name in ["fp_lsp", "fp_lsp_no_context"] {
            let entry = format!("void HELPER({name})(CPUState *env)\n{{");
            pinned_replace(&mut code,&entry,&format!("{entry}\n    extern void *global_retaddr;\n    global_retaddr=GETPC();\n    env->mem_io_pc=(uintptr_t)global_retaddr;"))?;
        }
        let div = "        case EXCP_DIV_0:";
        pinned_replace(&mut code, div, &format!("        case EXCP_UNALIGNED:\n            env->v7m.fault_status[env->secure] |= USAGE_FAULT_UNALIGNED;\n            v7m_raise_synchronous_exception(env, v7m_exception_number_with_security(env, ARMV7M_EXCP_USAGE, env->secure));\n            return;\n{div}"))?;
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
            format!("{code}\n{BARRIER_HELPER}\n{ALIGNMENT_HELPER}\n{EXCEPTION_WRAPPERS}"),
        )?;
        let cpu_header = tlib_dir.join("arch/arm/cpu.h");
        let mut code = fs::read_to_string(&cpu_header)?;
        pinned_replace(
            &mut code,
            "#define EXCP_DIV_0",
            "#define EXCP_UNALIGNED      22 /* Data alignment UsageFault */\n#define EXCP_DIV_0",
        )?;
        fs::write(cpu_header, code)?;
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
                &format!("DEF_HELPER_4(daisy_alignment, void, env, i32, i32, i32)\nDEF_HELPER_2(daisy_barrier, void, env, i32)\n{needle}"),
            ),
        )?;
        let translate = tlib_dir.join("arch/arm/translate.c");
        let mut code = fs::read_to_string(&translate)?;
        let include = "#include \"cpu.h\"";
        pinned_replace(&mut code,include,&format!("{include}\nextern void *daisy_fetch_state;\nextern uint32_t (*daisy_fetch_runtime)(void *, uint32_t, uint32_t);"))?;
        let page_check =
            "    if((base->pc - (base->tb->pc & TARGET_PAGE_MASK)) >= TARGET_PAGE_SIZE) {";
        pinned_replace(&mut code,page_check,&format!("    if(base->tb->icount==1 && daisy_fetch_runtime) {{\n        uint32_t policy=daisy_fetch_runtime(daisy_fetch_state,base->tb->pc,0);\n        if(policy!=0 && policy!=5) {{ tlib_abort(\"Invalid Daisy translation policy\"); }}\n        if(policy==5) {{ return 0; }}\n    }}\n{page_check}"))?;
        for entry in [
            "static void gen_load_exclusive(DisasContext *s, int rt, int rt2, TCGv addr, int size)\n{",
            "static void gen_store_exclusive(DisasContext *s, int rd, int rt, int rt2, TCGv addr, int size)\n{",
        ] {
            pinned_replace(&mut code,entry,&format!("{entry}\n#ifdef TARGET_PROTO_ARM_M\n    gen_helper_daisy_alignment(cpu_env,addr,tcg_const_i32(size>2 ? 4:(1<<size)),tcg_const_i32(1));\n#endif"))?;
        }
        for name in ["ld", "st"] {
            let entry=format!("static inline void gen_vfp_{name}(DisasContext *s, enum arm_fp_precision precision, TCGv addr, int reg)\n{{");
            pinned_replace(&mut code,&entry,&format!("{entry}\n#ifdef TARGET_PROTO_ARM_M\n    gen_helper_daisy_alignment(cpu_env,addr,tcg_const_i32(precision==HALF_PRECISION ? 2:4),tcg_const_i32(1));\n#endif"))?;
        }
        // Existing decoders own instruction semantics; only insert the mandatory
        // word-alignment check before their first memory transfer.
        for needle in [
            "                    if(insn & (1 << 20)) {\n                        /* ldrd */",
            "                    /* Load/store multiple.  */\n                    addr = load_reg(s, rn);",
            "            rn = (insn >> 8) & 0x7;\n            addr = load_reg(s, rn);\n            for(i = 0; i < 8; i++) {",
        ] {
            let replacement=if needle.contains("/* ldrd */") {
                format!("                    gen_helper_daisy_alignment(cpu_env,addr,tcg_const_i32(4),tcg_const_i32(1));\n{needle}")
            } else {
                needle.replace("addr = load_reg(s, rn);","addr = load_reg(s, rn);\n            gen_helper_daisy_alignment(cpu_env,addr,tcg_const_i32(4),tcg_const_i32(1));")
            };
            pinned_replace(&mut code,needle,&replacement)?;
        }
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
    let build = cache.join("build").join(rid);
    let mut configure = Command::new("cmake");
    configure
        .arg("-S")
        .arg(virtual_daisy::host::tool_path(
            &source.join("src/Emulator/Cores"),
        )?)
        .arg("-B")
        .arg(virtual_daisy::host::tool_path(&build)?)
        .args(["-DTARGET_ARCH=arm-m", "-DCMAKE_BUILD_TYPE=Release"])
        .arg(format!("-DTLIB_COMMIT_SHA={TLIB}"));
    if cfg!(windows) {
        configure.args(["-G", "Ninja", "-DCMAKE_C_COMPILER=gcc"]);
    }
    checked(&mut configure)?;
    checked(
        Command::new("cmake")
            .arg("--build")
            .arg(&build)
            .args(["-j", "4"]),
    )?;
    let core = build.join("tlib/translate-arm-m-le.so");
    virtual_daisy::host::stage_runtime(
        &renode.join("platform-lib"),
        &managed.join("platform-lib"),
        rid,
        &core,
    )?;
    fs::write(directory.join("cpu-core.txt"),format!("infrastructure_commit={INFRA}\ntlib_commit={TLIB}\nfetch_tag_version=11\ncore_sha256={:x}\n",Sha256::digest(fs::read(core)?)))?;
    Ok(())
}
