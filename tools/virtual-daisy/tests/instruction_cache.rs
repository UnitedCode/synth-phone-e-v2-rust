use virtual_daisy::memory::SystemMemory;

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
