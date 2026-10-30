use virtual_daisy::bus::Fabric;

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
