use virtual_daisy::{daisy_gpio_drive, Peripheral};

#[test]
fn simultaneous_bsrr_set_and_reset_sets_the_output() {
    let mut gpio = Peripheral::new(102);
    gpio.write(0x18, (1 << 3) | (1 << 19)).unwrap();
    assert_eq!(gpio.read(0x14).unwrap(), 1 << 3);
    assert_eq!(gpio.read(0x18).unwrap(), 0);
}

#[test]
fn analog_input_reads_zero_and_open_drain_high_releases_to_the_external_level() {
    let mut gpio = Peripheral::new(102);
    unsafe {
        daisy_gpio_drive(&mut gpio, 3, 1);
    }
    assert_eq!(gpio.read(0x10).unwrap() & (1 << 3), 0); // Reset: analog.
    gpio.write(0, 1 << 6).unwrap(); // PC3 output.
    gpio.write(4, 1 << 3).unwrap(); // Open drain.
    gpio.write(0x14, 1 << 3).unwrap(); // Release, external high.
    assert_ne!(gpio.read(0x10).unwrap() & (1 << 3), 0);
    unsafe {
        daisy_gpio_drive(&mut gpio, 3, 0);
    }
    assert_eq!(gpio.read(0x10).unwrap() & (1 << 3), 0);
    gpio.write(0x14, 0).unwrap(); // Drive low, even with external high.
    unsafe {
        daisy_gpio_drive(&mut gpio, 3, 1);
    }
    assert_eq!(gpio.read(0x10).unwrap() & (1 << 3), 0);
}

#[test]
fn lock_key_sequence_freezes_selected_pin_configuration_but_not_output_data() {
    let mut gpio = Peripheral::new(102);
    gpio.write(0, 1 << 6).unwrap();
    for key in [1, 0, 1] {
        gpio.write(0x1c, (key << 16) | (1 << 3)).unwrap();
    }
    gpio.read(0x1c).unwrap();
    assert_eq!(gpio.read(0x1c).unwrap(), 0x10008);
    gpio.write(0, 2 << 6 | 1 << 8).unwrap();
    assert_eq!(gpio.read(0).unwrap(), 1 << 6 | 1 << 8);
    for (register, expected) in [
        (4, 0xfff7),
        (8, 0xffff_ff3f),
        (0xc, 0xffff_ff3f),
        (0x20, 0xffff_0fff),
    ] {
        gpio.write(register, u32::MAX).unwrap();
        assert_eq!(gpio.read(register).unwrap(), expected);
    }
    gpio.write(0x18, 1 << 3).unwrap();
    assert_eq!(gpio.read(0x14).unwrap(), 1 << 3);
    gpio.reset();
    assert_eq!(gpio.read(0x1c).unwrap(), 0);
    gpio.write(0, 2 << 6).unwrap();
    assert_eq!(gpio.read(0).unwrap(), 2 << 6);
}

#[test]
fn a_changed_mask_or_interrupted_key_sequence_does_not_lock_pins() {
    let mut gpio = Peripheral::new(102);
    for sequence in [[0x10008, 8, 0x10010], [0x10008, 0x10008, 0x10008]] {
        for value in sequence {
            gpio.write(0x1c, value).unwrap();
        }
        assert_eq!(gpio.read(0x1c).unwrap() & 0x10000, 0);
        gpio.write(0, 0).unwrap();
        assert_eq!(gpio.read(0).unwrap(), 0);
    }
    gpio.write(0x1c, 0x10800).unwrap();
    gpio.read(0x1c).unwrap(); // Reading before all three writes aborts the sequence.
    gpio.write(0x1c, 0x800).unwrap();
    gpio.write(0x1c, 0x10800).unwrap();
    assert_eq!(gpio.read(0x1c).unwrap() & 0x10000, 0);
    for value in [0x10800, 0x800, 0x10800] {
        gpio.write(0x1c, value).unwrap();
    }
    gpio.read(0x1c).unwrap();
    gpio.write(0x24, u32::MAX).unwrap();
    assert_eq!(gpio.read(0x24).unwrap(), 0xffff0fff); // AFRH pin11 frozen.
    gpio.write(0x1c, 0).unwrap();
    assert_eq!(gpio.read(0x1c).unwrap(), 0x10800);
}
