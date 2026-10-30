use std::process::Command;
use virtual_daisy::{daisy_sai_input, daisy_sai_requests, daisy_sai_tick, Peripheral};

fn wm_register(codec: &mut Peripheral, register: u8, value: u16) {
    codec
        .i2c_write(&[(register << 1) | ((value >> 8) as u8 & 1), value as u8])
        .unwrap();
}

#[test]
fn wm8731_dac_mute_preserves_adc_capture() {
    let mut codec = Peripheral::new(4);
    for (register, value) in [
        (0, 0x17),
        (1, 0x17),
        (4, 0x12),
        (5, 0),
        (6, 0x42),
        (7, 9),
        (9, 1),
    ] {
        wm_register(&mut codec, register, value);
    }
    assert_eq!(codec.read(0x100).unwrap(), 3, "ADC and DAC enabled");
    wm_register(&mut codec, 5, 8);
    assert_eq!(codec.read(0x100).unwrap(), 1, "DACMU silences DAC, not ADC");
}

#[test]
fn rev4_and_rev7_receive_on_b_and_transmit_on_a() {
    let mut sai = Peripheral::new(7);
    sai.configure_audio(1).unwrap();
    let input = [-0.5, 0.25];
    unsafe {
        daisy_sai_input(&mut sai, input.as_ptr(), 1);
    }
    sai.write(4, (6 << 5) | (3 << 16)).unwrap(); // A master TX, 24 bits.
    sai.write(0x24, (6 << 5) | 3 | (3 << 16)).unwrap(); // B slave RX.
    sai.write(0x20, 0x0010_0000).unwrap();
    sai.write(0x20, 0x00e0_0000).unwrap();
    unsafe {
        daisy_sai_tick(&mut sai, 3);
    }
    assert_eq!(sai.read(0x40).unwrap() as i32, -4_194_304);
    assert_eq!(sai.read(0x40).unwrap(), 2_097_152);
    assert_eq!(unsafe { daisy_sai_requests(&mut sai) } & 1, 1);
    assert!(
        sai.read(0x20).is_err(),
        "TX register must not return RX data"
    );
}

#[test]
fn users_can_inspect_the_board_codec_and_sai_wiring_before_booting() {
    // Expected hardware identities/wiring come from the manufacturer datasheet
    // and libDaisy, not from another copy of the emulator's profile definitions.
    let output = Command::new(env!("CARGO_BIN_EXE_virtual-daisy"))
        .arg("profiles")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let profiles = String::from_utf8(output.stdout).unwrap();
    for expected in [
        "rev4 AK4556 RX=B TX=A",
        "rev5 WM8731 RX=A TX=B",
        "rev7 PCM3060 RX=B TX=A",
        "hardware_equivalence=UNVERIFIED",
    ] {
        assert!(
            profiles.contains(expected),
            "Missing {expected}: {profiles}"
        );
    }
}

#[test]
fn selecting_a_codec_that_is_not_on_the_board_is_rejected_before_boot() {
    let output = Command::new(env!("CARGO_BIN_EXE_virtual-daisy"))
        .args([
            "run",
            "/missing/renode",
            "/missing/results",
            "--board",
            "rev7",
            "--codec",
            "wm8731",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("rev7 has PCM3060"));
}

#[test]
fn ak4556_reset_is_a_gpio_signal_and_pcm3060_seed_is_hardware_controlled() {
    let mut ak = Peripheral::new(9);
    assert_eq!(ak.read(0x100).unwrap(), 0);
    ak.codec_control(true).unwrap();
    assert_eq!(ak.read(0x100).unwrap(), 3);
    ak.codec_control(false).unwrap();
    assert_eq!(ak.read(0x100).unwrap(), 0);
    assert!(ak.i2c_write(&[18, 1]).is_err());
    let mut pcm = Peripheral::new(10);
    assert_eq!(pcm.read(0x100).unwrap(), 3);
    assert!(pcm.i2c_write(&[18, 1]).is_err());
}

#[test]
fn wm8731_reset_restores_documented_register_defaults() {
    // WM8731 Rev4.9 Table30 field defaults (Table29 contains an R7 typo).
    let defaults = [0x97, 0x97, 0x79, 0x79, 0x0a, 8, 0x9f, 0x0a, 0, 0];
    let mut codec = Peripheral::new(4);
    for (register, expected) in defaults.into_iter().enumerate() {
        assert_eq!(codec.read(register as u64).unwrap(), expected);
    }
    wm_register(&mut codec, 0, 0x17);
    wm_register(&mut codec, 15, 0);
    assert_eq!(codec.read(0).unwrap(), 0x97);
    assert_eq!(codec.read(9).unwrap(), 0);
}

#[test]
fn pcm_input_uses_the_entire_signed_24_bit_range() {
    let mut sai = Peripheral::new(7);
    let input = [1.0, -1.0];
    unsafe {
        daisy_sai_input(&mut sai, input.as_ptr(), 1);
    }
    sai.write(4, (6 << 5) | 1 | (3 << 16)).unwrap();
    unsafe {
        daisy_sai_tick(&mut sai, 3);
    }
    assert_eq!(sai.read(0x20).unwrap(), 0x007f_ffff);
    assert_eq!(sai.read(0x20).unwrap() as i32, -8_388_608);
}

#[test]
fn wm8731_line_mute_only_silences_its_selected_input_channel() {
    let mut codec = Peripheral::new(4);
    for (register, value) in [
        (0, 0x97),
        (1, 0x17),
        (4, 0x12),
        (5, 0),
        (6, 0x42),
        (7, 9),
        (9, 1),
    ] {
        wm_register(&mut codec, register, value);
    }
    let mut sai = Peripheral::new(7);
    let input = [0.5, -0.25];
    unsafe {
        daisy_sai_input(&mut sai, input.as_ptr(), 1);
    }
    sai.write(4, (6 << 5) | 1 | (3 << 16)).unwrap();
    unsafe {
        daisy_sai_tick(&mut sai, codec.read(0x100).unwrap());
    }
    assert_eq!(sai.read(0x20).unwrap(), 0);
    assert_eq!(sai.read(0x20).unwrap() as i32, -2_097_152);
}

#[test]
fn a_functional_model_cannot_certify_hardware_equivalence() {
    let output = Command::new(env!("CARGO_BIN_EXE_virtual-daisy"))
        .args([
            "run",
            "/missing/renode",
            "/missing/results",
            "--board",
            "rev5",
            "--require-hardware-match",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Hardware equivalence is UNVERIFIED"));
}

#[test]
fn firmware_cannot_enable_wm8731_audio_directions_on_an_ak4556_board() {
    let mut sai = Peripheral::new(7);
    sai.configure_audio(1).unwrap();
    assert!(sai.write(4, (6 << 5) | 1 | (3 << 16)).is_err());
    assert!(sai.write(0x24, (6 << 5) | 2 | (3 << 16)).is_err());
}

#[test]
fn wm8731_both_channel_load_updates_the_opposite_volume_and_mute() {
    let mut codec = Peripheral::new(4);
    wm_register(&mut codec, 0, 0x117);
    assert_eq!(codec.read(1).unwrap() & 0x9f, 0x17);
    wm_register(&mut codec, 1, 0x197);
    assert_eq!(codec.read(0).unwrap() & 0x9f, 0x97);
}

#[test]
fn unimplemented_active_codec_polarity_is_rejected_instead_of_ignored() {
    let mut codec = Peripheral::new(4);
    wm_register(&mut codec, 7, 0x29); // 24-bit LJ with L/R swap.
    wm_register(&mut codec, 9, 1);
    assert!(codec.read(0x100).is_err());
}

#[test]
fn unstrapped_revision_pins_float_instead_of_being_forced_high() {
    let mut port = Peripheral::new(103);
    port.configure_revision_straps(4).unwrap(); // Rev7 grounds PD5 only.
    port.write(0, 0).unwrap();
    port.write(0xc, 0x540).unwrap(); // Pull-ups on PD3/4/5.
    assert_eq!(port.read(0x10).unwrap() & 0x38, 0x18);
    port.write(0xc, 0xa80).unwrap(); // Pull-downs: floating PD3/4 follow the bias.
    assert_eq!(port.read(0x10).unwrap() & 0x38, 0);
    port.reset();
    port.write(0, 0).unwrap();
    port.write(0xc, 0x540).unwrap();
    assert_eq!(port.read(0x10).unwrap() & 0x38, 0x18);
}

#[test]
fn wm8731_accepts_rm0433_master_receive_and_slave_transmit_modes() {
    // RM0433 SAI_xCR1 MODE: 01 master RX, 10 slave TX.
    let mut sai = Peripheral::new(7);
    assert!(sai.write(4, (6 << 5) | 1 | (3 << 16)).is_ok());
    assert!(sai.write(0x24, (6 << 5) | 2 | (3 << 16)).is_ok());
}

#[test]
fn sai_dma_requests_stop_at_the_programmed_fifo_threshold() {
    // RM0433: quarter-full TX requests stop at two words; FLVL uses ranges,
    // including 010 at exactly 1/4 and 100 at exactly 3/4 capacity.
    let mut sai = Peripheral::new(7);
    sai.write(0x28, 1).unwrap();
    sai.write(0x24, (6 << 5) | 2 | (1 << 17)).unwrap();
    assert_eq!(unsafe { daisy_sai_requests(&mut sai) }, 2);
    sai.write(0x40, 0).unwrap();
    assert_eq!(unsafe { daisy_sai_requests(&mut sai) }, 2);
    sai.write(0x40, 0).unwrap();
    assert_eq!(sai.read(0x38).unwrap() >> 16, 2);
    assert_eq!(unsafe { daisy_sai_requests(&mut sai) }, 0);
    for _ in 0..4 {
        sai.write(0x40, 0).unwrap();
    }
    assert_eq!(sai.read(0x38).unwrap() >> 16, 4);
}
