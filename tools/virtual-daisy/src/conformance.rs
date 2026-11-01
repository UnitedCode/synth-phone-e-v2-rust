//! A functional pass is never evidence of an unmeasured hardware property.
use super::Result;
use sha2::{Digest, Sha256};
use std::{fmt::Write, fs, path::Path};
use virtual_daisy::board::Board;

pub const UNVERIFIED: &str = "Hardware equivalence is UNVERIFIED: there are no independent board captures, calibrated CPU/cache/bus timing, or validated analog/filter models. Functional audio tests cannot satisfy --require-hardware-match. See docs/daisy-conformance.md.";

pub fn write(
    directory: &Path,
    board: Board,
    firmware_hash: &str,
    library: &Path,
    ideal_line: bool,
    scope: &str,
) -> Result<()> {
    let mut report = String::new();
    for (key,value) in [
        ("schema","virtual-daisy-conformance-v7"),
        ("requested_scope",scope),
        ("host_os",std::env::consts::OS),
        ("host_arch",std::env::consts::ARCH),
        ("board",board.id()),
        ("codec",board.codec()),
        ("audio_input_boundary",if ideal_line {"ideal-line"} else {"serial"}),
        ("firmware_sha256",firmware_hash),
        ("hardware_equivalence","UNVERIFIED"),
        ("independent_board_captures","0"),
        ("verified_hardware_scope","none"),
        ("required_checks_scope","FULL_SUITE_ONLY_SEE_RUN_SUMMARY_FOR_ACTUAL_SCOPE"),
        ("required_functional_checks","boot,stereo-pcm,autotune,silence-fault,rx-dma-fault,cacheable-dma-fault,dtcm-dma-fault,bus-contention-fault,arm-generated-code,runtime-cache-fault,arm-flash-program-erase,flash-wait-bypass,arm-writeback-dsb,dsb-bypass,arm-exception-frame-return,entry-wait-bypass,arm-wait-irq-restart,wait-preemption-bypass,arm-qspi-storage,arm-qspi-mapped-execution,arm-qspi-timeout-irq,qspi-missing-irq,qspi-corrupt-data,qspi-stale-timer"),
        ("source_checks","revision-straps,codec-reset-and-mute,signed-pcm-range,sai-wiring,mpu-cache-policy,dcache-maintenance,icache-maintenance,dma-tcm-access,pll-fraction-latch,sai-dividers,dwt-cyccnt,sdram-timing-bounds,codec-nominal-delay,flash-read-queue,axi-qos-and-issuing,dma-priority-arbitration,flash-program-buffer-and-flags,live-flash-buffer,deferred-dirty-writeback,exception-vector-default-map,gpio-lock-analog-open-drain,qspi-nor-commands,qspi-dummy-clocks,qspi-clock-gates"),
        ("cpu_timing","UNCALIBRATED"),
        ("data_cache_coherence","FUNCTIONAL_MODEL"),
        ("cache_eviction_policy","DETERMINISTIC_ROUND_ROBIN_APPROXIMATION"),
        ("instruction_cache_coherence","FUNCTIONAL_INTERNAL_FLASH_QSPI_AND_SYSTEM_RAM_MODEL"),
        ("runtime_instruction_fetch","FUNCTIONAL_BLOCK_GRANULAR_MODEL"),
        ("cache_victim_sequence_and_speculation","UNVERIFIED"),
        ("arm_generated_code_probe","REQUIRED"),
        ("dma_single_beat_request_order","FUNCTIONAL_PRIORITY_ARBITRATION"),
        ("axi_admission_and_grants","SHARED_CPU_DMA_FABRIC"),
        ("flash_read_pipeline","LIVE_AND_STANDALONE_FUNCTIONAL_SERVICE_SCENARIO"),
        ("flash_program_erase","DEFERRED_FUNCTIONAL_MODEL"),
        ("flash_durations","DS12556_TYPICAL_OR_MAXIMUM_SCENARIO"),
        ("flash_ecc_reprogramming_options_crc","UNMODELED"),
        ("whole_system_bus_stalls","COUPLED_FUNCTIONAL_SERVICE_SCENARIO"),
        ("dma_service_grid_ns","1000"),
        ("cache_writeback_timing","DEFERRED_FABRIC_COMPLETION"),
        ("cache_writeback_buffer_depth_forwarding_merging","UNMODELED"),
        ("dsbdmb_scope","MODELED_RAM_WRITEBACKS_ONLY"),
        ("store_buffer_and_pipeline_overlap","UNMODELED"),
        ("exception_entry_and_return_bus_stalls","RESUMABLE_PORT_SERVICE_SCENARIO"),
        ("normal_load_wait_irq_preemption","FUNCTIONAL_RESTART_MODEL"),
        ("exception_late_arrival_tailchain_pipeline","INCOMPLETE"),
        ("documented_qspi_flash_bus_errors","FUNCTIONAL_IMMEDIATE_RESPONSE_GUEST_PROBES"),
        ("instruction_bus_fault_scope","DEMAND_QSPI_INSTRUCTION_BOUNDARIES"),
        ("qspi_translation_granularity","ONE_INSTRUCTION_TCG_BLOCK"),
        ("external_prefetch_only_and_posted_store_errors","UNSUPPORTED"),
        ("dma_qspi_reads_and_bus_errors","COUPLED_FUNCTIONAL_TC_TE_DISABLE_AND_IRQ"),
        ("dma_qspi_error_check","ADMISSION_AND_READ_RESPONSE_CONTROLLER_STATE"),
        ("qspi_abort_race_and_partial_burst_timing","UNVERIFIED"),
        ("arm_divide_alignment_mpu_nocp_faults","FUNCTIONAL_GUEST_PROBES_REQUIRED"),
        ("fpv5_d16_identification_and_f64","SOURCE_CHECKED_GUEST_PROBES_REQUIRED"),
        ("automatic_and_lazy_fp_frame_tcm_sram_and_return","FUNCTIONAL_RETRY_AND_PORT_SERVICE_SCENARIO"),
        ("fp_numerical_corner_cases_and_inflight_snapshot_reset","UNVERIFIED"),
        ("sdram_programmed_timing","OPEN_ROW_SERVICE_SCENARIO_AND_ANALYTICAL_BOUNDS"),
        ("sdram_row_refresh_scheduling","UNMODELED"),
        ("analog_and_codec_filter_response","INCOMPLETE"),
        ("wm8731_ideal_line_gain_and_hpf","SOURCE_CHECKED_NOMINAL_MODEL"),
        ("wm8731_passband_group_delay","NOMINAL_20_ADC_18_DAC_FRAMES"),
        ("complete_codec_filter_response","UNMODELED"),
        ("pll_and_sai_clock_coupling","RATIONAL_FUNCTIONAL_MODEL"),
        ("pll_lock_and_oscillator_tolerance","UNMODELED"),
        ("qspi_chip","IS25LP064A_REV_A_2015_11_06_3V3"),
        ("qspi_storage_operations","DEFERRED_NOR_PROGRAM_ERASE_PROTECTION_AND_RESET"),
        ("qspi_poll_and_timeout_irq","CLOCKED_SERIAL_SERVICE_SCENARIO"),
        ("qspi_mapped_reads","SHARED_NOR_M7_CACHES_AND_FABRIC_TARGET2"),
        ("qspi_mapped_prefetch","ONE_TRANSACTION_PER_REQUEST_32BYTE_TIMEOUT_FILL_SCENARIO"),
        ("qspi_indirect_shifting","IMMEDIATE_FUNCTIONAL_FIFO_NOT_SERIAL_EDGE_TIMING"),
        ("qspi_suspend","100US_SERVICE_SCENARIO_PROGRESS_CONTINUES_DURING_LATENCY"),
        ("qspi_function_register_write_duration","USES_STATUS_WRITE_SCENARIO_NOT_INDEPENDENTLY_SPECIFIED"),
        ("qspi_unimplemented","DDR,SIOO,SECURITY_UID_SFDP,DUAL_AND_QUAD_OUTPUT_SPI_COMMANDS"),
        ("reference_capture_provenance","ABSENT"),
    ] {writeln!(report,"{key}={value}")?;}
    for (key, path) in [
        ("native_model_sha256", library.to_owned()),
        ("platform_sha256", directory.join("daisy.repl")),
        ("cpu_core_manifest_sha256", directory.join("cpu-core.txt")),
        (
            "managed_host_sha256",
            directory.join("managed/DaisyHost.dll"),
        ),
        (
            "managed_engine_sha256",
            directory.join("managed/Infrastructure.dll"),
        ),
    ] {
        writeln!(report, "{key}={:x}", Sha256::digest(fs::read(path)?))?;
    }
    fs::write(directory.join("conformance.txt"), report)?;
    Ok(())
}
