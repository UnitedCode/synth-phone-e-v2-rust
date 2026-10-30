use virtual_daisy::{daisy_sai_input, daisy_sai_rate, daisy_sai_tick, Peripheral};

#[test]
fn timestamped_input_is_sampled_at_the_sai_rate_and_variable_rate_wav_is_rejected() {
    let mut sai = Peripheral::new(7);
    let input = [0.0_f32, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75];
    unsafe {
        daisy_sai_input(&mut sai, input.as_ptr(), 4);
        assert_eq!(daisy_sai_rate(&mut sai, 24007.0), 0); // Half the input clock.
    }
    sai.write(4, (6 << 5) | 1 | (1 << 16)).unwrap();
    unsafe {
        daisy_sai_tick(&mut sai, 3);
    }
    assert_eq!(sai.read(0x20).unwrap(), 0);
    assert_eq!(sai.read(0x20).unwrap(), 0);
    unsafe {
        daisy_sai_tick(&mut sai, 3);
    }
    assert_eq!(sai.read(0x20).unwrap(), 0x400000); // At t=1/24007, input index2.
    assert_eq!(sai.read(0x20).unwrap(), 0x400000);
    assert_eq!(unsafe { daisy_sai_rate(&mut sai, 48014.0) }, 1);
}
