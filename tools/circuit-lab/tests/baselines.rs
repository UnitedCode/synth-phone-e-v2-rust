use circuit_workbench::{
    carrier::SynthphoneCarrier, cli, fixture::Fixture, netlist::Graph, profiles,
};
use std::{fs, path::PathBuf};
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn graph(name: &str) -> Graph {
    Graph::parse(
        &fs::read_to_string(root().join(format!("circuits/baselines/{name}.net"))).unwrap(),
    )
    .unwrap()
}
#[test]
fn both_existing_revisions_and_broken_audio_route() {
    let fixture = Fixture::parse(
        &fs::read_to_string(root().join("circuits/fixtures/reference.fixture")).unwrap(),
    )
    .unwrap();
    for name in ["synthphone-original", "synthphone-v2"] {
        let out =
            std::env::temp_dir().join(format!("circuit-regression-{}-{name}", std::process::id()));
        fs::create_dir(&out).unwrap();
        let mut g = graph(name);
        let result = profiles::verify(name, &g, &fixture, &out).unwrap();
        assert!(result.contains("CONNECTIVITY=PASS"));
        #[cfg(feature = "spice")]
        assert!(result.contains("midi_sink_load_a=0.014"));
        g.disconnect("A1.16");
        assert!(profiles::verify(name, &g, &fixture, &out).is_err());
        fs::remove_dir_all(out).unwrap();
    }
}
#[test]
fn keypad_scans_all_positions_and_rejects_contention() {
    let g = graph("synthphone-v2");
    for row in 0..4 {
        for col in 0..3 {
            let mut c = SynthphoneCarrier::new(&g).unwrap();
            c.contact(row, col, true).unwrap();
            let mut outputs = [0; 11];
            outputs[0] |= 1 << 6;
            outputs[2] |= (1 << 1) | (1 << 4);
            let (p, b) = [(0, 6), (2, 1), (2, 4)][col];
            outputs[p] &= !(1 << b);
            let inputs = c.row_contacts(&outputs).unwrap();
            let (p, b) = [(2, 0), (0, 3), (1, 1), (0, 7)][row];
            assert_ne!(inputs[p].0 & (1 << b), 0);
            assert_eq!(inputs[p].1 & (1 << b), 0);
            c.contact(row, col, false).unwrap();
            assert_eq!(c.row_contacts(&outputs).unwrap(), [(0, 0); 11]);
            c.contact(row, 0, true).unwrap();
            c.contact(row, 1, true).unwrap();
            outputs[0] &= !(1 << 6);
            outputs[2] |= 1 << 1;
            assert!(c.row_contacts(&outputs).is_err());
        }
    }
}
#[test]
fn source_drift_and_output_clobber_are_rejected() {
    let baseline = root().join("circuits/baselines/synthphone-v2.net");
    let source =
        root().join("kicad/synth-phone-e-v2-hardware-v2/synth-phone-e-v2-hardware-v2.kicad_sch");
    cli(vec![
        "check-source".into(),
        baseline.display().to_string(),
        source.display().to_string(),
    ])
    .unwrap();
    assert!(cli(vec![
        "check-source".into(),
        baseline.display().to_string(),
        baseline.display().to_string()
    ])
    .is_err());
    assert!(cli(vec![
        "verify".into(),
        "synthphone-v2".into(),
        baseline.display().to_string(),
        root()
            .join("circuits/fixtures/reference.fixture")
            .display()
            .to_string(),
        root().display().to_string()
    ])
    .is_err());
}

#[cfg(feature = "daisy")]
#[test]
fn sdk_adapter_exposes_input_changes_and_faults_through_shared_handle() {
    let (handle, mut adapter) = circuit_workbench::carrier::shared(
        SynthphoneCarrier::new(&graph("synthphone-v2")).unwrap(),
    );
    let mut outputs = [0; 11];
    outputs[2] |= (1 << 1) | (1 << 4);
    handle.borrow_mut().contact(0, 0, true).unwrap();
    assert_eq!(adapter.pins(&outputs, 0)[2], (1, 0));
    adapter.i2c_write(1, 0x3c, &[0x00, 0xae]).unwrap();
    assert_eq!(handle.borrow().oled_writes, 1);
    assert!(adapter.i2c_write(1, 0x40, &[0]).is_err());
    handle.borrow_mut().contact(0, 1, true).unwrap();
    adapter.pins(&outputs, 1);
    assert!(handle.borrow().fault.is_some());
    assert!(adapter.i2c_write(1, 0x3c, &[0]).is_err());
}
