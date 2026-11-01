use virtual_daisy::memory::SystemMemory;
#[test]
fn alignment_policy_distinguishes_normal_memory_device_memory_and_forced_word_accesses() {
    let mut memory = SystemMemory::default();
    assert!(!memory.alignment_fault(0x20000001, 4, false).unwrap());
    assert!(memory.alignment_fault(0x20000001, 4, true).unwrap());
    assert!(!memory.alignment_fault(0x20000004, 4, true).unwrap());
    assert!(memory.alignment_fault(0x40000001, 4, false).unwrap());
    memory.write_scb(0xd14, 8).unwrap();
    assert!(memory.alignment_fault(0x20000001, 4, false).unwrap());
    assert!(memory.alignment_fault(0x20000001, 2, false).unwrap());
    assert!(!memory.alignment_fault(0x20000001, 1, false).unwrap());
}
#[test]
fn the_mpu_can_make_an_otherwise_normal_ram_address_a_device_alignment_target() {
    let mut memory = SystemMemory::default();
    memory.write_scb(0xd98, 0).unwrap();
    memory.write_scb(0xd9c, 0x30000000).unwrap();
    memory.write_scb(0xda0, 0x03000009).unwrap();
    memory.write_scb(0xd94, 5).unwrap();
    assert!(memory.alignment_fault(0x30000001, 4, false).unwrap());
    assert!(!memory.alignment_fault(0x30000021, 4, false).unwrap());
}
