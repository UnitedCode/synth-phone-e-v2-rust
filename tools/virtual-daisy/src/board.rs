//! Physical Seed variants. Sources and evidence limits: docs/daisy-conformance.md.
use std::str::FromStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Board {
    Rev4,
    Rev5,
    Rev7,
}
impl Board {
    pub const ALL: [Self; 3] = [Self::Rev4, Self::Rev5, Self::Rev7];
    pub fn id(self) -> &'static str {
        match self {
            Self::Rev4 => "rev4",
            Self::Rev5 => "rev5",
            Self::Rev7 => "rev7",
        }
    }
    pub fn codec(self) -> &'static str {
        match self {
            Self::Rev4 => "AK4556",
            Self::Rev5 => "WM8731",
            Self::Rev7 => "PCM3060",
        }
    }
    pub fn rx_channel(self) -> u32 {
        u32::from(self != Self::Rev5)
    }
    pub fn codec_kind(self) -> u32 {
        match self {
            Self::Rev4 => 9,
            Self::Rev5 => 4,
            Self::Rev7 => 10,
        }
    }
    pub fn grounded_straps(self) -> u32 {
        match self {
            Self::Rev4 => 0,
            Self::Rev5 => 1,
            Self::Rev7 => 4,
        }
    }
    pub fn validate_codec(self, codec: &str) -> Result<(), String> {
        if self.codec().eq_ignore_ascii_case(codec) {
            Ok(())
        } else {
            Err(format!(
                "{} has {}, not {codec}; choose the matching board revision",
                self.id(),
                self.codec()
            ))
        }
    }
    pub fn description(self) -> String {
        let (rx, tx) = if self.rx_channel() == 0 {
            ('A', 'B')
        } else {
            ('B', 'A')
        };
        format!(
            "{} {} RX={rx} TX={tx} hardware_equivalence=UNVERIFIED",
            self.id(),
            self.codec()
        )
    }
}
impl FromStr for Board {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value { "rev4" => Ok(Self::Rev4), "rev5" => Ok(Self::Rev5), "rev7" => Ok(Self::Rev7), _ => Err(format!("Unsupported board {value}; supported profiles: rev4, rev5, rev7. Seed2 DFM and Seed3 are separate hardware, never aliases.")) }
    }
}
