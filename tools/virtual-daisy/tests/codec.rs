use virtual_daisy::Peripheral;

fn register(codec: &mut Peripheral, index: u8, value: u16) {
    codec
        .i2c_write(&[(index << 1) | ((value >> 8) as u8), value as u8])
        .unwrap();
}
fn active() -> Peripheral {
    let mut codec = Peripheral::new(4);
    for (index, value) in [
        (0, 23),
        (1, 23),
        (4, 0x12),
        (5, 0),
        (6, 0x42),
        (7, 9),
        (8, 0),
        (9, 1),
    ] {
        register(&mut codec, index, value);
    }
    codec
}

#[test]
fn wm8731_ideal_line_stage_rejects_dc_with_the_published_high_pass_response() {
    let mut codec = active();
    let first = codec.ideal_line_input([0.25, -0.5]).unwrap();
    assert_eq!(first, [0.25, -0.5]);
    let second = codec.ideal_line_input([0.25, -0.5]).unwrap();
    assert!((second[0] - 0.249875).abs() < 1e-7);
    for _ in 0..20_000 {
        codec.ideal_line_input([0.25, -0.5]).unwrap();
    }
    let settled = codec.ideal_line_input([0.25, -0.5]).unwrap();
    assert!(settled.iter().all(|sample| sample.abs() < 0.00003));
}

#[test]
fn wm8731_gain_uses_1_5_db_steps_and_channels_remain_independent() {
    let mut codec = active();
    register(&mut codec, 5, 1); // Disable HPF.
    register(&mut codec, 0, 31); // +12dB; right remains 0dB.
    let output = codec.ideal_line_input([0.1, 0.1]).unwrap();
    assert!((output[0] - 0.39810717).abs() < 1e-7);
    assert_eq!(output[1], 0.1);
    register(&mut codec, 0, 0); // -34.5dB.
    assert!((codec.ideal_line_input([0.1, 0.1]).unwrap()[0] - 0.001883649).abs() < 1e-8);
    register(&mut codec, 0, 0x80 | 23);
    assert_eq!(codec.ideal_line_input([0.1, 0.1]).unwrap(), [0.0, 0.1]);
}

#[test]
fn wm8731_hpor_retains_dc_offset_only_when_requested() {
    let mut codec = active();
    for _ in 0..20_000 {
        codec.ideal_line_input([0.25; 2]).unwrap();
    }
    register(&mut codec, 5, 0x11); // Hold offset, disable HPF.
    assert!(codec.ideal_line_input([0.25; 2]).unwrap()[0].abs() < 0.00002);
    register(&mut codec, 5, 1); // Clear offset, HPF still disabled.
    assert_eq!(codec.ideal_line_input([0.25; 2]).unwrap(), [0.25; 2]);
    register(&mut codec, 15, 0);
    assert_eq!(codec.ideal_line_input([0.25; 2]).unwrap(), [0.0; 2]);
}

#[test]
fn active_wm8731_unsupported_sampling_and_deemphasis_do_not_pass_silently() {
    let mut codec = active();
    register(&mut codec, 8, 1); // USB MCLK mode.
    assert!(codec.read(0x100).is_err());
    register(&mut codec, 8, 0);
    register(&mut codec, 5, 6); // De-emphasis requires a missing DAC filter model.
    assert!(codec.read(0x100).is_err());
}

#[test]
fn wm8731_type1_nominal_passband_delays_adc_by_20_and_dac_by_18_frames() {
    let mut codec = active();
    register(&mut codec, 5, 1); // HPF disabled to isolate converter latency.
    assert_eq!(codec.nominal_adc_passband([0.25, -0.5]).unwrap(), [0.0; 2]);
    for _ in 1..20 {
        assert_eq!(codec.nominal_adc_passband([0.0; 2]).unwrap(), [0.0; 2]);
    }
    assert_eq!(codec.nominal_adc_passband([0.0; 2]).unwrap(), [0.25, -0.5]);
    assert_eq!(codec.nominal_dac_passband([0.5, -0.25]).unwrap(), [0.0; 2]);
    for _ in 1..18 {
        assert_eq!(codec.nominal_dac_passband([0.0; 2]).unwrap(), [0.0; 2]);
    }
    assert_eq!(codec.nominal_dac_passband([0.0; 2]).unwrap(), [0.5, -0.25]);
}
