use virtual_daisy::{
    daisy_cpu_access, daisy_memory_read, daisy_qspi_attach, daisy_runtime_fetch, daisy_write_width,
    Peripheral,
};

#[test]
fn documented_qspi_bus_errors_have_a_distinct_ffi_status() {
    let mut architecture = Peripheral::new(12);
    let mut controller = Peripheral::new(8);
    unsafe {
        daisy_qspi_attach(&mut architecture, &mut controller);
        let mut value = 0;
        assert_eq!(
            daisy_cpu_access(
                &mut architecture,
                0x200000002001f000,
                0x90000000,
                4,
                &mut value,
                0
            ),
            u64::MAX - 1
        );
        assert_eq!(daisy_write_width(&mut controller, 0, 1, 4), 0);
        assert_eq!(
            daisy_cpu_access(
                &mut architecture,
                0x200000042001f000,
                0x90000000,
                4,
                &mut value,
                0
            ),
            u64::MAX - 1
        );
        assert_eq!(daisy_write_width(&mut controller, 4, 22 << 16, 4), 0);
        assert_eq!(daisy_write_width(&mut controller, 0x14, 0x0d002503, 4), 0);
        assert_eq!(
            daisy_cpu_access(
                &mut architecture,
                0x200000082001f000,
                0x90800000,
                4,
                &mut value,
                0
            ),
            u64::MAX - 1
        );
        assert_eq!(
            daisy_memory_read(&mut architecture, 0x90800000, 4, 2),
            u64::from(u32::MAX)
        );
        assert_eq!(daisy_runtime_fetch(&mut architecture, 0x90800000, 4), 4);
        assert_eq!(
            daisy_memory_read(&mut architecture, 0x907ffffe, 4, 2),
            u64::from(u32::MAX)
        );
        // A valid prefix followed by an external fault needs instruction-level
        // service. Do not fabricate a block-start fault and skip that prefix.
        assert_eq!(daisy_runtime_fetch(&mut architecture, 0x907ffffe, 4), 3);
        architecture.write(0xd14, 1 << 16).unwrap();
        assert_eq!(
            daisy_cpu_access(
                &mut architecture,
                0x200000102001f000,
                0x90000000,
                4,
                &mut value,
                0
            ),
            0
        );
        assert_eq!(value, u64::from(u32::MAX));
        assert_eq!(daisy_write_width(&mut controller, 0, 0, 4), 0);
        assert_eq!(
            daisy_cpu_access(
                &mut architecture,
                0x200000142001f000,
                0x90000000,
                4,
                &mut value,
                0
            ),
            0
        );
        assert_eq!(value, u64::from(u32::MAX)); // Cache hit does not issue an external transaction.
    }
}

#[test]
fn unmodeled_bus_failures_do_not_masquerade_as_hardware_faults() {
    let mut architecture = Peripheral::new(12);
    let mut value = 0;
    unsafe {
        assert_eq!(
            daisy_cpu_access(
                &mut architecture,
                0x200000002001f000,
                0x90000000,
                4,
                &mut value,
                0
            ),
            u64::MAX
        );
    }
}
