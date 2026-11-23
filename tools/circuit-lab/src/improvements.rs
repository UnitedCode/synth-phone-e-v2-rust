//! Isolated, reproducible circuit candidates. Never edits a production design.
use crate::{fixture::Fixture, netlist::Graph, sha256, Result};
use std::{fs, path::Path};
pub const DIODES: &str = "synthphone-key-diodes";
pub const FILTER: &str = "keyboard-filter-1n";
pub const FILTER470: &str = "keyboard-filter-470p";

/// Return false for the original external harness. A partial/reversed candidate is an error.
pub fn diode_harness(g: &Graph) -> Result<bool> {
    if !g
        .components
        .keys()
        .any(|r| r.starts_with("KD") || r.starts_with("KS"))
    {
        return Ok(false);
    }
    for r in 0..4 {
        for c in 0..3 {
            let n = r * 3 + c + 1;
            if g.components.get(&format!("KD{n}")).map(String::as_str) != Some("BAT54H")
                || g.components.get(&format!("KS{n}")).map(String::as_str) != Some("SW_Push")
            {
                return Err("incomplete keypad diode harness".into());
            }
            g.connected(&format!("KD{n}.2"), &format!("A1.{}", 22 + r))?;
            g.connected(&format!("KD{n}.1"), &format!("KS{n}.1"))?;
            g.connected(&format!("KS{n}.2"), &format!("A1.{}", 26 + c))?;
            if g.nets[g.net(&format!("KD{n}.1"))?].len() != 2 {
                return Err("diode bypass or shared key junction".into());
            }
        }
    }
    Ok(true)
}

pub fn candidate(name: &str, base: &Graph) -> Result<Graph> {
    let mut g = base.clone();
    match name {
        DIODES => {
            crate::profiles::synth_wiring(base)?;
            if diode_harness(base)? {
                return Err("baseline already has key diodes".into());
            }
            for r in 0..4 {
                for c in 0..3 {
                    let n = r * 3 + c + 1;
                    g.components.insert(format!("KD{n}"), "BAT54H".into());
                    g.components.insert(format!("KS{n}"), "SW_Push".into());
                    g.nets
                        .get_mut(&format!("row{}", r + 1))
                        .unwrap()
                        .push(format!("KD{n}.2"));
                    g.nets
                        .get_mut(&format!("col{}", c + 1))
                        .unwrap()
                        .push(format!("KS{n}.2"));
                    g.nets.insert(
                        format!("key{n}"),
                        vec![format!("KD{n}.1"), format!("KS{n}.1")],
                    );
                }
            }
        }
        FILTER | FILTER470 => {
            crate::profiles::keyboard_routes(base)?;
            let mut count = 0;
            for (r, value) in &mut g.components {
                if r.starts_with('C') && value == "4.7n" {
                    *value = if name == FILTER { "1n" } else { "470p" }.into();
                    count += 1;
                }
            }
            if count != 100 {
                return Err("expected exactly 100 Hall output filter capacitors".into());
            }
        }
        _ => return Err("unknown candidate".into()),
    }
    Graph::parse(&g.canonical())
}

pub fn validate(name: &str, base: &Graph, changed: &Graph) -> Result<()> {
    if candidate(name, base)? != *changed {
        return Err("candidate includes changes outside its single experiment".into());
    }
    Ok(())
}

pub fn cli(args: &[String]) -> Result<()> {
    match args[0].as_str() {
        "candidate" if args.len() == 5 => {
            let base = Graph::parse(&fs::read_to_string(&args[2])?)?;
            let out = Path::new(&args[4]);
            fs::create_dir(out)?;
            let changed = candidate(&args[1], &base)?;
            fs::write(out.join("candidate.net"), format!("# parent_sha256={}\n{}", sha256(Path::new(&args[2]))?, changed.canonical()))?;
            fs::copy(&args[3], out.join("assumptions.fixture"))?;
            let mut delta = String::from("reference,before,after\n");
            for (r, v) in &changed.components {
                if base.components.get(r) != Some(v) {
                    delta.push_str(&format!("{r},{},{v}\n", base.components.get(r).map(String::as_str).unwrap_or("ADDED")));
                }
            }
            fs::write(out.join("changes.csv"), delta)?;
        }
        "compare" if args.len() == 6 => {
            let out = Path::new(&args[5]);
            fs::create_dir(out)?;
            let base = Graph::parse(&fs::read_to_string(&args[2])?)?;
            let changed = Graph::parse(&fs::read_to_string(&args[3])?)?;
            let f = Fixture::parse(&fs::read_to_string(&args[4])?)?;
            fs::copy(&args[2], out.join("parent.net"))?;
            fs::copy(&args[3], out.join("candidate.net"))?;
            fs::copy(&args[4], out.join("assumptions.fixture"))?;
            fs::write(out.join("runner-sources.sha256"), env!("CIRCUIT_WORKBENCH_SOURCES").replace(';',"\n"))?;
            let result = (|| -> Result<String> {
                validate(&args[1], &base, &changed)?;
                let mut report = format!("ISOLATION=PASS\nparent_sha256={}\ncandidate_sha256={}\nfixture_sha256={}\nPHYSICAL_MEASURED=NOT_RUN\n", sha256(Path::new(&args[2]))?,sha256(Path::new(&args[3]))?,sha256(Path::new(&args[4]))?);
                match args[1].as_str() {
                    DIODES => report.push_str(&key_comparison(&base, &changed, &f, out)?),
                    FILTER | FILTER470 => {
                        for (label, g) in [("baseline", &base), ("candidate", &changed)] {
                            let dir=out.join(label); fs::create_dir(&dir)?;
                            let r=crate::profiles::verify("keyboard-current",g,&f,&dir)?;
                            fs::write(dir.join("report.txt"),r)?;
                        }
                        #[cfg(feature="spice")]
                        report.push_str(&filter_comparison(&base,&changed,&f,out)?);
                        #[cfg(not(feature="spice"))]
                        report.push_str("ACQUISITION=NOT_RUN\n");
                    }
                    _ => unreachable!(),
                }
                Ok(report)
            })();
            let report=match &result {Ok(r)=>format!("COMPARISON=PASS\nexperiment={}\n{r}",args[1]),Err(e)=>format!("COMPARISON=FAIL\nerror={e}\n")};
            fs::write(out.join("report.txt"),&report)?; println!("{report}"); result?;
        }
        _ => return Err("candidate NAME PARENT FIXTURE FRESH_DIR | compare NAME PARENT CANDIDATE FIXTURE FRESH_DIR".into()),
    }
    Ok(())
}

fn key_comparison(base: &Graph, g: &Graph, f: &Fixture, out: &Path) -> Result<String> {
    use crate::carrier::SynthphoneCarrier;
    let mut original = SynthphoneCarrier::new(base)?;
    original.contact(0, 0, true)?;
    original.contact(0, 1, true)?;
    let mut outputs = [0; 11];
    outputs[2] = (1 << 1) | (1 << 4);
    if original.row_contacts(&outputs).is_ok() {
        return Err("baseline contention negative escaped".into());
    }
    // Exhaustive contact sets include every rectangle, every same-row chord and all-held.
    let mut csv = String::from("mask,scan_column,expected_low_rows,observed_low_rows\n");
    for mask in 0..4096u16 {
        let mut c = SynthphoneCarrier::new(g)?;
        for r in 0..4 {
            for col in 0..3 {
                c.contact(r, col, mask & (1 << (r * 3 + col)) != 0)?;
            }
        }
        for scan in 0..3 {
            let mut odr = [0; 11];
            odr[0] = 1 << 6;
            odr[2] = (1 << 1) | (1 << 4);
            let (p, b) = [(0, 6), (2, 1), (2, 4)][scan];
            odr[p] &= !(1 << b);
            let inputs = c.row_contacts(&odr)?;
            let mut actual = 0;
            let mut expected = 0;
            for (r, (p, b)) in [(2, 0), (0, 3), (1, 1), (0, 7)].iter().enumerate() {
                if inputs[*p].0 & (1 << b) != 0 && inputs[*p].1 & (1 << b) == 0 {
                    actual |= 1 << r;
                }
                if mask & (1 << (r * 3 + scan)) != 0 {
                    expected |= 1 << r;
                }
            }
            if actual != expected {
                return Err("diode matrix phantom/missing contact".into());
            }
            csv.push_str(&format!("{mask},{scan},{expected},{actual}\n"));
        }
    }
    fs::write(out.join("exhaustive-contacts.csv"), csv)?;
    // Deliberately reverse one diode: wiring gate must reject it before simulation.
    let mut broken = g.clone();
    for nodes in broken.nets.values_mut() {
        for pin in nodes {
            if pin == "KD1.1" {
                *pin = "KD1.2".into();
            } else if pin == "KD1.2" {
                *pin = "KD1.1".into();
            }
        }
    }
    if diode_harness(&broken).is_ok() {
        return Err("reversed diode escaped".into());
    }
    let mut report=String::from("DIGITAL_CONTACT_CASES=12288\nphantom_or_missing_contacts=0\nbaseline_contention_negative=PASS\nreversed_diode_negative=PASS\n");
    #[cfg(feature = "spice")]
    report.push_str(&key_analog(f, out)?);
    #[cfg(not(feature = "spice"))]
    {
        let _ = f;
        report.push_str("ANALOG_MARGINS=NOT_RUN\n");
    }
    Ok(report)
}

#[cfg(feature = "spice")]
fn solve(
    out: &Path,
    name: &str,
    deck: &str,
    analysis: &str,
    names: &[&str],
) -> Result<crate::spice::Trace> {
    fs::write(out.join(format!("{name}.cir")), deck)?;
    let trace = crate::spice::solve(deck, analysis, names)?;
    fs::write(out.join(format!("{name}.csv")), trace.csv()?)?;
    fs::write(out.join(format!("{name}.log")), &trace.log)?;
    Ok(trace)
}

#[cfg(feature = "spice")]
fn key_analog(f: &Fixture, out: &Path) -> Result<String> {
    // Piecewise constant-drop diode bound, NOT a manufacturer transistor model.
    // Forward <=0.32V at <=1mA and reverse <=2uA at 25C, per BAT54H table.
    let mut csv =
        String::from("vdd,pullup_ohm,driver_ohm,low_row_v,high_row_min_v,column_sink_a\n");
    let mut worst_low: f64 = 0.;
    let mut min_margin = f64::INFINITY;
    for vdd in [3.0, 3.3, 3.6] {
        for pull in [30000., 50000.] {
            for driver in [f.get("driver_ohm"), 100.] {
                let mut deck=format!("All twelve held; worst bounded diode drop and leakage at 25C\nVrail rail 0 {vdd}\nRdriver col 0 {driver}\n");
                for r in 0..4 {
                    deck.push_str(&format!("Rp{r} rail row{r} {pull}\nBd{r} row{r} col I=max(0,(v(row{r},col)-0.32)/{})\nIl{r} rail row{r} 4u\n",f.get("contact_ohm")));
                }
                deck.push_str(".end\n");
                let t = solve(
                    out,
                    &format!(
                        "diode-{}-{}-{}",
                        (vdd * 1000.) as u32,
                        pull as u32,
                        driver as u32
                    ),
                    &deck,
                    "op",
                    &["v(row0)", "v(col)"],
                )?;
                let low = t.last("v(row0)")?;
                // Conservative HIGH bound: all three diodes leak out of a released row.
                let high = vdd - 3. * 2e-6 * pull;
                let sink = t.last("v(col)")? / driver;
                if low >= 0.3 * vdd || high <= 0.7 * vdd || sink >= 0.001 {
                    return Err("keypad electrical corner failed".into());
                }
                worst_low = worst_low.max(low);
                min_margin = min_margin.min(0.3 * vdd - low).min(high - 0.7 * vdd);
                csv.push_str(&format!("{vdd},{pull},{driver},{low},{high},{sink}\n"));
            }
        }
    }
    fs::write(out.join("diode-corners.csv"), csv)?;
    Ok(format!("ANALOG_MARGINS=PASS (12 bounded DC corners,25C)\nworst_low_row_v={worst_low}\nminimum_logic_margin_v={min_margin}\nDESIGN_ACCEPTANCE=VIRTUAL_CANDIDATE (25C bounded model)\nNOT_RUN=temperature extremes; cable capacitance; contact bounce; physical harness\n"))
}

#[cfg(feature = "spice")]
fn filter_comparison(base: &Graph, changed: &Graph, f: &Fixture, out: &Path) -> Result<String> {
    let mut csv=String::from("design,source_ohm,mux_ohm,cap_tolerance,adc_cap_f,error_at_4_5us_v,cutoff_hz,noise_gain_100khz\n");
    let mut nominal = [0.; 2];
    for (i, (label, g)) in [("baseline", base), ("candidate", changed)]
        .iter()
        .enumerate()
    {
        // Extract the capacitor connected to an actual Hall OUT; do not assume its reference.
        let net = g.net("HE1.2")?;
        let cap = g
            .components
            .keys()
            .filter(|r| r.starts_with('C'))
            .find(|r| {
                let a = g.net(&format!("{r}.1")).ok();
                let b = g.net(&format!("{r}.2")).ok();
                (a == Some(net) && b == g.net("A1.40").ok())
                    || (b == Some(net) && a == g.net("A1.40").ok())
            })
            .ok_or("Hall filter capacitor missing")?;
        let capacitance = g.value(cap)?;
        // Full-scale magnetic-output step is an additional stress, not the earlier unique-voltage scan.
        for source in [100., f.get("sensor_source_ohm"), 3000.] {
            for ron in [f.get("mux_ron_ohm"), 500.] {
                for tol in [0.9, 1.1] {
                    let c = capacitance * tol;
                    let deck=format!("Full scale step; provisional sensor/ADC model\nVs src 0 PWL(0 0 1n 0 2n 3.3)\nRs src sensor {source}\nCf sensor 0 {c}\nRm sensor adc {ron}\nCa adc 0 {}\nRl adc 0 1e9\n.end\n",f.get("adc_cap_f"));
                    let t = solve(
                        out,
                        &format!(
                            "step-{label}-{}-{}-{}",
                            source as u32,
                            ron as u32,
                            (tol * 100.) as u32
                        ),
                        &deck,
                        "tran 10n 4.5u 0 10n",
                        &["time", "v(adc)"],
                    )?;
                    let error = (3.3 - t.last("v(adc)")?).abs();
                    let cutoff = 1. / (2. * std::f64::consts::PI * source * c);
                    let gain = 1. / (1. + (100000. / cutoff).powi(2)).sqrt();
                    if source == f.get("sensor_source_ohm")
                        && ron == f.get("mux_ron_ohm")
                        && tol == 1.1
                    {
                        nominal[i] = error;
                    }
                    csv.push_str(&format!(
                        "{label},{source},{ron},{tol},{},{error},{cutoff},{gain}\n",
                        f.get("adc_cap_f")
                    ));
                }
            }
        }
    }
    fs::write(out.join("filter-corners.csv"), csv)?;
    if nominal[1] >= nominal[0] {
        return Err("filter candidate did not improve settling".into());
    }
    let before = fs::read_to_string(out.join("baseline/report.txt"))?;
    let after = fs::read_to_string(out.join("candidate/report.txt"))?;
    let count = |s: &str| -> Result<usize> {
        Ok(s.lines()
            .find_map(|l| l.strip_prefix("acquisition_1mv_cases_failed="))
            .ok_or("missing scan result")?
            .parse()?)
    };
    if count(&after)? >= count(&before)? {
        return Err("full mux comparison did not improve".into());
    }
    Ok(format!("ACQUISITION=IMPROVED\nfull_scan_failures_before={}\nfull_scan_failures_after={}\nfull_scale_step_error_before_v={}\nfull_scale_step_error_after_v={}\nDESIGN_ACCEPTANCE=CONDITIONAL (weaker high-frequency filtering; source impedance unmeasured)\nNOT_RUN=physical noise; Hall magnetic dynamics; temperature; EMI\n",count(&before)?,count(&after)?,nominal[0],nominal[1]))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_candidates_fail_closed() {
        assert!(candidate("typo", &Graph::default()).is_err());
    }
}
