use crate::Result;
use std::collections::BTreeMap;

/// Positive, finite electrical assumptions are kept outside the frozen connectivity.
#[derive(Clone)]
pub struct Fixture(BTreeMap<String, f64>);
impl Fixture {
    pub fn parse(text: &str) -> Result<Self> {
        let mut values = BTreeMap::new();
        for line in text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            let (key, value) = line.split_once('=').ok_or("fixture requires key=value")?;
            let key = key.trim();
            if ![
                "driver_ohm",
                "contact_ohm",
                "pullup_ohm",
                "mux_ron_ohm",
                "sensor_source_ohm",
                "adc_cap_f",
            ]
            .contains(&key)
            {
                return Err(format!("unknown fixture parameter {key}").into());
            }
            let value = value.trim().parse::<f64>()?;
            if !value.is_finite() || value <= 0. || values.insert(key.into(), value).is_some() {
                return Err("invalid or duplicate fixture parameter".into());
            }
        }
        for key in [
            "driver_ohm",
            "contact_ohm",
            "pullup_ohm",
            "mux_ron_ohm",
            "sensor_source_ohm",
            "adc_cap_f",
        ] {
            if !values.contains_key(key) {
                return Err(format!("missing fixture parameter {key}").into());
            }
        }
        Ok(Self(values))
    }
    pub fn get(&self, key: &str) -> f64 {
        self.0[key]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_assumptions_fail_closed() {
        assert!(Fixture::parse("driver_ohm=NaN").is_err());
        assert!(Fixture::parse("driver_ohm=-1").is_err());
        assert!(Fixture::parse("silent_typo=33").is_err());
        assert!(Fixture::parse("").is_err());
    }
}
