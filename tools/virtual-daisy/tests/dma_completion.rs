use virtual_daisy::{daisy_memory_read, daisy_qspi_attach, daisy_write_width, Peripheral};

#[test]
fn dma_read_completion_distinguishes_documented_bus_errors_from_unsupported_accesses() {
    let mut architecture = Peripheral::new(12);
    let mut controller = Peripheral::new(8);
    unsafe {
        // An absent model must remain unsupported rather than manufacture TEIF.
        assert_eq!(
            daisy_memory_read(&mut architecture, 0x90000000, 4, 0),
            1 << 32
        );
        daisy_qspi_attach(&mut architecture, &mut controller);
        assert_eq!(
            daisy_memory_read(&mut architecture, 0x90000000, 4, 0),
            2 << 32
        );
        assert_eq!(daisy_write_width(&mut controller, 0, 1, 4), 0);
        assert_eq!(daisy_write_width(&mut controller, 4, 22 << 16, 4), 0);
        assert_eq!(daisy_write_width(&mut controller, 0x14, 0x0d002503, 4), 0);
        assert_eq!(
            daisy_memory_read(&mut architecture, 0x90000000, 4, 0),
            u64::from(u32::MAX)
        );
        assert_eq!(
            daisy_memory_read(&mut architecture, 0x90800000, 4, 0),
            2 << 32
        );
        assert_eq!(daisy_write_width(&mut controller, 0, 0, 4), 0);
        assert_eq!(
            daisy_memory_read(&mut architecture, 0x90000000, 4, 0),
            2 << 32
        );
        assert_eq!(daisy_write_width(&mut controller, 0, 1, 4), 0);
        assert_eq!(daisy_write_width(&mut controller, 0x14, 0x0d002503, 4), 0);
        assert_eq!(daisy_write_width(&mut controller, 0, 3, 4), 0);
        assert_eq!(
            daisy_memory_read(&mut architecture, 0x90000000, 4, 0),
            2 << 32
        );
        assert_eq!(daisy_memory_read(&mut architecture, 0x24000000, 4, 0), 0);
    }
}
