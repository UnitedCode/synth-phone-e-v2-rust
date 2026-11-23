//! Bounded power/thermal and continuous selected-channel noise studies.
//! Scenario values are inputs, not measurements or inferred fitted component ratings.
use crate::{netlist::Graph, sha256, Result};
use std::{collections::BTreeMap, fs, path::Path};

pub struct Scenarios(BTreeMap<String, Vec<f64>>);
impl Scenarios {
    pub fn parse(text: &str) -> Result<Self> {
        let keys = [
            "sensor_a",
            "seed_a",
            "aux_a",
            "vin_v",
            "source_a",
            "path_ohm",
            "efficiency",
            "ambient_c",
            "enclosure_k_w",
            "local_k_w",
            "heat_capacity_j_k",
            "temperature_budget_c",
            "bridge_overhead_a",
        ];
        let mut map = BTreeMap::new();
        for l in text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            let (k, v) = l
                .split_once('=')
                .ok_or("scenario requires key=comma separated values")?;
            if !keys.contains(&k) {
                return Err(format!("unknown scenario key {k}").into());
            }
            let values = v
                .split(',')
                .map(|s| s.trim().parse::<f64>())
                .collect::<std::result::Result<Vec<_>, _>>()?;
            if values.is_empty()
                || values.iter().any(|v| !v.is_finite() || *v <= 0.)
                || map.insert(k.into(), values).is_some()
            {
                return Err("invalid or duplicate scenario".into());
            }
        }
        if keys.iter().any(|k| !map.contains_key(*k)) {
            return Err("missing scenario key".into());
        }
        if map["efficiency"].iter().any(|e| *e >= 1.) || map["sensor_a"].iter().any(|a| *a > 0.02) {
            return Err("invalid efficiency or per-sensor current".into());
        }
        for key in [
            "heat_capacity_j_k",
            "temperature_budget_c",
            "bridge_overhead_a",
        ] {
            if map[key].len() != 1 {
                return Err(format!("{key} must be a single scalar").into());
            }
        }
        Ok(Self(map))
    }
    fn v(&self, k: &str) -> &[f64] {
        &self.0[k]
    }
}

/// Continuous selected two-node RC response. Mux switching/charge injection are separate tests.
/// Returns coefficients multiplying sin(wt) and cos(wt); ADC leakage = prior 1 Gohm fixture.
pub fn response(rs: f64, ron: f64, cf: f64, ca: f64, hz: f64) -> Result<(f64, f64)> {
    if [rs, ron, cf, ca, hz]
        .iter()
        .any(|v| !v.is_finite() || *v <= 0.)
    {
        return Err("invalid RC noise parameter".into());
    }
    let w = std::f64::consts::TAU * hz;
    let re = 1. + (rs + ron) / 1e9 - w * w * rs * ron * cf * ca;
    let im = w * (rs * cf * (1. + ron / 1e9) + (rs + ron) * ca);
    let d = re * re + im * im;
    Ok((re / d, -im / d))
}
/// Kelvin/W and Joule/K thermal RC; constant dissipation, no fitted enclosure calibration.
pub fn temperature(ambient: f64, power: f64, theta: f64, capacity: f64, seconds: f64) -> f64 {
    ambient + power * theta * (1. - (-seconds / (theta * capacity)).exp())
}

#[cfg(feature = "spice")]
pub fn spice_noise(out: &Path) -> Result<()> {
    for (label, cf) in [("4n7", 4.7e-9), ("1n", 1e-9), ("470p", 470e-12)] {
        let deck=format!("Selected Hall output noise crosscheck; not switched ADC or sensor intrinsic noise\nVs src 0 SIN(1.65 0.05 100000)\nRs src hall 1000\nCf hall 0 {cf}\nRm hall adc 100\nCa adc 0 50p\nRl adc 0 1e9\n.end\n");
        let t = simulate(
            out,
            &format!("noise-{label}"),
            &deck,
            "tran 10n 100u 0 10n",
            &["time", "v(adc)"],
        )?;
        let (a, b) = response(1000., 100., cf, 50e-12, 100000.)?;
        let mut error: f64 = 0.;
        for (&time, &v) in t.vectors["time"].iter().zip(&t.vectors["v(adc)"]) {
            if time > 80e-6 {
                let phase = std::f64::consts::TAU * 100000. * time;
                let expected =
                    1.65 * 1e9 / (1e9 + 1100.) + 0.05 * (a * phase.sin() + b * phase.cos());
                error = error.max((v - expected).abs());
            }
        }
        if error > 0.0001 {
            return Err(format!("analytic/SPICE noise mismatch {label}: {error}").into());
        }
    }
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
    fs::write(out.join(format!("{name}.cir")), deck)?;
    let t = crate::spice::solve(deck, analysis, names)?;
    fs::write(out.join(format!("{name}.csv")), t.csv()?)?;
    fs::write(out.join(format!("{name}.log")), &t.log)?;
    Ok(t)
}

pub const OFFLOAD: &str = "keyboard-guarded-offload";
pub const RX: &str = "synthphone-rx-2k2";
pub const HALL: &str = "keyboard-drv5053ra";
pub const HALL_PART: &str = "DRV5053RAQDBZR";
pub fn hall_candidate(base: &Graph) -> Result<Graph> {
    crate::profiles::keyboard_routes(base)?;
    let mut g = base.clone();
    let mut count = 0;
    for (r, v) in &mut g.components {
        if r.starts_with("HE") {
            if v != "MT9102ET" {
                return Err("unexpected original Hall symbol value".into());
            }
            *v = HALL_PART.into();
            count += 1;
        }
    }
    if count != 100 {
        return Err("expected 100 Hall sensors".into());
    }
    Ok(g)
}
fn join(g: &mut Graph, net: &str, pin: String) {
    g.nets.entry(net.into()).or_default().push(pin);
}
fn move_pin(g: &mut Graph, pin: &str, to: &str) {
    for n in g.nets.values_mut() {
        n.retain(|p| p != pin);
    }
    join(g, to, pin.into());
}
fn add(g: &mut Graph, r: &str, value: &str, pins: &[(usize, String)]) {
    g.components.insert(r.into(), value.into());
    for (p, n) in pins {
        join(g, n, format!("{r}.{p}"));
    }
}
/// One electrical experiment. Original filters, MIDI receiver and other peripherals are untouched.
pub fn offload(base: &Graph) -> Result<Graph> {
    crate::profiles::keyboard_routes(base)?;
    let mut g = base.clone();
    let seed = base.net("A1.38")?.to_string();
    let ground = base.net("A1.40")?.to_string();
    let ext = "CARRIER_3V3";
    let guard = "POWER_VALID";
    let mid = "SEED_MID";
    for r in base.components.keys() {
        let supply = if r.starts_with("HE") {
            Some(1)
        } else if r.starts_with("AM") || r == "U1" {
            Some(16)
        } else if r.starts_with("RV") {
            Some(1)
        } else {
            None
        };
        if let Some(p) = supply {
            move_pin(&mut g, &format!("{r}.{p}"), ext);
        }
        if r.starts_with('C') {
            for p in [1, 2] {
                let pin = format!("{r}.{p}");
                if base.net(&pin)? == seed {
                    move_pin(&mut g, &pin, ext);
                }
            }
        }
    }
    move_pin(&mut g, "U1.6", ext);
    add(
        &mut g,
        "PX1",
        "D24V22F3",
        &[(1, "EXT_5V".into()), (2, ground.clone()), (3, ext.into())],
    );
    add(
        &mut g,
        "PJ1",
        "External_5V_input",
        &[(1, "EXT_5V".into()), (2, ground.clone())],
    );
    // SN74LVC244A: inputs 2/4/6/8/11/13 -> outputs 18/16/14/12/9/7.
    let mut pins = vec![
        (20, ext.into()),
        (10, ground.clone()),
        (1, ground.clone()),
        (19, ground.clone()),
        (15, ground.clone()),
        (17, ground.clone()),
    ];
    for (i, pad) in [3, 4, 5, 8, 9, 10].iter().enumerate() {
        let old = base.net(&format!("A1.{pad}"))?.to_string();
        let n = format!("SEED_GPIO{pad}");
        move_pin(&mut g, &format!("A1.{pad}"), &n);
        pins.push(([2, 4, 6, 8, 11, 13][i], n));
        pins.push(([18, 16, 14, 12, 9, 7][i], old));
    }
    add(&mut g, "PU1", "SN74LVC244A", &pins);
    for p in [3, 5] {
        join(&mut g, &format!("unconnected-PU1-{p}"), format!("PU1.{p}"));
    }
    for i in 0..3 {
        let mut pins = vec![(14, seed.clone()), (7, ground.clone())];
        for ch in 0..4 {
            let pad = [22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 35][i * 4 + ch];
            let old = base.net(&format!("A1.{pad}"))?.to_string();
            let n = format!("SEED_ADC{pad}");
            move_pin(&mut g, &format!("A1.{pad}"), &n);
            pins.extend([
                ([2, 5, 9, 12][ch], old),
                ([3, 6, 8, 11][ch], n.clone()),
                ([1, 4, 10, 13][ch], guard.into()),
            ]);
            add(
                &mut g,
                &format!("PRB{pad}"),
                "100k",
                &[(1, n), (2, mid.into())],
            );
        }
        add(&mut g, &format!("PU{}", i + 2), "TMUX1511PWR", &pins);
    }
    // TLV9001 SOT23-5 buffer: out1, V-2, IN+3, IN-4, V+5.
    add(
        &mut g,
        "PU5",
        "TLV9001IDBVR",
        &[
            (1, mid.into()),
            (2, ground.clone()),
            (3, "MID_DIV".into()),
            (4, mid.into()),
            (5, seed.clone()),
        ],
    );
    add(
        &mut g,
        "PR1",
        "10k",
        &[(1, seed.clone()), (2, "MID_DIV".into())],
    );
    add(
        &mut g,
        "PR2",
        "10k",
        &[(1, "MID_DIV".into()), (2, ground.clone())],
    );
    add(
        &mut g,
        "PR3",
        "10k",
        &[(1, seed.clone()), (2, guard.into())],
    );
    // Functional guardian allocation, not an invented physical pinout or selected supervisor.
    add(
        &mut g,
        "PG1",
        "DUAL_RAIL_GUARD_UNSELECTED",
        &[
            (1, seed.clone()),
            (2, ext.into()),
            (3, ground.clone()),
            (4, guard.into()),
        ],
    );
    for (r, rail) in [("PR4", seed.clone()), ("PR5", ext.into())] {
        add(&mut g, r, "1k", &[(1, rail), (2, ground.clone())]);
    }
    for (i, rail) in [
        ext.to_string(),
        seed.clone(),
        seed.clone(),
        seed.clone(),
        seed.clone(),
    ]
    .iter()
    .enumerate()
    {
        add(
            &mut g,
            &format!("PC{}", i + 1),
            "100n",
            &[(1, rail.clone()), (2, ground.clone())],
        );
    }
    Graph::parse(&g.canonical())
}

pub fn validate_offload(parent: &Graph, candidate: &Graph) -> Result<()> {
    if offload(parent)? != *candidate {
        return Err("offload candidate has an unrelated or missing change".into());
    }
    Ok(())
}
pub fn candidate_cli(args: &[String]) -> Result<()> {
    if args.len() != 3 {
        return Err("power-candidate NAME PARENT FRESH_DIR".into());
    }
    let base = Graph::parse(&fs::read_to_string(&args[1])?)?;
    let out = Path::new(&args[2]);
    fs::create_dir(out)?;
    let g = match args[0].as_str() {
        HALL => hall_candidate(&base)?,
        OFFLOAD => offload(&base)?,
        RX => {
            crate::profiles::synth_wiring(&base)?;
            let mut g = base.clone();
            if g.components["R5"] != "220" {
                return Err("R5 baseline changed".into());
            }
            g.components.insert("R5".into(), "2.2k".into());
            g
        }
        _ => return Err("unknown power candidate".into()),
    };
    fs::write(
        out.join("candidate.net"),
        format!(
            "# parent_sha256={}\n{}",
            sha256(Path::new(&args[1]))?,
            g.canonical()
        ),
    )?;
    let mut components = String::from("reference\tbefore\tafter\n");
    for (r, v) in &g.components {
        if base.components.get(r) != Some(v) {
            components.push_str(&format!(
                "{r}\t{}\t{v}\n",
                base.components
                    .get(r)
                    .map(String::as_str)
                    .unwrap_or("ADDED")
            ));
        }
    }
    let mut connections = String::from("pin\tbefore_net\tafter_net\n");
    for (net, nodes) in &base.nets {
        for pin in nodes {
            let next = g.net(pin)?;
            if net != next {
                connections.push_str(&format!("{pin}\t{net}\t{next}\n"));
            }
        }
    }
    fs::write(out.join("component-changes.tsv"), components)?;
    fs::write(out.join("connection-changes.tsv"), connections)?;
    if args[0] == OFFLOAD {
        crate::schematics::allocation(&base, &g, &out.join("power-allocation.kicad_sch"))?;
    }
    Ok(())
}

pub fn power_cli(args: &[String]) -> Result<()> {
    if args.len() != 5 {
        return Err("power-study NAME PARENT CANDIDATE SCENARIOS FRESH_DIR".into());
    }
    let out = Path::new(&args[4]);
    fs::create_dir(out)?;
    let base = Graph::parse(&fs::read_to_string(&args[1])?)?;
    let changed = Graph::parse(&fs::read_to_string(&args[2])?)?;
    let s = Scenarios::parse(&fs::read_to_string(&args[3])?)?;
    for (p, n) in [
        (&args[1], "parent.net"),
        (&args[2], "candidate.net"),
        (&args[3], "scenarios.fixture"),
    ] {
        fs::copy(p, out.join(n))?;
    }
    fs::write(
        out.join("runner-sources.sha256"),
        env!("CIRCUIT_WORKBENCH_SOURCES").replace(';', "\n"),
    )?;
    let result = (|| -> Result<String> {
        let text = match args[0].as_str() {
            HALL => {
                if hall_candidate(&base)? != changed {
                    return Err("Hall candidate changes more than the 100 part values".into());
                }
                hall_power(&s, out)?
            }
            OFFLOAD => {
                validate_offload(&base, &changed)?;
                keyboard(&base, &changed, &s, out)?
            }
            RX => {
                let mut expected = base.clone();
                expected.components.insert("R5".into(), "2.2k".into());
                if expected != changed {
                    return Err("RX load candidate changes more than R5".into());
                }
                receiver(&base, &changed, &s, out)?
            }
            _ => return Err("unknown power study".into()),
        };
        Ok(format!("STUDY_EXECUTION=PASS\nISOLATION=PASS\nparent_sha256={}\ncandidate_sha256={}\nscenarios_sha256={}\nPHYSICAL_MEASURED=NOT_RUN\nSEED_REVISION=UNKNOWN\nORIGINAL_SOURCE=UNKNOWN\n{text}",sha256(Path::new(&args[1]))?,sha256(Path::new(&args[2]))?,sha256(Path::new(&args[3]))?))
    })();
    let report = match &result {
        Ok(r) => r.clone(),
        Err(e) => format!("STUDY_EXECUTION=FAIL\nerror={e}\n"),
    };
    fs::write(out.join("report.txt"), &report)?;
    println!("{report}");
    result?;
    Ok(())
}
fn keyboard(_base: &Graph, g: &Graph, s: &Scenarios, out: &Path) -> Result<String> {
    if g.net("A1.38")? == g.net("HE1.1")? {
        return Err("regulator outputs are paralleled".into());
    }
    // Positive steady-state isolation envelopes, explicit guardian truth table only.
    let mut states=String::from("seed_v,carrier_v,guardian_enabled,max_adc_off_v,max_delta_to_mid_v,interface_leak_a,qualification\n");
    for seed in [0., 0.5, 1., 1.5, 2.5, 3.0, 3.3] {
        for rail in [0., 3.168, 3.3, 3.432] {
            let enabled = seed >= 3.1 && rail >= 3.0;
            let status = if seed > 0. && seed < 1.5 {
                "UNQUALIFIED_SUPPLY_BAND"
            } else if seed > 0. && seed < 3.1 {
                "BROWNOUT_FUNCTIONAL_ONLY"
            } else {
                "DC_BOUND_ONLY"
            };
            let leakage = if seed == 0. { 2e-6 } else { 100e-9 };
            let displacement = leakage * g.value("PRB22")?;
            states.push_str(&format!(
                "{seed},{rail},{enabled},{},{displacement},{},{status}\n",
                seed / 2. + displacement,
                12. * leakage
            ));
            if seed == 0. && displacement >= 0.3 {
                return Err("unpowered ADC exceeds 0.3V in leakage bound".into());
            }
        }
    }
    fs::write(out.join("sequencing.csv"), states)?;
    let mut failure = String::from("calibrated_rest_v,off_bias_v,delta_adc,below_release_gate\n");
    let mut unsafe_rest = 0;
    for rest in [1.0_f64, 1.3, 1.65, 2.0, 2.3] {
        let delta = ((rest - 1.65).abs() * 4095. / 3.3).round() as u32;
        let safe = delta < 100;
        unsafe_rest += usize::from(!safe);
        failure.push_str(&format!("{rest},1.65,{delta},{safe}\n"));
    }
    if unsafe_rest == 0 {
        return Err("power loss calibrated-rest counterexample missing".into());
    }
    fs::write(out.join("power-loss-rest-counterexample.csv"), failure)?;
    let mut loading = String::from(
        "source_v,source_ohm,mux_ohm,isolator_ohm,bias_ohm,adc_v,added_static_error_v\n",
    );
    for source in [0.3_f64, 1.0, 1.65, 2.2, 3.0, 3.3] {
        for rs in [100., 1000., 3000.] {
            let series = rs + 100. + 4.5;
            let bias = g.value("PRB22")?;
            let adc = (source / series + 1.65 / bias) / (1. / series + 1. / bias);
            loading.push_str(&format!(
                "{source},{rs},100,4.5,{bias},{adc},{}\n",
                (adc - source).abs()
            ));
        }
    }
    fs::write(out.join("normal-signal-bias.csv"), loading)?;
    let mut power=String::from("sensor_a,seed_a,aux_a,efficiency,vin_v,source_limit_a,path_ohm,carrier_a,total_input_a,input_at_load_v,source_headroom_a,seed_regulator_relief_a,original_regulator_loss_w,split_seed_loss_w,external_loss_w\n");
    let mut therm=String::from("sensor_a,seed_a,aux_a,efficiency,ambient_c,enclosure_k_w,local_k_w,total_heat_w,steady_enclosure_c,original_local_c,split_local_c,required_enclosure_k_w,within_project_budget\n");
    let mut source_fails = 0;
    let mut thermal_fails = 0;
    for &hall in s.v("sensor_a") {
        for &seed in s.v("seed_a") {
            for &aux in s.v("aux_a") {
                for &eff in s.v("efficiency") {
                    // Credit only the specified Hall load to the new rail. Charge all
                    // unmeasured auxiliary loads to Seed, conservatively, even though
                    // some mux/pot loads also move in the allocation.
                    let carrier = 100. * hall;
                    let overhead = s.v("bridge_overhead_a")[0];
                    let prior_loss = 3.3 * (seed + aux + carrier) * (1. / eff - 1.);
                    let seed_loss = 3.3 * (seed + aux + overhead / 2.) * (1. / eff - 1.);
                    let extra = 3.3 * (carrier + overhead / 2.) * (1. / eff - 1.);
                    // Same-efficiency split: conservation proves offloading does not erase sensor heat.
                    if (seed_loss + extra - prior_loss - 3.3 * overhead * (1. / eff - 1.)).abs()
                        > 1e-9
                    {
                        return Err("thermal energy accounting failed".into());
                    }
                    for &vin in s.v("vin_v") {
                        for &limit in s.v("source_a") {
                            for &path in s.v("path_ohm") {
                                let watts = 3.3 * (seed + aux + carrier + overhead) / eff;
                                // Solve constant-power input with line resistance; reject collapse rather than linearize silently.
                                let discr = vin * vin - 4. * path * watts;
                                let input = if discr > 0. {
                                    (vin + discr.sqrt()) / 2.
                                } else {
                                    0.
                                };
                                let actual_a = if input > 0. {
                                    watts / input
                                } else {
                                    f64::INFINITY
                                };
                                source_fails += usize::from(actual_a > limit || input < 4.);
                                power.push_str(&format!("{hall},{seed},{aux},{eff},{vin},{limit},{path},{carrier},{actual_a},{input},{},{},{prior_loss},{seed_loss},{extra}\n",limit-actual_a,carrier-overhead/2.));
                            }
                        }
                    }
                    let heat = 3.3 * (seed + aux + carrier + overhead) / eff;
                    for &ambient in s.v("ambient_c") {
                        for &enc in s.v("enclosure_k_w") {
                            for &local in s.v("local_k_w") {
                                let air = ambient + heat * enc;
                                let prior = ambient
                                    + 3.3 * (seed + aux + carrier) / eff * enc
                                    + prior_loss * local;
                                let split = air + seed_loss * local;
                                let budget = s.v("temperature_budget_c")[0];
                                let safe = split < budget;
                                thermal_fails += usize::from(!safe);
                                therm.push_str(&format!("{hall},{seed},{aux},{eff},{ambient},{enc},{local},{heat},{air},{prior},{split},{},{safe}\n",(budget-ambient-seed_loss*local)/heat));
                            }
                        }
                    }
                }
            }
        }
    }
    fs::write(out.join("source-envelopes.csv"), power)?;
    fs::write(out.join("thermal-envelopes.csv"), therm)?;
    #[cfg(feature = "spice")]
    {
        let heat = 3.3
            * (100. * s.v("sensor_a").iter().copied().reduce(f64::max).unwrap()
                + s.v("seed_a").iter().copied().reduce(f64::max).unwrap()
                + s.v("aux_a").iter().copied().reduce(f64::max).unwrap()
                + s.v("bridge_overhead_a")[0])
            / s.v("efficiency").iter().copied().reduce(f64::min).unwrap();
        for theta in [8., 20.] {
            let capacity = s.v("heat_capacity_j_k")[0];
            let ambient = 40.;
            let deck=format!("Thermal RC analogy: V=degC,I=W,R=K/W,C=J/K; NOT calibrated enclosure\nVa ambient 0 {ambient}\nIheat 0 body PWL(0 0 0.001 {heat})\nRt body ambient {theta}\nCt body 0 {capacity} IC={ambient}\n.end\n");
            let t = simulate(
                out,
                &format!("thermal-{}", theta as u32),
                &deck,
                "tran 1 3600 0 1 uic",
                &["time", "v(body)"],
            )?;
            let predicted = temperature(ambient, heat, theta, capacity, 3600.);
            if (t.last("v(body)")? - predicted).abs() > 0.03 {
                return Err("thermal SPICE/analytic mismatch".into());
            }
        }
    }
    let low = 330. * s.v("sensor_a").iter().copied().reduce(f64::min).unwrap();
    let high = 330. * s.v("sensor_a").iter().copied().reduce(f64::max).unwrap();
    Ok(format!("SOURCE_ENVELOPE_FAILURES={source_fails}\nTHERMAL_BUDGET_FAILURES={thermal_fails}\nSENSOR_HEAT_UNCHANGED={low}-{high}W at nominal 3.3V in declared envelope\nTOTAL_HEAT_NOTE=offloading relocates heat; bridge overhead increases enclosure heat\nIDEAL_GUARD_LEAKAGE_BOUND=PASS\nPOWER_LOSS_REST_COUNTEREXAMPLES={unsafe_rest}\nREFERENCE_RELEASE_GATE=100 ADC counts; native firmware kernel tested separately\nDESIGN_ACCEPTANCE=REJECTED_FOR_PROMOTION\nBLOCKERS=guardian part/timing unselected; mid-bias cannot guarantee release for arbitrary calibrated resting magnet positions; brownout/ramp band unqualified; normal ADC bias loading; actual supply/module capacity and thermal parameters unknown\nSPICE_CROSSCHECK={}\nNOT_RUN=complete ARM device; physical thermal/noise; manufacturer transistor models\n",if cfg!(feature="spice"){ "PASS (thermal RC only)" }else{ "NOT_RUN" }))
}
fn receiver(base: &Graph, g: &Graph, s: &Scenarios, out: &Path) -> Result<String> {
    crate::profiles::synth_wiring(base)?;
    if base.value("R5")? != 220. || g.value("R5")? != 2200. {
        return Err("receiver study requires 220 ohm baseline and 2.2k candidate".into());
    }
    let mut csv=String::from("design,rail_v,pullup_ohm,sink_current_a,resistor_w,ambient_c,theta_k_w,resistor_estimate_c\n");
    for (name, graph) in [("baseline", base), ("candidate", g)] {
        for rail in [3.168, 3.3, 3.432] {
            let r = graph.value("R5")?;
            let amps = (rail - 0.2) / r;
            let p = amps * amps * r;
            for &ambient in s.v("ambient_c") {
                for theta in [200., 400., 600.] {
                    csv.push_str(&format!(
                        "{name},{rail},{r},{amps},{p},{ambient},{theta},{}\n",
                        ambient + p * theta
                    ));
                }
            }
            #[cfg(feature = "spice")]
            {
                let deck=format!("Receiver output load ONLY, not CTR or switching\nVrail rail 0 {rail}\nRpull rail rx {r}\nVsink rx 0 0.2\n.end\n");
                let t = simulate(
                    out,
                    &format!("rx-{name}-{}", (rail * 1000.) as u32),
                    &deck,
                    "op",
                    &["i(vsink)"],
                )?;
                if (t.last("i(vsink)")? - amps).abs() > 1e-9 {
                    return Err("receiver load mismatch".into());
                }
            }
        }
    }
    fs::write(out.join("receiver-thermal.csv"), csv)?;
    let mut device=String::from("design,seed_a,unmodeled_aux_a,efficiency,vin_v,source_limit_a,path_ohm,ambient_c,enclosure_k_w,local_k_w,total_input_w,source_current_a,source_at_load_v,regulator_hotspot_c,within_project_budget\n");
    for (label, graph) in [("baseline", base), ("candidate", g)] {
        let midi_input = 3.3 * (3.3 - 0.2) / graph.value("R5")?;
        for &seed in s.v("seed_a") {
            for &aux in s.v("aux_a") {
                for &eff in s.v("efficiency") {
                    let load = 3.3 * (seed + aux) + midi_input;
                    let watts = load / eff;
                    let loss = watts - load;
                    for &vin in s.v("vin_v") {
                        for &limit in s.v("source_a") {
                            for &path in s.v("path_ohm") {
                                let disc = vin * vin - 4. * path * watts;
                                let loaded = if disc > 0. {
                                    (vin + disc.sqrt()) / 2.
                                } else {
                                    0.
                                };
                                let amps = if loaded > 0. {
                                    watts / loaded
                                } else {
                                    f64::INFINITY
                                };
                                for &ambient in s.v("ambient_c") {
                                    for &enc in s.v("enclosure_k_w") {
                                        for &local in s.v("local_k_w") {
                                            let temp = ambient + watts * enc + loss * local;
                                            let pass = temp < s.v("temperature_budget_c")[0]
                                                && amps < limit
                                                && loaded >= 4.;
                                            device.push_str(&format!("{label},{seed},{aux},{eff},{vin},{limit},{path},{ambient},{enc},{local},{watts},{amps},{loaded},{temp},{pass}\n"));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    fs::write(out.join("device-envelope.csv"), device)?;
    Ok("POWER_ONLY=PASS (ideal 0.2V output sink)\nR5_NOMINAL_BEFORE_W=0.0436818\nR5_NOMINAL_AFTER_W=0.00436818\nPOWER_REDUCTION=90percent for R5 alone\nDESIGN_ACCEPTANCE=UNVERIFIED_RECEIVER\nUNKNOWN=6N138 vendor/supply/CTR/storage delay; receiver capacitance; actual resistor thermal resistance; complete device power\n".into())
}
/// Load-step bound at the datasheet's 2.5V resistance test point. Not a vendor macromodel.
#[cfg(feature = "spice")]
pub fn charge_cli(args: &[String]) -> Result<()> {
    if args.len() != 5 {
        return Err("charge-study ORIGINAL NET_1N NET_470P FIXTURE FRESH_DIR".into());
    }
    let out = Path::new(&args[4]);
    fs::create_dir(out)?;
    let graphs = args[..3]
        .iter()
        .map(|p| Graph::parse(&fs::read_to_string(p)?))
        .collect::<Result<Vec<_>>>()?;
    crate::improvements::validate(crate::improvements::FILTER, &graphs[0], &graphs[1])?;
    crate::improvements::validate(crate::improvements::FILTER470, &graphs[0], &graphs[2])?;
    let f = crate::fixture::Fixture::parse(&fs::read_to_string(&args[3])?)?;
    let mut csv=String::from("design,filter_f,source_ohm,source_current_limit_a,adc_v_at_4_5us,error_v,max_source_current_a\n");
    for (g, label) in graphs.iter().zip(["4n7", "1n", "470p"]) {
        let net = g.net("HE1.2")?;
        let ground = g.net("A1.40")?;
        let cap = g
            .components
            .keys()
            .filter(|r| r.starts_with('C'))
            .find(|r| {
                let a = g.net(&format!("{r}.1")).ok();
                let b = g.net(&format!("{r}.2")).ok();
                (a == Some(net) && b == Some(ground)) || (b == Some(net) && a == Some(ground))
            })
            .ok_or("Hall filter missing")?;
        let cf = g.value(cap)?;
        let deck=format!("2.5V output load step; bounded Thevenin 4ohm / 1.5mA clamp; no sensor loop macromodel\nVs src 0 2.5\nBdrive src hall I=max(-0.0015,min(0.0015,(v(src)-v(hall))/{}))\nCf hall 0 {cf} IC=2.5\nRm hall adc {}\nCa adc 0 {} IC=0\nRl adc 0 1e9\n.end\n",f.get("sensor_source_ohm"),f.get("mux_ron_ohm"),f.get("adc_cap_f"));
        let t = simulate(
            out,
            &format!("charge-{label}"),
            &deck,
            "tran 1n 4.5u 0 1n uic",
            &["time", "v(adc)", "i(vs)"],
        )?;
        let v = t.last("v(adc)")?;
        let err = (2.5 - v).abs();
        let current = t.vectors["i(vs)"]
            .iter()
            .map(|i| i.abs())
            .fold(0., f64::max);
        if err >= 0.001 || current > 0.0015001 {
            return Err("bounded charge-step gate failed".into());
        }
        csv.push_str(&format!(
            "{label},{cf},{},0.0015,{v},{err},{current}\n",
            f.get("sensor_source_ohm")
        ));
    }
    for (p, n) in
        args[..4]
            .iter()
            .zip(["original.net", "1n.net", "470p.net", "assumptions.fixture"])
    {
        fs::copy(p, out.join(n))?;
    }
    fs::write(
        out.join("runner-sources.sha256"),
        env!("CIRCUIT_WORKBENCH_SOURCES").replace(';', "\n"),
    )?;
    fs::write(out.join("charge-summary.csv"), csv)?;
    fs::write(out.join("report.txt"),"CHARGE_STUDY=PASS\nSOURCE_MODEL=2.5V DC test point, declared resistance and conservative +/-1.5mA clamp\nGATE=all three filters within 1mV after 4.5us\nNOT_RUN=dynamic sensor output impedance/feedback loop; sample aperture; physical load step\nDECISION=4ohm slice is conditional evidence against replacing 4.7n solely because of the older 1kohm stress fixture\n")?;
    println!("CHARGE_STUDY=PASS");
    Ok(())
}

fn hall_power(s: &Scenarios, out: &Path) -> Result<String> {
    let mut csv=String::from("corner,original_sensor_a,candidate_sensor_a,seed_a,aux_a,efficiency,ambient_c,enclosure_k_w,local_k_w,original_sensor_w,candidate_sensor_w,original_total_w,candidate_total_w,original_hotspot_c,candidate_hotspot_c,project_budget_met\n");
    for (corner, old, new) in [
        ("typical_25C", 0.006, 0.0027),
        ("specified_hot_max", 0.007, 0.0036),
    ] {
        for &seed in s.v("seed_a") {
            for &aux in s.v("aux_a") {
                for &eff in s.v("efficiency") {
                    let pold = 3.3 * (seed + aux + 100. * old);
                    let pnew = 3.3 * (seed + aux + 100. * new);
                    for &ambient in s.v("ambient_c") {
                        for &enc in s.v("enclosure_k_w") {
                            for &local in s.v("local_k_w") {
                                let before =
                                    ambient + pold / eff * enc + pold * (1. / eff - 1.) * local;
                                let after =
                                    ambient + pnew / eff * enc + pnew * (1. / eff - 1.) * local;
                                csv.push_str(&format!("{corner},{old},{new},{seed},{aux},{eff},{ambient},{enc},{local},{},{},{},{},{before},{after},{}\n",330.*old,330.*new,pold/eff,pnew/eff,after<s.v("temperature_budget_c")[0]));
                            }
                        }
                    }
                }
            }
        }
    }
    fs::write(out.join("hall-thermal.csv"), csv)?;
    Ok("SENSOR_POWER_TYPICAL_W=1.98 -> 0.891\nSENSOR_POWER_HOT_MAX_W=2.31 -> 1.188\nSENSOR_POWER_REDUCTION_TYPICAL=55percent\nSENSOR_POWER_REDUCTION_HOT_MAX=48.5714percent\nDESIGN_ACCEPTANCE=CONDITIONAL_MAGNETIC_AND_OUTPUT_QUALIFICATION\nCONDITIONS=TI current typical at25C, maximum at125C; Hall sensitivity/Vq/endpoints differ; mechanical footprint and dynamic output loading unverified\nNOT_RUN=physical magnet travel; intrinsic noise qualification; full vendor sensor model\n".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_parameters_fail_closed() {
        assert!(Scenarios::parse("efficiency=1").is_err());
        assert!(response(0., 100., 1e-9, 50e-12, 1000.).is_err());
    }
    #[test]
    fn thermal_conserves_energy_and_noise_bounds() {
        let t = temperature(40., 4., 10., 100., 1000.);
        assert!((t - (40. + 40. * (1. - (-1_f64).exp()))).abs() < 1e-12);
        let (a, b) = response(1000., 100., 4.7e-9, 50e-12, 100000.).unwrap();
        let (c, d) = response(1000., 100., 470e-12, 50e-12, 100000.).unwrap();
        assert!(a.hypot(b) < c.hypot(d));
        assert!(c.hypot(d) < 1.);
    }
}
