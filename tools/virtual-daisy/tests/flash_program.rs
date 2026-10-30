use virtual_daisy::memory::SystemMemory;

fn unlock(memory: &mut SystemMemory) {
    memory.flash_write_reg(4, 0x45670123, 4).unwrap();
    memory.flash_write_reg(4, 0xcdef89ab, 4).unwrap();
}

#[test]
fn flash_programming_waits_for_a_complete_word_and_nonvolatile_completion() {
    let mut memory = SystemMemory::default();
    assert_eq!(memory.flash_read_reg(0xc).unwrap(), 0x31);
    unlock(&mut memory);
    memory.flash_write_reg(0xc, 0x32 | (1 << 16), 4).unwrap(); // PG, x64, EOP interrupt.
    for word in 0..7 {
        memory
            .write_cpu(0x0801e000 + word * 4, 4, 0x11223344)
            .unwrap();
    }
    assert_eq!(memory.flash_read_reg(0x10).unwrap(), 2); // WBNE, no operation yet.
    memory.write_cpu(0x0801e01c, 4, 0xaabbccdd).unwrap();
    assert_ne!(memory.flash_read_reg(0x10).unwrap() & 1, 0);
    memory.advance_time(99_999).unwrap();
    assert!(memory.flash_wait(false) > 0);
    memory.advance_time(100_000).unwrap(); // DS12556 x64 typical scenario.
    assert_eq!(memory.read_dma(0x0801e000, 4).unwrap(), 0x11223344);
    assert_eq!(memory.read_dma(0x0801e01c, 4).unwrap(), 0xaabbccdd);
    assert_eq!(memory.flash_read_reg(0x10).unwrap(), 1 << 16);
    assert!(memory.flash_irq());
    memory.flash_write_reg(0x14, 1 << 16, 4).unwrap();
    assert!(!memory.flash_irq());
}

#[test]
fn sector_zero_erase_is_deferred_and_preserves_stale_instruction_cache() {
    let mut memory = SystemMemory::default();
    memory.load_flash(0x08000000, 4, 0x47702009).unwrap();
    memory.load_flash(0x0801fffc, 4, 0).unwrap();
    memory.write_scb(0xd14, 1 << 17).unwrap();
    assert_eq!(memory.read_instruction(0x08000000, 4).unwrap(), 0x47702009);
    unlock(&mut memory);
    memory.flash_write_reg(0xc, 0x34 | 0x80, 4).unwrap(); // SER, sector0, x64, START.
    memory.advance_time(999_999_999).unwrap();
    assert_ne!(memory.flash_read_reg(0x10).unwrap() & 1, 0);
    memory.advance_time(1_000_000_000).unwrap();
    assert_eq!(memory.read_dma(0x08000000, 4).unwrap(), u32::MAX);
    assert_eq!(memory.read_dma(0x0801fffc, 4).unwrap(), u32::MAX);
    assert_eq!(memory.read_instruction(0x08000000, 4).unwrap(), 0x47702009);
    memory.write_scb(0xf50, 0).unwrap();
    assert_eq!(memory.read_instruction(0x08000000, 4).unwrap(), u32::MAX);
}

#[test]
fn force_write_pads_missing_bytes_and_duplicate_strobes_keep_the_latest_byte() {
    let mut memory = SystemMemory::default();
    unlock(&mut memory);
    memory.flash_write_reg(0xc, 0x32, 4).unwrap();
    memory.write_cpu(0x08000000, 1, 0x12).unwrap();
    memory.write_cpu(0x08000000, 1, 0x34).unwrap();
    assert_eq!(memory.flash_read_reg(0x10).unwrap(), (1 << 19) | 2);
    memory.flash_write_reg(0xc, 0x72, 4).unwrap();
    assert_eq!(memory.flash_read_reg(0xc).unwrap() & 0x40, 0);
    memory.advance_time(100_000).unwrap();
    assert_eq!(memory.read_dma(0x08000000, 4).unwrap(), 0xffffff34);
    assert_eq!(memory.read_dma(0x0800001c, 4).unwrap(), u32::MAX);
    assert!(memory.write_cpu(0x08000004, 4, 0).is_err()); // ECC rewrite unsupported, never silently certified.
}

#[test]
fn flash_queue_backpressure_and_maximum_duration_preserve_deferred_writes() {
    let mut memory = SystemMemory::default();
    memory.set_flash_maximum(true).unwrap();
    unlock(&mut memory);
    memory.flash_write_reg(0xc, 0x32, 4).unwrap();
    for word in 0..16 {
        memory.write_cpu(0x08000000 + word * 4, 4, word).unwrap();
    }
    assert_eq!(memory.flash_wait(true), 200_000);
    assert!(memory.write_cpu(0x08000040, 4, 0).is_err());
    memory.advance_time(200_000).unwrap();
    assert_eq!(memory.read_dma(0x08000000, 4).unwrap(), 0);
    assert_eq!(memory.flash_wait(true), 0);
    memory.advance_time(400_000).unwrap();
    assert_eq!(memory.read_dma(0x0800003c, 4).unwrap(), 15);
}

#[test]
fn wrong_unlock_keys_poison_the_lock_until_reset_and_lock_only_sets_lock() {
    let mut memory = SystemMemory::default();
    assert!(memory.flash_write_reg(4, 0x45670123, 2).is_err());
    assert!(memory.flash_write_reg(4, 0x45670123, 4).is_err());
    memory.reset_registers();
    unlock(&mut memory);
    memory.flash_write_reg(0xc, 0x32, 4).unwrap();
    memory.flash_write_reg(0xc, 1, 4).unwrap();
    assert_eq!(memory.flash_read_reg(0xc).unwrap(), 0x33);
}

#[test]
fn missing_program_enable_and_inconsistent_word_addresses_require_flag_clear() {
    let mut memory = SystemMemory::default();
    memory.write_cpu(0x08000000, 4, 0).unwrap();
    assert_eq!(memory.flash_read_reg(0x10).unwrap(), 1 << 18);
    unlock(&mut memory);
    memory.flash_write_reg(0xc, 0x32, 4).unwrap();
    memory.flash_write_reg(0x14, 1 << 18, 4).unwrap();
    memory.write_cpu(0x08000000, 4, 0).unwrap();
    memory.write_cpu(0x08000020, 4, 0).unwrap();
    assert_eq!(memory.flash_read_reg(0x10).unwrap() & (1 << 21), 1 << 21);
    memory.write_cpu(0x08000004, 4, 0).unwrap();
    assert_eq!(
        memory.flash_read_reg(0x10).unwrap() & ((1 << 18) | 2),
        1 << 18
    ); // PGSERR flushed partial word.
}

#[test]
fn clearing_pg_discards_partial_data_but_does_not_cancel_a_queued_word() {
    let mut memory = SystemMemory::default();
    unlock(&mut memory);
    memory.flash_write_reg(0xc, 0x32, 4).unwrap();
    for word in 0..8 {
        memory.write_cpu(0x08000000 + word * 4, 4, word).unwrap();
    }
    memory.write_cpu(0x08000020, 4, 0).unwrap();
    memory.flash_write_reg(0xc, 0x30, 4).unwrap();
    assert_eq!(memory.flash_read_reg(0x10).unwrap() & 2, 0);
    memory.advance_time(100_000).unwrap();
    assert_eq!(memory.read_dma(0x0800001c, 4).unwrap(), 7);
    assert_eq!(memory.read_dma(0x08000020, 4).unwrap(), u32::MAX);
}

#[test]
fn bank_erase_wins_over_sector_selection_in_the_declared_mass_time_scenario() {
    let mut memory = SystemMemory::default();
    memory.load_flash(0x08000000, 4, 0).unwrap();
    unlock(&mut memory);
    memory.flash_write_reg(0xc, 0x7bc, 4).unwrap();
    memory.advance_time(4_999_999_999).unwrap();
    assert_ne!(memory.flash_read_reg(0x10).unwrap() & 1, 0);
    memory.advance_time(5_000_000_000).unwrap();
    assert_eq!(memory.read_dma(0x08000000, 4).unwrap(), u32::MAX);
}

#[test]
fn forcing_an_all_ones_partial_buffer_does_not_start_programming() {
    let mut memory = SystemMemory::default();
    unlock(&mut memory);
    memory.flash_write_reg(0xc, 0x32, 4).unwrap();
    memory.write_cpu(0x08000000, 1, 0xff).unwrap();
    memory.flash_write_reg(0xc, 0x72, 4).unwrap();
    assert_eq!(memory.flash_read_reg(0x10).unwrap(), 2); // WBNE only; FW has no effect.
    memory.advance_time(1_000_000).unwrap();
    assert_eq!(memory.flash_read_reg(0x10).unwrap(), 2);
    assert!(memory.stats().contains("flash_programs=0\n"));
}

#[test]
fn unsupported_unaligned_program_transactions_do_not_invent_hardware_error_flags() {
    let mut memory = SystemMemory::default();
    unlock(&mut memory);
    memory.flash_write_reg(0xc, 0x32, 4).unwrap();
    assert!(memory.write_cpu(0x08000001, 4, 0).is_err());
    assert_eq!(memory.flash_read_reg(0x10).unwrap(), 0);
}
