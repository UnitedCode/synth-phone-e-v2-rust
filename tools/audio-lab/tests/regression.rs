use audio_lab::{
    cents,
    constants::{FFT_SIZE, HOP_SIZE, SAMPLE_RATE},
    estimate_pitch, metrics, render, render_with_fault, steady, tone, vocal_frame,
};
use synthphone_e_vocal_dsp::{MusicalSettings, ProcessingMode};

#[test]
fn lab_uses_the_firmware_dsp_pin_and_locked_dependencies() {
    let firmware = include_str!("../../../Cargo.toml");
    let lab = include_str!("../Cargo.toml");
    let dependency = |s: &str| {
        s.lines()
            .find(|line| line.starts_with("synthphone-e-vocal-dsp ="))
            .unwrap()
            .to_owned()
    };
    assert_eq!(
        dependency(firmware),
        dependency(lab),
        "Update both DSP pins together"
    );
    let identities = |lock: &str| {
        lock.split("[[package]]")
            .skip(1)
            .map(|package| {
                package
                    .lines()
                    .filter(|line| {
                        ["name =", "version =", "source =", "checksum ="]
                            .iter()
                            .any(|prefix| line.starts_with(prefix))
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .collect::<Vec<_>>()
    };
    let firmware_packages = identities(include_str!("../../../Cargo.lock"));
    for package in identities(include_str!("../Cargo.lock")) {
        if package.starts_with("name = \"audio-lab\"") || package.starts_with("name = \"hound\"") {
            continue;
        }
        assert!(
            firmware_packages.contains(&package),
            "DSP dependency drift: {package}"
        );
    }
}

#[test]
fn independent_pitch_oracle_handles_harmonics_and_rejects_silence() {
    for hz in [82.4069, 110.0, 220.0, 440.0, 587.33] {
        let input = tone(hz, 0.4, &[(1, 0.04), (2, 0.18), (3, 0.1)]);
        let pitch = estimate_pitch(&input).expect("periodic input should be measurable");
        assert!(cents(pitch, hz).abs() < 3.0, "{hz}: {pitch}");
    }
    assert!(estimate_pitch(&vec![0.0; 8192]).is_none());
    // A 100-cent error must not pass the 20-cent acceptance threshold.
    assert!(
        cents(
            estimate_pitch(&tone(440.0 * 2_f64.powf(1.0 / 12.0), 0.4, &[(1, 0.2)])).unwrap(),
            440.0
        )
        .abs()
            > 90.0
    );
}

#[test]
fn sustained_pitch_correction_reaches_known_musical_targets() {
    for target in [110.0, 146.832384, 220.0, 329.627557, 440.0] {
        let input = tone(target * 2_f64.powf(35.0 / 1200.0), 0.65, &[(1, 0.25)]);
        let output = render(
            &input,
            &[],
            &vocal_frame::default_config(),
            &MusicalSettings::default(),
        );
        let samples = steady(&output.audio);
        let hz = estimate_pitch(samples).expect("lost periodic output");
        assert!(
            cents(hz, target).abs() < 20.0,
            "{target} Hz target, {hz} Hz output"
        );
        assert_eq!(metrics(samples).silent_hops, 0);
        assert_eq!(metrics(&output.raw).non_finite, 0);
        assert!(metrics(samples).rms > 0.01);
    }
}

#[test]
fn silence_remains_silent_in_all_effect_modes() {
    let input = vec![0.0; SAMPLE_RATE as usize / 4];
    for mode in [
        ProcessingMode::PitchControl,
        ProcessingMode::Dry,
        ProcessingMode::Harmony,
        ProcessingMode::Vocode,
    ] {
        let settings = MusicalSettings {
            mode,
            ..Default::default()
        };
        let output = render(&input, &input, &vocal_frame::default_config(), &settings);
        let m = metrics(&output.raw);
        assert_eq!(m.non_finite, 0, "{mode:?}");
        assert!(m.peak < 1e-6, "{mode:?}: {m:?}");
    }
}

#[test]
fn dry_mode_preserves_frequency_across_ring_wraps() {
    let input = tone(237.0, 1.0, &[(1, 0.15)]);
    let settings = MusicalSettings {
        mode: ProcessingMode::Dry,
        ..Default::default()
    };
    let output = render(&input, &[], &vocal_frame::default_config(), &settings);
    let pitch = estimate_pitch(steady(&output.audio)).unwrap();
    assert!(cents(pitch, 237.0).abs() < 5.0, "{pitch}");
    assert_eq!(metrics(steady(&output.audio)).silent_hops, 0);
    assert_eq!(output.hops, (output.raw.len() - 1) / HOP_SIZE);
}

#[test]
fn missed_frame_is_detectable_in_the_audio() {
    let input = tone(220.0, 0.65, &[(1, 0.2)]);
    let settings = MusicalSettings {
        mode: ProcessingMode::Dry,
        ..Default::default()
    };
    let config = vocal_frame::default_config();
    let good = render(&input, &[], &config, &settings);
    let lost = render_with_fault(&input, &[], &config, &settings, Some(60));
    let error = good
        .audio
        .iter()
        .zip(&lost.audio)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0_f32, f32::max);
    assert!(
        error > 0.02,
        "missed-frame negative control was inaudible: {error}"
    );
    let good_ratio = metrics(steady(&good.audio)).hop_rms_ratio;
    let lost_ratio = metrics(steady(&lost.audio)).hop_rms_ratio;
    assert!(
        good_ratio > 0.85,
        "clean signal has amplitude dropouts: {good_ratio}"
    );
    assert!(
        lost_ratio < 0.8,
        "dropout metric missed the fault: {lost_ratio}"
    );
    // Tail must drain; wraparound must not replay old audio.
    assert!(metrics(&good.audio[good.audio.len() - FFT_SIZE / 2..]).peak < 1e-6);
}

#[test]
fn metrics_expose_nan_clipping_and_dropouts() {
    let mut broken = vec![0.0; HOP_SIZE * 2];
    broken[0] = f32::NAN;
    broken[1] = 1.2;
    let m = metrics(&broken);
    assert_eq!(m.non_finite, 1);
    assert_eq!(m.over_full_scale, 1);
    assert_eq!(m.silent_hops, 1);
}
