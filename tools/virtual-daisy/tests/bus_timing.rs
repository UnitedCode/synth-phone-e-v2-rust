use virtual_daisy::{bus::SdramTiming, clock::Rate};
#[test]
fn programmed_sdram_timings_define_read_and_refresh_lower_bounds() {
    // FMC=200MHz, SDCLK=/2, 32-bit bus, CAS3, RPIPE1;
    // TMRD2, TXSR8, TRAS5, TRC7, TWR3, TRP2, TRCD3.
    let timing = SdramTiming::decode(
        (2 << 4) | (3 << 7) | (2 << 10) | (1 << 13),
        0x02126471,
        1560 << 1,
        Rate {
            numerator: 200_000_000,
            denominator: 1,
        },
    )
    .unwrap();
    // RPIPE adds one 5ns kernel cycle, not one 10ns SDRAM cycle.
    assert_eq!(timing.read_min_ns(4, false, false).unwrap(), 65);
    assert_eq!(timing.read_min_ns(32, false, false).unwrap(), 135);
    assert_eq!(timing.read_min_ns(4, true, false).unwrap(), 35);
    assert_eq!(timing.read_min_ns(4, false, true).unwrap(), 85);
    assert_eq!(timing.refresh_interval_ns(), 15600);
    assert_eq!(timing.refresh_busy_ns(8), 560);
}

#[test]
fn refresh_disable_and_reserved_controller_settings_are_explicit() {
    let rate = Rate {
        numerator: 200_000_000,
        denominator: 1,
    };
    let valid = (2 << 4) | (3 << 7) | (2 << 10);
    assert_eq!(
        SdramTiming::decode(valid, 0, 0, rate)
            .unwrap()
            .refresh_interval_ns(),
        0
    );
    for count in [1, 40] {
        assert!(SdramTiming::decode(valid, 0, count << 1, rate).is_err());
    }
    assert!(SdramTiming::decode(valid | (3 << 13), 0, 100 << 1, rate).is_err());
    assert!(SdramTiming::decode(valid & !(3 << 10), 0, 100 << 1, rate).is_err());
}
