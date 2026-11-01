use virtual_daisy::{daisy_shared_reg, Peripheral};
#[test]
fn wrong_flash_keys_report_a_bus_error_and_remain_locked() {
    let mut architecture = Peripheral::new(12);
    unsafe {
        assert_eq!(
            daisy_shared_reg(&mut architecture, 0, 4, 0x12345678, 1, 4),
            2 << 32
        );
        assert_eq!(daisy_shared_reg(&mut architecture, 0, 0xc, 0, 0, 4) & 1, 1);
        assert_eq!(
            daisy_shared_reg(&mut architecture, 0, 4, 0x45670123, 1, 4),
            2 << 32
        );
        assert_eq!(
            daisy_shared_reg(&mut architecture, 0, 0xdead, 0, 0, 4),
            1 << 32
        );
    }
}
#[test]
fn wrong_second_key_and_interrupted_sequence_report_bus_errors() {
    for (offset, value) in [(4, 0x12345678), (0xc, 1)] {
        let mut architecture = Peripheral::new(12);
        unsafe {
            assert_eq!(
                daisy_shared_reg(&mut architecture, 0, 4, 0x45670123, 1, 4),
                0
            );
            assert_eq!(
                daisy_shared_reg(&mut architecture, 0, offset, value, 1, 4),
                2 << 32
            );
            assert_eq!(daisy_shared_reg(&mut architecture, 0, 0xc, 0, 0, 4) & 1, 1);
        }
    }
}
