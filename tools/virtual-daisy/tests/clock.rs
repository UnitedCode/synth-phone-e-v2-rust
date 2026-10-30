use virtual_daisy::clock::ClockTree;

fn fractional_audio_clock() -> ClockTree {
    let mut clock = ClockTree::default();
    clock.write(0, 1 | (1 << 16));
    clock.write(0x28, 2 | (4 << 20)); // HSE16 / M3=4.
    clock.write(0x40, 99 | (7 << 9)); // N3=100, P3=8.
    clock.write(0x44, 4096 << 3); // +0.5.
    clock.write(0x2c, (1 << 8) | (1 << 22));
    clock.write(0, 1 | (1 << 16) | (1 << 28));
    clock.write(0x50, 2); // PLL3P.
    clock.write(0xf0, 1 << 22); // APB2 SAI1 clock.
    clock
}

#[test]
fn pll_fraction_and_sai_dividers_drive_frame_count_without_integer_hz_drift() {
    let mut clock = fractional_audio_clock();
    let cr1 = (1 << 16) | (4 << 20);
    assert_eq!(clock.sample_rate(cr1, 63).unwrap().hz(), 49072.265625);
    let mut frames = 0;
    for _ in 0..400_000 {
        frames += clock.advance(cr1, 63, 10_000).unwrap();
    }
    assert_eq!(frames, 196289); // floor(4 s * 49,072.265625 Hz).
}

#[test]
fn stopped_pll_or_gated_sai_cannot_produce_frames() {
    let mut clock = fractional_audio_clock();
    clock.write(0, 1 | (1 << 16));
    assert_eq!(clock.advance(1 << 16, 63, 1_000_000).unwrap(), 0);
    clock.write(0, 1 | (1 << 16) | (1 << 28));
    assert_eq!(clock.advance(0, 63, 1_000_000).unwrap(), 0);
    clock.write(0xf0, 0);
    assert_eq!(clock.advance(1 << 16, 63, 1_000_000).unwrap(), 0);
}

#[test]
fn dwt_cycle_counter_uses_core_clock_changes_and_wraps_at_32_bits() {
    let mut clock = ClockTree::default();
    clock.dwt(0xfb0, Some(0xc5acce55), 0).unwrap();
    clock.trace_enable(true, 0).unwrap();
    clock.dwt(4, 0xfffffff0_u32.into(), 0).unwrap();
    clock.dwt(0, Some(1), 0).unwrap(); // Reset HSI64MHz.
    assert_eq!(clock.dwt(4, None, 1000).unwrap(), 48);
    clock.sync_cycles(1000).unwrap();
    clock.write(0, 1 | (1 << 16));
    clock.write(0x10, 2); // HSE16.
    assert_eq!(clock.dwt(4, None, 2000).unwrap(), 64);
    clock.dwt(0, Some(0), 2000).unwrap();
    assert_eq!(clock.dwt(4, None, 3000).unwrap(), 64);
}

#[test]
fn dwt_software_lock_requires_the_coresight_key() {
    let mut clock = ClockTree::default();
    clock.trace_enable(true, 0).unwrap();
    assert_eq!(clock.dwt(0xfb4, None, 0).unwrap(), 3);
    clock.dwt(0, Some(1), 0).unwrap();
    assert_eq!(clock.dwt(4, None, 1000).unwrap(), 0);
    clock.dwt(0xfb0, Some(0xc5acce55), 1000).unwrap();
    assert_eq!(clock.dwt(0xfb4, None, 1000).unwrap(), 1);
    clock.dwt(0, Some(1), 1000).unwrap();
    assert_eq!(clock.dwt(4, None, 2000).unwrap(), 64);
    clock.dwt(0xfb0, Some(0), 2000).unwrap();
    clock.dwt(4, Some(0), 2000).unwrap();
    assert_eq!(clock.dwt(4, None, 2000).unwrap(), 64);
}

#[test]
fn dwt_trace_disable_stops_cycles_at_the_demcr_write_time() {
    let mut clock = ClockTree::default();
    clock.dwt(0xfb0, Some(0xc5acce55), 0).unwrap();
    clock.dwt(0, Some(1), 0).unwrap();
    assert_eq!(clock.dwt(4, None, 1000).unwrap(), 0);
    clock.trace_enable(true, 1000).unwrap();
    clock.trace_enable(false, 2000).unwrap();
    assert_eq!(clock.dwt(4, None, 4000).unwrap(), 64);
}

#[test]
fn production_pll_programming_exposes_the_firmware_sample_rate_difference() {
    let mut clock = ClockTree::default();
    clock.write(0, 1 | (1 << 16));
    clock.write(0x28, 0x00308012);
    clock.write(0x40, 0x4c4c98b1);
    clock.write(0x44, 0x19d8);
    clock.write(0x2c, 0x01cf092c);
    clock.write(0, 0x3f0331a5);
    clock.write(0x50, 2);
    let rate = clock.sample_rate(0x001302c1, 0x00011f3f).unwrap();
    // Worked RM0433 case: HSE16MHz / 3 * (178 + 827/8192) / 77 / 256.
    assert_eq!(rate.numerator, 3_256_703_125);
    assert_eq!(rate.denominator, 67_584);
    assert!((rate.hz() - 48_187.487053).abs() < 0.000001);
}

#[test]
fn apb_timer_multiplier_and_nonuniform_high_prescalers_follow_the_clock_tree() {
    let mut clock = ClockTree::default(); // HSI64.
    clock.write(0x18, 8); // HCLK /2.
    clock.write(0x1c, 5 << 4); // APB1 /4.
    assert_eq!(clock.domain_rate(1).unwrap().unwrap().hz(), 32_000_000.0);
    assert_eq!(clock.domain_rate(2).unwrap().unwrap().hz(), 8_000_000.0);
    assert_eq!(clock.domain_rate(6).unwrap().unwrap().hz(), 16_000_000.0);
    clock.write(0x10, 1 << 15); // TIMPRE=1: HCLK for APB /1,/2,/4.
    assert_eq!(clock.domain_rate(6).unwrap().unwrap().hz(), 32_000_000.0);
    clock.write(0x18, 12 << 8); // D1CPRE code12 means /64, not /32.
    assert_eq!(clock.core_rate().unwrap().unwrap().hz(), 1_000_000.0);
}

#[test]
fn ckper_sai_clock_stops_when_its_oscillator_is_disabled() {
    let mut clock = ClockTree::default();
    clock.write(0x50, 4);
    clock.write(0x4c, 2 << 28); // HSE not enabled yet.
    assert_eq!(clock.sample_rate(0, 63).unwrap().hz(), 0.0);
    clock.write(0, 1 | (1 << 16));
    assert_eq!(clock.sample_rate(0, 63).unwrap().hz(), 62500.0);
}

#[test]
fn fractional_changes_latch_only_after_fracen_rising_edge() {
    let mut clock = fractional_audio_clock();
    clock.write(0x44, 0); // Locked while FRACEN is set.
    assert_eq!(clock.sample_rate(4 << 20, 63).unwrap().hz(), 49072.265625);
    clock.write(0x2c, 1 << 22);
    assert_eq!(clock.sample_rate(4 << 20, 63).unwrap().hz(), 49072.265625);
    clock.write(0x44, 0);
    clock.write(0x2c, (1 << 22) | (1 << 8));
    assert_eq!(clock.sample_rate(4 << 20, 63).unwrap().hz(), 48828.125);
}
