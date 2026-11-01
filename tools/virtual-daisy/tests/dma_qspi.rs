use virtual_daisy::{daisy_dma_bus_error, daisy_dma_irqs, daisy_dma_peek, Peripheral};
#[test]
fn dma_can_read_a_memory_mapped_qspi_source() {
    let mut dma = Peripheral::new(6);
    dma.write(0x14, 1).unwrap();
    dma.write(0x18, 0x40015840).unwrap();
    dma.write(0x1c, 0x90000000).unwrap();
    dma.write(0x10, 1 | 4 | 16 | 64 | (2 << 11) | (2 << 13))
        .unwrap();
    assert_eq!(unsafe { daisy_dma_peek(&mut dma, 0) }, 0x9000000040015840);
    assert_eq!(dma.read(0x14).unwrap(), 1);
    assert_eq!(dma.read(0).unwrap(), 0);
    assert_eq!(unsafe { daisy_dma_bus_error(&mut dma, 0) }, 0);
    assert_eq!(dma.read(0).unwrap(), 8);
    assert_eq!(dma.read(0x10).unwrap() & 1, 0);
    assert_eq!(dma.read(0x14).unwrap(), 1);
    assert_eq!(unsafe { daisy_dma_irqs(&mut dma) }, 1);
    dma.write(8, 8).unwrap();
    assert_eq!(unsafe { daisy_dma_irqs(&mut dma) }, 0);
}
