use audio_lab::{
    constants::*,
    emulator::{cases, decode, sample_error},
};

fn capture() -> Vec<u8> {
    let total = (cases::INPUT_SAMPLES + 2 * FFT_SIZE).div_ceil(BLOCK_SIZE) * BLOCK_SIZE;
    let mut bytes = cases::MAGIC.to_vec();
    bytes.extend_from_slice(&(cases::COUNT as u32).to_le_bytes());
    for case in 0..cases::COUNT {
        bytes.extend_from_slice(&(case as u32).to_le_bytes());
        bytes.extend_from_slice(&(total as u32).to_le_bytes());
        bytes.resize(bytes.len() + total * 4, 0);
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes
}

#[test]
fn incomplete_guest_output_cannot_pass_as_silence() {
    let mut bytes = capture();
    let decoded = decode(&bytes).unwrap();
    assert_eq!(decoded.audio.len(), cases::COUNT);
    assert_eq!(decoded.interrupt_count, 1);
    bytes.truncate(bytes.len() - 100);
    assert!(decode(&bytes).is_err());
}

#[test]
fn corrupt_samples_and_missing_interrupts_are_rejected() {
    let mut bytes = capture();
    // Header is 12 bytes; first case ID/count adds 8 bytes.
    bytes[20..24].copy_from_slice(&f32::NAN.to_le_bytes());
    assert!(decode(&bytes).is_err());
    let mut bytes = capture();
    let end = bytes.len();
    bytes[end - 4..].fill(0);
    assert!(decode(&bytes).is_err());
    bytes[end - 4..].copy_from_slice(&1_u32.to_le_bytes());
    bytes.push(0);
    assert!(decode(&bytes).is_err());
}

#[test]
fn sample_comparison_rejects_lost_samples_and_non_finite_data() {
    assert!(sample_error(&[0.0], &[0.0, 0.0]).is_err());
    assert!(sample_error(&[f32::INFINITY], &[0.0]).is_err());
    let (peak, _) = sample_error(&[0.0, 0.25, 0.0], &[0.0; 3]).unwrap();
    assert!(peak > audio_lab::emulator::MAX_SAMPLE_ERROR);
}
