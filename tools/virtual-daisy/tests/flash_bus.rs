use virtual_daisy::bus::{FlashAdmission, FlashReadPipeline};

#[test]
fn flash_read_queue_has_one_active_and_two_waiting_commands_and_reuses_256_bit_buffer() {
    let mut flash = FlashReadPipeline::new(4).unwrap(); // ACR LATENCY4: five interface clocks.
    assert_eq!(flash.submit(0x08000000).unwrap(), FlashAdmission::Queued);
    assert_eq!(flash.submit(0x08000020).unwrap(), FlashAdmission::Queued);
    assert_eq!(flash.submit(0x08000040).unwrap(), FlashAdmission::Queued);
    assert_eq!(
        flash.submit(0x08000060).unwrap(),
        FlashAdmission::Backpressure
    );
    assert!(flash.advance(4).is_empty());
    assert_eq!(flash.advance(1), vec![0x08000000]);
    assert_eq!(flash.submit(0x08000060).unwrap(), FlashAdmission::Queued);
    assert_eq!(flash.advance(15), vec![0x08000020, 0x08000040, 0x08000060]);
    // Same 32-byte flash word needs no additional nonvolatile read operation.
    assert_eq!(flash.submit(0x0800007c).unwrap(), FlashAdmission::BufferHit);
    assert_eq!(flash.submit(0x08000080).unwrap(), FlashAdmission::Queued);
    assert_eq!(flash.advance(5), vec![0x08000080]);
    assert!(flash.submit(0x08020000).is_err());
}

#[test]
fn cpu_and_dma_reuse_the_live_bank_buffer_only_for_consecutive_flashwords() {
    use virtual_daisy::{clock::Rate, memory::SystemMemory};
    let mut memory = SystemMemory::default();
    let rate = Rate {
        numerator: 200_000_000,
        denominator: 1,
    };
    memory.configure_bus(rate, rate, None);
    memory.flash_write_reg(0, 0x24, 4).unwrap(); // LATENCY=4: first data after 5 HCLK.
    memory.load_flash(0x08000000, 4, 42).unwrap();
    let first = memory.dma_submit(0x08000000, false).unwrap().unwrap();
    memory.advance_time(24).unwrap();
    assert!(!memory.dma_ready(first));
    memory.advance_time(25).unwrap();
    assert!(memory.dma_ready(first));
    let same = memory.dma_submit(0x08000004, false).unwrap().unwrap();
    memory.advance_time(29).unwrap();
    assert!(!memory.dma_ready(same));
    memory.advance_time(30).unwrap();
    assert!(memory.dma_ready(same)); // Buffer hit needs its bus beat, no NVM read.
    let other = memory.dma_submit(0x08000020, false).unwrap().unwrap();
    memory.advance_time(55).unwrap();
    assert!(memory.dma_ready(other));
    let mut value = 0;
    assert!(
        memory
            .cpu_access(0x10002001f000, 0x08000024, 4, false, &mut value)
            .unwrap()
            > 0
    );
    memory.advance_time(60).unwrap();
    memory.begin_cpu_block();
    assert_eq!(
        memory
            .cpu_access(0x10002001f000, 0x08000024, 4, false, &mut value)
            .unwrap(),
        0
    );
    let evicted = memory.dma_submit(0x08000000, false).unwrap().unwrap();
    memory.advance_time(84).unwrap();
    assert!(!memory.dma_ready(evicted));
    memory.advance_time(85).unwrap();
    assert!(memory.dma_ready(evicted));
    assert_eq!(memory.read_dma(0x08000000, 4).unwrap(), 42);
}

#[test]
fn queued_live_reads_look_up_the_buffer_after_earlier_grants_replace_it() {
    let mut fabric = virtual_daisy::bus::Fabric::default();
    let warm = fabric.submit_flash(2, 0x08000000, 25, 5).unwrap().unwrap();
    fabric.advance(25).unwrap();
    assert!(fabric.take_ready(warm));
    let other = fabric.submit_flash(2, 0x08000020, 25, 5).unwrap().unwrap();
    // This address is warm at submission, but the earlier request replaces it.
    let original = fabric.submit_flash(2, 0x08000004, 25, 5).unwrap().unwrap();
    fabric.advance(50).unwrap();
    assert!(fabric.take_ready(other));
    fabric.advance(74).unwrap();
    assert!(!fabric.take_ready(original));
    fabric.advance(75).unwrap();
    assert!(fabric.take_ready(original));
    assert_eq!(fabric.flash_buffer_hits, 0);
    assert_eq!(fabric.flash_buffer_misses, 3);
}

#[test]
fn flash_programming_invalidates_the_live_read_buffer() {
    use virtual_daisy::{clock::Rate, memory::SystemMemory};
    let mut memory = SystemMemory::default();
    let rate = Rate {
        numerator: 200_000_000,
        denominator: 1,
    };
    memory.configure_bus(rate, rate, None);
    memory.flash_write_reg(0, 0x24, 4).unwrap();
    let first = memory.dma_submit(0x08000000, false).unwrap().unwrap();
    memory.advance_time(25).unwrap();
    assert!(memory.dma_ready(first));
    memory.flash_write_reg(4, 0x45670123, 4).unwrap();
    memory.flash_write_reg(4, 0xcdef89ab, 4).unwrap();
    memory.flash_write_reg(0xc, 0x32, 4).unwrap();
    for word in 0..8 {
        memory.write_cpu(0x08000100 + word * 4, 4, 42).unwrap();
    }
    assert!(memory.dma_submit(0x08000000, false).unwrap().is_none());
    memory.advance_time(100_025).unwrap();
    let next = memory.dma_submit(0x08000000, false).unwrap().unwrap();
    memory.advance_time(100_049).unwrap();
    assert!(!memory.dma_ready(next));
    memory.advance_time(100_050).unwrap();
    assert!(memory.dma_ready(next));
}
