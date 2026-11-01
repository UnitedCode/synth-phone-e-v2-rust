use virtual_daisy::{bus::SdramTiming, clock::Rate, memory::SystemMemory};

fn memory(refresh: u32) -> SystemMemory {
    let mut memory = SystemMemory::default();
    configure(&mut memory, refresh);
    memory
}
fn configure(memory: &mut SystemMemory, refresh: u32) {
    let rate = Rate {
        numerator: 200_000_000,
        denominator: 1,
    };
    // 32-bit, 8 column bits, 11 row bits, two internal banks, SDCLK100MHz.
    // CAS3/RPIPE1, TRAS5/TRC7/TWR3/TRP2/TRCD3.
    let timing = SdramTiming::decode(
        (2 << 4) | (3 << 7) | (2 << 10) | (1 << 13),
        0x02126471,
        refresh << 1,
        rate,
    )
    .unwrap();
    memory.configure_bus(rate, rate, Some(timing));
}
fn response(memory: &mut SystemMemory, address: u32, write: bool, finish: u64) {
    let ticket = memory.dma_submit(address, write).unwrap().unwrap();
    memory.advance_time(finish - 1).unwrap();
    assert!(
        !memory.dma_ready(ticket),
        "response before {finish}ns at {address:#x}"
    );
    memory.advance_time(finish).unwrap();
    assert!(
        memory.dma_ready(ticket),
        "response missing at {finish}ns at {address:#x}"
    );
}

#[test]
fn sdram_dma_waits_for_cold_rows_hits_conflicts_and_independent_banks() {
    let mut memory = memory(0);
    response(&mut memory, 0xc0000000, false, 65); // ACT + TRCD + CAS/RPIPE
    configure(&mut memory, 0); // Periodic clock sync must retain open rows.
    response(&mut memory, 0xc0000004, false, 100); // Same row, no ACT.
    response(&mut memory, 0xc0000400, false, 185); // New row: PRE + ACT.
    response(&mut memory, 0xc0200000, false, 250); // Bank1, cold.
    response(&mut memory, 0xc0000404, false, 285); // Bank0 row remains open.
}

#[test]
fn sdram_write_recovery_prevents_precharging_the_previous_row_early() {
    let mut memory = memory(0);
    response(&mut memory, 0xc0000000, true, 40); // ACT0, WRITE30.
                                                 // Last write data30 + TWR30 -> PRE60, ACT80, WRITE110, finish120.
    response(&mut memory, 0xc0000400, true, 120);
}

#[test]
fn periodic_refresh_has_priority_and_does_not_block_an_unrelated_memory_target() {
    let mut memory = memory(41); // COUNT+1=42 SDCLK periods =420ns.
    memory.advance_time(420).unwrap();
    let read = memory.dma_submit(0xc0000000, false).unwrap().unwrap();
    let other = memory.dma_submit(0x24000000, true).unwrap().unwrap();
    memory.advance_time(425).unwrap();
    assert!(memory.dma_ready(other));
    memory.advance_time(554).unwrap();
    assert!(!memory.dma_ready(read));
    memory.advance_time(555).unwrap(); // Refresh70 + cold row65.
    assert!(memory.dma_ready(read));
}

#[test]
fn overdue_refresh_sets_the_fmc_status_flag_and_cre_clears_it_without_restarting_the_timer() {
    use virtual_daisy::{daisy_bus_load, daisy_bus_sync, daisy_fmc_irq, Peripheral};
    let mut arch = Peripheral::new(12);
    let mut rcc = Peripheral::new(13); // HSI64, SDRAM clock32MHz.
    rcc.write(0xd4, 1 << 12).unwrap();
    let mut fmc = Peripheral::new(1);
    fmc.write(0x140, (2 << 4) | (3 << 7) | (2 << 10)).unwrap();
    fmc.write(0x148, 0x02126471).unwrap();
    fmc.write(0x154, (41 << 1) | (1 << 14)).unwrap();
    unsafe {
        assert_eq!(daisy_bus_sync(&mut arch, &mut rcc, &mut fmc, 0), 0);
        assert_eq!(daisy_bus_load(&mut arch, 5, 3000, 0), 0);
        assert_eq!(daisy_bus_sync(&mut arch, &mut rcc, &mut fmc, 2625), 0);
    }
    assert_eq!(fmc.read(0x158).unwrap() & 1, 1); // Second pulse; first still pending.
    assert_eq!(unsafe { daisy_fmc_irq(&mut fmc) }, 1);
    fmc.write(0x154, 41 << 1).unwrap();
    assert_eq!(
        unsafe { daisy_fmc_irq(&mut fmc) },
        0,
        "REIE gates only the IRQ"
    );
    fmc.write(0x154, (41 << 1) | (1 << 14) | 1).unwrap();
    unsafe {
        assert_eq!(daisy_bus_sync(&mut arch, &mut rcc, &mut fmc, 2700), 0);
    }
    assert_eq!(fmc.read(0x158).unwrap() & 1, 0);
    assert_eq!(fmc.read(0x154).unwrap() & 1, 0, "CRE is write-only");
    assert_eq!(unsafe { daisy_fmc_irq(&mut fmc) }, 0);
    unsafe {
        assert_eq!(daisy_bus_sync(&mut arch, &mut rcc, &mut fmc, 3500), 0);
    }
    assert_eq!(fmc.read(0x158).unwrap() & 1, 0);
    unsafe {
        assert_eq!(daisy_bus_load(&mut arch, 5, 3000, 0), 0);
        assert_eq!(daisy_bus_sync(&mut arch, &mut rcc, &mut fmc, 5250), 0);
    }
    assert_eq!(
        fmc.read(0x158).unwrap() & 1,
        1,
        "timer phase survived CRE/REIE writes"
    );
}

#[test]
fn self_refresh_stops_the_external_refresh_timer_and_normal_exit_waits_txsr() {
    use virtual_daisy::{daisy_bus_sync, daisy_dma_port, Peripheral};
    let mut arch = Peripheral::new(12);
    let mut rcc = Peripheral::new(13);
    rcc.write(0xd4, 1 << 12).unwrap();
    let mut fmc = Peripheral::new(1);
    fmc.write(0x140, (2 << 4) | (3 << 7) | (2 << 10) | (1 << 13))
        .unwrap();
    fmc.write(0x148, 0x02126471).unwrap();
    fmc.write(0x154, 41 << 1).unwrap();
    fmc.write(0x150, 0x15).unwrap(); // CTB1 + self-refresh.
    unsafe {
        assert_eq!(daisy_bus_sync(&mut arch, &mut rcc, &mut fmc, 0), 0);
    }
    unsafe {
        assert_eq!(daisy_bus_sync(&mut arch, &mut rcc, &mut fmc, 10000), 0);
    }
    assert_eq!(fmc.read(0x158).unwrap(), 2); // MODES1=01; no external RE.
    fmc.write(0x150, 0x10).unwrap();
    unsafe {
        assert_eq!(daisy_bus_sync(&mut arch, &mut rcc, &mut fmc, 10000), 0);
    }
    assert_eq!(fmc.read(0x158).unwrap(), 0);
    let ticket = unsafe { daisy_dma_port(&mut arch, 0xc0000000, 0, 0) };
    assert!((2..u64::MAX - 1).contains(&ticket));
    unsafe {
        assert_eq!(daisy_bus_sync(&mut arch, &mut rcc, &mut fmc, 10453), 0);
        assert_eq!(daisy_dma_port(&mut arch, 0xc0000000, 0, ticket), ticket);
        assert_eq!(daisy_bus_sync(&mut arch, &mut rcc, &mut fmc, 10454), 0);
        assert_eq!(daisy_dma_port(&mut arch, 0xc0000000, 0, ticket), 0);
    }
}

#[test]
fn manual_precharge_refresh_count_and_mode_register_quiet_time_delay_data_service() {
    let mut ignored = memory(0);
    let mut memory = memory(0);
    memory.fabric.sdram_command(0x12).unwrap(); // PALL20ns
    memory.fabric.sdram_command(0x13 | (7 << 5)).unwrap(); // Eight TRC7 refreshes=560ns
    memory.fabric.sdram_command(0x14 | (0x30 << 9)).unwrap(); // TMRD2=20ns
    response(&mut memory, 0xc0000000, false, 665); // Commands600 + cold read65.
    assert!(memory.fabric.sdram_report().contains("sdram_refreshes=8\n"));
    assert_eq!(
        memory.fabric.sdram_status() & !31,
        0,
        "H7 SDSR bit5 is reserved"
    );
    ignored.fabric.sdram_command(2).unwrap(); // No CTB bits: ignored.
    ignored.fabric.sdram_command(0x0a).unwrap(); // Unpopulated bank2: ignored.
    response(&mut ignored, 0xc0000000, false, 65);
}

#[test]
fn power_down_keeps_controller_refresh_running_and_access_returns_to_normal_mode() {
    let mut memory = memory(41);
    memory.fabric.sdram_command(0x16).unwrap();
    memory.advance_time(420).unwrap();
    assert_eq!(memory.fabric.sdram_status(), 4);
    assert!(memory.fabric.sdram_report().contains("sdram_refreshes=1\n"));
    let ticket = memory.dma_submit(0xc0000000, false).unwrap().unwrap();
    memory.advance_time(1000).unwrap();
    assert!(memory.dma_ready(ticket));
    assert_eq!(memory.fabric.sdram_status(), 0);
}

#[test]
fn fmc_write_protection_ignores_cpu_and_dma_stores_without_corrupting_memory() {
    let mut memory = memory(0);
    memory.write_cpu(0xc0000000, 4, 0x11223344).unwrap();
    let rate = Rate {
        numerator: 200_000_000,
        denominator: 1,
    };
    let protected = SdramTiming::decode(
        (2 << 4) | (3 << 7) | (2 << 10) | (1 << 9),
        0x02126471,
        0,
        rate,
    )
    .unwrap();
    memory.configure_bus(rate, rate, Some(protected));
    memory.write_cpu(0xc0000000, 4, 0xaabbccdd).unwrap();
    memory.write_dma(0xc0000000, 4, 0x88776655).unwrap();
    assert_eq!(memory.read_dma(0xc0000000, 4).unwrap(), 0x11223344);
    configure(&mut memory, 0);
    memory.write_dma(0xc0000000, 4, 0x55667788).unwrap();
    assert_eq!(memory.read_cpu(0xc0000000, 4).unwrap(), 0x55667788);
}

#[test]
fn fmc_reset_restores_documented_sdram_control_and_timing_registers() {
    let mut fmc = virtual_daisy::Peripheral::new(1);
    for offset in [0x140, 0x144] {
        assert_eq!(fmc.read(offset).unwrap(), 0x2d0);
    }
    for offset in [0x148, 0x14c] {
        assert_eq!(fmc.read(offset).unwrap(), 0x0fffffff);
    }
    fmc.write(0x140, 0).unwrap();
    fmc.write(0x148, 0).unwrap();
    fmc.reset();
    assert_eq!(fmc.read(0x140).unwrap(), 0x2d0);
    assert_eq!(fmc.read(0x148).unwrap(), 0x0fffffff);
}

#[test]
fn read_ahead_fifo_waits_for_prefetched_data_consumes_tags_and_flushes_on_write() {
    let mut memory = memory(0);
    let rate = Rate {
        numerator: 200_000_000,
        denominator: 1,
    };
    let timing = SdramTiming::decode(
        (2 << 4) | (3 << 7) | (2 << 10) | (1 << 13) | (1 << 12),
        0x02126471,
        0,
        rate,
    )
    .unwrap();
    memory.configure_bus(rate, rate, Some(timing));
    response(&mut memory, 0xc0000000, false, 65);
    // CAS3 anticipates four additional words; first is ready75ns. A FIFO
    // response uses the declared one-kernel-clock acknowledgement scenario.
    response(&mut memory, 0xc0000004, false, 80);
    // Physical read-ahead continues through105ns; a following WRITE must drain it.
    response(&mut memory, 0xc0000008, true, 115);
    memory.write_dma(0xc0000008, 4, 0x99887766).unwrap();
    response(&mut memory, 0xc0000008, false, 150); // Write flushed the old tag.
    assert_eq!(memory.read_dma(0xc0000008, 4).unwrap(), 0x99887766);
    response(&mut memory, 0xc000000c, false, 165); // Consume the new prefetched tag.
    response(&mut memory, 0xc000000c, false, 225); // Consumed tag misses; drain tail190 first.
}

#[test]
fn read_ahead_does_not_cross_rows_and_refresh_discards_prefetched_tags() {
    let rate = Rate {
        numerator: 200_000_000,
        denominator: 1,
    };
    for refresh in [0, 41] {
        let mut memory = memory(refresh);
        let timing = SdramTiming::decode(
            (2 << 4) | (3 << 7) | (2 << 10) | (1 << 13) | (1 << 12),
            0x02126471,
            refresh << 1,
            rate,
        )
        .unwrap();
        memory.configure_bus(rate, rate, Some(timing));
        if refresh == 0 {
            response(&mut memory, 0xc00003fc, false, 65);
            response(&mut memory, 0xc0000400, false, 150);
        } else {
            response(&mut memory, 0xc0000000, false, 65);
            memory.advance_time(420).unwrap();
            response(&mut memory, 0xc0000004, false, 575);
        }
    }
}

#[test]
fn full_read_ahead_fifo_discards_oldest_unconsumed_words() {
    let mut memory = memory(0);
    let rate = Rate {
        numerator: 200_000_000,
        denominator: 1,
    };
    let timing = SdramTiming::decode(
        (2 << 4) | (3 << 7) | (2 << 10) | (1 << 13) | (1 << 12),
        0x02126471,
        0,
        rate,
    )
    .unwrap();
    memory.configure_bus(rate, rate, Some(timing));
    response(&mut memory, 0xc0000000, false, 65);
    response(&mut memory, 0xc0000020, false, 140);
    // RM revision history corrects FIFO to six64-bit entries. Under the
    // declared paired-word validity scenario, these eight prefetched words
    // occupy six entries; the oldest word still fits.
    response(&mut memory, 0xc0000004, false, 145);
    response(&mut memory, 0xc0000040, false, 215);
    response(&mut memory, 0xc0000008, false, 290); // Oldest remaining line evicted.
}

#[test]
fn stopped_fmc_clock_does_not_silently_service_new_memory_transactions() {
    use virtual_daisy::{daisy_bus_sync, daisy_dma_port, Peripheral};
    let mut arch = Peripheral::new(12);
    let mut rcc = Peripheral::new(13);
    let mut fmc = Peripheral::new(1);
    fmc.write(0x140, (2 << 4) | (3 << 7) | (2 << 10)).unwrap();
    fmc.write(0x148, 0x02126471).unwrap();
    unsafe {
        assert_eq!(daisy_bus_sync(&mut arch, &mut rcc, &mut fmc, 0), 0);
        assert_eq!(daisy_dma_port(&mut arch, 0xc0000000, 0, 0), u64::MAX);
        rcc.write(0xd4, 1 << 12).unwrap();
        assert_eq!(daisy_bus_sync(&mut arch, &mut rcc, &mut fmc, 0), 0);
        let ticket = daisy_dma_port(&mut arch, 0xc0000000, 0, 0);
        assert!((2..u64::MAX - 1).contains(&ticket));
        assert_eq!(daisy_bus_sync(&mut arch, &mut rcc, &mut fmc, 1000), 0);
        assert_eq!(daisy_dma_port(&mut arch, 0xc0000000, 0, ticket), 0);
        rcc.write(0xd4, 0).unwrap();
        assert_eq!(daisy_bus_sync(&mut arch, &mut rcc, &mut fmc, 1000), 0);
        assert_eq!(daisy_dma_port(&mut arch, 0xc0000000, 0, 0), u64::MAX);
    }
}

#[test]
fn configured_sdram_address_overflow_is_an_axi_bus_error_for_cpu_and_dma() {
    use virtual_daisy::{
        daisy_bus_sync, daisy_cpu_access, daisy_dma_port, daisy_memory_read, daisy_memory_write,
        Peripheral,
    };
    let mut arch = Peripheral::new(12);
    let mut rcc = Peripheral::new(13);
    let mut fmc = Peripheral::new(1);
    rcc.write(0xd4, 1 << 12).unwrap();
    fmc.write(0x140, (2 << 4) | (3 << 7) | (2 << 10)).unwrap();
    fmc.write(0x148, 0x02126471).unwrap();
    unsafe {
        assert_eq!(daisy_bus_sync(&mut arch, &mut rcc, &mut fmc, 0), 0);
        let mut word = 0_u64;
        for write in [0, 1] {
            assert_eq!(
                daisy_cpu_access(&mut arch, 0x10002001f000, 0xc0400000, 4, &mut word, write),
                u64::MAX - 1
            );
            assert_eq!(
                daisy_dma_port(&mut arch, 0xc0400000, write, 0),
                u64::MAX - 1
            );
        }
        assert_eq!(daisy_memory_read(&mut arch, 0xc0400000, 4, 0) >> 32, 2);
        assert_eq!(daisy_memory_write(&mut arch, 0xc0400000, 4, 123, 0), 2);
    }
}
