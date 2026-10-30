use virtual_daisy::memory::SystemMemory;

#[test]
fn flash_shares_the_instruction_cache_but_is_not_writable_ram() {
    let mut memory = SystemMemory::default();
    memory.load_flash(0x08000000, 4, 0x47702001).unwrap();
    memory.write_scb(0xd14, 3 << 16).unwrap();
    assert_eq!(memory.read_instruction(0x08000000, 4).unwrap(), 0x47702001);
    memory.load_flash(0x08000000, 4, 0x47702002).unwrap();
    assert_eq!(memory.read_instruction(0x08000000, 4).unwrap(), 0x47702001);
    memory.write_scb(0xf50, 0).unwrap();
    assert_eq!(memory.read_instruction(0x08000000, 4).unwrap(), 0x47702002);
    memory.write_cpu(0x08000000, 4, 0).unwrap();
    memory.write_dma(0x08000000, 4, 0).unwrap();
    assert_eq!(memory.flash_read_reg(0x10).unwrap() & (1 << 18), 1 << 18); // PG disabled: PGSERR, no data change.
    assert_eq!(memory.read_dma(0x08000000, 4).unwrap(), 0x47702002);
    assert!(memory.load_flash(0x08020000, 4, 0).is_err());
    assert!(!SystemMemory::is_ram(0x08000000, 4));
}

#[test]
fn reused_translations_must_refetch_after_mixed_flash_ram_eviction() {
    let mut memory = SystemMemory::default();
    memory.load_flash(0x08000000, 4, 0x47702001).unwrap();
    for address in [0x24000000, 0x24002000] {
        memory.write_dma(address, 4, 0x47702009).unwrap();
    }
    memory.write_scb(0xd14, 1 << 17).unwrap();
    assert_eq!(
        memory.decode_instruction(0x08000000, 4).unwrap(),
        0x47702001
    );
    assert!(!memory.runtime_fetch(0x08000000, 2).unwrap());
    memory.load_flash(0x08000000, 4, 0x47702002).unwrap();
    assert!(!memory.runtime_fetch(0x08000000, 2).unwrap()); // Cached flash still returns 1.
    for address in [0x24000000, 0x24002000] {
        memory.decode_instruction(address, 4).unwrap();
        assert!(!memory.runtime_fetch(address, 2).unwrap());
    }
    // The old translation must be rejected, not executed after its line was evicted.
    assert!(memory.runtime_fetch(0x08000000, 2).unwrap());
    assert_eq!(
        memory.decode_instruction(0x08000000, 4).unwrap(),
        0x47702002
    );
    assert!(!memory.runtime_fetch(0x08000000, 2).unwrap());
}

#[test]
fn flash_literals_use_the_separate_data_cache_and_erased_bytes_are_all_ones() {
    let mut memory = SystemMemory::default();
    assert_eq!(memory.read_cpu(0x0801fffc, 4).unwrap(), u32::MAX);
    memory.load_flash(0x08000000, 4, 0x12345678).unwrap();
    memory.write_scb(0xd14, 3 << 16).unwrap();
    assert_eq!(memory.read_cpu(0x08000000, 4).unwrap(), 0x12345678);
    memory.load_flash(0x08000000, 4, 0x87654321).unwrap();
    memory.write_scb(0xf50, 0).unwrap();
    assert_eq!(memory.read_cpu(0x08000000, 4).unwrap(), 0x12345678);
    memory.write_scb(0xf5c, 0x08000000).unwrap();
    assert_eq!(memory.read_cpu(0x08000000, 4).unwrap(), 0x87654321);
    memory.reset_registers();
    assert_eq!(memory.read_cpu(0x08000000, 4).unwrap(), 0x87654321);
}

#[test]
fn runtime_thumb32_instruction_fetch_can_cross_a_cache_line() {
    let mut memory = SystemMemory::default();
    memory.load_flash(0x0800001e, 4, 0xf800f000).unwrap(); // Thumb-2 BL halfwords.
    memory.write_scb(0xd14, 1 << 17).unwrap();
    memory.decode_instruction(0x0800001e, 4).unwrap();
    assert!(!memory.runtime_fetch(0x0800001e, 4).unwrap());
    memory.load_flash(0x08000020, 2, 0xf801).unwrap();
    assert!(!memory.runtime_fetch(0x0800001e, 4).unwrap());
    memory.write_scb(0xf58, 0x08000020).unwrap();
    assert!(memory.runtime_fetch(0x0800001e, 4).unwrap());
}

#[test]
fn generated_code_needs_both_data_clean_and_instruction_invalidation() {
    let mut memory = SystemMemory::default();
    memory.write_dma(0x24000000, 4, 0x47702001).unwrap(); // movs r0,#1; bx lr.
    memory.write_scb(0xd14, 3 << 16).unwrap();
    assert_eq!(memory.read_instruction(0x24000000, 4).unwrap(), 0x47702001);
    memory.write_cpu(0x24000000, 4, 0x47702002).unwrap();
    memory.write_scb(0xf58, 0x24000000).unwrap(); // Invalidation alone sees old RAM.
    assert_eq!(memory.read_instruction(0x24000000, 4).unwrap(), 0x47702001);
    memory.write_scb(0xf64, 0x24000000).unwrap(); // DCCMVAU clean to PoU.
    assert_eq!(memory.read_instruction(0x24000000, 4).unwrap(), 0x47702001);
    memory.write_scb(0xf58, 0x24000000).unwrap();
    assert_eq!(memory.read_instruction(0x24000000, 4).unwrap(), 0x47702002);
}

#[test]
fn icache_geometry_and_read_only_level_selection_match_the_arm_register_table() {
    let mut memory = SystemMemory::default();
    memory.write_scb(0xd84, 15).unwrap();
    assert_eq!(memory.read_scb(0xd84).unwrap(), 1); // LEVEL is read-only zero.
    assert_eq!(memory.read_scb(0xd80).unwrap(), 0xf01fe009); // 16KiB, two ways, 32B.
}

#[test]
fn shared_normal_memory_bypasses_the_default_instruction_cache() {
    let mut memory = SystemMemory::default();
    memory.write_scb(0xd9c, 0x24000000).unwrap();
    memory.write_scb(0xda0, 0x030f001d).unwrap(); // Shared WBWA 32KiB.
    memory.write_scb(0xd94, 5).unwrap();
    memory.write_scb(0xd14, 1 << 17).unwrap();
    memory.write_dma(0x24000000, 4, 1).unwrap();
    assert_eq!(memory.read_instruction(0x24000000, 4).unwrap(), 1);
    memory.write_dma(0x24000000, 4, 2).unwrap();
    assert_eq!(memory.read_instruction(0x24000000, 4).unwrap(), 2);
}

#[test]
fn dma_code_updates_need_instruction_invalidation_but_cache_disable_does_not_clean_data() {
    let mut memory = SystemMemory::default();
    memory.write_dma(0x24000000, 4, 1).unwrap();
    memory.write_scb(0xd14, 3 << 16).unwrap();
    assert_eq!(memory.read_instruction(0x24000000, 4).unwrap(), 1);
    memory.write_dma(0x24000000, 4, 2).unwrap();
    assert_eq!(memory.read_instruction(0x24000000, 4).unwrap(), 1);
    memory.write_scb(0xd14, 1 << 16).unwrap();
    assert_eq!(memory.read_instruction(0x24000000, 4).unwrap(), 2);
    memory.write_scb(0xd14, 3 << 16).unwrap();
    assert_eq!(memory.read_instruction(0x24000000, 4).unwrap(), 1);
    memory.write_scb(0xf50, 0).unwrap();
    assert_eq!(memory.read_instruction(0x24000000, 4).unwrap(), 2);
}

#[test]
fn execute_never_is_checked_even_when_instruction_caching_is_disabled() {
    let mut memory = SystemMemory::default();
    memory.write_scb(0xd9c, 0x24000000).unwrap();
    memory.write_scb(0xda0, 0x130b001d).unwrap();
    memory.write_scb(0xd94, 5).unwrap();
    assert!(memory.read_instruction(0x24000000, 4).is_err());
}

#[test]
fn invalidate_by_address_removes_the_whole_32_byte_line_only() {
    let mut memory = SystemMemory::default();
    memory.write_scb(0xd14, 1 << 17).unwrap();
    memory.write_dma(0x24000000, 4, 1).unwrap();
    memory.write_dma(0x24000020, 4, 2).unwrap();
    assert_eq!(memory.read_instruction(0x24000000, 4).unwrap(), 1);
    assert_eq!(memory.read_instruction(0x24000020, 4).unwrap(), 2);
    memory.write_cpu(0x24000000, 4, 3).unwrap(); // D-cache disabled: immediately in RAM.
    memory.write_cpu(0x24000020, 4, 4).unwrap();
    memory.write_scb(0xf58, 0x2400001f).unwrap();
    assert_eq!(memory.read_instruction(0x24000000, 4).unwrap(), 3);
    assert_eq!(memory.read_instruction(0x24000020, 4).unwrap(), 2);
}
