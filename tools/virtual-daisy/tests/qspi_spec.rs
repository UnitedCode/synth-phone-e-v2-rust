use virtual_daisy::qspi::Qspi;

#[test]
fn memory_mapped_data_register_reads_zero_without_draining_or_clearing_busy() {
    let mut qspi = Qspi::default();
    qspi.configure_clock(Some(virtual_daisy::clock::Rate {
        numerator: 50_000_000,
        denominator: 1,
    }))
    .unwrap();
    qspi.write(0, 1, 4).unwrap();
    qspi.write(4, 22 << 16, 4).unwrap();
    qspi.write(0x14, 0x0d002503, 4).unwrap();
    assert_eq!(qspi.read_mapped(0, 4).unwrap(), [0xff; 4]);
    for width in [1, 2, 4] {
        assert_eq!(qspi.read(0x20, width).unwrap(), 0);
    }
    assert_eq!(qspi.read(8, 4).unwrap() & 32, 32);
}

#[test]
fn free_running_clock_mode_does_not_silently_execute_a_normal_flash_command() {
    let mut qspi = Qspi::default();
    qspi.write(0, 1, 4).unwrap();
    assert!(qspi.write(0x14, (1 << 29) | 0x100, 4).is_err());
}

#[test]
fn spi_dual_and_quad_output_reads_share_nor_bytes_and_use_documented_lane_clocks() {
    for (opcode, data_mode, service) in [(0x3b, 2, 570), (0x6b, 3, 490)] {
        let mut qspi = Qspi::default();
        qspi.configure_clock(Some(virtual_daisy::clock::Rate {
            numerator: 100_000_000,
            denominator: 1,
        }))
        .unwrap();
        qspi.write(0, 1, 4).unwrap();
        qspi.write(4, 22 << 16, 4).unwrap();
        qspi.write(0x14, 0x106, 4).unwrap();
        qspi.write(0x10, 0, 4).unwrap();
        qspi.write(0x14, 0x01000101, 4).unwrap();
        qspi.write(0x20, 0x40, 1).unwrap(); // QE; completion2ms.
        qspi.advance_time(2_000_000).unwrap();
        qspi.write(0x14, 0x106, 4).unwrap();
        qspi.write(0x10, 39, 4).unwrap();
        qspi.write(0x14, 0x01002502, 4).unwrap();
        qspi.write(0x18, 0x100, 4).unwrap();
        for byte in 0..40 {
            qspi.write(0x20, byte, 1).unwrap();
        }
        qspi.advance_time(2_200_000).unwrap();
        let phases = (data_mode << 24) | (8 << 18) | (2 << 12) | (1 << 10) | (1 << 8) | opcode;
        qspi.write(0x14, (1 << 26) | phases, 4).unwrap();
        qspi.write(0x18, 0x100, 4).unwrap();
        for word in 0..10 {
            let expected = 0x03020100 + word * 0x04040404;
            assert_eq!(qspi.read(0x20, 4).unwrap(), expected);
        }
        qspi.write(0x14, (3 << 26) | phases, 4).unwrap();
        assert_eq!(qspi.mapped_service_ns(0x100, 4).unwrap(), service);
        assert_eq!(qspi.read_mapped(0x100, 4).unwrap(), [0, 1, 2, 3]);
        qspi.write(0, 3, 4).unwrap(); // abort mapped transaction
        qspi.write(0x14, (3 << 26) | phases ^ (1 << 18), 4)
            .unwrap_err(); // wrong dummy count
    }
}

#[test]
fn quad_input_program_opcodes_obey_qe_wel_deferred_completion_and_page_wrap() {
    for opcode in [0x32, 0x38] {
        let mut qspi = Qspi::default();
        qspi.write(0, 1, 4).unwrap();
        qspi.write(4, 22 << 16, 4).unwrap();
        qspi.write(0x14, 0x106, 4).unwrap();
        qspi.write(0x10, 0, 4).unwrap();
        qspi.write(0x14, 0x01000101, 4).unwrap();
        qspi.write(0x20, 0x40, 1).unwrap();
        qspi.advance_time(2_000_000).unwrap();
        // WEL has cleared: the complete packet must not program anything.
        qspi.write(0x14, 0x03002500 | opcode, 4).unwrap();
        qspi.write(0x18, 0, 4).unwrap();
        qspi.write(0x20, 0, 1).unwrap();
        qspi.write(0x10, 0, 4).unwrap();
        qspi.write(0x14, 0x05000105, 4).unwrap();
        assert_eq!(qspi.read(0x20, 1).unwrap() & 3, 0);
        qspi.write(0x14, 0x106, 4).unwrap();
        qspi.write(0x10, 257, 4).unwrap();
        qspi.write(0x14, 0x03002500 | opcode, 4).unwrap();
        qspi.write(0x18, 0xff, 4).unwrap();
        for _ in 0..256 {
            qspi.write(0x20, 0, 1).unwrap();
        }
        qspi.write(0x20, 0x3412, 2).unwrap();
        qspi.advance_time(2_199_999).unwrap();
        qspi.write(0x10, 0, 4).unwrap();
        qspi.write(0x14, 0x05000105, 4).unwrap();
        assert_eq!(qspi.read(0x20, 1).unwrap() & 3, 3);
        qspi.advance_time(2_200_000).unwrap();
        qspi.write(0x10, 3, 4).unwrap();
        qspi.write(0x14, 0x05002503, 4).unwrap();
        qspi.write(0x18, 0xfe, 4).unwrap();
        assert_eq!(qspi.read(0x20, 4).unwrap(), 0xffff1200);
        qspi.write(0x10, 1, 4).unwrap();
        qspi.write(0x14, 0x05002503, 4).unwrap();
        qspi.write(0x18, 0, 4).unwrap();
        assert_eq!(qspi.read(0x20, 2).unwrap(), 0x0034);
    }
}

#[test]
fn dual_io_read_uses_its_own_dummy_table_and_two_lane_address_and_mode_phases() {
    for (parameters, dummy, service) in [
        (0xe0, 0, 410),
        (0xe8, 0, 410),
        (0xf0, 4, 450),
        (0xf8, 4, 450),
    ] {
        let mut qspi = Qspi::default();
        qspi.configure_clock(Some(virtual_daisy::clock::Rate {
            numerator: 100_000_000,
            denominator: 1,
        }))
        .unwrap();
        qspi.write(0, 1, 4).unwrap();
        qspi.write(4, 22 << 16, 4).unwrap();
        qspi.write(0x14, 0x106, 4).unwrap();
        qspi.write(0x10, 3, 4).unwrap();
        qspi.write(0x14, 0x01002502, 4).unwrap();
        qspi.write(0x18, 0x100, 4).unwrap();
        qspi.write(0x20, 0x78563412, 4).unwrap();
        qspi.advance_time(200_000).unwrap();
        qspi.write(0x10, 0, 4).unwrap();
        qspi.write(0x14, 0x010001c0, 4).unwrap();
        qspi.write(0x20, parameters, 1).unwrap();
        let phases =
            (2 << 24) | (dummy << 18) | (2 << 14) | (2 << 12) | (2 << 10) | (1 << 8) | 0xbb;
        qspi.write(0x1c, 0, 4).unwrap();
        qspi.write(0x14, (3 << 26) | phases, 4).unwrap();
        assert_eq!(qspi.mapped_service_ns(0x100, 4).unwrap(), service);
        assert_eq!(
            qspi.read_mapped(0x100, 4).unwrap(),
            [0x12, 0x34, 0x56, 0x78]
        );
    }
}

#[test]
fn sioo_skips_only_later_instructions_and_controller_abort_does_not_reset_chip_ax_mode() {
    let mut qspi = Qspi::default();
    qspi.configure_clock(Some(virtual_daisy::clock::Rate {
        numerator: 100_000_000,
        denominator: 1,
    }))
    .unwrap();
    qspi.write(0, 1, 4).unwrap();
    qspi.write(4, 22 << 16, 4).unwrap();
    qspi.write(0x14, 0x106, 4).unwrap();
    qspi.write(0x10, 3, 4).unwrap();
    qspi.write(0x14, 0x01002502, 4).unwrap();
    qspi.write(0x18, 0x100, 4).unwrap();
    qspi.write(0x20, 0x78563412, 4).unwrap();
    qspi.advance_time(200_000).unwrap();
    qspi.write(0x1c, 0xa0, 4).unwrap();
    let phases = (3 << 26) | (2 << 24) | (2 << 14) | (2 << 12) | (2 << 10) | (1 << 8) | 0xbb;
    qspi.write(0x14, phases | (1 << 28), 4).unwrap();
    assert_eq!(qspi.mapped_service_ns(0x100, 4).unwrap(), 410);
    assert_eq!(
        qspi.read_mapped(0x100, 4).unwrap(),
        [0x12, 0x34, 0x56, 0x78]
    );
    assert_eq!(qspi.mapped_service_ns(0x100, 4).unwrap(), 330);
    assert_eq!(
        qspi.read_mapped(0x100, 4).unwrap(),
        [0x12, 0x34, 0x56, 0x78]
    );
    qspi.write(0, 3, 4).unwrap();
    // A CCR write re-arms the controller's instruction, but the chip still
    // expects an address. Do not silently treat this as a fresh BB command.
    qspi.write(0x14, phases | (1 << 28), 4).unwrap();
    assert!(qspi
        .read_mapped(0x100, 4)
        .unwrap_err()
        .contains("continuous"));
    qspi.write(0, 3, 4).unwrap();
    qspi.write(0x1c, 0, 4).unwrap();
    qspi.write(0x14, phases & !(3 << 8), 4).unwrap(); // instruction-less read, non-AX mode byte
    assert_eq!(
        qspi.read_mapped(0x100, 4).unwrap(),
        [0x12, 0x34, 0x56, 0x78]
    );
    assert!(qspi.read_mapped(0x100, 4).is_err(), "chip exited AX mode");
    qspi.write(0, 3, 4).unwrap();
    qspi.write(0x14, 0x106, 4).unwrap();
    qspi.write(0x10, 0, 4).unwrap();
    qspi.write(0x14, 0x05000105, 4).unwrap();
    assert_eq!(qspi.read(0x20, 1).unwrap() & 2, 2);
}

#[test]
fn indirect_address_retrigger_preserves_sioo_history_without_rewriting_ccr() {
    let mut qspi = Qspi::default();
    qspi.write(0, 1, 4).unwrap();
    qspi.write(4, 22 << 16, 4).unwrap();
    qspi.write(0x14, 0x106, 4).unwrap();
    qspi.write(0x10, 3, 4).unwrap();
    qspi.write(0x14, 0x01002502, 4).unwrap();
    qspi.write(0x18, 0, 4).unwrap();
    qspi.write(0x20, 0x78563412, 4).unwrap();
    qspi.advance_time(200_000).unwrap();
    qspi.write(0x1c, 0xa0, 4).unwrap();
    qspi.write(
        0x14,
        (1 << 28) | (1 << 26) | (2 << 24) | (2 << 14) | (2 << 12) | (2 << 10) | (1 << 8) | 0xbb,
        4,
    )
    .unwrap();
    for _ in 0..3 {
        qspi.write(0x18, 0, 4).unwrap();
        assert_eq!(qspi.read(0x20, 4).unwrap(), 0x78563412);
    }
}

#[test]
fn only_the_configured_24_address_bits_reach_the_flash_chip() {
    let mut qspi = Qspi::default();
    qspi.write(0, 1, 4).unwrap();
    qspi.write(4, 31 << 16, 4).unwrap();
    qspi.write(0x14, 0x106, 4).unwrap();
    qspi.write(0x10, 3, 4).unwrap();
    qspi.write(0x14, 0x01002502, 4).unwrap();
    qspi.write(0x18, 0xff000100, 4).unwrap();
    qspi.write(0x20, 0x78563412, 4).unwrap();
    qspi.advance_time(200_000).unwrap();
    for address in [0x100, 0xff000100] {
        qspi.write(0x14, 0x05002503, 4).unwrap();
        qspi.write(0x18, address, 4).unwrap();
        assert_eq!(qspi.read(0x20, 4).unwrap(), 0x78563412);
    }
}

#[test]
fn receive_fifo_keeps_busy_until_drained_and_reports_a_short_final_threshold() {
    let mut qspi = Qspi::default();
    qspi.write(0, 1 | (7 << 8), 4).unwrap(); // Eight-byte threshold.
    qspi.write(0x10, 2, 4).unwrap();
    qspi.write(0x14, 0x0500019f, 4).unwrap(); // Indirect read, 1-lane instruction/data.
    assert_eq!(qspi.read(8, 4).unwrap() & 0x3f3f, 0x0326);
    qspi.write(0xc, 2, 4).unwrap(); // Clearing TCF does not discard final FIFO data.
    assert_ne!(qspi.read(8, 4).unwrap() & 4, 0);
    assert_eq!(qspi.read(0x20, 1).unwrap(), 0x9d);
    assert_eq!(qspi.read(0x20, 2).unwrap(), 0x1760);
    assert_eq!(qspi.read(8, 4).unwrap() & 0x3f3f, 0);
}

#[test]
fn indirect_page_program_uses_the_address_phase_and_separate_chip_busy_status() {
    let mut qspi = Qspi::default();
    qspi.write(0, 1, 4).unwrap();
    qspi.write(4, 22 << 16, 4).unwrap(); // 8MiB mapped aperture.
    qspi.write(0x14, 0x106, 4).unwrap();
    qspi.write(0x10, 3, 4).unwrap();
    qspi.write(0x14, 0x01002502, 4).unwrap(); // SPI program, 24-bit address, four data bytes.
    qspi.write(0x18, 0xfe, 4).unwrap();
    qspi.write(0x20, 0x78563412, 4).unwrap();
    assert_eq!(qspi.read(8, 4).unwrap() & 0x22, 2); // Serial transfer done; NVM still busy.
    qspi.write(0x10, 0, 4).unwrap();
    qspi.write(0x14, 0x05000105, 4).unwrap();
    assert_eq!(qspi.read(0x20, 1).unwrap() & 3, 3);
    qspi.advance_time(200_000).unwrap();
    qspi.write(0x10, 3, 4).unwrap();
    qspi.write(0x14, 0x05002503, 4).unwrap();
    qspi.write(0x18, 0xfe, 4).unwrap();
    assert_eq!(qspi.read(0x20, 4).unwrap(), 0xffff3412);
    qspi.write(0x10, 1, 4).unwrap();
    qspi.write(0x14, 0x05002503, 4).unwrap();
    qspi.write(0x18, 0, 4).unwrap();
    assert_eq!(qspi.read(0x20, 2).unwrap(), 0x7856);
}

#[test]
fn transmit_starts_on_data_and_discards_bytes_beyond_dlr() {
    let mut qspi = Qspi::default();
    qspi.write(0, 1, 4).unwrap();
    qspi.write(4, 22 << 16, 4).unwrap();
    qspi.write(0x14, 0x106, 4).unwrap();
    qspi.write(0x10, 1, 4).unwrap();
    qspi.write(0x14, 0x01002502, 4).unwrap();
    qspi.write(0x18, 0, 4).unwrap();
    assert_eq!(qspi.read(8, 4).unwrap() & 32, 0);
    qspi.write(0x20, 0x78563412, 4).unwrap();
    qspi.advance_time(200_000).unwrap();
    qspi.write(0x10, 3, 4).unwrap();
    qspi.write(0x14, 0x05002503, 4).unwrap();
    qspi.write(0x18, 0, 4).unwrap();
    assert_eq!(qspi.read(0x20, 4).unwrap(), 0xffff3412);
}

#[test]
fn a_full_receive_fifo_waits_for_four_vacant_bytes_before_resuming() {
    let mut qspi = Qspi::default();
    qspi.write(0, 1 | (31 << 8), 4).unwrap();
    qspi.write(4, 22 << 16, 4).unwrap();
    qspi.write(0x10, 35, 4).unwrap();
    qspi.write(0x14, 0x05002503, 4).unwrap();
    qspi.write(0x18, 0, 4).unwrap();
    assert_eq!((qspi.read(8, 4).unwrap() >> 8) & 63, 32);
    for level in [31, 30, 29] {
        qspi.read(0x20, 1).unwrap();
        let status = qspi.read(8, 4).unwrap();
        assert_eq!((status >> 8) & 63, level);
        assert_eq!(status & 6, 0);
    }
    qspi.read(0x20, 1).unwrap();
    let status = qspi.read(8, 4).unwrap();
    assert_eq!((status >> 8) & 63, 32);
    assert_eq!(status & 6, 6);
}

#[test]
fn automatic_status_polling_reports_unmasked_data_and_stops_on_match_with_irq() {
    let mut qspi = Qspi::default();
    qspi.write(0, 1, 4).unwrap();
    qspi.write(4, 22 << 16, 4).unwrap();
    qspi.write(0x14, 0x106, 4).unwrap();
    qspi.write(0x10, 0, 4).unwrap();
    qspi.write(0x14, 0x01002502, 4).unwrap();
    qspi.write(0x18, 0, 4).unwrap();
    qspi.write(0x20, 0x12, 1).unwrap();
    qspi.write(0x24, 1, 4).unwrap(); // Compare only WIP, ignoring WEL.
    qspi.write(0x28, 0, 4).unwrap();
    qspi.write(0, 1 | (1 << 22) | (1 << 19), 4).unwrap(); // APMS + SMIE.
    qspi.write(0xc, 2, 4).unwrap();
    qspi.write(0x14, 0x09000105, 4).unwrap();
    let status = qspi.read(8, 4).unwrap();
    assert_eq!(status & 0x3f3f, 0x24); // No FIFO data, poll available, busy.
    assert_eq!(qspi.read(0x20, 1).unwrap(), 3); // Unmasked WIP and WEL.
    assert!(!qspi.irq());
    qspi.advance_time(199_999).unwrap();
    assert_ne!(qspi.read(8, 4).unwrap() & 32, 0);
    qspi.advance_time(200_000).unwrap();
    assert_eq!(qspi.read(8, 4).unwrap() & 0x3f3f, 0xc);
    assert!(qspi.irq());
    qspi.write(0xc, 8, 4).unwrap();
    assert!(!qspi.irq());
    assert_eq!(qspi.read(0x20, 1).unwrap(), 0);
    assert_eq!(qspi.read(8, 4).unwrap() & 0x3f3f, 0);
}

#[test]
fn polling_uses_serial_prescaler_frame_length_and_programmed_interval() {
    use virtual_daisy::clock::Rate;
    let mut qspi = Qspi::default();
    qspi.configure_clock(Some(Rate {
        numerator: 100_000_000,
        denominator: 1,
    }))
    .unwrap();
    qspi.write(0, 1 | (9 << 24), 4).unwrap(); // 10MHz serial clock.
    qspi.write(4, 3 << 8, 4).unwrap(); // Minimum four clocks with CS high.
    qspi.write(0x24, 1, 4).unwrap();
    qspi.write(0x28, 1, 4).unwrap(); // WIP will not match.
    qspi.write(0x2c, 20, 4).unwrap(); // Twenty clocks between reads.
    qspi.write(0x14, 0x09000105, 4).unwrap();
    assert_eq!(qspi.read(8, 4).unwrap() & 4, 0);
    qspi.advance_time(1599).unwrap();
    assert_eq!(qspi.read(8, 4).unwrap() & 4, 0);
    qspi.advance_time(1600).unwrap(); // Eight instruction + eight data clocks.
    assert_ne!(qspi.read(8, 4).unwrap() & 4, 0);
    qspi.read(0x20, 1).unwrap();
    qspi.advance_time(5199).unwrap();
    assert_eq!(qspi.read(8, 4).unwrap() & 4, 0);
    qspi.advance_time(5200).unwrap(); // (16 + max(20,4)) * 100ns after prior read.
    assert_ne!(qspi.read(8, 4).unwrap() & 4, 0);
}

#[test]
fn controller_reset_ends_a_partial_program_packet_without_resetting_the_flash_chip() {
    let mut qspi = Qspi::default();
    qspi.write(0, 1, 4).unwrap();
    qspi.write(4, 22 << 16, 4).unwrap();
    qspi.write(0x14, 0x106, 4).unwrap();
    qspi.write(0x10, 7, 4).unwrap();
    qspi.write(0x14, 0x01002502, 4).unwrap();
    qspi.write(0x18, 0, 4).unwrap();
    qspi.write(0x20, 0x78563412, 4).unwrap();
    assert_ne!(qspi.read(8, 4).unwrap() & 32, 0);
    qspi.reset_controller(); // CE rises after the four transmitted bytes.
    assert_eq!(qspi.read(0, 4).unwrap(), 0);
    qspi.write(0, 1, 4).unwrap();
    qspi.write(0x14, 0x05000105, 4).unwrap();
    assert_eq!(qspi.read(0x20, 1).unwrap() & 3, 3);
    qspi.advance_time(200_000).unwrap();
    qspi.write(4, 22 << 16, 4).unwrap();
    qspi.write(0x10, 7, 4).unwrap();
    qspi.write(0x14, 0x05002503, 4).unwrap();
    qspi.write(0x18, 0, 4).unwrap();
    assert_eq!(qspi.read(0x20, 4).unwrap(), 0x78563412);
    assert_eq!(qspi.read(0x20, 4).unwrap(), u32::MAX);
}

#[test]
fn preparing_standard_mode_without_instruction_is_not_a_chip_command() {
    let mut qspi = Qspi::default();
    qspi.write(0, 1, 4).unwrap();
    qspi.write(0x14, 0x01000400, 4).unwrap(); // HAL standard setup: address/data, no opcode.
    assert_eq!(qspi.read(8, 4).unwrap() & 32, 0);
    qspi.write(0x14, 0x106, 4).unwrap(); // Extended command supersedes the unused setup.
    qspi.write(0x14, 0x05000105, 4).unwrap();
    assert_eq!(qspi.read(0x20, 1).unwrap(), 2);
}

#[test]
fn starting_a_new_transaction_does_not_clear_sticky_completion() {
    let mut qspi = Qspi::default();
    qspi.write(0, 1, 4).unwrap();
    qspi.write(4, 22 << 16, 4).unwrap();
    qspi.write(0x14, 0x106, 4).unwrap();
    assert_ne!(qspi.read(8, 4).unwrap() & 2, 0);
    qspi.write(0x10, 7, 4).unwrap();
    qspi.write(0x14, 0x01002502, 4).unwrap();
    qspi.write(0x18, 0, 4).unwrap();
    qspi.write(0x20, 0x78563412, 4).unwrap();
    assert_eq!(qspi.read(8, 4).unwrap() & 34, 34); // Prior TCF and current BUSY.
    qspi.write(0xc, 2, 4).unwrap();
    assert_eq!(qspi.read(8, 4).unwrap() & 34, 32);
}

#[test]
fn polling_or_match_can_continue_until_abort_and_clock_gating_pauses_reads() {
    use virtual_daisy::clock::Rate;
    let rate = Rate {
        numerator: 100_000_000,
        denominator: 1,
    };
    let mut qspi = Qspi::default();
    qspi.configure_clock(Some(rate)).unwrap();
    qspi.write(0, 1 | (1 << 23) | (1 << 19), 4).unwrap(); // OR, no auto-stop.
    qspi.write(0x14, 0x106, 4).unwrap();
    qspi.write(0xc, 2, 4).unwrap();
    qspi.write(0x24, 3, 4).unwrap();
    qspi.write(0x28, 0, 4).unwrap();
    qspi.write(0x14, 0x09000105, 4).unwrap();
    qspi.advance_time(160).unwrap();
    assert_eq!(qspi.read(0x20, 1).unwrap(), 2);
    assert_eq!(qspi.read(8, 4).unwrap() & 0x3f, 0x28); // One matching bit, still busy.
    assert!(qspi.irq());
    qspi.write(0xc, 8, 4).unwrap();
    qspi.configure_clock(None).unwrap();
    qspi.advance_time(10_000).unwrap();
    assert_eq!(qspi.read(8, 4).unwrap() & 0x3f, 0x20);
    assert_eq!(qspi.next_event(), None);
    qspi.configure_clock(Some(rate)).unwrap();
    qspi.advance_time(10_159).unwrap();
    assert!(!qspi.irq());
    qspi.advance_time(10_160).unwrap();
    assert!(qspi.irq());
    qspi.write(0xc, 8, 4).unwrap();
    qspi.write(0, 3 | (1 << 23) | (1 << 19), 4).unwrap();
    assert_eq!(qspi.read(8, 4).unwrap() & 0x3f, 2);
    assert_eq!(qspi.next_event(), None);
    assert!(!qspi.irq());
}

#[test]
fn writes_to_locked_configuration_while_busy_are_ignored_not_bus_faults() {
    let mut qspi = Qspi::default();
    qspi.write(0, 1, 4).unwrap();
    qspi.write(0x10, 2, 4).unwrap();
    qspi.write(0x14, 0x0500019f, 4).unwrap();
    qspi.write(0x1c, 0x1234, 4).unwrap(); // HAL writes unused alternate bytes after CCR.
    assert_eq!(qspi.read(0x1c, 4).unwrap(), 0);
    assert_eq!(qspi.read(0x20, 4).unwrap() & 0xffffff, 0x17609d);
    qspi.write(0x1c, 0x1234, 4).unwrap();
    assert_eq!(qspi.read(0x1c, 4).unwrap(), 0x1234);
}

#[test]
fn quad_io_read_counts_alternate_mode_clocks_inside_the_chips_dummy_requirement() {
    let mut qspi = Qspi::default();
    qspi.write(0, 1, 4).unwrap();
    qspi.write(4, 22 << 16, 4).unwrap();
    qspi.write(0x14, 0x135, 4).unwrap();
    qspi.write(0x14, 0x030003c0, 4).unwrap();
    qspi.write(0x20, 0xf8, 1).unwrap(); // QPI EB requires ten total dummy/mode clocks.
    qspi.write(0x14, 0x0720efeb, 4).unwrap(); // Two alternate-byte + eight dummy clocks.
    qspi.write(0x1c, 0, 4).unwrap();
    qspi.write(0x18, 0, 4).unwrap();
    assert_eq!(qspi.read(0x20, 1).unwrap(), 0xff);
    qspi.write(0x14, 0x0718efeb, 4).unwrap(); // Two + six: reads before chip data is ready.
    qspi.write(0x1c, 0, 4).unwrap();
    assert!(qspi.write(0x18, 0, 4).is_err());
}
