//! Functional stereo SAI FIFO/PCM boundary; clock and analog timing are not calibrated.
use std::collections::VecDeque;

#[derive(Default)]
pub struct Sai {
    registers: [u32; 18],
    rx: VecDeque<u32>,
    tx: VecDeque<u32>,
    pub input: Vec<[f32; 2]>,
    pub output: Vec<[f32; 2]>,
    pub frames: usize,
    pub underruns: u64,
    pub overruns: u64,
    pub drop_rx: bool,
}
impl Sai {
    fn level(words: usize) -> u32 {
        if words == 8 {
            5
        } else {
            words.div_ceil(2) as u32
        }
    }
    fn packed(sample: f32) -> u32 {
        (sample.clamp(-0.999985, 0.999985) * 8_388_608.0) as i32 as u32
    }
    fn unpacked(sample: u32) -> f32 {
        ((sample as i32) << 8 >> 8) as f32 / 8_388_608.0
    }
    pub fn read(&mut self, offset: u64) -> Result<u32, &'static str> {
        if !offset.is_multiple_of(4) || offset > 0x44 {
            return Err("unsupported SAI register");
        }
        match offset {
            0x18 => {
                Ok((Self::level(self.rx.len()) << 16)
                    | if !self.rx.is_empty() { 1 << 3 } else { 0 })
            }
            0x38 => {
                Ok((Self::level(self.tx.len()) << 16) | if self.tx.len() < 4 { 1 << 3 } else { 0 })
            }
            0x20 => self.rx.pop_front().ok_or("SAI RX FIFO underflow"),
            0x40 => Err("cannot read transmit-only SAI B data"),
            _ => Ok(self.registers[offset as usize / 4]),
        }
    }
    pub fn write(&mut self, offset: u64, value: u32) -> Result<(), &'static str> {
        if !offset.is_multiple_of(4) || offset > 0x44 {
            return Err("unsupported SAI register");
        }
        if [4, 0x24].contains(&offset) && value & (3 << 16) != 0 && (value >> 5) & 7 != 6 {
            return Err("only 24-bit SAI PCM is modeled");
        }
        if offset == 0x40 {
            if self.tx.len() >= 8 {
                return Err("SAI TX FIFO overflow");
            }
            self.tx.push_back(value);
            return Ok(());
        }
        if [0x18, 0x38, 0x20].contains(&offset) {
            return Err("read-only SAI register");
        }
        if offset == 8 && value & 8 != 0 {
            self.rx.clear();
        }
        if offset == 0x28 && value & 8 != 0 {
            self.tx.clear();
        }
        self.registers[offset as usize / 4] = if [8, 0x28].contains(&offset) {
            value & !8
        } else {
            value
        };
        Ok(())
    }
    pub fn requests(&self) -> u32 {
        let mut flags = 0;
        if !self.drop_rx && self.registers[1] & (1 << 17) != 0 && !self.rx.is_empty() {
            flags |= 1;
        }
        if self.registers[9] & (1 << 17) != 0 && self.tx.len() < 8 {
            flags |= 2;
        }
        flags
    }
    pub fn tick(&mut self, codec_active: bool) {
        if self.registers[1] & (1 << 16) == 0 {
            return;
        }
        if self.rx.len() > 6 {
            self.overruns += 1;
            self.rx.clear();
        }
        let sample = self.input.get(self.frames).copied().unwrap_or([0.0; 2]);
        for s in sample {
            self.rx
                .push_back(Self::packed(if codec_active { s } else { 0.0 }));
        }
        let left = self.tx.pop_front();
        let right = self.tx.pop_front();
        if left.is_none() || right.is_none() {
            self.underruns += 1;
        }
        if self.output.len() < 1_000_000 {
            self.output.push(if codec_active {
                [
                    Self::unpacked(left.unwrap_or(0)),
                    Self::unpacked(right.unwrap_or(0)),
                ]
            } else {
                [0.0; 2]
            });
        }
        self.frames += 1;
    }
}
