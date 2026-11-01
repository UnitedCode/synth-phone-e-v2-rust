use virtual_daisy::Peripheral;

#[test]
fn wm8731_lrswap_changes_captured_dac_channels_without_swapping_adc_channels() {
    use std::ffi::CString;
    use virtual_daisy::{daisy_sai_input, daisy_sai_save, daisy_sai_tick_codec};
    for ideal in [0, 1] {
        let mut codec = active();
        register(&mut codec, 5, 1); // Disable HPF to isolate channel order.
        register(&mut codec, 7, 9 | (1 << 5)); // Table 15: DAC-only LRSWAP.
        let mut sai = Peripheral::new(7);
        sai.write(4, (6 << 5) | 1 | (3 << 16)).unwrap();
        sai.write(0x24, (6 << 5) | 2 | (3 << 16)).unwrap();
        let input = [0.5_f32, -0.75].repeat(22);
        unsafe { daisy_sai_input(&mut sai, input.as_ptr(), 22) };
        for frame in 0..22 {
            sai.write(0x40, 0x0010_0000).unwrap(); // +0.125, left
            sai.write(0x40, 0x00e0_0000).unwrap(); // -0.25, right
            assert_eq!(
                unsafe { daisy_sai_tick_codec(&mut sai, &mut codec, ideal) },
                0
            );
            let adc = [sai.read(0x20).unwrap(), sai.read(0x20).unwrap()];
            assert_eq!(
                adc,
                if ideal == 0 || frame >= 20 {
                    [0x0040_0000, 0xffa0_0000]
                } else {
                    [0; 2]
                }
            );
        }
        // No new LRC frame: do not swap or delay the previous captured frame
        // again when the master SAI block is disabled.
        sai.write(4, (6 << 5) | 1).unwrap();
        assert_eq!(
            unsafe { daisy_sai_tick_codec(&mut sai, &mut codec, ideal) },
            0
        );
        let path =
            std::env::temp_dir().join(format!("daisy-lrswap-{}-{ideal}.wav", std::process::id()));
        let cpath = CString::new(path.to_str().unwrap()).unwrap();
        assert_eq!(
            unsafe { daisy_sai_save(&mut sai, cpath.as_ptr(), 48014) },
            0
        );
        let samples: Vec<f32> = hound::WavReader::open(&path)
            .unwrap()
            .into_samples()
            .map(Result::unwrap)
            .collect();
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_file(format!("{}.stats", path.display())).unwrap();
        assert_eq!(samples.len(), 44, "exactly 22 stereo frames were clocked");
        for (frame, samples) in samples.chunks_exact(2).enumerate() {
            assert_eq!(
                samples,
                if ideal == 0 || frame >= 18 {
                    &[-0.25, 0.125]
                } else {
                    &[0.0; 2]
                }
            );
        }
    }
}

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
