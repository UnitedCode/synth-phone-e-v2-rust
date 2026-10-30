use virtual_daisy::{daisy_dma_irqs, daisy_dma_request, Peripheral};

#[test]
fn pending_dma_descriptor_does_not_decrement_ndtr_or_raise_completion_irq() {
    let mut dma = Peripheral::new(6);
    dma.write(0x14, 1).unwrap();
    dma.write(0x18, 0x40015820).unwrap();
    dma.write(0x1c, 0x30000000).unwrap();
    dma.write(0x10, 1 | 16 | (2 << 11) | (2 << 13)).unwrap();
    let expected = (0x40015820_u64 << 32) | 0x30000000;
    assert_eq!(
        unsafe { virtual_daisy::daisy_dma_peek(&mut dma, 0) },
        expected
    );
    assert_eq!(dma.read(0x14).unwrap(), 1);
    assert_eq!(unsafe { daisy_dma_irqs(&mut dma) }, 0);
    assert_eq!(unsafe { daisy_dma_request(&mut dma, 0) }, expected);
    assert_eq!(dma.read(0x14).unwrap(), 0);
    assert_eq!(unsafe { daisy_dma_irqs(&mut dma) }, 1);
}

#[test]
fn simultaneous_dma_requests_obey_software_priority_then_stream_number() {
    let mut dma = Peripheral::new(6);
    for stream in [0_u64, 1, 2] {
        let offset = 0x10 + stream * 0x18;
        dma.write(offset + 4, 4).unwrap();
        dma.write(offset + 8, 0x40015820).unwrap();
        dma.write(offset + 12, 0x30000000).unwrap();
        dma.write(
            offset,
            1 | (2 << 11) | (2 << 13) | if stream == 2 { 3 << 16 } else { 0 },
        )
        .unwrap();
    }
    assert_eq!(unsafe { virtual_daisy::daisy_dma_grant(&mut dma, 7) }, 2);
    assert_eq!(unsafe { virtual_daisy::daisy_dma_grant(&mut dma, 3) }, 0);
    dma.write(0x10, 0).unwrap();
    assert_eq!(unsafe { virtual_daisy::daisy_dma_grant(&mut dma, 3) }, 1);
    assert_eq!(
        unsafe { virtual_daisy::daisy_dma_grant(&mut dma, 0) },
        u32::MAX
    );
}

#[test]
fn dma1_cannot_transfer_into_dtcm_and_reports_a_transfer_error_irq() {
    let mut dma = Peripheral::new(6);
    dma.write(0x14, 4).unwrap();
    dma.write(0x18, 0x40015820).unwrap();
    dma.write(0x1c, 0x20000000).unwrap();
    dma.write(0x10, 1 | 4 | (1 << 10) | (2 << 11) | (2 << 13))
        .unwrap();
    assert_eq!(unsafe { daisy_dma_request(&mut dma, 0) }, 0);
    assert_eq!(dma.read(0).unwrap() & 8, 8);
    assert_eq!(dma.read(0x10).unwrap() & 1, 0);
    assert_eq!(dma.read(0x14).unwrap(), 4);
    assert_eq!(unsafe { daisy_dma_irqs(&mut dma) }, 1);
    dma.write(8, 8).unwrap();
    assert_eq!(unsafe { daisy_dma_irqs(&mut dma) }, 0);
}

#[test]
fn dma_rejects_tcm_holes_but_flash_accesses_reach_the_flash_controller() {
    for (address, transmit, error) in [
        (0, true, true),
        (0x20000000, true, true),
        (0x24080000, false, true),
        (0x30048000, false, true),
        (0x38801000, false, true),
        (0xc4000000, false, true),
        (0x08000000, false, false),
        (0x08000000, true, false),
    ] {
        let mut dma = Peripheral::new(6);
        dma.write(0x14, 4).unwrap();
        dma.write(0x18, 0x40015820).unwrap();
        dma.write(0x1c, address).unwrap();
        dma.write(0x10, 1 | (u32::from(transmit) << 6) | (2 << 11) | (2 << 13))
            .unwrap();
        let transfer = unsafe { daisy_dma_request(&mut dma, 0) };
        assert_eq!(dma.read(0).unwrap() & 8 != 0, error, "{address:#x}");
        assert_eq!(transfer == 0, error, "{address:#x}");
    }
}

#[test]
fn an_enabled_dma_stream_does_not_accept_reconfiguration() {
    let mut dma = Peripheral::new(6);
    dma.write(0x14, 4).unwrap();
    dma.write(0x18, 0x40015820).unwrap();
    dma.write(0x1c, 0x30000000).unwrap();
    dma.write(0x10, 1 | (2 << 11) | (2 << 13)).unwrap();
    dma.write(0x14, 9).unwrap();
    dma.write(0x18, 0).unwrap();
    dma.write(0x1c, 0x20000000).unwrap();
    assert_eq!(dma.read(0x14).unwrap(), 4);
    assert_eq!(dma.read(0x18).unwrap(), 0x40015820);
    assert_eq!(dma.read(0x1c).unwrap(), 0x30000000);
    assert_ne!(unsafe { daisy_dma_request(&mut dma, 0) }, 0);
}
