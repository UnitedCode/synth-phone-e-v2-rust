//! Event-driven solution of the declared keyboard RC/mux network.
//! GPIO changes evolve the previous topology before switching. ADC callbacks read circuit state.
use crate::{fixture::Fixture, netlist::Graph, profiles, Result};

#[derive(Clone, Copy)]
struct Input {
    source: f64,
    voltage: f64,
    cap: f64,
}
pub struct KeyboardCircuit {
    slots: [[Option<Input>; 8]; 15],
    controls: Vec<(String, usize, usize)>,
    adc: [f64; 12],
    outputs: [u16; 11],
    now: u64,
    rs: f64,
    ron: f64,
    ca: f64,
    pub open_control: Option<String>,
}
fn selection(outputs: &[u16; 11]) -> (usize, usize) {
    let ch =
        ((outputs[6] >> 10) & 1) | (((outputs[6] >> 11) & 1) << 1) | (((outputs[1] >> 4) & 1) << 2);
    let dec =
        ((outputs[2] >> 8) & 1) | (((outputs[2] >> 9) & 1) << 1) | (((outputs[2] >> 10) & 1) << 2);
    (ch as usize, dec as usize)
}
fn enabled(adc: usize, decoder: usize) -> Option<usize> {
    match adc {
        0..=8 => Some(adc),
        9 if decoder < 2 => Some(9 + decoder),
        10 if (2..4).contains(&decoder) => Some(9 + decoder),
        11 if (4..6).contains(&decoder) => Some(9 + decoder),
        _ => None,
    }
}
pub fn adc_index(channel: u8) -> Result<usize> {
    [10, 15, 5, 7, 3, 11, 4, 19, 18, 17, 16, 14]
        .iter()
        .position(|v| *v == channel)
        .ok_or_else(|| "unconnected ADC channel".into())
}
/// Stable closed-form exponential for two passive nodes; no integration timestep is chosen.
pub fn rc_pair(
    vi: f64,
    va: f64,
    source: f64,
    rs: f64,
    ron: f64,
    ci: f64,
    ca: f64,
    dt: f64,
) -> (f64, f64) {
    if dt <= 0. {
        return (vi, va);
    }
    let rl = 1e9;
    if ci <= 0. {
        let steady = source * rl / (rs + ron + rl);
        return (
            source,
            steady + (va - steady) * (-dt / ((rs + ron) * ca)).exp(),
        );
    }
    let a = -(1. / rs + 1. / ron) / ci;
    let b = 1. / (ron * ci);
    let c = 1. / (ron * ca);
    let d = -(1. / ron + 1. / rl) / ca;
    let mid = (a + d) * 0.5;
    let delta = (((a - d) * 0.5).powi(2) + b * c).sqrt();
    let l1 = mid + delta;
    let l2 = mid - delta;
    let e1 = (l1 * dt).exp();
    let e2 = (l2 * dt).exp();
    let den = l1 - l2;
    let i_ss = source * (ron + rl) / (rs + ron + rl);
    let a_ss = source * rl / (rs + ron + rl);
    let x = vi - i_ss;
    let y = va - a_ss;
    (
        i_ss + ((e1 * (a - l2) - e2 * (a - l1)) * x + (e1 - e2) * b * y) / den,
        a_ss + ((e1 - e2) * c * x + (e1 * (d - l2) - e2 * (d - l1)) * y) / den,
    )
}
impl KeyboardCircuit {
    pub fn new(g: &Graph, f: &Fixture) -> Result<Self> {
        profiles::decoder(g, false)?;
        let mut slots = [[None; 8]; 15];
        let mut controls = Vec::new();
        for (r, m, ch, _) in profiles::keyboard_routes(g)? {
            let mux = m[2..].parse::<usize>()? - 1;
            let net = g.net(&format!("{r}.2"))?;
            let mut cap = 0.;
            for n in &g.nets[net] {
                if let Some((reference, pin)) = n.rsplit_once('.') {
                    if reference.starts_with('C') && ["1", "2"].contains(&pin) {
                        let other = if pin == "1" { "2" } else { "1" };
                        if g.net(&format!("{reference}.{other}"))? == g.net("A1.40")? {
                            cap += g.value(reference)?;
                        }
                    }
                }
            }
            let source = if r.starts_with("HE") { 1.65 } else { 0. };
            slots[mux][ch] = Some(Input {
                source,
                voltage: source,
                cap,
            });
            controls.push((r, mux, ch));
        }
        Ok(Self {
            slots,
            controls,
            adc: [0.; 12],
            outputs: [0; 11],
            now: 0,
            rs: f.get("sensor_source_ohm"),
            ron: f.get("mux_ron_ohm"),
            ca: f.get("adc_cap_f"),
            open_control: None,
        })
    }
    pub fn set_microvolts(&mut self, now: u64, control: &str, value: u32) -> Result<()> {
        if value > 3_300_000 {
            return Err("source exceeds rail".into());
        }
        self.advance(now)?;
        let (_, mux, ch) = self
            .controls
            .iter()
            .find(|(r, _, _)| r == control)
            .ok_or("unknown control")?;
        self.slots[*mux][*ch].as_mut().unwrap().source = f64::from(value) * 1e-6;
        Ok(())
    }
    fn resistance(&self, mux: usize, ch: usize) -> f64 {
        if self.open_control.as_ref().is_some_and(|r| {
            self.controls
                .iter()
                .any(|(name, m, c)| name == r && *m == mux && *c == ch)
        }) {
            1e12
        } else {
            self.rs
        }
    }
    fn advance(&mut self, now: u64) -> Result<()> {
        if now < self.now {
            return Err("circuit time moved backwards".into());
        }
        let dt = (now - self.now) as f64 * 1e-9;
        if dt == 0. {
            return Ok(());
        }
        let (ch, dec) = selection(&self.outputs);
        for mux in 0..15 {
            for channel in 0..8 {
                let selected = channel == ch && (mux < 9 || mux == 9 + dec);
                if !selected {
                    let rs = self.resistance(mux, channel);
                    if let Some(input) = &mut self.slots[mux][channel] {
                        input.voltage = if input.cap > 0. {
                            input.source
                                + (input.voltage - input.source) * (-dt / (rs * input.cap)).exp()
                        } else {
                            input.source
                        };
                    }
                }
            }
        }
        for adc in 0..12 {
            if let Some(mux) = enabled(adc, dec) {
                let rs = self.resistance(mux, ch);
                if let Some(input) = &mut self.slots[mux][ch] {
                    let (vi, va) = rc_pair(
                        input.voltage,
                        self.adc[adc],
                        input.source,
                        rs,
                        self.ron,
                        input.cap,
                        self.ca,
                        dt,
                    );
                    input.voltage = vi;
                    self.adc[adc] = va;
                } else {
                    self.adc[adc] *= (-dt / (self.ron * self.ca)).exp();
                }
            } else {
                self.adc[adc] *= (-dt / (1e9 * self.ca)).exp();
            }
        }
        self.now = now;
        Ok(())
    }
    pub fn observe(&mut self, now: u64, outputs: &[u16; 11]) -> Result<()> {
        self.advance(now)?;
        self.outputs = *outputs;
        Ok(())
    }
    pub fn sample(&mut self, now: u64, channel: u8, outputs: &[u16; 11]) -> Result<u32> {
        self.observe(now, outputs)?;
        let voltage = self.adc[adc_index(channel)?];
        if !voltage.is_finite() || !(-1e-6..=3.300001).contains(&voltage) {
            return Err("invalid circuit voltage".into());
        }
        Ok((voltage.clamp(0., 3.3) * 1e6).round() as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn event_solution_is_partition_independent() {
        let whole = rc_pair(1.65, 0., 2.2, 1000., 100., 4.7e-9, 50e-12, 5e-6);
        let first = rc_pair(1.65, 0., 2.2, 1000., 100., 4.7e-9, 50e-12, 2e-6);
        let split = rc_pair(first.0, first.1, 2.2, 1000., 100., 4.7e-9, 50e-12, 3e-6);
        assert!((whole.0 - split.0).abs() < 1e-10);
        assert!((whole.1 - split.1).abs() < 1e-10);
    }
    #[cfg(feature = "spice")]
    #[test]
    fn coupled_rc_matches_independent_ngspice_transient() {
        let deck="RC reference\nVsource src 0 PULSE(0 2.2 1u 1n 1n 1m 2m)\nRs src sensor 1k\nCi sensor 0 4.7n\nRon sensor adc 100\nCa adc 0 50p\nRload adc 0 1e9\n.end\n";
        let trace = crate::spice::solve(deck, "tran 1n 30u 0 1n", &["time", "v(sensor)", "v(adc)"])
            .unwrap();
        let times = &trace.vectors["time"];
        for t in [2e-6, 5e-6, 10e-6, 20e-6] {
            let index = times.iter().position(|v| *v >= t).unwrap();
            let actual = rc_pair(
                0.,
                0.,
                2.2,
                1000.,
                100.,
                4.7e-9,
                50e-12,
                times[index] - 1.0005e-6,
            );
            assert!((actual.0 - trace.vectors["v(sensor)"][index]).abs() < 0.001);
            assert!((actual.1 - trace.vectors["v(adc)"][index]).abs() < 0.001);
        }
    }
}
