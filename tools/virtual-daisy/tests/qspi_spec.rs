use virtual_daisy::qspi::Qspi;

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
