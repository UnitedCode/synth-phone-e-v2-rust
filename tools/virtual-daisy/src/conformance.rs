//! A functional pass is never evidence of an unmeasured hardware property.
use super::Result;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
use virtual_daisy::board::Board;

pub const UNVERIFIED: &str = "Hardware equivalence is UNVERIFIED: there are no independent board captures, calibrated CPU/cache/bus timing, or validated analog/filter models. Functional audio tests cannot satisfy --require-hardware-match. See docs/daisy-conformance.md.";

pub fn write(
    directory: &Path,
    board: Board,
    firmware_hash: &str,
    library: &Path,
    ideal_line: bool,
) -> Result<()> {
    let native_hash = format!("{:x}", Sha256::digest(fs::read(library)?));
    let platform_hash = format!(
        "{:x}",
        Sha256::digest(fs::read(directory.join("daisy.repl"))?)
    );
    let cpu_core_hash = format!(
        "{:x}",
        Sha256::digest(fs::read(directory.join("cpu-core.txt"))?)
    );
    let report = format!("schema=virtual-daisy-conformance-v4\nboard={}\ncodec={}\naudio_input_boundary={}\nfirmware_sha256={firmware_hash}\nnative_model_sha256={native_hash}\nplatform_sha256={platform_hash}\nhardware_equivalence=UNVERIFIED\nindependent_board_captures=0\nverified_hardware_scope=none\nfunctional_checks=boot,stereo-pcm,autotune,silence-fault,rx-dma-fault,cacheable-dma-fault,dtcm-dma-fault,arm-generated-code,runtime-cache-fault\nsource_checks=revision-straps,codec-reset-and-mute,signed-pcm-range,sai-wiring,mpu-cache-policy,dcache-maintenance,icache-maintenance,dma-tcm-access,pll-fraction-latch,sai-dividers,dwt-cyccnt,sdram-timing-bounds,codec-nominal-delay,flash-read-queue,axi-qos-and-issuing,dma-priority-arbitration\ncpu_timing=UNCALIBRATED\ndata_cache_coherence=FUNCTIONAL_MODEL\ncache_eviction_policy=DETERMINISTIC_ROUND_ROBIN_APPROXIMATION\ninstruction_cache_coherence=FUNCTIONAL_FLASH_AND_SYSTEM_RAM_MODEL\nruntime_instruction_fetch=FUNCTIONAL_BLOCK_GRANULAR_MODEL\ncache_victim_sequence_and_speculation=UNVERIFIED\narm_generated_code_probe=REQUIRED\ndma_single_beat_request_order=FUNCTIONAL_PRIORITY_ARBITRATION\naxi_admission_and_grants=STANDALONE_SOURCE_MODEL\nflash_command_queue=STANDALONE_LOWER_BOUND_MODEL\nwhole_system_bus_stalls=NOT_CONNECTED\nsdram_programmed_timing=ANALYTICAL_LOWER_BOUNDS\nanalog_and_codec_filter_response=INCOMPLETE\nwm8731_ideal_line_gain_and_hpf=SOURCE_CHECKED_NOMINAL_MODEL\nwm8731_passband_group_delay=NOMINAL_20_ADC_18_DAC_FRAMES\ncomplete_codec_filter_response=UNMODELED\npll_and_sai_clock_coupling=RATIONAL_FUNCTIONAL_MODEL\npll_lock_and_oscillator_tolerance=UNMODELED\nqspi_storage_operations=INCOMPLETE\nreference_capture_provenance=ABSENT\n", board.id(), board.codec(),if ideal_line { "ideal-line" } else { "serial" });
    fs::write(
        directory.join("conformance.txt"),
        format!("{report}cpu_core_manifest_sha256={cpu_core_hash}\n"),
    )?;
    Ok(())
}
