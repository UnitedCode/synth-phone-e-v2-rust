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
fn wm8731_512fs_clock_mode_and_unused_clockout_divider_do_not_mute_valid_audio() {
    let mut codec = active();
    register(&mut codec, 8, 1 << 6); // CLKIDIV2 accepts 512fs external MCLK.
    assert_eq!(codec.read(0x100).unwrap(), 3);
    register(&mut codec, 8, (1 << 6) | (1 << 7));
    assert_eq!(codec.read(0x100).unwrap(), 3); // CLKOUT is not the ADC/DAC clock.
}

#[test]
fn all_four_wm8731_filter_types_use_their_published_nominal_group_delays() {
    // Table32: ADC/DAC delay pairs for filter types0/1/2/3.
    for (sampling, adc_delay, dac_delay) in [(1, 12, 11), (0, 20, 18), (28, 3, 5), (29, 6, 5)] {
        let mut codec = active();
        register(&mut codec, 5, 1);
        register(&mut codec, 8, sampling);
        for frame in 0..=20 {
            let impulse = if frame == 0 { [0.25, -0.5] } else { [0.0; 2] };
            assert_eq!(
                codec.nominal_adc_passband(impulse).unwrap(),
                if frame == adc_delay {
                    [0.25, -0.5]
                } else {
                    [0.0; 2]
                }
            );
            assert_eq!(
                codec.nominal_dac_passband(impulse).unwrap(),
                if frame == dac_delay {
                    [0.25, -0.5]
                } else {
                    [0.0; 2]
                }
            );
        }
    }
}

#[test]
fn wm8731_sampling_tables_preserve_fractional_rates_and_separate_clock_dividers() {
    use virtual_daisy::clock::Rate;
    // Tables18/19/21/22, including the deliberately non-integer "8k" rates.
    for (usb, sr, adc, dac, filter) in [
        (false, 0, 48000.0, 48000.0, 1),
        (false, 1, 48000.0, 8000.0, 1),
        (false, 2, 8000.0, 48000.0, 1),
        (false, 3, 8000.0, 8000.0, 1),
        (false, 6, 32000.0, 32000.0, 1),
        (false, 7, 96000.0, 96000.0, 2),
        (false, 8, 44100.0, 44100.0, 1),
        (false, 9, 44100.0, 8018.181818181818, 1),
        (false, 10, 8018.181818181818, 44100.0, 1),
        (false, 11, 8018.181818181818, 8018.181818181818, 1),
        (false, 15, 88200.0, 88200.0, 2),
        (true, 0, 48000.0, 48000.0, 0),
        (true, 1, 48000.0, 8000.0, 0),
        (true, 2, 8000.0, 48000.0, 0),
        (true, 3, 8000.0, 8000.0, 0),
        (true, 6, 32000.0, 32000.0, 0),
        (true, 7, 96000.0, 96000.0, 3),
        (true, 8, 44117.64705882353, 44117.64705882353, 1),
        (true, 9, 44117.64705882353, 8021.39037433155, 1),
        (true, 10, 8021.39037433155, 44117.64705882353, 1),
        (true, 11, 8021.39037433155, 8021.39037433155, 1),
        (true, 15, 88235.29411764706, 88235.29411764706, 2),
    ] {
        for bosr in [0, 1] {
            if usb && bosr != u16::from(sr >= 8) {
                continue;
            }
            let hz = if usb {
                12000000
            } else if sr < 8 {
                if bosr == 0 {
                    12288000
                } else {
                    18432000
                }
            } else if bosr == 0 {
                11289600
            } else {
                16934400
            };
            for divide in [0, 1] {
                let mut codec = active();
                register(
                    &mut codec,
                    8,
                    u16::from(usb) | (bosr << 1) | (sr << 2) | (divide << 6) | 128,
                );
                let clocks = codec
                    .codec_clocks(Rate {
                        numerator: hz * (1 + u64::from(divide)),
                        denominator: 1,
                    })
                    .unwrap();
                assert!((clocks.adc.hz() - adc).abs() < 1e-8);
                assert!((clocks.dac.hz() - dac).abs() < 1e-8);
                assert_eq!(clocks.filter, filter);
                assert_eq!(clocks.clkout.hz(), hz as f64 / 2.0);
            }
        }
    }
    for control in [3, 4 << 2, 5 << 2, 12 << 2, 13 << 2, 14 << 2, 256] {
        let mut codec = active();
        register(&mut codec, 8, control);
        assert!(codec
            .codec_clocks(Rate {
                numerator: 12288000,
                denominator: 1
            })
            .is_err());
    }
}

#[test]
fn live_codec_clock_validation_catches_mismatches_instead_of_returning_valid_looking_pcm() {
    use virtual_daisy::{daisy_codec_clocks, Peripheral};
    let mut codec = active();
    let mut rcc = Peripheral::new(13);
    rcc.write(0x50, 4).unwrap(); // SAI source CKPER=HSI64MHz.
    let cr1 = (1 << 16) | (4 << 20);
    assert_eq!(
        unsafe { daisy_codec_clocks(&mut codec, &mut rcc, cr1, 63) },
        0
    );
    register(&mut codec, 8, 64); // Core clock /2 but SAI still256fs.
    assert_eq!(
        unsafe { daisy_codec_clocks(&mut codec, &mut rcc, cr1, 63) },
        1
    );
    assert_eq!(
        unsafe { daisy_codec_clocks(&mut codec, &mut rcc, cr1 | (1 << 26), 63) },
        0
    ); // 512fs
    register(&mut codec, 8, 1); // USB250fs cannot use a256fs master clock.
    assert_eq!(
        unsafe { daisy_codec_clocks(&mut codec, &mut rcc, cr1, 63) },
        1
    );
    register(&mut codec, 8, 0);
    assert_eq!(
        unsafe { daisy_codec_clocks(&mut codec, &mut rcc, cr1 | (1 << 19), 63) },
        1
    ); // No MCLK.
    register(&mut codec, 8, 4); // ADC48k/DAC8k cannot both use the shared LRC.
    assert_eq!(
        unsafe { daisy_codec_clocks(&mut codec, &mut rcc, cr1, 63) },
        1
    );
    register(&mut codec, 6, 0x4a); // DAC powered down: ADC-only48k is valid.
    assert_eq!(
        unsafe { daisy_codec_clocks(&mut codec, &mut rcc, cr1, 63) },
        0
    );
}

#[test]
fn hardware_mode_codecs_require_mclk_and_pcm3060_adc_cannot_use_dac_only_ratios() {
    use virtual_daisy::{clock::Rate, daisy_codec_clocks};
    let mut rcc = Peripheral::new(13);
    rcc.write(0x50, 4).unwrap();
    let cr1 = (1 << 16) | (4 << 20);
    for kind in [9, 10] {
        let mut codec = Peripheral::new(kind);
        if kind == 9 {
            codec.codec_control(true).unwrap();
        }
        assert_eq!(
            unsafe { daisy_codec_clocks(&mut codec, &mut rcc, cr1, 63) },
            0
        );
        assert_eq!(
            unsafe { daisy_codec_clocks(&mut codec, &mut rcc, cr1 | (1 << 19), 63) },
            1
        );
        assert!(codec
            .validate_codec_clocks(
                Rate {
                    numerator: 12_288_000,
                    denominator: 1
                },
                Rate {
                    numerator: 48_000,
                    denominator: 1
                }
            )
            .is_ok());
        if kind == 10 {
            assert!(codec
                .validate_codec_clocks(
                    Rate {
                        numerator: 12_288_000,
                        denominator: 1
                    },
                    Rate {
                        numerator: 96_000,
                        denominator: 1
                    }
                )
                .is_err()); //128fs valid for DAC only
        }
    }
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
    register(&mut codec, 8, 3); // USB 272fs with SR=0000 is reserved (Table21).
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
