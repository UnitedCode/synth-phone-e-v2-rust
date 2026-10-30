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
