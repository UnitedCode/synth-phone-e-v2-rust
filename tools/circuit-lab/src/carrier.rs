//! Digital boundary adapters derived from frozen wiring. SPICE and this boundary are separate models.
//! Enable `daisy` to attach these to Virtual Daisy's Rust Machine. No per-sample analog coupling is implied.
use crate::{netlist::Graph, profiles::keyboard_routes, Result};
use std::collections::BTreeMap;

/// Retain a host-side handle while Machine owns its boxed adapter.
#[cfg(feature = "daisy")]
pub fn shared<C: virtual_daisy::machine::Carrier + 'static>(
    carrier: C,
) -> (
    std::rc::Rc<std::cell::RefCell<C>>,
    Box<dyn virtual_daisy::machine::Carrier>,
) {
    struct Shared<C>(std::rc::Rc<std::cell::RefCell<C>>);
    impl<C: virtual_daisy::machine::Carrier> virtual_daisy::machine::Carrier for Shared<C> {
        fn pins(&mut self, outputs: &[u16; 11], now: u64) -> [(u16, u16); 11] {
            self.0.borrow_mut().pins(outputs, now)
        }
        fn adc_input(
            &mut self,
            adc: u8,
            ch: u8,
            outputs: &[u16; 11],
            now: u64,
        ) -> std::result::Result<u32, virtual_daisy::machine::Error> {
            self.0.borrow_mut().adc_input(adc, ch, outputs, now)
        }
        fn i2c_write(
            &mut self,
            bus: u8,
            address: u8,
            bytes: &[u8],
        ) -> std::result::Result<(), virtual_daisy::machine::Error> {
            self.0.borrow_mut().i2c_write(bus, address, bytes)
        }
        fn next_event(&self) -> Option<u64> {
            self.0.borrow().next_event()
        }
    }
    let handle = std::rc::Rc::new(std::cell::RefCell::new(carrier));
    (handle.clone(), Box::new(Shared(handle)))
}

pub struct KeyboardCarrier {
    routes: Vec<(String, String, usize, String)>,
    inputs: BTreeMap<String, u32>,
    pub samples: u64,
    pub oled_writes: u64,
}
impl KeyboardCarrier {
    pub fn new(graph: &Graph) -> Result<Self> {
        if graph.net("U1.7")? == graph.net("A1.40")? || graph.net("U1.9")? == graph.net("A1.40")? {
            return Err(
                "digital-only carrier refuses the pre-repair output shorts; use its SPICE profile"
                    .into(),
            );
        }
        let routes = keyboard_routes(graph)?;
        crate::profiles::decoder(graph, false)?;
        Ok(Self {
            inputs: routes
                .iter()
                .map(|(r, _, _, _)| (r.clone(), 1_650_000))
                .collect(),
            routes,
            samples: 0,
            oled_writes: 0,
        })
    }
    /// Supply volts at the fitted Hall output or pot wiper; no magnetic travel curve is invented.
    pub fn set_microvolts(&mut self, control: &str, value: u32) -> Result<()> {
        if value > 3_300_000 {
            return Err("carrier input exceeds 3.3V".into());
        }
        *self.inputs.get_mut(control).ok_or("unknown control")? = value;
        Ok(())
    }
    pub fn sample(&mut self, adc: u8, outputs: &[u16; 11]) -> Result<u32> {
        let channel = ((outputs[6] >> 10) & 1)
            | (((outputs[6] >> 11) & 1) << 1)
            | (((outputs[1] >> 4) & 1) << 2);
        let decoder = ((outputs[2] >> 8) & 1)
            | (((outputs[2] >> 9) & 1) << 1)
            | (((outputs[2] >> 10) & 1) << 2);
        let pad = match adc {
            10 => 22,
            15 => 23,
            5 => 24,
            7 => 25,
            3 => 26,
            11 => 27,
            4 => 28,
            19 => 29,
            18 => 30,
            17 => 31,
            16 => 32,
            14 => 35,
            _ => return Err("ADC not connected to keyboard".into()),
        };
        let enabled = match pad {
            22..=30 => Some(pad - 21),
            31 if decoder < 2 => Some(10 + decoder),
            32 if (2..4).contains(&decoder) => Some(10 + decoder),
            35 if (4..6).contains(&decoder) => Some(10 + decoder),
            _ => None,
        };
        let mux = enabled.ok_or("no mux enabled on shared ADC")?;
        self.samples += 1;
        if let Some((r, _, _, _)) = self.routes.iter().find(|(_, m, ch, a)| {
            m == &format!("AM{mux}") && *ch == channel as usize && a == &format!("A1.{pad}")
        }) {
            return Ok(self.inputs[r]);
        }
        // Unpopulated channels are grounded in this board design.
        Ok(0)
    }
}

pub struct SynthphoneCarrier {
    keys: [[bool; 3]; 4],
    pub oled_writes: u64,
    pub fault: Option<String>,
}
impl SynthphoneCarrier {
    pub fn new(graph: &Graph) -> Result<Self> {
        for (pin, name) in [
            (22, "row1"),
            (23, "row2"),
            (24, "row3"),
            (25, "row4"),
            (26, "col1"),
            (27, "col2"),
            (28, "col3"),
        ] {
            if graph.net(&format!("A1.{pin}"))? != name {
                return Err("keypad pad mapping differs".into());
            }
        }
        Ok(Self {
            keys: [[false; 3]; 4],
            oled_writes: 0,
            fault: None,
        })
    }
    pub fn contact(&mut self, row: usize, column: usize, closed: bool) -> Result<()> {
        *self
            .keys
            .get_mut(row)
            .and_then(|r| r.get_mut(column))
            .ok_or("keypad position out of range")? = closed;
        Ok(())
    }
    pub fn row_contacts(&self, outputs: &[u16; 11]) -> Result<[(u16, u16); 11]> {
        // Physical cols 1,2,3 = PA6,PC1,PC4. Firmware scans them in reverse order.
        let columns = [
            outputs[0] & (1 << 6) != 0,
            outputs[2] & (1 << 1) != 0,
            outputs[2] & (1 << 4) != 0,
        ];
        let rows = [(2, 0), (0, 3), (1, 1), (0, 7)];
        let mut inputs = [(0, 0); 11];
        for (row, keys) in self.keys.iter().enumerate() {
            let mut levels = Vec::new();
            for (col, pressed) in keys.iter().enumerate() {
                if *pressed {
                    levels.push(columns[col]);
                }
            }
            if levels.contains(&true) && levels.contains(&false) {
                return Err("keypad row connects opposing push-pull columns".into());
            }
            if let Some(high) = levels.first() {
                let (port, pin) = rows[row];
                inputs[port].0 |= 1 << pin;
                if *high {
                    inputs[port].1 |= 1 << pin;
                }
            }
        }
        Ok(inputs)
    }
}

#[cfg(feature = "daisy")]
impl virtual_daisy::machine::Carrier for KeyboardCarrier {
    fn adc_input(
        &mut self,
        adc: u8,
        channel: u8,
        outputs: &[u16; 11],
        _: u64,
    ) -> std::result::Result<u32, virtual_daisy::machine::Error> {
        if adc != 1 {
            return Err("only keyboard ADC1 is connected".into());
        }
        self.sample(channel, outputs)
            .map_err(|_| "keyboard ADC has no enabled route".into())
    }
    fn i2c_write(
        &mut self,
        bus: u8,
        address: u8,
        _: &[u8],
    ) -> std::result::Result<(), virtual_daisy::machine::Error> {
        if bus != 1 || address != 0x3c {
            return Err("unmodeled keyboard I2C device".into());
        }
        self.oled_writes += 1;
        Ok(())
    }
}
#[cfg(feature = "daisy")]
impl virtual_daisy::machine::Carrier for SynthphoneCarrier {
    fn pins(&mut self, outputs: &[u16; 11], _: u64) -> [(u16, u16); 11] {
        match self.row_contacts(outputs) {
            Ok(inputs) => inputs,
            Err(error) => {
                self.fault = Some(error.to_string());
                [(0, 0); 11]
            }
        }
    }
    fn i2c_write(
        &mut self,
        bus: u8,
        address: u8,
        _: &[u8],
    ) -> std::result::Result<(), virtual_daisy::machine::Error> {
        if self.fault.is_some() {
            return Err("keypad contention detected; stop this run".into());
        }
        if bus != 1 || address != 0x3c {
            return Err("unmodeled Synthphone I2C device".into());
        }
        self.oled_writes += 1;
        Ok(())
    }
}
