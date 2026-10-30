use virtual_daisy::bus::Axi;

#[test]
fn axi_qos_and_lru_choose_the_documented_winner_per_target_and_direction() {
    let mut axi = Axi::default();
    assert_eq!(axi.read(0x43100).unwrap(), 0);
    axi.write(0x43100, 0xfffffff7).unwrap(); // CPU read QoS 7, reserved bits ignored.
    assert_eq!(axi.read(0x43100).unwrap(), 7);
    assert_eq!(axi.grant(7, false, 3).unwrap(), Some(2)); // CPU beats D2 AHB.
    axi.write(0x43100, 0).unwrap();
    assert_eq!(axi.grant(7, false, 3).unwrap(), Some(1)); // Least recently served.
    assert_eq!(axi.grant(7, false, 3).unwrap(), Some(2));
    // Other slave and independent write channel were not aged by these reads.
    assert_eq!(axi.grant(5, false, 3).unwrap(), Some(1));
    assert_eq!(axi.grant(7, true, 3).unwrap(), Some(1));
    axi.write(0x43104, 15).unwrap();
    assert_eq!(axi.grant(7, true, 3).unwrap(), Some(2));
    assert_eq!(axi.grant(7, false, 0).unwrap(), None);
    assert!(axi.grant(8, false, 3).is_err());
}

#[test]
fn axi_outstanding_limits_apply_backpressure_until_completion_and_respect_overrides() {
    let mut axi = Axi::default();
    for _ in 0..7 {
        assert!(axi.reserve(2, false).unwrap());
    }
    assert!(!axi.reserve(2, false).unwrap());
    axi.complete(2, false).unwrap();
    assert!(axi.reserve(2, false).unwrap());
    axi.write(0x43108, 1).unwrap(); // Force read issuing to one, no cancellation.
    assert!(!axi.reserve(2, false).unwrap());
    for _ in 0..7 {
        axi.complete(2, false).unwrap();
    }
    assert!(axi.reserve(2, false).unwrap());
    assert!(!axi.reserve(2, false).unwrap());
    assert!(axi.reserve(2, true).unwrap()); // Independent write issuing limit.
    assert!(axi.complete(1, true).is_err());
}
