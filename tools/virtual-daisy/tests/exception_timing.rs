use virtual_daisy::{clock::Rate, memory::SystemMemory};

#[test]
fn exception_frame_stores_and_busy_flash_vector_fetch_complete_before_handler_entry() {
    let mut memory = SystemMemory::default();
    let rate = Rate {
        numerator: 200_000_000,
        denominator: 1,
    };
    memory.configure_bus(rate, rate, None);
    memory.load_flash(0x08000040, 4, 0x08002001).unwrap();
    memory.flash_write_reg(4, 0x45670123, 4).unwrap();
    memory.flash_write_reg(4, 0xcdef89ab, 4).unwrap();
    memory.flash_write_reg(0xc, 0x32, 4).unwrap();
    for word in 0..8 {
        memory.write_cpu(0x08001000 + word * 4, 4, 42).unwrap();
    }
    for word in 0..8 {
        let mut value = u64::from(word + 10);
        assert_eq!(
            memory
                .cpu_access(u64::MAX - 1, 0x3001ffe0 + word * 4, 4, true, &mut value)
                .unwrap(),
            0
        );
    }
    let mut completed = false;
    for _ in 0..64 {
        let wait = memory.exception_entry(0x08000040).unwrap();
        if wait == 0 {
            completed = true;
            break;
        }
        memory.take_cpu_debt();
        memory.advance_time(memory.now() + wait).unwrap();
    }
    assert!(completed, "entry must resume without pushing another frame");
    assert_eq!(memory.now(), 100_040); // x64 program100µs, then LATENCY7+one HCLK.
    let mut vector = 0;
    assert_eq!(
        memory
            .cpu_access(u64::MAX, 0x08000040, 4, false, &mut vector)
            .unwrap(),
        0
    );
    assert_eq!(vector, 0x08002001);
    for word in 0..8 {
        assert_eq!(
            memory.read_dma(0x3001ffe0 + word * 4, 4).unwrap(),
            word + 10
        );
    }
}

#[test]
fn a_warm_instruction_vector_does_not_drain_an_unrelated_writeback_on_entry() {
    let mut memory = SystemMemory::default();
    let rate = Rate {
        numerator: 200_000_000,
        denominator: 1,
    };
    memory.configure_bus(rate, rate, None);
    memory.load_flash(0x08000040, 4, 0x08002001).unwrap();
    memory.write_scb(0xd14, 3 << 16).unwrap();
    let mut vector = 0;
    memory
        .cpu_access(u64::MAX, 0x08000040, 4, false, &mut vector)
        .unwrap();
    assert_eq!(vector, 0x08002001);
    memory.fabric.submit(1, 7, true, 1000).unwrap().unwrap();
    memory.advance_time(0).unwrap();
    memory.write_cpu(0x24000000, 4, 42).unwrap();
    memory.write_scb(0xf68, 0x24000000).unwrap();
    assert_eq!(memory.exception_entry(0x08000040).unwrap(), 0);
    assert_eq!(memory.read_dma(0x24000000, 4).unwrap(), 0);
    assert!(memory.barrier(4).unwrap() > 0);
}

#[test]
fn unstack_reads_are_latched_once_and_fresh_on_the_next_exception_return() {
    let mut memory = SystemMemory::default();
    let rate = Rate {
        numerator: 200_000_000,
        denominator: 1,
    };
    memory.configure_bus(rate, rate, None);
    for word in 0..8 {
        memory
            .write_dma(0x3001ffe0 + word * 4, 4, word + 10)
            .unwrap();
    }
    let mut complete = false;
    for _ in 0..32 {
        let wait = memory.exception_return(0x3001ffe0, 32).unwrap();
        if wait == 0 {
            complete = true;
            break;
        }
        memory.take_cpu_debt();
        memory.advance_time(memory.now() + wait).unwrap();
    }
    assert!(complete);
    assert_eq!(memory.now(), 40);
    memory.write_dma(0x3001ffe0, 4, 99).unwrap();
    let mut value = 0;
    memory
        .cpu_access(u64::MAX - 2, 0x3001ffe0, 4, false, &mut value)
        .unwrap();
    assert_eq!(value, 10);
    memory.finish_exception_return();
    assert!(memory.exception_return(0x3001ffe0, 32).unwrap() > 0);
}

#[test]
fn vector_fetch_uses_default_instruction_cache_policy_despite_mpu_xn_and_noncacheable() {
    let mut memory = SystemMemory::default();
    memory.load_flash(0x08000040, 4, 0x08002001).unwrap();
    memory.write_scb(0xd14, 3 << 16).unwrap();
    memory.write_scb(0xd98, 0).unwrap();
    memory.write_scb(0xd9c, 0x08000000).unwrap();
    // MPU region: XN, full access, normal noncacheable, 128KiB.
    memory.write_scb(0xda0, 0x13080021).unwrap();
    memory.write_scb(0xd94, 5).unwrap();
    let mut vector = 0;
    memory
        .cpu_access(u64::MAX, 0x08000040, 4, false, &mut vector)
        .unwrap();
    assert_eq!(vector, 0x08002001);
    // Changing backing bytes simulates a new vector without I-cache upkeep.
    // The vector must retain its cached default-map value despite MPU attrs.
    memory.load_flash(0x08000040, 4, 0x08003001).unwrap();
    memory
        .cpu_access(u64::MAX, 0x08000040, 4, false, &mut vector)
        .unwrap();
    assert_eq!(vector, 0x08002001);
    memory.write_scb(0xf50, 0x08000040).unwrap();
    memory
        .cpu_access(u64::MAX, 0x08000040, 4, false, &mut vector)
        .unwrap();
    assert_eq!(vector, 0x08003001);
}

#[test]
fn an_abandoned_load_does_not_serialize_exception_stacking_on_a_different_target() {
    let mut memory = SystemMemory::default();
    let rate = Rate {
        numerator: 200_000_000,
        denominator: 1,
    };
    memory.configure_bus(rate, rate, None);
    memory.inject_bus_load(7, 100_000).unwrap();
    memory.advance_time(0).unwrap();
    let mut data = 0;
    assert!(
        memory
            .cpu_access(0x10002001f000, 0x24000000, 4, false, &mut data)
            .unwrap()
            > 0
    );
    for word in 0..8 {
        let mut value = u64::from(word + 10);
        memory
            .cpu_access(u64::MAX - 1, 0x3001ffe0 + word * 4, 4, true, &mut value)
            .unwrap();
    }
    // The vector is in native ITCM, and D2 SRAM has its own service target.
    for _ in 0..32 {
        let wait = memory.exception_entry(0x6040).unwrap();
        if wait == 0 {
            memory.finish_exception_entry().unwrap();
            break;
        }
        memory.take_cpu_debt();
        memory.advance_time(memory.now() + wait).unwrap();
    }
    assert_eq!(memory.now(), 40);
    assert_eq!(memory.read_dma(0x3001ffe0, 4).unwrap(), 10);
    // Returning to the load must still wait for the occupied AXI target.
    assert!(
        memory
            .cpu_access(0x10002001f000, 0x24000000, 4, false, &mut data)
            .unwrap()
            > 0
    );
}
