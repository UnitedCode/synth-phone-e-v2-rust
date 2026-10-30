//! Run the unmodified production ELF on a functional virtual Daisy.
mod conformance;
mod core;
mod grade;
mod platform;
mod probe;
use sha2::{Digest, Sha256};
use std::{
    env,
    error::Error,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use virtual_daisy::board::Board;
type Result<T> = std::result::Result<T, Box<dyn Error>>;
const RATE: u32 = 48014;
#[derive(PartialEq)]
enum RunMode {
    Full,
    Probes,
    Timing,
}

fn checked(command: &mut Command) -> Result<()> {
    if !command.status()?.success() {
        return Err(format!("Command failed: {command:?}").into());
    }
    Ok(())
}

// A hung/crashed emulator cannot count as a successful negative control.
fn bounded(command: &mut Command, log: &Path) -> Result<()> {
    let output = File::create(log)?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(output)
        .spawn()?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                return Err(format!("Emulator failed ({status}); see {}", log.display()).into());
            }
            return Ok(());
        }
        if start.elapsed() > Duration::from_secs(180) {
            child.kill()?;
            child.wait()?;
            return Err(format!("Emulator timed out; see {}", log.display()).into());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn fixture(path: &Path, silent: bool) -> Result<()> {
    let samples = audio_lab::tone(224.492958, 1.1, &[(1, 0.2)]);
    let mut raw = File::create(path)?;
    for sample in samples {
        let bytes = if silent { 0.0_f32 } else { sample }.to_le_bytes();
        raw.write_all(&bytes)?;
        raw.write_all(&bytes)?;
    }
    Ok(())
}

fn wav_input(source: &Path, destination: &Path) -> Result<()> {
    let mut reader = hound::WavReader::open(source)?;
    let spec = reader.spec();
    if spec.sample_rate != RATE || ![1, 2].contains(&spec.channels) {
        return Err("Input must be mono/stereo WAV at 48014 Hz (no implicit resampling)".into());
    }
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<std::result::Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 2_f32.powi(spec.bits_per_sample as i32 - 1);
            reader
                .samples::<i32>()
                .map(|s| s.map(|s| s as f32 / scale))
                .collect::<std::result::Result<_, _>>()?
        }
    };
    if samples.len() / (spec.channels as usize) < RATE as usize
        || samples.iter().any(|s| !s.is_finite() || s.abs() > 1.0)
    {
        return Err("Input needs at least one second of finite audio within full scale".into());
    }
    let mut output = File::create(destination)?;
    for frame in samples
        .chunks_exact(spec.channels as usize)
        .take(RATE as usize + RATE as usize / 10)
    {
        output.write_all(&frame[0].to_le_bytes())?;
        output.write_all(&frame[usize::from(spec.channels == 2)].to_le_bytes())?;
    }
    Ok(())
}

struct Host<'a> {
    dotnet: &'a std::ffi::OsStr,
    managed: &'a Path,
    renode: &'a Path,
    directory: &'a Path,
    board: Board,
    ideal_line: bool,
}
impl Host<'_> {
    fn boot(&self, label: &str, input: &Path, fault: &str) -> Result<PathBuf> {
        let capture = self.directory.join(label);
        // A reused output directory must not provide a result from an old run.
        for suffix in [
            ".architecture.bin",
            ".architecture.txt",
            ".architecture-negative.txt",
            ".flash.bin",
            ".flash-probe.txt",
            ".flash-negative.txt",
            ".timing.bin",
            ".timing-early.bin",
            ".timing.txt",
            ".timing-negative.txt",
            ".interrupt.bin",
            ".interrupt-early.bin",
            ".interrupt.txt",
            ".interrupt-negative.txt",
        ] {
            let path = grade::artifact(&capture, suffix);
            if path.exists() {
                fs::remove_file(path)?;
            }
        }
        let mut command = Command::new(self.dotnet);
        command
            .arg(self.managed.join("DaisyHost.dll"))
            .arg(self.renode)
            .arg(self.directory.join("daisy.repl"))
            .arg(self.directory.join("firmware.elf"))
            .arg(&capture)
            .arg(input)
            .arg(fault)
            .arg(self.board.id())
            .arg(self.board.grounded_straps().to_string())
            .arg(if self.ideal_line || fault == "ideal-line" {
                "ideal-line"
            } else {
                "serial"
            })
            .arg(
                if env::var_os("VIRTUAL_DAISY_FLASH_MAXIMUM").is_some_and(|v| v == "1") {
                    "maximum"
                } else {
                    "typical"
                },
            )
            .env(
                "VIRTUAL_DAISY_INFRASTRUCTURE",
                self.managed.join("Infrastructure.dll"),
            );
        bounded(
            &mut command,
            &self.directory.join(format!("{label}.host.log")),
        )?;
        grade::execution_evidence(&capture)?;
        if ![
            "architecture",
            "runtime-fault",
            "flash",
            "flash-wait-fault",
            "timing",
            "dsb-fault",
            "entry-fault",
            "wait-irq",
            "wait-irq-fault",
            "wait-irq-target",
        ]
        .contains(&fault)
        {
            grade::boot_evidence(&capture)?;
        }
        Ok(capture)
    }
}

fn run(
    renode: &Path,
    directory: &Path,
    input_wav: Option<&Path>,
    board: Board,
    ideal_line: bool,
    mode: RunMode,
) -> Result<()> {
    if ideal_line && board != Board::Rev5 {
        return Err(
            "Ideal line input is only implemented for Rev5/WM8731; select serial for this board"
                .into(),
        );
    }
    fs::create_dir_all(directory)?;
    let directory = directory.canonicalize()?;
    // Reusing an output directory must not leave an old success report behind
    // if a new build, boot or quality gate fails.
    for name in ["summary.txt", "probe-summary.txt"] {
        let summary = directory.join(name);
        if summary.exists() {
            fs::remove_file(summary)?;
        }
    }
    let renode = renode.canonicalize()?;
    if !renode.join("Infrastructure.dll").is_file()
        || !renode.join("platforms/cpus/stm32h7.repl").is_file()
    {
        return Err(
            "Expected Renode 1.17.0 runtime directory (with Infrastructure.dll and platforms/)"
                .into(),
        );
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let dotnet = env::var_os("VIRTUAL_DAISY_DOTNET").unwrap_or_else(|| "dotnet".into());
    let host_info = Command::new("rustc").arg("-vV").output()?;
    let host_info = String::from_utf8(host_info.stdout)?;
    let host = host_info
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .ok_or("rustc host missing")?;
    let native_target = directory.join("native");
    checked(
        Command::new("cargo")
            .current_dir(&root)
            .args([
                "build",
                "--release",
                "--locked",
                "--lib",
                "--target",
                host,
                "--manifest-path",
            ])
            .arg(root.join("tools/virtual-daisy/Cargo.toml"))
            .env("CARGO_TARGET_DIR", &native_target)
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS"),
    )?;
    let library = native_target.join(host).join("release").join(format!(
        "{}virtual_daisy{}",
        env::consts::DLL_PREFIX,
        env::consts::DLL_SUFFIX
    ));
    let elf = if let Some(path) = env::var_os("VIRTUAL_DAISY_ELF") {
        PathBuf::from(path).canonicalize()?
    } else {
        let target = directory.join("firmware-build");
        checked(
            Command::new("cargo")
                .current_dir(&root)
                .args([
                    "build",
                    "--release",
                    "--locked",
                    "--target",
                    "thumbv7em-none-eabihf",
                ])
                .env("CARGO_TARGET_DIR", &target)
                .env_remove("RUSTFLAGS")
                .env_remove("CARGO_ENCODED_RUSTFLAGS"),
        )?;
        target.join("thumbv7em-none-eabihf/release/app")
    };
    let firmware = fs::read(&elf)?;
    if !firmware.starts_with(b"\x7fELF") {
        return Err("Firmware is not an ELF".into());
    }
    fs::write(directory.join("firmware.elf"), &firmware)?;
    let sha = format!("{:x}", Sha256::digest(&firmware));
    fs::write(
        directory.join("firmware.sha256"),
        format!("{sha}  firmware.elf\nsource={}\n", elf.display()),
    )?;
    platform::prepare(&renode, &directory, &library, board)?;
    let managed = directory.join("managed");
    fs::create_dir_all(&managed)?;
    checked(
        Command::new(&dotnet)
            .current_dir(&root)
            .args(["build"])
            .arg(root.join("tools/virtual-daisy/bridge/DaisyHost.csproj"))
            .arg(format!("-p:RenodeRoot={}", renode.display()))
            .arg("-p:UseSharedCompilation=false")
            .arg("--output")
            .arg(&managed)
            .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1"),
    )?;
    core::prepare(&renode, &directory, &managed)?;
    conformance::write(&directory, board, &sha, &library, ideal_line)?;
    bounded(
        Command::new(&dotnet)
            .arg(managed.join("DaisyHost.dll"))
            .arg(&renode)
            .arg("--disable-python")
            .arg(managed.join("Infrastructure.dll")),
        &directory.join("guard.log"),
    )?;
    let tone = directory.join("tone.f32le");
    let silence = directory.join("silence.f32le");
    fixture(&tone, false)?;
    fixture(&silence, true)?;
    let host = Host {
        dotnet: &dotnet,
        managed: &managed,
        renode: &renode,
        directory: &directory,
        board,
        ideal_line,
    };
    if mode != RunMode::Timing {
        fs::write(directory.join("architecture-probe.bin"), probe::image())?;
        let probes = host.boot("architecture-probes", &tone, "architecture")?;
        probe::check(&probes)?;
        let broken_runtime = host.boot("runtime-cache-fault", &tone, "runtime-fault")?;
        let error = probe::check(&broken_runtime)
            .err()
            .ok_or("Disabled runtime fetching was not caught")?
            .to_string();
        if !error.contains("ARM coherence result 10: expected 0x3, got 0x2") {
            return Err(format!("Unexpected runtime negative-control failure: {error}").into());
        }
        fs::write(
            grade::artifact(&broken_runtime, ".architecture-negative.txt"),
            format!("disabled_runtime_fetch=caught\n{error}\n"),
        )?;
        for counter in [
            "instruction_fetch_reads",
            "instruction_cache_fills",
            "runtime_retranslations",
        ] {
            if grade::counter(&grade::artifact(&probes, ".memory.txt"), counter)? == 0 {
                return Err(format!("ARM probe did not exercise {counter}").into());
            }
        }
        fs::write(directory.join("flash-probe.bin"), probe::flash_image())?;
        let flash = host.boot("flash-probes", &tone, "flash")?;
        probe::check_flash(&flash)?;
        let flash_stats = grade::artifact(&flash, ".memory.txt");
        for counter in ["flash_programs", "flash_erases"] {
            if grade::counter(&flash_stats, counter)? != 1 {
                return Err(
                    format!("ARM flash probe did not complete exactly one {counter}").into(),
                );
            }
        }
        let minimum_wait = if env::var_os("VIRTUAL_DAISY_FLASH_MAXIMUM").is_some_and(|v| v == "1") {
            2_000_000_000
        } else {
            1_000_000_000
        };
        if grade::counter(&flash_stats, "cpu_bus_wait_ns")? < minimum_wait {
            return Err("ARM flash probe did not consume the declared erase stall".into());
        }
        let broken_flash = host.boot("flash-wait-fault", &tone, "flash-wait-fault")?;
        let error = probe::check_flash(&broken_flash)
            .err()
            .ok_or("Bypassed flash bus waits were not caught")?
            .to_string();
        if !error.contains("ARM flash result 2: expected 0xffffffff, got 0x47702009") {
            return Err(format!("Unexpected flash wait negative-control failure: {error}").into());
        }
        fs::write(
            grade::artifact(&broken_flash, ".flash-negative.txt"),
            format!("flash_wait_bypass=caught\n{error}\n"),
        )?;
    }
    fs::write(directory.join("timing-probe.bin"), probe::timing_image())?;
    let timing = host.boot("timing-probes", &tone, "timing")?;
    probe::check_timing(&timing)?;
    let stats = grade::artifact(&timing, ".memory.txt");
    for counter in [
        "cache_line_writebacks",
        "writeback_drain_yields",
        "exception_entry_wait_yields",
        "exception_return_wait_yields",
    ] {
        if grade::counter(&stats, counter)? == 0 {
            return Err(format!("ARM timing probe did not exercise {counter}").into());
        }
    }
    if grade::counter(&stats, "exception_stack_words")? != 8 {
        return Err("ARM timing probe did not stack exactly one basic frame".into());
    }
    for (label, fault, expected) in [
        (
            "dsb-fault",
            "dsb-fault",
            "ARM writeback early marker: expected 0, got 0xab",
        ),
        (
            "entry-fault",
            "entry-fault",
            "ARM exception entry started too soon:",
        ),
    ] {
        let capture = host.boot(label, &tone, fault)?;
        let error = probe::check_timing(&capture)
            .err()
            .ok_or("Timing negative control was not caught")?
            .to_string();
        if !error.contains(expected) {
            return Err(format!("Unexpected timing negative-control failure: {error}").into());
        }
        fs::write(
            grade::artifact(&capture, ".timing-negative.txt"),
            format!("{fault}=caught\n{error}\n"),
        )?;
    }
    fs::write(
        directory.join("interrupt-probe.bin"),
        probe::interrupt_image(),
    )?;
    fs::write(
        directory.join("interrupt-target-probe.bin"),
        probe::interrupt_target_image(),
    )?;
    let interrupt = host.boot("wait-irq", &tone, "wait-irq")?;
    probe::check_interrupt(&interrupt)?;
    if grade::counter(
        &grade::artifact(&interrupt, ".memory.txt"),
        "abandoned_cpu_waits",
    )? == 0
    {
        return Err("ARM IRQ probe did not abandon a normal-memory wait".into());
    }
    let target_irq = host.boot("wait-irq-target", &tone, "wait-irq-target")?;
    probe::check_interrupt(&target_irq)?;
    let stats = grade::artifact(&target_irq, ".memory.txt");
    if grade::counter(&stats, "abandoned_bus_requests")? == 0
        || grade::counter(&stats, "exception_stack_words")? != 8
    {
        return Err("ARM IRQ probe did not abandon a bus request and stack one SRAM frame".into());
    }
    let broken = host.boot("wait-irq-fault", &tone, "wait-irq-fault")?;
    let error = probe::check_interrupt(&broken)
        .err()
        .ok_or("Non-preemptible wait was not caught")?
        .to_string();
    if !error.contains("ARM interrupt early marker: expected 0xab, got 0x0") {
        return Err(format!("Unexpected wait-preemption control failure: {error}").into());
    }
    fs::write(
        grade::artifact(&broken, ".interrupt-negative.txt"),
        format!("wait_preemption_bypass=caught\n{error}\n"),
    )?;
    if mode != RunMode::Full {
        if fs::read(&elf)? != firmware || fs::read(directory.join("firmware.elf"))? != firmware {
            return Err("Firmware changed during the probe run".into());
        }
        let summary = if mode == RunMode::Timing {
            "ARM writeback/exception timing probes passed; legacy cache/flash probes and production audio NOT RUN\n"
        } else {
            "ARM cache, flash and writeback/exception probes passed; production firmware/audio NOT RUN\n"
        };
        fs::write(directory.join("probe-summary.txt"), summary)?;
        print!("{summary}");
        return Ok(());
    }
    let baseline = host.boot("baseline", &tone, "none")?;
    let report = grade::check(&baseline)?;
    for counter in [
        "cpu_bus_wait_ns",
        "fabric_completed",
        "dma_bus_wait_yields",
        "flash_buffer_hits",
        "vector_reads",
        "vector_cache_hits",
    ] {
        if grade::counter(&grade::artifact(&baseline, ".memory.txt"), counter)? == 0 {
            return Err(format!("Production boot did not exercise {counter}").into());
        }
    }
    if grade::counter(&grade::artifact(&baseline, ".dma.txt"), "contested_grants")? == 0 {
        return Err("Production boot did not exercise simultaneous DMA arbitration".into());
    }
    if board == Board::Rev5 && !ideal_line {
        grade::check(&host.boot("codec-line", &tone, "ideal-line")?)?;
    }
    for (label, input, fault) in [
        ("silent-input", &silence, "none"),
        ("lost-rx-dma", &tone, "drop-rx"),
    ] {
        let capture = host.boot(label, input, fault)?;
        let failures = grade::failures(&capture)?;
        if !failures.iter().any(|failure| failure.contains("RMS")) {
            return Err(format!(
                "Negative control {label} did not fail the RMS criterion: {failures:?}"
            )
            .into());
        }
        if label == "lost-rx-dma" && grade::stat(&capture, "overruns")? == 0 {
            return Err("Lost RX requests did not produce FIFO overrun evidence".into());
        }
    }
    let cache_fault = host.boot("cacheable-dma", &tone, "cacheable-dma")?;
    if grade::failures(&cache_fault)?.is_empty() {
        return Err("Cacheable DMA fault did not corrupt the returned audio".into());
    }
    if grade::counter(
        &grade::artifact(&cache_fault, ".memory.txt"),
        "dma_cacheable_bytes",
    )? == 0
    {
        return Err("Cache fault did not reach cacheable DMA memory".into());
    }
    let bus_fault = host.boot("dtcm-dma", &tone, "dtcm-dma")?;
    if grade::failures(&bus_fault)?.is_empty() {
        return Err("DMA TCM bus fault did not fail the audio gates".into());
    }
    let bus_stats = grade::artifact(&bus_fault, ".bus.txt");
    if grade::counter(&bus_stats, "dma1_lisr")? & 8 == 0
        || grade::counter(&bus_stats, "dma1_s0cr")? & 1 != 0
    {
        return Err("DMA bus fault missing TEIF or EN-clear evidence".into());
    }
    let contention = host.boot("bus-contention", &tone, "bus-contention")?;
    if grade::failures(&contention)?.is_empty()
        || (grade::stat(&contention, "overruns")? == 0
            && grade::stat(&contention, "underruns")? == 0)
    {
        return Err("Competing bus traffic did not disrupt DMA/audio service".into());
    }
    if grade::counter(
        &grade::artifact(&contention, ".memory.txt"),
        "dma_bus_wait_yields",
    )? == 0
    {
        return Err("Bus contention did not delay DMA completion".into());
    }
    if let Some(source) = input_wav {
        let raw = directory.join("user.f32le");
        wav_input(source, &raw)?;
        host.boot("user", &raw, "none")?;
    }
    if fs::read(&elf)? != firmware || fs::read(directory.join("firmware.elf"))? != firmware {
        return Err("Firmware changed during the run".into());
    }
    let summary = format!("Production firmware ELF SHA256: {sha}\nBoard: {}\nAudio input boundary: {}\n{report}\nSilent-input negative control: caught by RMS gate\nLost RX DMA negative control: caught by RMS gate and FIFO overrun counter\nCacheable DMA negative control: caught by audio gates and cacheable DMA traffic counter\nDTCM DMA negative control: caught by audio gates, TEIF and cleared stream EN\nBus-contention negative control: caught by audio/FIFO gates and delayed DMA\nARM generated-code coherence: CPU/DMA, flash/RAM conflict and stale translation controls passed\nDisabled runtime-fetch negative control: caught by ARM result gate\nARM flash programming/erase: status, deferred completion, STM/STRD retries and stale I-cache checks passed\nFlash wait-bypass negative control: caught by CPU result gate\nARM dirty writeback/DSB: deferred shared-port completion and early-marker control passed\nARM exception entry/return: busy flash vector, single SRAM frame and wait-bypass control passed\nARM normal-load IRQ preemption: early handler, restarted load and blocked-preemption control passed\nCPU/DMA stalls: coupled functional service scenario; not cycle accurate\nSDRAM timing: open-row read/transfer service scenario; row/refresh scheduling unmodeled\nWM8731 ideal line stage: {}\nHardware equivalence: UNVERIFIED\nSilicon cache replacement/speculation, full store buffers, late arrival/tail-chain timing, pipeline overlap, calibrated CPU timing and complete codec response remain unfinished or unverified\n", board.description(),if ideal_line { "ideal-line" } else { "serial" },if board==Board::Rev5 { "gain/DC-removal and nominal 20/18-frame passband delay boot passed" } else { "not implemented for this codec" });
    fs::write(directory.join("summary.txt"), &summary)?;
    print!("{summary}");
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() == 1 && args[0] == "profiles" {
        for board in virtual_daisy::board::Board::ALL {
            println!("{}", board.description());
        }
        return Ok(());
    }
    if args.len() == 2 && args[0] == "grade" {
        println!("{}", grade::check(Path::new(&args[1]))?);
        return Ok(());
    }
    if args.len() >= 3 && (args[0] == "run" || args[0] == "probe" || args[0] == "timing") {
        let mut board = None;
        let mut codec = None;
        let mut input = None;
        let mut require_hardware = false;
        let mut ideal_line = false;
        let mut options = args[3..].iter();
        while let Some(option) = options.next() {
            if option == "--audio-boundary" {
                ideal_line = match options.next().and_then(|v| v.to_str()) {
                    Some("ideal-line") => true,
                    Some("serial") => false,
                    _ => return Err("--audio-boundary requires serial|ideal-line".into()),
                };
                continue;
            }
            if option == "--board" {
                if board.is_some() {
                    return Err("Duplicate --board".into());
                }
                board = Some(
                    options
                        .next()
                        .and_then(|v| v.to_str())
                        .ok_or("--board requires rev4/rev5/rev7")?
                        .parse::<Board>()?,
                );
            } else if option == "--codec" {
                if codec.is_some() {
                    return Err("Duplicate --codec".into());
                }
                codec = Some(
                    options
                        .next()
                        .and_then(|v| v.to_str())
                        .ok_or("--codec requires a name")?,
                );
            } else if option == "--require-hardware-match" {
                require_hardware = true;
            } else if option.to_string_lossy().starts_with('-') {
                return Err(format!("Unknown option {}", option.to_string_lossy()).into());
            } else {
                if input.is_some() {
                    return Err("Only one input WAV is allowed".into());
                }
                input = Some(Path::new(option));
            }
        }
        let board = board.ok_or(
            "Select the physical board with --board rev4/rev5/rev7; there is no implicit codec",
        )?;
        if let Some(codec) = codec {
            board.validate_codec(codec)?;
        }
        if require_hardware {
            return Err(conformance::UNVERIFIED.into());
        }
        return run(
            Path::new(&args[1]),
            Path::new(&args[2]),
            input,
            board,
            ideal_line,
            if args[0] == "timing" {
                RunMode::Timing
            } else if args[0] == "probe" {
                RunMode::Probes
            } else {
                RunMode::Full
            },
        );
    }
    Err("Usage: virtual-daisy run|probe|timing RENODE_RUNTIME OUTPUT_DIRECTORY --board rev4|rev5|rev7 [--codec NAME] [--audio-boundary serial|ideal-line] [INPUT.wav]\n       virtual-daisy profiles\n       virtual-daisy grade CAPTURE_PREFIX".into())
}
