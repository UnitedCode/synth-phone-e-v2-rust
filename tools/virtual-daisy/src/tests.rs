use super::*;

fn stream(dma: &mut dma::Dma, index: u64, extra: u32) {
    let base = 0x10 + index * 0x18;
    dma.write(base + 4, 4).unwrap();
    dma.write(base + 8, 0x4001_5820).unwrap();
    dma.write(base + 12, 0x3000_0000).unwrap();
    dma.write(base + 16, 0x3000_1000).unwrap();
    dma.write(base, 1 | 8 | 16 | (1 << 10) | (2 << 11) | (2 << 13) | extra)
        .unwrap();
}

#[test]
fn dma_is_request_paced_and_half_full_interrupts_clear_independently() {
    let mut dma = dma::Dma::default();
    stream(&mut dma, 1, 1 << 8);
    assert_eq!(dma.read(0x2c), Ok(4));
    assert_eq!(dma.irqs(), 0);
    assert_eq!(dma.request(1).unwrap(), Some((0x4001_5820, 0x3000_0000)));
    assert_eq!(dma.irqs(), 0);
    assert_eq!(dma.request(1).unwrap(), Some((0x4001_5820, 0x3000_0004)));
    assert_eq!(dma.read(0), Ok(1 << 10));
    assert_eq!(dma.irqs(), 2);
    dma.write(8, 1 << 11).unwrap();
    assert_eq!(dma.irqs(), 2); // TC clear must not clear HT.
    dma.write(8, 1 << 10).unwrap();
    assert_eq!(dma.irqs(), 0);
    dma.request(1).unwrap();
    dma.request(1).unwrap();
    assert_eq!(dma.read(0), Ok(1 << 11));
    assert_eq!(dma.read(0x2c), Ok(4));
    assert_eq!(dma.request(1).unwrap(), Some((0x4001_5820, 0x3000_0000)));
}

#[test]
fn dma_transmit_double_buffer_high_flags_and_normal_stop() {
    let mut dma = dma::Dma::default();
    stream(&mut dma, 5, (1 << 6) | (1 << 18));
    assert_eq!(dma.request(5).unwrap(), Some((0x3000_0000, 0x4001_5820)));
    for _ in 0..3 {
        dma.request(5).unwrap();
    }
    assert_eq!(dma.read(4), Ok(3 << 10));
    assert_eq!(dma.request(5).unwrap(), Some((0x3000_1000, 0x4001_5820)));
    dma.write(12, u32::MAX).unwrap();
    assert_eq!(dma.irqs(), 0);
    stream(&mut dma, 0, 0);
    for _ in 0..4 {
        dma.request(0).unwrap();
    }
    assert_eq!(dma.request(0).unwrap(), None);
    assert!(dma.write(0x10, 1).is_err()); // 8-bit/empty audio stream unsupported.
}

#[test]
fn gpio_pullups_external_straps_and_bsrr_survive_correctly() {
    let mut port = Peripheral::new(103);
    port.gpio.drive(3, false);
    port.gpio.drive(4, true);
    port.reset();
    assert_eq!(port.read(0x10).unwrap() & 0x18, 0x10);
    port.write(0, 0).unwrap(); // Inputs, including keypad rows.
    port.write(0xc, 0x55).unwrap(); // Pull up pins 0..3.
    assert_eq!(port.read(0x10).unwrap() & 0x1f, 0x17);
    port.write(0, 1 << 10).unwrap(); // Pin5 output, unaffected by external drive.
    port.write(0x18, 1 << 5).unwrap();
    assert_ne!(port.read(0x10).unwrap() & (1 << 5), 0);
    port.write(0x18, 1 << 21).unwrap();
    assert_eq!(port.read(0x14).unwrap() & (1 << 5), 0);
    assert!(port.write(0x10, 1).is_err());
}

#[test]
fn gpio_alternate_signal_requires_matching_af_and_mode() {
    let mut gpio = gpio::Gpio::default();
    gpio.reset(2);
    gpio.alternate(7, 2, true);
    gpio.write(0, 2 << 14).unwrap();
    gpio.write(0x20, 1 << 28).unwrap();
    assert_eq!(gpio.input() & 128, 0);
    gpio.write(0x20, 2 << 28).unwrap();
    assert_eq!(gpio.input() & 128, 128);
    gpio.write(0, 0).unwrap();
    assert_eq!(gpio.input() & 128, 0);
}

#[test]
fn sai_stereo_sign_extension_fifo_prefill_and_codec_gate() {
    let mut sai = sai::Sai::default();
    sai.input = vec![[-0.5, 0.25]; 2];
    assert!(sai.write(0x24, 1 << 17).is_err());
    sai.write(0x24, (1 << 17) | (6 << 5) | 2).unwrap();
    assert_eq!(sai.requests(), 2); // DMA can prefill before TX enable.
    sai.write(0x40, 0x00c0_0000).unwrap();
    sai.write(0x40, 0x0020_0000).unwrap();
    sai.tick(true);
    assert_eq!(sai.frames, 0); // RX disabled.
    sai.write(4, (1 << 16) | (1 << 17) | (6 << 5) | 1).unwrap();
    sai.tick(true);
    assert_eq!(sai.output[0], [-0.5, 0.25]);
    assert_eq!(sai.read(0x20).unwrap() as i32, -4_194_304);
    assert_eq!(sai.read(0x20).unwrap(), 2_097_152);
    assert!(sai.read(0x20).is_err());
    sai.write(0x40, 123).unwrap();
    sai.write(0x40, 456).unwrap();
    sai.tick(false);
    assert_eq!(sai.output[1], [0.0; 2]);
    assert_eq!(sai.read(0x20).unwrap(), 0);
}

#[test]
fn sai_lost_rx_requests_overrun_and_empty_tx_underruns() {
    let mut sai = sai::Sai::default();
    sai.write(4, (1 << 16) | (1 << 17) | (6 << 5) | 1).unwrap();
    sai.drop_rx = true;
    for _ in 0..5 {
        sai.tick(true);
        assert_eq!(sai.requests() & 1, 0);
    }
    assert_eq!(sai.overruns, 1);
    assert_eq!(sai.underruns, 5);
    sai.write(8, 8).unwrap();
    assert!(sai.read(0x20).is_err());
    for _ in 0..8 {
        sai.write(0x40, 0).unwrap();
    }
    assert_eq!(sai.read(0x38).unwrap() >> 16, 5);
    assert!(sai.write(0x40, 0).is_err());
}

#[test]
fn qspi_jedec_qpi_lanes_write_enable_and_status_fifo() {
    let mut qspi = qspi::Qspi::default();
    assert!(qspi.write(0x14, 0x19f, 4).is_err()); // Controller disabled.
    qspi.write(0, 1, 4).unwrap();
    qspi.write(0x10, 2, 4).unwrap();
    qspi.write(0x14, 0x19f, 4).unwrap();
    assert_eq!(qspi.read(0x20, 1), Ok(0x9d));
    assert_eq!(qspi.read(0x20, 2), Ok(0x1760));
    assert!(qspi.read(0x20, 1).is_err());
    assert!(qspi.write(0x14, 0x135, 4).is_err());
    qspi.write(0x14, 0x106, 4).unwrap();
    qspi.write(0x14, 0x135, 4).unwrap();
    assert!(qspi.write(0x14, 0x105, 4).is_err()); // QPI needs four lanes.
    qspi.write(0x10, 0, 4).unwrap();
    qspi.write(0x14, 0x306, 4).unwrap();
    qspi.write(0x14, 0x301, 4).unwrap();
    assert_ne!(qspi.read(8, 4).unwrap() & 32, 0);
    qspi.write(0x20, 2, 1).unwrap();
    assert_eq!(qspi.read(8, 4).unwrap() & 34, 2);
    qspi.write(0xc, 2, 4).unwrap();
    assert_eq!(qspi.read(8, 4).unwrap() & 2, 0);
    qspi.write(0x14, 0x305, 4).unwrap();
    assert_eq!(qspi.read(0x20, 1), Ok(0));
    assert!(qspi.write(0x14, 0x302, 4).is_err()); // Flash programming unsupported.
}

#[test]
fn codec_protocol_active_reset_and_unsupported_registers() {
    let mut codec = Peripheral::new(4);
    codec.i2c_write(&[18, 1]).unwrap();
    assert_eq!(codec.read(9), Ok(1));
    assert!(codec.read(0x100).is_err());
    codec.i2c_write(&[14, 9]).unwrap();
    codec.i2c_write(&[12, 0x42]).unwrap();
    codec.i2c_write(&[0, 0x17]).unwrap();
    codec.i2c_write(&[2, 0x17]).unwrap();
    codec.i2c_write(&[8, 0x12]).unwrap();
    codec.i2c_write(&[10, 0]).unwrap();
    assert_eq!(codec.read(0x100), Ok(3));
    codec.i2c_write(&[10, 8]).unwrap();
    assert_eq!(codec.read(0x100), Ok(1));
    assert!(codec.i2c_write(&[18]).is_err());
    assert!(codec.i2c_write(&[20, 1]).is_err());
    codec.i2c_write(&[30, 0]).unwrap();
    assert_eq!(codec.read(9), Ok(0));
}

#[test]
fn adc_calibration_requires_regulator_and_exits_when_ready() {
    let mut adc = Peripheral::new(3);
    assert!(adc.write(0x108, 1 << 31).is_err());
    adc.write(0x108, 1 << 28).unwrap();
    assert_ne!(adc.read(0x100).unwrap() & (1 << 12), 0);
    adc.write(0x108, (1 << 28) | (1 << 31)).unwrap();
    assert_eq!(adc.read(0x108).unwrap() & (1 << 31), 0);
    assert!(adc.read(0x154).is_err());
}
