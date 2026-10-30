//! A functional pass is never evidence of an unmeasured hardware property.
use super::Result;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
use virtual_daisy::board::Board;

pub const UNVERIFIED: &str = "Hardware equivalence is UNVERIFIED: there are no independent board captures, calibrated CPU/cache/bus timing, or validated analog/filter models. Functional audio tests cannot satisfy --require-hardware-match. See docs/daisy-conformance.md.";

pub fn write(directory: &Path, board: Board, firmware_hash: &str, library: &Path) -> Result<()> {
    let native_hash = format!("{:x}", Sha256::digest(fs::read(library)?));
    let platform_hash = format!(
        "{:x}",
        Sha256::digest(fs::read(directory.join("daisy.repl"))?)
    );
    let report = format!("schema=virtual-daisy-conformance-v1\nboard={}\ncodec={}\nfirmware_sha256={firmware_hash}\nnative_model_sha256={native_hash}\nplatform_sha256={platform_hash}\nhardware_equivalence=UNVERIFIED\nindependent_board_captures=0\nverified_scope=none\nfunctional_checks=boot,stereo-pcm,autotune,silence-fault,rx-dma-fault\nsource_checks=revision-straps,codec-reset-and-mute,signed-pcm-range,sai-wiring\ncpu_timing=UNCALIBRATED\ncache_and_bus_contention=UNMODELED\nanalog_and_codec_filter_response=UNMODELED\npll_and_sai_clock_coupling=UNMODELED\nqspi_storage_operations=INCOMPLETE\nreference_capture_provenance=ABSENT\n", board.id(), board.codec());
    fs::write(directory.join("conformance.txt"), report)?;
    Ok(())
}
