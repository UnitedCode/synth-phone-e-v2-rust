use virtual_daisy::{clock::Rate, memory::SystemMemory};

#[test]
fn cache_clean_reaches_dma_only_after_its_shared_write_port_completes() {
    let mut memory = SystemMemory::default();
    let rate = Rate {
        numerator: 200_000_000,
        denominator: 1,
    };
    memory.configure_bus(rate, rate, None);
    memory.write_dma(0x24000000, 4, 0x11223344).unwrap();
    memory.write_scb(0xd14, 1 << 16).unwrap();
    memory.write_cpu(0x24000000, 4, 0x55667788).unwrap();
    // A DMA write already owns the AXI SRAM write channel for 100ns.
    memory.fabric.submit(1, 7, true, 100).unwrap().unwrap();
    memory.advance_time(0).unwrap();
    memory.write_scb(0xf68, 0x24000000).unwrap();
    assert_eq!(memory.read_dma(0x24000000, 4).unwrap(), 0x11223344);
    memory.advance_time(119).unwrap();
    assert_eq!(memory.read_dma(0x24000000, 4).unwrap(), 0x11223344);
    // Four 64-bit HCLK beats transfer the 32-byte dirty line after the DMA write.
    memory.advance_time(120).unwrap();
    assert_eq!(memory.read_dma(0x24000000, 4).unwrap(), 0x55667788);
}

#[test]
fn dsb_waits_for_earlier_clean_snapshots_and_later_cached_stores_survive() {
    let mut memory = SystemMemory::default();
    let rate = Rate {
        numerator: 200_000_000,
        denominator: 1,
    };
    memory.configure_bus(rate, rate, None);
    memory.write_scb(0xd14, 1 << 16).unwrap();
    memory.write_cpu(0x24000000, 4, 42).unwrap();
    memory.write_scb(0xf68, 0x24000000).unwrap();
    memory.write_cpu(0x24000000, 4, 99).unwrap();
    assert!(memory.barrier(4).unwrap() > 0);
    memory.advance_time(20).unwrap();
    assert_eq!(memory.barrier(4).unwrap(), 0);
    assert_eq!(memory.read_dma(0x24000000, 4).unwrap(), 42);
    assert_eq!(memory.read_cpu(0x24000000, 4).unwrap(), 99);
    memory.write_scb(0xf68, 0x24000000).unwrap();
    assert!(memory.barrier(4).unwrap() > 0);
    memory.advance_time(40).unwrap();
    assert_eq!(memory.barrier(4).unwrap(), 0);
    assert_eq!(memory.read_dma(0x24000000, 4).unwrap(), 99);
}

#[test]
fn a_full_four_way_set_queues_one_dirty_victim_without_exposing_it_early() {
    let mut memory = SystemMemory::default();
    let rate = Rate {
        numerator: 200_000_000,
        denominator: 1,
    };
    memory.configure_bus(rate, rate, None);
    memory.write_scb(0xd14, 1 << 16).unwrap();
    for line in 0..4 {
        memory
            .write_cpu(0x24000000 + line * 0x1000, 4, 42 + line)
            .unwrap();
    }
    memory.write_cpu(0x24004000, 4, 99).unwrap();
    for line in 0..4 {
        assert_eq!(memory.read_dma(0x24000000 + line * 0x1000, 4).unwrap(), 0);
    }
    memory.advance_time(19).unwrap();
    for line in 0..4 {
        assert_eq!(memory.read_dma(0x24000000 + line * 0x1000, 4).unwrap(), 0);
    }
    memory.advance_time(20).unwrap();
    let written = (0..4)
        .filter(|&line| memory.read_dma(0x24000000 + line * 0x1000, 4).unwrap() == 42 + line)
        .count();
    assert_eq!(written, 1); // Geometry determines the count; no silicon victim sequence asserted.
    assert_eq!(memory.read_dma(0x24004000, 4).unwrap(), 0);
}

#[test]
fn dmb_orders_a_following_memory_access_without_waiting_at_the_barrier() {
    let mut memory = SystemMemory::default();
    let rate = Rate {
        numerator: 200_000_000,
        denominator: 1,
    };
    memory.configure_bus(rate, rate, None);
    memory.write_scb(0xd14, 1 << 16).unwrap();
    memory.write_cpu(0x24000000, 4, 42).unwrap();
    memory.write_scb(0xf68, 0x24000000).unwrap();
    assert_eq!(memory.barrier(5).unwrap(), 0);
    assert_eq!(memory.read_dma(0x24000000, 4).unwrap(), 0);
    // Even a cached subsequent load cannot pass the pending clean in this
    // conservative ordering scenario. MMIO/TCM store buffering is not modeled.
    assert!(memory.access_wait(0x24000000, 4, false, false).unwrap() > 0);
    memory.advance_time(20).unwrap();
    assert_eq!(memory.access_wait(0x24000000, 4, false, false).unwrap(), 0);
    assert_eq!(memory.read_dma(0x24000000, 4).unwrap(), 42);
}
