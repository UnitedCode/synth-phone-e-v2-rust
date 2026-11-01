use virtual_daisy::bus::Fabric;

#[test]
fn fmc_sdram_read_and_write_service_share_one_port_in_both_directions() {
    for first_write in [false, true] {
        let mut fabric = Fabric::default();
        let first = fabric.submit(2, 5, first_write, 35).unwrap().unwrap();
        fabric.advance(0).unwrap();
        let second = fabric.submit(1, 5, !first_write, 20).unwrap().unwrap();
        let independent = fabric.submit(1, 7, true, 5).unwrap().unwrap();
        fabric.advance(5).unwrap();
        assert!(fabric.take_ready(independent));
        fabric.advance(35).unwrap();
        assert!(fabric.take_ready(first));
        assert!(!fabric.take_ready(second));
        fabric.advance(54).unwrap();
        assert!(!fabric.take_ready(second));
        fabric.advance(55).unwrap();
        assert!(fabric.take_ready(second));
    }
}

#[test]
fn sdram_direction_reservation_preserves_axi_qos_and_per_direction_credits() {
    let mut fabric = Fabric::default();
    fabric.axi.write(0x42100, 15).unwrap(); // DMA read priority.
    let cpu_read = fabric.submit(2, 5, false, 10).unwrap().unwrap();
    let cpu_write = fabric.submit(2, 5, true, 20).unwrap().unwrap();
    let dma_read = fabric.submit(1, 5, false, 30).unwrap().unwrap();
    fabric.advance(30).unwrap();
    assert!(fabric.take_ready(dma_read));
    assert!(!fabric.take_ready(cpu_read));
    assert!(!fabric.take_ready(cpu_write));
    fabric.advance(40).unwrap();
    assert!(fabric.take_ready(cpu_read));
    assert!(!fabric.take_ready(cpu_write));
    fabric.advance(60).unwrap();
    assert!(fabric.take_ready(cpu_write));
    // Completed reads must not release the still-reserved write credit.
    // All three completions must leave both channels usable.
    assert!(fabric.submit(2, 5, false, 1).unwrap().is_some());
    assert!(fabric.submit(2, 5, true, 1).unwrap().is_some());
}

#[test]
fn cpu_and_dma_opposite_direction_sdram_accesses_wait_for_shared_service() {
    use virtual_daisy::{bus::SdramTiming, clock::Rate, memory::SystemMemory};
    for cpu_write in [false, true] {
        let mut memory = SystemMemory::default();
        let rate = Rate {
            numerator: 200_000_000,
            denominator: 1,
        };
        let sdram = SdramTiming::decode(
            (2 << 4) | (3 << 7) | (2 << 10) | (1 << 13),
            0x02126471,
            1560 << 1,
            rate,
        )
        .unwrap();
        memory.configure_bus(rate, rate, Some(sdram));
        let dma = memory.dma_submit(0xc0000000, !cpu_write).unwrap().unwrap();
        memory.advance_time(0).unwrap();
        let mut value = 0x12345678;
        let context = 0x10002001f000;
        assert!(
            memory
                .cpu_access(context, 0xc0000004, 4, cpu_write, &mut value)
                .unwrap()
                > 0
        );
        let dma_finish = if cpu_write { 65 } else { 40 };
        memory.advance_time(dma_finish).unwrap();
        assert!(memory.dma_ready(dma));
        memory.begin_cpu_block();
        assert!(
            memory
                .cpu_access(context, 0xc0000004, 4, cpu_write, &mut value)
                .unwrap()
                > 0
        );
        memory.advance_time(74).unwrap();
        memory.begin_cpu_block();
        assert!(
            memory
                .cpu_access(context, 0xc0000004, 4, cpu_write, &mut value)
                .unwrap()
                > 0
        );
        memory.advance_time(75).unwrap();
        memory.begin_cpu_block();
        assert_eq!(
            memory
                .cpu_access(context, 0xc0000004, 4, cpu_write, &mut value)
                .unwrap(),
            0
        );
    }
}

#[test]
fn shared_targets_delay_cpu_and_dma_but_independent_targets_run_in_parallel() {
    let mut fabric = Fabric::default();
    fabric.axi.write(0x42100, 15).unwrap(); // D2 DMA read QoS outranks CPU.
    let cpu = fabric.submit(2, 7, false, 40).unwrap().unwrap();
    let dma = fabric.submit(1, 7, false, 10).unwrap().unwrap();
    let independent = fabric.submit(2, 3, false, 25).unwrap().unwrap();
    fabric.advance(0).unwrap();
    fabric.advance(10).unwrap();
    assert!(fabric.take_ready(dma));
    assert!(!fabric.take_ready(cpu));
    fabric.advance(25).unwrap();
    assert!(fabric.take_ready(independent));
    fabric.advance(49).unwrap();
    assert!(!fabric.take_ready(cpu));
    fabric.advance(50).unwrap();
    assert!(fabric.take_ready(cpu));
}

#[test]
fn retrying_a_multistore_does_not_repeat_an_accepted_flash_strobe() {
    use virtual_daisy::{clock::Rate, memory::SystemMemory};
    let mut memory = SystemMemory::default();
    let rate = Rate {
        numerator: 200_000_000,
        denominator: 1,
    };
    memory.configure_bus(rate, rate, None);
    memory.flash_write_reg(4, 0x45670123, 4).unwrap();
    memory.flash_write_reg(4, 0xcdef89ab, 4).unwrap();
    memory.flash_write_reg(0xc, 0x32, 4).unwrap();
    let context = (0x1000_u64 << 32) | 0x2001f000;
    let mut first = 0x11223344;
    assert!(
        memory
            .cpu_access(context, 0x08000000, 4, true, &mut first)
            .unwrap()
            > 0
    );
    memory.advance_time(5).unwrap();
    memory.begin_cpu_block();
    assert_eq!(
        memory
            .cpu_access(context, 0x08000000, 4, true, &mut first)
            .unwrap(),
        0
    );
    let mut second = 0xaabbccdd;
    assert!(
        memory
            .cpu_access(context, 0x08000004, 4, true, &mut second)
            .unwrap()
            > 0
    );
    memory.advance_time(10).unwrap();
    memory.begin_cpu_block();
    assert_eq!(
        memory
            .cpu_access(context, 0x08000000, 4, true, &mut first)
            .unwrap(),
        0
    );
    assert_eq!(
        memory
            .cpu_access(context, 0x08000004, 4, true, &mut second)
            .unwrap(),
        0
    );
    assert_eq!(memory.flash_read_reg(0x10).unwrap(), 2); // WBNE only: no duplicate STRBERR.
}

#[test]
fn dma_flash_reads_cannot_complete_before_nonvolatile_programming_and_port_service() {
    use virtual_daisy::{clock::Rate, memory::SystemMemory};
    let mut memory = SystemMemory::default();
    let rate = Rate {
        numerator: 200_000_000,
        denominator: 1,
    };
    memory.configure_bus(rate, rate, None);
    memory.flash_write_reg(0, 0x24, 4).unwrap();
    memory.flash_write_reg(4, 0x45670123, 4).unwrap();
    memory.flash_write_reg(4, 0xcdef89ab, 4).unwrap();
    memory.flash_write_reg(0xc, 0x32, 4).unwrap();
    for word in 0..8 {
        memory.write_cpu(0x08000000 + word * 4, 4, 42).unwrap();
    }
    assert_eq!(memory.dma_submit(0x08000000, false).unwrap(), None);
    memory.advance_time(100_000).unwrap();
    let ticket = memory.dma_submit(0x08000000, false).unwrap().unwrap();
    memory.advance_time(100_024).unwrap();
    assert!(!memory.dma_ready(ticket));
    memory.advance_time(100_025).unwrap();
    assert!(memory.dma_ready(ticket));
    assert_eq!(memory.read_dma(0x08000000, 4).unwrap(), 42);
}

#[test]
fn fetch_stalls_make_progress_when_the_cpu_shortens_a_block_on_retry() {
    use virtual_daisy::{clock::Rate, memory::SystemMemory};
    let mut memory = SystemMemory::default();
    let rate = Rate {
        numerator: 200_000_000,
        denominator: 1,
    };
    memory.configure_bus(rate, rate, None);
    for word in 0..32 {
        memory
            .load_flash(0x08000000 + word * 4, 4, 0xbf00bf00)
            .unwrap();
    }
    for half in 0..64 {
        memory.decode_instruction(0x08000000 + half * 2, 2).unwrap();
    }
    let mut completed = false;
    for attempt in 0..12 {
        // Renode trims the translation to fit its remaining execution budget.
        let bytes = if attempt % 2 == 0 { 96 } else { 64 };
        assert!(!memory.runtime_fetch(0x08000000, bytes).unwrap());
        let debt = memory.take_cpu_debt();
        if debt == 0 {
            completed = true;
            break;
        }
        memory.advance_time(memory.now() + debt).unwrap();
    }
    assert!(
        completed,
        "changing block extents must not restart already-serviced fetches"
    );
}

#[test]
fn native_vector_reads_are_explicit_and_busy_flash_never_returns_an_old_vector() {
    use virtual_daisy::{clock::Rate, memory::SystemMemory};
    let mut memory = SystemMemory::default();
    let rate = Rate {
        numerator: 200_000_000,
        denominator: 1,
    };
    memory.configure_bus(rate, rate, None);
    memory.load_flash(0x08000070, 4, 0x08001001).unwrap();
    let mut value = 0;
    assert_eq!(
        memory
            .cpu_access(u64::MAX, 0x08000070, 4, false, &mut value)
            .unwrap(),
        0
    );
    assert_eq!(value, 0x08001001);
    memory.flash_write_reg(4, 0x45670123, 4).unwrap();
    memory.flash_write_reg(4, 0xcdef89ab, 4).unwrap();
    memory.flash_write_reg(0xc, 0xb4, 4).unwrap();
    assert!(memory
        .cpu_access(u64::MAX, 0x08000070, 4, false, &mut value)
        .is_err());
    assert!(memory
        .cpu_access(0, 0x24000000, 4, false, &mut value)
        .is_err());
}

#[test]
fn live_flash_read_commands_share_the_banks_three_command_capacity() {
    let mut fabric = Fabric::default();
    let first = fabric.submit(2, 3, false, 25).unwrap().unwrap();
    fabric.advance(0).unwrap();
    fabric.submit(1, 3, false, 25).unwrap().unwrap();
    fabric.submit(4, 3, false, 25).unwrap().unwrap();
    assert_eq!(fabric.submit(2, 3, false, 25).unwrap(), None);
    // The independent write-command channel is not a fourth read slot.
    assert!(fabric.submit(1, 3, true, 5).unwrap().is_some());
    fabric.advance(25).unwrap();
    assert!(fabric.take_ready(first));
    assert!(fabric.submit(2, 3, false, 25).unwrap().is_some());
}

#[test]
fn architecture_reset_clears_shared_bus_configuration_and_abandoned_cpu_retries() {
    use virtual_daisy::{clock::Rate, memory::SystemMemory};
    let mut memory = SystemMemory::default();
    let rate = Rate {
        numerator: 200_000_000,
        denominator: 1,
    };
    memory.configure_bus(rate, rate, None);
    memory.advance_time(100).unwrap();
    memory.fabric.axi.write(0x42100, 15).unwrap();
    let mut word = 42;
    assert!(
        memory
            .cpu_access(0x10002001f000, 0x30000000, 4, true, &mut word)
            .unwrap()
            > 0
    );
    memory.reset_registers();
    assert_eq!(memory.fabric.axi.read(0x42100).unwrap(), 0);
    assert_eq!(memory.take_cpu_debt(), 0);
    assert!(
        memory
            .cpu_access(0x10002001f000, 0x30000000, 4, true, &mut word)
            .unwrap()
            > 0
    );
    memory.advance_time(104).unwrap();
    memory.begin_cpu_block();
    assert!(
        memory
            .cpu_access(0x10002001f000, 0x30000000, 4, true, &mut word)
            .unwrap()
            > 0
    );
    memory.advance_time(105).unwrap();
    memory.begin_cpu_block();
    assert_eq!(
        memory
            .cpu_access(0x10002001f000, 0x30000000, 4, true, &mut word)
            .unwrap(),
        0
    );
    assert_eq!(memory.read_dma(0x30000000, 4).unwrap(), 42);
}

#[test]
fn device_load_waits_are_not_abandoned_like_normal_memory_loads() {
    use virtual_daisy::{clock::Rate, memory::SystemMemory};
    for (attributes, interruptible) in [(0x03010023, false), (0x03080023, true)] {
        let mut memory = SystemMemory::default();
        let rate = Rate {
            numerator: 200_000_000,
            denominator: 1,
        };
        memory.configure_bus(rate, rate, None);
        memory.write_scb(0xd9c, 0x30000000).unwrap();
        memory.write_scb(0xda0, attributes).unwrap();
        memory.write_scb(0xd94, 5).unwrap();
        let mut value = 0;
        assert!(
            memory
                .cpu_access(0x10002001f000, 0x30000000, 4, false, &mut value)
                .unwrap()
                > 0
        );
        assert_eq!(memory.wait_interruptible, interruptible);
    }
}
