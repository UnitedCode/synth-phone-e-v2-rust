use virtual_daisy::nor::NorFlash;

#[test]
fn reprogramming_requires_erase_even_for_an_all_ones_byte_and_rejected_packets_are_atomic() {
    // Sections8.8–8.10 require erase before reprogramming a byte, including FF.
    for (opcode, qpi) in [(2, false), (2, true), (0x32, false), (0x38, false)] {
        let mut chip = NorFlash::default();
        chip.transact(6, 0, &[], 0, false).unwrap();
        chip.transact(1, 0, &[0x40], 0, false).unwrap();
        chip.advance_time(2_000_000).unwrap();
        if qpi {
            chip.transact(0x35, 0, &[], 0, false).unwrap();
        }
        chip.transact(6, 0, &[], 0, qpi).unwrap();
        chip.transact(opcode, 0x101, &[0xff], 0, qpi).unwrap();
        chip.advance_time(2_008_000).unwrap();
        chip.transact(6, 0, &[], 0, qpi).unwrap();
        assert_eq!(
            chip.transact(opcode, 0x100, &[0x12, 0x34], 0, qpi),
            Err("IS25LP reprogramming a byte requires erase")
        );
        assert_eq!(chip.transact(5, 0, &[], 1, qpi).unwrap()[0] & 3, 2);
        assert_eq!(chip.transact(0x0b, 0x100, &[], 2, qpi).unwrap(), [0xff; 2]);
        // No partial programming, and a different byte in this page is writable.
        chip.transact(opcode, 0x100, &[0x12], 0, qpi).unwrap();
        chip.advance_time(2_016_000).unwrap();
        assert_eq!(
            chip.transact(0x0b, 0x100, &[], 2, qpi).unwrap(),
            [0x12, 0xff]
        );
        chip.transact(0x66, 0, &[], 0, qpi).unwrap();
        chip.transact(0x99, 0, &[], 0, qpi).unwrap();
        chip.advance_time(2_116_000).unwrap();
        chip.transact(6, 0, &[], 0, false).unwrap();
        assert_eq!(
            chip.transact(2, 0x101, &[0], 0, false),
            Err("IS25LP reprogramming a byte requires erase")
        );
        chip.transact(0x20, 0x101, &[], 0, false).unwrap();
        chip.advance_time(72_116_000).unwrap();
        chip.transact(6, 0, &[], 0, false).unwrap();
        chip.transact(2, 0x101, &[0x34], 0, false).unwrap();
        chip.advance_time(72_124_000).unwrap();
        assert_eq!(
            chip.transact(3, 0x100, &[], 2, false).unwrap(),
            [0xff, 0x34]
        );
    }
}

#[test]
fn single_byte_program_uses_the_published_8us_or_40us_scenario() {
    // Section9.9 distinguishes Byte Program from the 0.2/0.8ms page row.
    for (maximum, duration) in [(false, 8_000), (true, 40_000)] {
        for (opcode, qpi) in [(2, false), (2, true), (0x32, false), (0x38, false)] {
            let mut chip = NorFlash::default();
            chip.set_maximum(maximum).unwrap();
            if opcode != 2 {
                chip.transact(6, 0, &[], 0, false).unwrap();
                chip.transact(1, 0, &[0x40], 0, false).unwrap();
                chip.advance_time(15_000_000).unwrap();
            } else {
                chip.advance_time(15_000_000).unwrap();
            }
            if qpi {
                chip.transact(0x35, 0, &[], 0, false).unwrap();
            }
            chip.transact(6, 0, &[], 0, qpi).unwrap();
            chip.transact(opcode, 0xff, &[0x12], 0, qpi).unwrap();
            chip.advance_time(15_000_000 + duration - 1).unwrap();
            assert_eq!(chip.transact(5, 0, &[], 1, qpi).unwrap()[0] & 3, 3);
            chip.advance_time(15_000_000 + duration).unwrap();
            assert_eq!(chip.transact(5, 0, &[], 1, qpi).unwrap()[0] & 3, 0);
            assert_eq!(
                chip.transact(0x0b, 0xfe, &[], 3, qpi).unwrap(),
                [0xff, 0x12, 0xff]
            );
        }
    }
}

#[test]
fn resumed_program_and_erase_require_400us_before_another_suspend() {
    // ISSI Rev A section8.21: 400us resume-to-suspend, 100us read-ready.
    for qpi in [false, true] {
        for (suspend, resume) in [(0x75, 0x7a), (0xb0, 0x30)] {
            for (command, duration, flag) in [(2, 800_000, 4), (0x20, 300_000_000, 8)] {
                let mut chip = NorFlash::default();
                chip.set_maximum(true).unwrap();
                if qpi {
                    chip.transact(0x35, 0, &[], 0, false).unwrap();
                }
                chip.transact(6, 0, &[], 0, qpi).unwrap();
                chip.transact(
                    command,
                    0,
                    if command == 2 { &[0x12; 256] } else { &[] },
                    0,
                    qpi,
                )
                .unwrap();
                chip.transact(suspend, 0, &[], 0, qpi).unwrap();
                chip.advance_time(100_000).unwrap();
                assert_eq!(chip.transact(0x48, 0, &[], 1, qpi).unwrap()[0] & 12, flag);
                chip.advance_time(1_000_000).unwrap();
                chip.transact(resume, 0, &[], 0, qpi).unwrap();
                chip.advance_time(1_399_999).unwrap();
                assert_eq!(
                    chip.transact(suspend, 0, &[], 0, qpi),
                    Err("IS25LP suspend requires 400us since resume")
                );
                assert_eq!(chip.transact(5, 0, &[], 1, qpi).unwrap()[0] & 1, 1);
                assert_eq!(
                    chip.transact(0x48, 0, &[], 1, qpi),
                    Err("IS25LP064A read output is undefined while busy")
                );
                chip.advance_time(1_400_000).unwrap();
                chip.transact(suspend, 0, &[], 0, qpi).unwrap();
                chip.advance_time(1_499_999).unwrap();
                assert_eq!(chip.transact(5, 0, &[], 1, qpi).unwrap()[0] & 1, 1);
                chip.advance_time(1_500_000).unwrap();
                assert_eq!(chip.transact(0x48, 0, &[], 1, qpi).unwrap()[0] & 12, flag);
                chip.advance_time(2_000_000).unwrap();
                chip.transact(resume, 0, &[], 0, qpi).unwrap();
                // Active work is 100us + 500us; suspension does not restart it.
                chip.advance_time(2_000_000 + duration - 600_000 - 1)
                    .unwrap();
                assert_eq!(chip.transact(5, 0, &[], 1, qpi).unwrap()[0] & 1, 1);
                chip.advance_time(2_000_000 + duration - 600_000).unwrap();
                assert_eq!(chip.transact(5, 0, &[], 1, qpi).unwrap()[0] & 3, 0);
                assert_eq!(
                    chip.transact(0x0b, 0, &[], 1, qpi).unwrap(),
                    [if command == 2 { 0x12 } else { 0xff }]
                );
            }
        }
    }
}

#[test]
fn deep_power_down_ignores_writes_and_reset_until_release_and_recovery() {
    let mut chip = NorFlash::default();
    chip.transact(0xb9, 0, &[], 0, false).unwrap();
    chip.advance_time(3000).unwrap();
    for command in [6, 0x66, 0x99] {
        chip.transact(command, 0, &[], 0, false).unwrap();
    }
    assert!(chip.transact(5, 0, &[], 1, false).is_err()); // High-Z is not status zero.
    chip.transact(0xab, 0, &[], 0, false).unwrap();
    chip.advance_time(5999).unwrap();
    assert!(chip.transact(0x9f, 0, &[], 3, false).is_err());
    chip.advance_time(6000).unwrap();
    assert_eq!(chip.transact(5, 0, &[], 1, false).unwrap(), [0]);
    assert_eq!(chip.transact(0xab, 0, &[], 3, false).unwrap(), [0x16; 3]);
}

#[test]
fn program_suspend_exposes_status_and_reads_then_resume_keeps_remaining_work() {
    let mut chip = NorFlash::default();
    chip.transact(6, 0, &[], 0, false).unwrap();
    chip.transact(2, 0, &[0x12; 256], 0, false).unwrap();
    chip.advance_time(50_000).unwrap();
    chip.transact(0x75, 0, &[], 0, false).unwrap();
    chip.advance_time(149_999).unwrap();
    assert_eq!(chip.transact(5, 0, &[], 1, false).unwrap()[0] & 1, 1);
    chip.advance_time(150_000).unwrap();
    assert_eq!(chip.transact(5, 0, &[], 1, false).unwrap()[0] & 3, 0);
    assert_eq!(chip.transact(0x48, 0, &[], 1, false).unwrap()[0] & 12, 4);
    chip.advance_time(1_000_000).unwrap();
    assert_eq!(chip.transact(3, 0x1000, &[], 1, false).unwrap(), [0xff]);
    chip.transact(6, 0, &[], 0, false).unwrap(); // WREN is not accepted while suspended.
    assert_eq!(chip.transact(5, 0, &[], 1, false).unwrap()[0] & 3, 0);
    chip.transact(0x30, 0, &[], 0, false).unwrap();
    assert_eq!(chip.transact(5, 0, &[], 1, false).unwrap()[0] & 1, 1);
    chip.advance_time(1_049_999).unwrap();
    assert_eq!(chip.transact(5, 0, &[], 1, false).unwrap()[0] & 1, 1);
    chip.advance_time(1_050_000).unwrap();
    assert_eq!(chip.transact(0x48, 0, &[], 1, false).unwrap()[0] & 12, 0);
    assert_eq!(chip.transact(3, 0, &[], 1, false).unwrap(), [0x12]);
}

#[test]
fn an_overlength_page_packet_retains_only_its_last_256_bytes_without_spilling() {
    let mut chip = NorFlash::default();
    let mut packet = vec![0; 256];
    packet.extend([0x12, 0x34]);
    chip.transact(6, 0, &[], 0, false).unwrap();
    chip.transact(2, 0xff, &packet, 0, false).unwrap();
    chip.advance_time(200_000).unwrap();
    assert_eq!(
        chip.transact(3, 0xfe, &[], 4, false).unwrap(),
        [0, 0x12, 0xff, 0xff]
    );
    assert_eq!(chip.transact(3, 0, &[], 2, false).unwrap(), [0x34, 0]);
    // Reprogramming is outside the specified sequence even when sending ones.
    chip.transact(6, 0, &[], 0, false).unwrap();
    assert_eq!(
        chip.transact(2, 0, &[0xff, 0xff], 0, false),
        Err("IS25LP reprogramming a byte requires erase")
    );
    assert_eq!(chip.transact(3, 0, &[], 2, false).unwrap(), [0x34, 0]);
}

#[test]
fn read_wrap_selects_8_16_32_or_64_byte_boundaries_and_can_be_disabled() {
    let mut chip = NorFlash::default();
    let bytes: Vec<u8> = (0..=255).collect();
    chip.transact(6, 0, &[], 0, false).unwrap();
    chip.transact(2, 0, &bytes, 0, false).unwrap();
    chip.advance_time(200_000).unwrap();
    for (code, mask) in [(4, 7), (5, 15), (6, 31), (7, 63)] {
        chip.transact(0xc0, 0, &[code], 0, false).unwrap();
        let expected = [0xfe, 0xff, !mask, !mask | 1];
        assert_eq!(chip.transact(3, 0xfe, &[], 4, false).unwrap(), expected);
    }
    chip.transact(0xc0, 0, &[0], 0, false).unwrap();
    assert_eq!(
        chip.transact(3, 0xfe, &[], 4, false).unwrap(),
        [0xfe, 0xff, 0xff, 0xff]
    );
    assert!(chip.transact(0xeb, 0, &[], 1, false).is_err()); // QE is required in SPI.
    chip.transact(0x35, 0, &[], 0, false).unwrap();
    assert_eq!(chip.transact(0xeb, 0, &[], 2, true).unwrap(), [0, 1]); // QPI entry suffices.
}

#[test]
fn page_program_requires_wel_keeps_wip_until_completion_and_wraps_inside_its_page() {
    let mut chip = NorFlash::default();
    assert_eq!(
        chip.transact(0x9f, 0, &[], 6, false).unwrap(),
        [0x9d, 0x60, 0x17, 0x9d, 0x60, 0x17]
    );
    // A write without WREN is ignored by the chip, not a successful program.
    chip.transact(2, 0xfe, &[0x12, 0x34, 0x56, 0x78], 0, false)
        .unwrap();
    assert_eq!(chip.transact(3, 0xfe, &[], 4, false).unwrap(), [0xff; 4]);
    chip.transact(6, 0, &[], 0, false).unwrap();
    chip.transact(2, 0xfe, &[0x12, 0x34, 0x56, 0x78], 0, false)
        .unwrap();
    assert_eq!(chip.transact(5, 0, &[], 1, false).unwrap()[0] & 3, 3);
    chip.advance_time(199_999).unwrap();
    assert_ne!(chip.transact(5, 0, &[], 1, false).unwrap()[0] & 1, 0);
    chip.advance_time(200_000).unwrap();
    assert_eq!(chip.transact(5, 0, &[], 1, false).unwrap()[0] & 3, 0);
    assert_eq!(
        chip.transact(3, 0xfe, &[], 4, false).unwrap(),
        [0x12, 0x34, 0xff, 0xff]
    );
    assert_eq!(chip.transact(3, 0, &[], 2, false).unwrap(), [0x56, 0x78]);
}

#[test]
fn qpi_and_read_parameters_do_not_require_or_clear_write_enable() {
    let mut chip = NorFlash::default();
    chip.transact(0x35, 0, &[], 0, false).unwrap();
    assert_eq!(chip.transact(5, 0, &[], 1, true).unwrap(), [0]);
    assert!(chip.transact(5, 0, &[], 1, false).is_err());
    chip.transact(6, 0, &[], 0, true).unwrap();
    chip.transact(0xc0, 0, &[0xe4], 0, true).unwrap();
    assert_eq!(chip.transact(5, 0, &[], 1, true).unwrap(), [2]);
    chip.transact(0xf5, 0, &[], 0, true).unwrap();
    assert_eq!(chip.transact(5, 0, &[], 1, false).unwrap(), [2]);
    chip.transact(4, 0, &[], 0, false).unwrap();
    chip.transact(0xc0, 0, &[0xe0], 0, false).unwrap();
    assert_eq!(chip.transact(5, 0, &[], 1, false).unwrap(), [0]);
}

#[test]
fn sector_erase_is_deferred_and_preserves_the_neighboring_sector() {
    let mut chip = NorFlash::default();
    for (i, address) in [0xffe, 0x1000].into_iter().enumerate() {
        chip.transact(6, 0, &[], 0, false).unwrap();
        chip.transact(2, address, &[0x12, 0x34], 0, false).unwrap();
        chip.advance_time((i as u64 + 1) * 200_000).unwrap();
    }
    chip.transact(0x20, 0xab, &[], 0, false).unwrap(); // No WREN: ignored.
    assert_eq!(
        chip.transact(3, 0xffe, &[], 4, false).unwrap(),
        [0x12, 0x34, 0x12, 0x34]
    );
    chip.transact(6, 0, &[], 0, false).unwrap();
    chip.transact(0xd7, 0xab, &[], 0, false).unwrap();
    chip.advance_time(70_399_999).unwrap();
    assert_eq!(chip.transact(5, 0, &[], 1, false).unwrap()[0] & 3, 3);
    chip.advance_time(70_400_000).unwrap();
    assert_eq!(
        chip.transact(3, 0xffe, &[], 4, false).unwrap(),
        [0xff, 0xff, 0x12, 0x34]
    );
    assert_eq!(chip.transact(5, 0, &[], 1, false).unwrap()[0] & 3, 0);
}

#[test]
fn nonvolatile_block_protection_inhibits_program_and_chip_erase() {
    let mut chip = NorFlash::default();
    chip.transact(6, 0, &[], 0, false).unwrap();
    chip.transact(2, 0, &[0x12], 0, false).unwrap();
    chip.advance_time(200_000).unwrap();
    chip.transact(6, 0, &[], 0, false).unwrap();
    chip.transact(1, 0, &[4], 0, false).unwrap(); // BP0: protect the top 64KiB.
    chip.advance_time(2_200_000).unwrap();
    assert_eq!(chip.transact(5, 0, &[], 1, false).unwrap(), [4]);
    chip.transact(6, 0, &[], 0, false).unwrap();
    chip.transact(2, 0x7f0000, &[0], 0, false).unwrap();
    assert_eq!(chip.transact(5, 0, &[], 1, false).unwrap()[0] & 1, 0);
    assert_eq!(chip.transact(3, 0x7f0000, &[], 1, false).unwrap(), [0xff]);
    chip.transact(0x60, 0, &[], 0, false).unwrap(); // Any BP bit inhibits chip erase.
    assert_eq!(chip.transact(5, 0, &[], 1, false).unwrap()[0] & 1, 0);
    assert_eq!(chip.transact(3, 0, &[], 1, false).unwrap(), [0x12]);
    // An unprotected page still programs.
    chip.transact(2, 0x100, &[0x34], 0, false).unwrap();
    chip.advance_time(2_400_000).unwrap();
    assert_eq!(chip.transact(3, 0x100, &[], 1, false).unwrap(), [0x34]);
}

#[test]
fn reset_enable_must_be_immediately_followed_by_reset_and_keeps_nonvolatile_state() {
    let mut chip = NorFlash::default();
    chip.transact(6, 0, &[], 0, false).unwrap();
    chip.transact(2, 0, &[0x12], 0, false).unwrap();
    chip.advance_time(200_000).unwrap();
    chip.transact(6, 0, &[], 0, false).unwrap();
    chip.transact(1, 0, &[0x44], 0, false).unwrap();
    chip.advance_time(2_200_000).unwrap();
    chip.transact(0x35, 0, &[], 0, false).unwrap();
    chip.transact(0x66, 0, &[], 0, true).unwrap();
    chip.transact(0, 0, &[], 0, true).unwrap(); // NOP cancels reset enable.
    chip.transact(0x99, 0, &[], 0, true).unwrap();
    assert!(chip.transact(5, 0, &[], 1, false).is_err()); // Still QPI.
    chip.transact(0x66, 0, &[], 0, true).unwrap();
    chip.transact(0x99, 0, &[], 0, true).unwrap();
    chip.advance_time(2_300_000).unwrap();
    assert_eq!(chip.transact(5, 0, &[], 1, false).unwrap(), [0x44]);
    assert_eq!(chip.transact(3, 0, &[], 1, false).unwrap(), [0x12]);
}

#[test]
fn srwd_and_the_wp_pin_lock_status_writes_in_spi_mode() {
    let mut chip = NorFlash::default();
    chip.transact(6, 0, &[], 0, false).unwrap();
    chip.transact(1, 0, &[0x80], 0, false).unwrap();
    chip.advance_time(2_000_000).unwrap();
    chip.set_write_protect(true);
    chip.transact(6, 0, &[], 0, false).unwrap();
    chip.transact(1, 0, &[0], 0, false).unwrap();
    assert_eq!(chip.transact(5, 0, &[], 1, false).unwrap()[0] & 0xfd, 0x80);
    chip.set_write_protect(false);
    chip.transact(1, 0, &[0], 0, false).unwrap();
    chip.advance_time(4_000_000).unwrap();
    assert_eq!(chip.transact(5, 0, &[], 1, false).unwrap(), [0]);
}

#[test]
fn maximum_duration_scenario_covers_page_register_and_all_erase_sizes() {
    let mut chip = NorFlash::default();
    chip.set_maximum(true).unwrap();
    let mut now = 0;
    for (command, data, duration) in [
        (2, &[0x12; 256][..], 800_000),
        (1, &[0][..], 15_000_000),
        (0x20, &[][..], 300_000_000),
        (0x52, &[][..], 500_000_000),
        (0xd8, &[][..], 1_000_000_000),
        (0x60, &[][..], 45_000_000_000),
    ] {
        chip.transact(6, 0, &[], 0, false).unwrap();
        chip.transact(command, 0, data, 0, false).unwrap();
        assert!(chip.set_maximum(false).is_err());
        chip.advance_time(now + duration - 1).unwrap();
        assert_eq!(chip.transact(5, 0, &[], 1, false).unwrap()[0] & 3, 3);
        now += duration;
        chip.advance_time(now).unwrap();
        assert_eq!(chip.transact(5, 0, &[], 1, false).unwrap()[0] & 3, 0);
    }
}

#[test]
fn function_register_top_bottom_selection_is_one_time_programmable() {
    let mut chip = NorFlash::default();
    assert_eq!(chip.transact(0x48, 0, &[], 1, false).unwrap(), [0]);
    for (i, value) in [2, 0].into_iter().enumerate() {
        chip.transact(6, 0, &[], 0, false).unwrap();
        chip.transact(0x42, 0, &[value], 0, false).unwrap();
        chip.advance_time((i as u64 + 1) * 2_000_000).unwrap();
        assert_eq!(chip.transact(0x48, 0, &[], 1, false).unwrap(), [2]);
    }
    chip.transact(6, 0, &[], 0, false).unwrap();
    chip.transact(1, 0, &[4], 0, false).unwrap();
    chip.advance_time(6_000_000).unwrap();
    chip.transact(6, 0, &[], 0, false).unwrap();
    chip.transact(2, 0, &[0], 0, false).unwrap(); // Bottom block is protected.
    assert_eq!(chip.transact(5, 0, &[], 1, false).unwrap()[0] & 1, 0);
    assert_eq!(chip.transact(3, 0, &[], 1, false).unwrap(), [0xff]);
    chip.transact(2, 0x7f0000, &[0x12], 0, false).unwrap();
    chip.advance_time(6_200_000).unwrap();
    assert_eq!(chip.transact(3, 0x7f0000, &[], 1, false).unwrap(), [0x12]);
}
