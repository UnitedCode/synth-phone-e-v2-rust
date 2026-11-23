use crate::{fixture::Fixture, netlist::Graph, Result};
use std::{collections::BTreeSet, fs, path::Path};

fn require(condition: bool, message: &str) -> Result<()> {
    if !condition {
        return Err(message.into());
    }
    Ok(())
}
fn negative(g: &Graph, node: &str, check: impl Fn(&Graph) -> Result<()>) -> Result<()> {
    let mut broken = g.clone();
    broken.disconnect(node);
    require(
        check(&broken).is_err(),
        &format!("negative control {node} was not detected"),
    )
}

/// Read the wiring, not sequential HE numbering: the actual PCB's channels are permuted.
pub fn keyboard_routes(g: &Graph) -> Result<Vec<(String, String, usize, String)>> {
    let mut routes = Vec::new();
    let channel_pins = ["13", "14", "15", "12", "1", "5", "2", "4"];
    let mut seen = BTreeSet::new();
    for i in 1..=112 {
        let (reference, pin) = if i <= 100 {
            (format!("HE{i}"), "2")
        } else {
            (format!("RV{}", i - 100), "2")
        };
        let source = g.net(&format!("{reference}.{pin}"))?;
        let nodes = &g.nets[source];
        let mut found = Vec::new();
        for mux in 1..=15 {
            for (ch, pin) in channel_pins.iter().enumerate() {
                if nodes.contains(&format!("AM{mux}.{pin}")) {
                    found.push((mux, ch));
                }
            }
        }
        require(
            found.len() == 1,
            "sensor/pot must connect to exactly one mux channel",
        )?;
        let (mux, ch) = found[0];
        require(seen.insert((mux, ch)), "two controls share a mux input")?;
        require(
            (i <= 100 && mux <= 13) || (i > 100 && mux >= 14),
            "sensor/pot mux group mismatch",
        )?;
        let expected_pad = match mux {
            1..=9 => 21 + mux,
            10 | 11 => 31,
            12 | 13 => 32,
            14 | 15 => 35,
            _ => unreachable!(),
        };
        let common = format!("AM{mux}.3");
        let adc = format!("A1.{expected_pad}");
        g.connected(&common, &adc)?;
        for (select, pad) in [("11", 8), ("10", 9), ("9", 10)] {
            g.connected(&format!("AM{mux}.{select}"), &format!("A1.{pad}"))?;
        }
        if mux >= 10 {
            let output = [15, 14, 13, 12, 11, 10][mux - 10];
            g.connected(&format!("AM{mux}.6"), &format!("U1.{output}"))?;
        } else {
            g.connected(&format!("AM{mux}.6"), "A1.40")?;
        }
        g.connected(&format!("AM{mux}.16"), "A1.38")?;
        for pin in [7, 8] {
            g.connected(&format!("AM{mux}.{pin}"), "A1.40")?;
        }
        if i <= 100 {
            g.connected(&format!("{reference}.1"), "A1.38")?;
            g.connected(&format!("{reference}.3"), "A1.40")?;
        }
        routes.push((reference, format!("AM{mux}"), ch, adc));
    }
    Ok(routes)
}
pub(crate) fn decoder(g: &Graph, pre_repair: bool) -> Result<()> {
    for (pin, pad) in [(1, 5), (2, 4), (3, 3)] {
        g.connected(&format!("U1.{pin}"), &format!("A1.{pad}"))?;
    }
    g.connected("U1.16", "A1.38")?;
    // Preserve historical topology instead of silently repairing the model.
    if !pre_repair {
        g.connected("U1.4", "A1.40")?;
        g.connected("U1.5", "A1.40")?;
        g.connected("U1.6", "A1.38")?;
        for pin in [7, 9] {
            require(
                g.nets[g.net(&format!("U1.{pin}"))?].len() == 1,
                "unused decoder output is loaded",
            )?;
        }
    }
    Ok(())
}
pub(crate) fn synth_wiring(g: &Graph) -> Result<()> {
    for (pin, net) in [
        (22, "row1"),
        (23, "row2"),
        (24, "row3"),
        (25, "row4"),
        (26, "col1"),
        (27, "col2"),
        (28, "col3"),
    ] {
        require(
            g.net(&format!("A1.{pin}"))? == net,
            "keypad physical pad map changed",
        )?;
    }
    for (a, b) in [
        ("A1.15", "U2.6"),
        ("U2.6", "R5.1"),
        ("U2.8", "A1.38"),
        ("U2.5", "A1.40"),
        ("R5.2", "A1.38"),
        ("R1.2", "U2.2"),
        ("R1.1", "J3.2"),
        ("U2.3", "J3.4"),
        ("U3.5", "A1.16"),
        ("U3.3", "R2.2"),
        ("R2.1", "A1.40"),
        ("A1.17", "J2.T"),
        ("A1.18", "J1.R1"),
        ("A1.19", "J1.T"),
        ("A1.12", "J12.2"),
        ("A1.13", "J12.1"),
        ("A1.5", "J15.5"),
        ("A1.4", "J15.4"),
        ("A1.3", "J15.3"),
    ] {
        g.connected(a, b)?;
    }
    require(
        g.components.get("R2").map(String::as_str) == Some("?"),
        "baseline microphone resistor changed; create a new profile",
    )?;
    Ok(())
}
#[cfg(feature = "spice")]
fn simulate(
    out: &Path,
    name: &str,
    deck: &str,
    analysis: &str,
    names: &[&str],
) -> Result<crate::spice::Trace> {
    // Keep the actual executed circuit and simulator log alongside all vectors.
    fs::write(out.join(format!("{name}.cir")), deck)?;
    let trace = crate::spice::solve(deck, analysis, names)?;
    fs::write(out.join(format!("{name}.csv")), trace.csv()?)?;
    fs::write(out.join(format!("{name}.log")), &trace.log)?;
    Ok(trace)
}

pub fn verify(profile: &str, g: &Graph, fixture: &Fixture, out: &Path) -> Result<String> {
    let mut coverage = String::from("reference,value,scope\n");
    for (reference, value) in &g.components {
        let scope = if profile.starts_with("keyboard") {
            if reference.starts_with("AM") {
                "ideal switched resistance; topology checked"
            } else if reference.starts_with("HE") {
                "declared output voltage and supply current; not magnetic curve"
            } else if reference.starts_with("RV") {
                "declared wiper voltage; resistance unknown"
            } else if reference.starts_with('C') {
                "schematic capacitor in acquisition network"
            } else if reference == "U1" {
                "ideal decoder selection; loaded unused-output check"
            } else {
                "inventory/connectivity only; no analog device model"
            }
        } else if reference == "R5" {
            "schematic resistance; ideal sink output-load test"
        } else if reference == "R2" || reference == "U3" || reference.starts_with("RV") {
            "unknown value or module internals; analog blocked"
        } else {
            "inventory/connectivity only; no analog device model"
        };
        coverage.push_str(&format!(
            "{reference},\"{}\",{scope}\n",
            value.replace('"', "\"\"")
        ));
    }
    fs::write(out.join("coverage.csv"), coverage)?;
    #[cfg(not(feature = "spice"))]
    let _ = fixture;
    let mut report = format!(
        "components={}\nnets={}\nPHYSICAL_MEASURED=NOT_RUN\nARM_COUPLED=NOT_RUN\n",
        g.components.len(),
        g.nets.len()
    );
    match profile {
        "keyboard-current" | "keyboard-pre-repair" => {
            let old = profile.ends_with("pre-repair");
            let routes = keyboard_routes(g)?;
            decoder(g, old)?;
            let mut csv = String::from("control,mux,channel,adc_pad\n");
            for (r, m, ch, adc) in &routes {
                csv.push_str(&format!("{r},{m},{ch},{adc}\n"));
            }
            fs::write(out.join("routes.csv"), csv)?;
            negative(g, "A1.35", |g| keyboard_routes(g).map(|_| ()))?;
            negative(g, "HE1.2", |g| keyboard_routes(g).map(|_| ()))?;
            negative(g, "AM10.6", |g| keyboard_routes(g).map(|_| ()))?;
            report.push_str("CONNECTIVITY=PASS\ncontrols_routed=112\nopen_adc_negative=PASS\nopen_hall_negative=PASS\nopen_enable_negative=PASS\n");
            report.push_str(if old {
                "DESIGN_ACCEPTANCE=FAIL (known grounded outputs)\n"
            } else {
                "DESIGN_ACCEPTANCE=UNVERIFIED\n"
            });
            #[cfg(feature = "spice")]
            {
                report.push_str(&keyboard_analog(g, fixture, out, old)?);
            }
        }
        "synthphone-original" | "synthphone-v2" => {
            synth_wiring(g)?;
            negative(g, "A1.15", synth_wiring)?;
            negative(g, "A1.16", synth_wiring)?;
            report.push_str(
                "CONNECTIVITY=PASS\nopen_midi_negative=PASS\nopen_preamp_negative=PASS\n",
            );
            report.push_str(
                "DESIGN_ACCEPTANCE=UNVERIFIED (unknown analog values and keypad harness)\n",
            );
            #[cfg(feature = "spice")]
            {
                report.push_str(&synth_analog(g, fixture, out)?);
            }
        }
        _ => return Err("unknown circuit profile".into()),
    }
    #[cfg(not(feature = "spice"))]
    report.push_str("ANALOG_SIMULATED=NOT_RUN (built without spice)\n");
    Ok(report)
}

#[cfg(feature = "spice")]
fn synth_analog(g: &Graph, f: &Fixture, out: &Path) -> Result<String> {
    // External contact harness declared explicitly: v2 exports connectors, not 12 on-board switches.
    // Firmware scans one low and two push-pull high columns. Row pullups = provisional 40k.
    let mut csv = String::from("row,column,voltage_v\n");
    for row in 0..4 {
        for col in 0..3 {
            require(
                g.net(&format!("A1.{}", 22 + row))? == format!("row{}", row + 1),
                "row not wired",
            )?;
            require(
                g.net(&format!("A1.{}", 26 + col))? == format!("col{}", col + 1),
                "column not wired",
            )?;
            let deck=format!("Keypad single contact: declared fixture impedances\nVrail vcc 0 3.3\nVcol drive 0 0\nRdriver drive col {}\nRcontact col row {}\nRpull row vcc {}\n.end\n",f.get("driver_ohm"),f.get("contact_ohm"),f.get("pullup_ohm"));
            let t = simulate(
                out,
                &format!("key-{}-{}", row + 1, col + 1),
                &deck,
                "op",
                &["v(row)"],
            )?;
            require(
                t.last("v(row)")? < 0.1,
                "single keypad contact failed low threshold",
            )?;
            csv.push_str(&format!("{},{},{}\n", row + 1, col + 1, t.last("v(row)")?));
        }
    }
    fs::write(out.join("keypad.csv"), csv)?;
    let resistance = f.get("driver_ohm") + f.get("contact_ohm");
    let deck=format!("Two held keys on same row: actual push-pull scan policy, declared impedances\nVrail vcc 0 3.3\nVlow lo 0 0\nVhigh hi 0 3.3\nRlo lo row {resistance}\nRhi hi row {resistance}\nRpull row vcc {}\n.end\n",f.get("pullup_ohm"));
    let t = simulate(
        out,
        "keypad-contention",
        &deck,
        "op",
        &["v(row)", "i(vhigh)"],
    )?;
    let row = t.last("v(row)")?;
    require(
        (1.6..1.7).contains(&row),
        "expected held-key contention disappeared",
    )?;
    let open = deck.replace(&format!("Rlo lo row {resistance}"), "Rlo lo row 1e12");
    let t_open = simulate(out, "keypad-open-negative", &open, "op", &["v(row)"])?;
    require(t_open.last("v(row)")? > 3., "open contact was not detected")?;
    // MIDI receiver's output load uses actual R5. An ideal sink checks loading only, not 6N138 behavior.
    let midi=format!("MIDI RX output load only -- not an optocoupler timing model\nVrail vcc 0 3.3\nRpull vcc rx {}\nVsink rx 0 0.2\n.end\n",g.value("R5")?);
    let midi = simulate(out, "midi-output-load", &midi, "op", &["i(vsink)"])?;
    Ok(format!("ANALOG_SIMULATED=PASS (bounded subcircuits)\nkeypad_single_keys=12\nKNOWN_FAULT=two same-row keys contend with push-pull columns\ncontention_row_v={row}\ncontention_high_current_a={}\nmidi_sink_load_a={}\nUNKNOWN=R2 microphone resistor value; SSM2167 module internals; pot values; fitted harness\nNOT_RUN=preamp gain/noise/headroom; optocoupler delay/CTR; codec analog response; thermal\n",t.last("i(vhigh)")?.abs(),midi.last("i(vsink)")?))
}

#[cfg(feature = "spice")]
fn keyboard_analog(g: &Graph, f: &Fixture, out: &Path, old: bool) -> Result<String> {
    // Generate the whole 15-mux network from real connectivity, including the shared ADC nets.
    // Sources are unique identifying voltages, not a claimed magnetic key-travel model.
    let routes = keyboard_routes(g)?;
    let net_names: Vec<_> = g.nets.keys().cloned().collect();
    let node = |pin: &str| -> Result<String> {
        let net = g.net(pin)?;
        if net == g.net("A1.40")? {
            return Ok("0".into());
        }
        if net == g.net("A1.38")? {
            return Ok("rail".into());
        }
        Ok(format!(
            "n{}",
            net_names
                .iter()
                .position(|n| n == net)
                .ok_or("missing net")?
        ))
    };
    let mut deck=format!("Full keyboard analog acquisition network -- declared ideal decoder stimulus\nVrail rail 0 3.3\n.model route SW(Ron={} Roff=1e12 Vt=1.65 Vh=0)\n",f.get("mux_ron_ohm"));
    for reference in g.components.keys().filter(|r| r.starts_with('C')) {
        let value = g.value(reference)?;
        deck.push_str(&format!(
            "{reference} {} {} {value}\n",
            node(&format!("{reference}.1"))?,
            node(&format!("{reference}.2"))?
        ));
    }
    let mut expected = Vec::new();
    for (index, (r, m, ch, adc)) in routes.iter().enumerate() {
        let input = 0.3 + index as f64 * 0.02;
        let n = node(&format!("{r}.2"))?;
        deck.push_str(&format!(
            "V{r} src{index} 0 {input}\nR{r} src{index} {n} {}\n",
            f.get("sensor_source_ohm")
        ));
        let mux = m.strip_prefix("AM").unwrap().parse::<usize>()?;
        let slot = if mux <= 9 { *ch } else { (mux - 10) * 8 + ch };
        expected.push((r.clone(), node(adc)?, slot, input));
    }
    let mut adc_nodes = BTreeSet::new();
    for mux in 1..=15 {
        let common = node(&format!("AM{mux}.3"))?;
        adc_nodes.insert(common.clone());
        for (ch, pin) in [13, 14, 15, 12, 1, 5, 2, 4].iter().enumerate() {
            let input = node(&format!("AM{mux}.{pin}"))?;
            let bank = if mux <= 9 {
                "1".into()
            } else {
                format!("(floor(time/40u)=={})", mux - 10)
            };
            deck.push_str(&format!("Bsel{mux}_{ch} e{mux}_{ch} 0 V=3.3*((floor(time/5u)-8*floor(time/40u))=={ch})*{bank}\nS{mux}_{ch} {input} {common} e{mux}_{ch} 0 route\nRfloat{mux}_{ch} {input} 0 1e12\n"));
        }
    }
    for (i, n) in adc_nodes.iter().enumerate() {
        deck.push_str(&format!(
            "Cadc{i} {n} 0 {}\nRload{i} {n} 0 1e9\n",
            f.get("adc_cap_f")
        ));
    }
    deck.push_str(".end\n");
    let mut names = vec!["time".to_string()];
    names.extend(adc_nodes.iter().map(|n| format!("v({n})")));
    let refs: Vec<_> = names.iter().map(String::as_str).collect();
    let trace = simulate(out, "full-mux-scan", &deck, "tran 100n 240u 0 50n", &refs)?;
    let mut csv = String::from("control,sample_s,input_v,adc_v,error_v,within_1mv\n");
    let mut acquisition_failures = 0;
    let mut maximum_error: f64 = 0.;
    for (r, n, slot, input) in &expected {
        let sample = (*slot as f64 + 0.9) * 5e-6;
        let i = trace.vectors["time"]
            .iter()
            .rposition(|t| *t <= sample)
            .ok_or("missing scan sample")?;
        let actual = trace.vectors[&format!("v({n})")][i];
        let error = (actual - input).abs();
        maximum_error = maximum_error.max(error);
        acquisition_failures += usize::from(error >= 0.001);
        csv.push_str(&format!(
            "{r},{sample},{input},{actual},{error},{}\n",
            error < 0.001
        ));
    }
    fs::write(out.join("mux-acquisition.csv"), csv)?;
    let broken = deck.replace("S1_0 ", "* disconnected S1_0 ");
    let fault = simulate(
        out,
        "mux-disabled-negative",
        &broken,
        "tran 100n 5u 0 50n",
        &refs,
    )?;
    let target = expected
        .iter()
        .find(|(r, _, _, _)| g.net(&format!("{r}.2")).ok() == g.net("AM1.13").ok())
        .ok_or("missing X0 source")?;
    require(
        (fault.last(&format!("v({})", target.1))? - target.3).abs() > 0.01,
        "disabled switch negative control escaped detection",
    )?;
    // Both muxes in a pair must never be enabled together. Show expected averaging if they are.
    let contention="Shared ADC pair enabled together -- fault injection\nVa a 0 0.3\nVb b 0 2.7\nRa a adc 100\nRb b adc 100\nRload adc 0 1e9\n.end\n";
    let t = simulate(out, "shared-pair-contention", contention, "op", &["v(adc)"])?;
    require(
        (t.last("v(adc)")? - 1.5).abs() < 0.001,
        "pair collision not detected",
    )?;
    // Supply envelopes intentionally use current sources: isolates rail and source capacity assumptions.
    let mut power =
        String::from("sensor_current_a,total_sensor_a,sensor_power_w,rail_v,source_limit_a\n");
    for per_sensor in [0.006, 0.007] {
        for limit in [0.5, 1.0] {
            let deck=format!("Carrier sensor-only supply envelope; excludes Seed and all other loads\nVsource supply 0 3.3\nBreg 0 rail I=max(0,min({limit},(v(supply)-v(rail))/0.1))\nRbleed rail 0 1e9\n");
            let mut deck = deck;
            for i in 1..=100 {
                g.connected(&format!("HE{i}.1"), "A1.38")?;
                deck.push_str(&format!(
                    "Bhe{i} rail 0 I={per_sensor}*max(0,min(1,v(rail)/3.3))\n"
                ));
            }
            deck.push_str(".end\n");
            let t = simulate(
                out,
                &format!(
                    "power-{}-{}",
                    (per_sensor * 1e3) as usize,
                    (limit * 1e3) as usize
                ),
                &deck,
                "op",
                &["v(rail)"],
            )?;
            let rail = t.last("v(rail)")?;
            require(
                if limit == 0.5 { rail < 3.0 } else { rail > 3.2 },
                "supply stress envelope failed",
            )?;
            power.push_str(&format!(
                "{per_sensor},{},{},{rail},{limit}\n",
                per_sensor * 100.,
                per_sensor * 100. * 3.3
            ));
        }
    }
    fs::write(out.join("power-envelope.csv"), power)?;
    let grounded: [bool; 2] = [
        g.net("U1.7")? == g.net("A1.40")?,
        g.net("U1.9")? == g.net("A1.40")?,
    ];
    if old {
        require(
            grounded == [true, true],
            "pre-repair grounded-output fault is missing",
        )?;
    }
    for (index, short) in grounded.iter().enumerate() {
        let load = if *short { 0.01 } else { 1e12 };
        let deck=format!("Unused decoder HIGH output load: declared output resistance\nVdrive drive 0 3.3\nRdriver drive output {}\nRboard output 0 {load}\n.end\n",f.get("driver_ohm"));
        let t = simulate(
            out,
            &format!("decoder-unused-{index}"),
            &deck,
            "op",
            &["i(vdrive)"],
        )?;
        require(
            if *short {
                t.last("i(vdrive)")?.abs() > 0.05
            } else {
                t.last("i(vdrive)")?.abs() < 1e-6
            },
            "decoder short detection failed",
        )?;
    }
    Ok(format!("ANALOG_SIMULATED=PASS (bounded subcircuits)\nmux_switches=120\ncontrols_sampled=112\nacquisition_1mv_cases_failed={acquisition_failures}\nacquisition_max_error_v={maximum_error}\ndisabled_mux_negative=PASS\nshared_pair_contention_negative=PASS\nsource_limit_sweep=0.5A,1.0A (provisional,not regulator ratings)\nsensor_current_envelope=0.60-0.70A,1.98-2.31W at 3.3V\nunused_outputs_grounded={grounded:?}\nKNOWN_FAULT={}\nUNKNOWN=fitted Seed regulator capacity; source impedance; Hall magnetic curves; ADC loading; pot resistances\nNOT_RUN=thermal; optocoupler; USB signal integrity; complete firmware integration\n",if old {"decoder HIGH outputs short to ground"} else {"none in tested decoder routing; power and acquisition margins remain unverified"}))
}
