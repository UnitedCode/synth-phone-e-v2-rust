use virtual_daisy::{daisy_sai_input, daisy_sai_tick, Peripheral};

#[test]
fn each_audio_block_reports_its_own_fifo_even_before_direction_configuration() {
    let mut sai = Peripheral::new(7);
    sai.write(0x24, (6 << 5) | 2 | (1 << 17)).unwrap();
    sai.write(0x40, 1).unwrap();
    sai.write(0x40, 2).unwrap();
    assert_eq!(
        sai.read(0x18).unwrap(),
        8,
        "reset A transmitter still has its own empty FIFO"
    );
    assert_eq!(sai.read(0x38).unwrap() >> 16, 1);
    sai.write(0x14, 8).unwrap();
    assert_eq!(unsafe { virtual_daisy::daisy_sai_irq(&mut sai) }, 1);
    sai.write(4, (6 << 5) | 1).unwrap(); // Reconfigure A as receive, still disabled.
    assert_eq!(sai.read(0x18).unwrap(), 0);
    assert_eq!(unsafe { virtual_daisy::daisy_sai_irq(&mut sai) }, 0);
}

fn stereo() -> Peripheral {
    let mut sai = Peripheral::new(7);
    sai.write(0xc, 63).unwrap();
    sai.write(4, (6 << 5) | 1 | (3 << 16)).unwrap();
    sai.write(0x24, (6 << 5) | 2 | (3 << 16)).unwrap();
    let input: Vec<f32> = (0..8)
        .flat_map(|i| [i as f32 / 16.0, -(i as f32 + 1.0) / 16.0])
        .collect();
    unsafe { daisy_sai_input(&mut sai, input.as_ptr(), 8) };
    sai
}
fn tick(sai: &mut Peripheral) {
    unsafe { daisy_sai_tick(sai, 3) };
}
fn output(sai: &mut Peripheral, name: &str) -> Vec<f32> {
    let path = std::env::temp_dir().join(format!("daisy-v11-{name}-{}.wav", std::process::id()));
    let cpath = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    assert_eq!(
        unsafe { virtual_daisy::daisy_sai_save(sai, cpath.as_ptr(), 48000) },
        0
    );
    let samples = hound::WavReader::open(&path)
        .unwrap()
        .into_samples()
        .map(Result::unwrap)
        .collect();
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_file(format!("{}.stats", path.display())).unwrap();
    samples
}

#[test]
fn transmit_underrun_in_right_slot_resumes_there_without_swapping_channels() {
    let mut sai = stereo();
    sai.write(0x40, 0x100000).unwrap();
    tick(&mut sai); // L0 sent; R0 missing.
    sai.write(0x40, 0x200000).unwrap();
    tick(&mut sai); // Mute L1; send R0 in the right slot.
    sai.write(0x40, 0x300000).unwrap();
    sai.write(0x40, 0x400000).unwrap();
    tick(&mut sai);
    assert_eq!(
        output(&mut sai, "underrun-slot"),
        [0.125, 0.0, 0.0, 0.25, 0.375, 0.5]
    );
}

#[test]
fn disabled_subblocks_preserve_fifo_data_and_do_not_receive_or_consume() {
    let mut sai = stereo();
    sai.write(0x24, (6 << 5) | 2).unwrap(); // B TX disabled, A still clocks frames.
    sai.write(0x40, 0x100000).unwrap();
    sai.write(0x40, 0x200000).unwrap();
    tick(&mut sai);
    assert_ne!(sai.read(0x38).unwrap() >> 16, 0); // queued TX words remain
    assert_eq!(sai.read(0x38).unwrap() & 1, 0);
    sai.write(0x24, (6 << 5) | 2 | (3 << 16)).unwrap();
    tick(&mut sai);
    assert_eq!(output(&mut sai, "disabled-tx"), [0.0, 0.0, 0.125, 0.25]);
    let mut reverse = Peripheral::new(7);
    reverse.write(0xc, 63).unwrap();
    reverse.configure_audio(1).unwrap();
    reverse.write(4, (6 << 5) | (3 << 16)).unwrap(); // A TX enabled; B RX disabled.
    reverse.write(0x24, (6 << 5) | 3).unwrap();
    tick(&mut reverse);
    assert!(reverse.read(0x40).is_err());
}

#[test]
fn transmit_mute_substitutes_zero_or_previous_slot_values_while_consuming_fifo() {
    for (hold, expected) in [(0, [0.0, 0.0]), (64, [0.125, 0.25])] {
        let mut sai = stereo();
        sai.write(0x24, (6 << 5) | 2).unwrap();
        sai.write(0x28, hold).unwrap(); // MUTEVAL configured while disabled.
        sai.write(0x24, (6 << 5) | 2 | (3 << 16)).unwrap();
        for word in [0x100000, 0x200000] {
            sai.write(0x40, word).unwrap();
        }
        tick(&mut sai);
        sai.write(0x28, hold | 32).unwrap();
        for word in [0x300000, 0x400000] {
            sai.write(0x40, word).unwrap();
        }
        tick(&mut sai);
        assert_eq!(
            sai.read(0x38).unwrap() >> 16,
            0,
            "muted words still consumed"
        );
        sai.write(0x28, hold).unwrap();
        for word in [0x500000, 0x600000] {
            sai.write(0x40, word).unwrap();
        }
        tick(&mut sai);
        assert_eq!(
            output(&mut sai, &format!("mute-{hold}")),
            [0.125, 0.25, expected[0], expected[1], 0.625, 0.75]
        );
    }
}

#[test]
fn mute_detection_counts_consecutive_serial_zero_frames_and_rearms_on_data_or_disable() {
    let mut sai = stereo();
    let input = [
        0.0_f32, 0.0, 0.0, 0.0, 0.0, 0.0, 0.25, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
    ];
    unsafe { daisy_sai_input(&mut sai, input.as_ptr(), 7) };
    sai.write(8, 2 << 7).unwrap();
    sai.write(0x14, 2).unwrap();
    tick(&mut sai);
    assert_eq!(sai.read(0x18).unwrap() & 2, 0);
    tick(&mut sai);
    assert_eq!(sai.read(0x18).unwrap() & 2, 2);
    assert_eq!(unsafe { virtual_daisy::daisy_sai_irq(&mut sai) }, 1);
    sai.write(0x1c, 2).unwrap();
    tick(&mut sai);
    assert_eq!(
        sai.read(0x18).unwrap() & 2,
        0,
        "continuous silence must not retrigger"
    );
    tick(&mut sai); // nonzero data re-arms, even when the FIFO is full.
    tick(&mut sai);
    assert_eq!(sai.read(0x18).unwrap() & 2, 0);
    tick(&mut sai);
    assert_eq!(sai.read(0x18).unwrap() & 2, 2);
    sai.write(0x1c, 2).unwrap();
    sai.write(4, (6 << 5) | 1).unwrap();
    sai.write(4, (6 << 5) | 1 | (3 << 16)).unwrap();
    tick(&mut sai);
    assert_eq!(sai.read(0x18).unwrap() & 2, 0);
    tick(&mut sai);
    assert_eq!(sai.read(0x18).unwrap() & 2, 2);
}

#[test]
fn two_slot_mono_duplicates_one_tx_word_and_stores_only_rx_slot_zero() {
    let mut sai = Peripheral::new(7);
    sai.write(0xc, 63).unwrap();
    for (cr, slots, mode) in [(4, 0x10, 1), (0x24, 0x30, 2)] {
        sai.write(slots, 0x00030180).unwrap(); // two active 32-bit slots
        sai.write(cr, (6 << 5) | mode | (1 << 12) | (3 << 16))
            .unwrap();
    }
    let input = [0.25_f32, 0.75, 0.5, -0.5];
    unsafe { daisy_sai_input(&mut sai, input.as_ptr(), 2) };
    sai.write(0x40, 0x100000).unwrap();
    sai.write(0x40, 0x200000).unwrap();
    tick(&mut sai);
    tick(&mut sai);
    assert_eq!(sai.read(0x20).unwrap(), 0x200000);
    assert_eq!(sai.read(0x20).unwrap(), 0x400000);
    assert!(sai.read(0x20).is_err());
    assert_eq!(
        sai.read(0x38).unwrap() & 1,
        0,
        "one FIFO word per mono frame"
    );
    assert_eq!(output(&mut sai, "mono"), [0.125, 0.125, 0.25, 0.25]);
}

#[test]
fn sai_reset_defaults_and_direction_specific_fifo_level_boundaries_match_register_tables() {
    let mut sai = Peripheral::new(7);
    for cr in [4, 0x24] {
        assert_eq!(sai.read(cr).unwrap(), 0x40);
    }
    for sr in [0x18, 0x38] {
        assert_eq!(sai.read(sr).unwrap(), 8);
    }
    sai.write(0x24, (6 << 5) | 2).unwrap();
    for (words, expected) in [0, 1, 1, 2, 2, 3, 3, 4, 5].into_iter().enumerate() {
        assert_eq!(sai.read(0x38).unwrap() >> 16, expected, "TX words={words}");
        if words < 8 {
            sai.write(0x40, 1).unwrap();
        }
    }
    let mut receive = stereo();
    for _ in 0..4 {
        tick(&mut receive);
    }
    for expected in [5, 4, 4, 3, 3, 2, 2, 1, 0] {
        assert_eq!(receive.read(0x18).unwrap() >> 16, expected);
        if expected != 0 {
            receive.read(0x20).unwrap();
        }
    }
    sai.reset();
    for sr in [0x18, 0x38] {
        assert_eq!(sai.read(sr).unwrap(), 8);
    }
}

#[test]
fn unsupported_serial_protocols_fail_and_fifo_flush_preserves_sticky_error_flags() {
    for extra in [1 << 2, 2 << 2, 1 << 8, 2 << 10, 3 << 10] {
        let mut sai = Peripheral::new(7);
        assert!(sai.write(4, (6 << 5) | 1 | extra | (3 << 16)).is_err());
    }
    let mut sai = stereo();
    for _ in 0..5 {
        tick(&mut sai);
    }
    sai.write(8, 8).unwrap(); // 51.4.9 explicitly describes data loss while enabled.
    assert!(sai.read(0x20).is_err());
    assert_eq!(
        sai.read(0x18).unwrap() & 1,
        1,
        "flush does not clear OVRUDR"
    );
    sai.write(4, (6 << 5) | 1).unwrap();
    sai.write(8, 8).unwrap();
    assert_eq!(sai.read(8).unwrap() & 8, 0);
    let mut companded = Peripheral::new(7);
    companded.write(8, 2 << 14).unwrap();
    assert!(companded.write(4, (6 << 5) | 1 | (3 << 16)).is_err());
}

#[test]
fn invalid_master_frame_length_sets_wckcfg_irq_and_clears_saien() {
    let mut sai = Peripheral::new(7);
    sai.write(0xc, 47).unwrap(); // 48 bits is not a power of two with MCLK.
    sai.write(0x14, 4).unwrap();
    sai.write(4, (6 << 5) | 1 | (1 << 16)).unwrap();
    assert_eq!(sai.read(0x18).unwrap() & 4, 4);
    assert_eq!(sai.read(4).unwrap() & (1 << 16), 0);
    assert_eq!(unsafe { virtual_daisy::daisy_sai_irq(&mut sai) }, 1);
    sai.write(0x1c, 4).unwrap();
    assert_eq!(unsafe { virtual_daisy::daisy_sai_irq(&mut sai) }, 0);
    sai.write(0xc, 63).unwrap();
    sai.write(4, (6 << 5) | 1 | (1 << 16)).unwrap();
    assert_ne!(sai.read(4).unwrap() & (1 << 16), 0);
    let mut no_mclk = Peripheral::new(7);
    no_mclk.write(0xc, 47).unwrap();
    no_mclk
        .write(4, (6 << 5) | 1 | (1 << 16) | (1 << 19))
        .unwrap();
    assert_eq!(no_mclk.read(0x18).unwrap() & 4, 0);
    let mut slave = Peripheral::new(7);
    slave.write(0x2c, 47).unwrap();
    slave.write(0x24, (6 << 5) | 2 | (1 << 16)).unwrap();
    assert_eq!(slave.read(0x38).unwrap() & 4, 0);
}

#[test]
fn unsupported_companding_or_pdm_cannot_be_enabled_after_pcm_startup() {
    let mut sai = stereo();
    assert!(sai.write(8, 2 << 14).is_err());
    assert!(sai.write(0x44, 1).is_err());
}

#[test]
fn receive_overrun_preserves_queued_words_and_resumes_at_the_failed_slot() {
    let mut sai = stereo();
    for _ in 0..5 {
        tick(&mut sai);
    }
    assert_eq!(
        sai.read(0x18).unwrap() >> 16,
        5,
        "eight old words must survive"
    );
    assert_eq!(sai.read(0x20).unwrap(), 0); // L0
    tick(&mut sai); // L5 fits; R5 overruns, remember slot1.
    assert_eq!(sai.read(0x20).unwrap(), 0xfff80000); // R0
    tick(&mut sai); // Skip L6 even though one word is free, then accept R6.
    let words: Vec<_> = (0..8).map(|_| sai.read(0x20).unwrap()).collect();
    assert_eq!(
        words,
        [0x80000, 0xfff00000, 0x100000, 0xffe80000, 0x180000, 0xffe00000, 0x280000, 0xffc80000]
    );
}

#[test]
fn overrun_underrun_flags_are_sticky_read_only_and_cleared_per_block() {
    let mut sai = stereo();
    for _ in 0..5 {
        tick(&mut sai);
    }
    assert_eq!(sai.read(0x18).unwrap() & 1, 1);
    assert_eq!(sai.read(0x38).unwrap() & 1, 1);
    sai.write(0x1c, 1).unwrap();
    assert_eq!(sai.read(0x18).unwrap() & 1, 0);
    assert_eq!(sai.read(0x38).unwrap() & 1, 1);
    assert_eq!(sai.read(0x1c).unwrap(), 0, "CLRFR is write-only");
    assert!(sai.write(0x18, 0).is_err());
    sai.write(0x3c, 8).unwrap(); // FREQ has no write-to-clear bit.
    assert_eq!(sai.read(0x38).unwrap() & 1, 1);
    sai.write(0x3c, 1).unwrap();
    assert_eq!(sai.read(0x38).unwrap() & 1, 0);
}

#[test]
fn shared_sai_interrupt_ors_both_blocks_and_tracks_mask_and_fifo_reads() {
    use virtual_daisy::daisy_sai_irq;
    let mut sai = stereo();
    tick(&mut sai); // TX underrun; RX has two words.
    assert_eq!(unsafe { daisy_sai_irq(&mut sai) }, 0);
    sai.write(0x34, 1).unwrap();
    assert_eq!(unsafe { daisy_sai_irq(&mut sai) }, 1);
    sai.write(0x14, 8).unwrap(); // RX FREQIE
    sai.write(0x3c, 1).unwrap(); // TX cleared; RX keeps IRQ asserted.
    assert_eq!(unsafe { daisy_sai_irq(&mut sai) }, 1);
    sai.read(0x20).unwrap();
    sai.read(0x20).unwrap();
    assert_eq!(unsafe { daisy_sai_irq(&mut sai) }, 0);
    sai.write(0x34, 8).unwrap(); // TX empty FREQ
    assert_eq!(unsafe { daisy_sai_irq(&mut sai) }, 1);
    sai.write(0x40, 1).unwrap();
    assert_eq!(unsafe { daisy_sai_irq(&mut sai) }, 0);
}
