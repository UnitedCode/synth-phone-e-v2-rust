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
    pub rx_channel: usize,
}
impl Sai {
    fn level(words: usize) -> u32 {
        if words == 0 {
            0
        } else if words == 8 {
            5
        } else {
            (words / 2 + 1) as u32
        }
    }
    fn fifo_request(&self, channel: usize) -> bool {
        let threshold = [1, 2, 4, 6, 8][(self.registers[2 + channel * 8] & 7) as usize];
        if channel == self.rx_channel {
            self.rx.len() >= threshold
        } else {
            self.tx.len() < threshold
        }
    }
    fn packed(sample: f32) -> u32 {
        // A serial 24-bit boundary saturates to the representable endpoints.
        // The firmware's separate FBIPMAX limiter is not an ADC specification.
        (sample.clamp(-1.0, 1.0 - 1.0 / 8_388_608.0) * 8_388_608.0) as i32 as u32
    }
    fn unpacked(sample: u32) -> f32 {
        ((sample as i32) << 8 >> 8) as f32 / 8_388_608.0
    }
    pub fn read(&mut self, offset: u64) -> Result<u32, &'static str> {
        if !offset.is_multiple_of(4) || offset > 0x44 {
            return Err("unsupported SAI register");
        }
        match offset {
            0x18 | 0x38 => {
                let receive = usize::from(offset == 0x38) == self.rx_channel;
                let words = if receive {
                    self.rx.len()
                } else {
                    self.tx.len()
                };
                let request = self.fifo_request(usize::from(offset == 0x38));
                Ok((Self::level(words) << 16) | if request { 1 << 3 } else { 0 })
            }
            0x20 | 0x40 if usize::from(offset == 0x40) == self.rx_channel => {
                self.rx.pop_front().ok_or("SAI RX FIFO underflow")
            }
            0x20 | 0x40 => Err("cannot read transmit-only SAI data"),
            _ => Ok(self.registers[offset as usize / 4]),
        }
    }
    pub fn write(&mut self, offset: u64, value: u32) -> Result<(), &'static str> {
        if !offset.is_multiple_of(4) || offset > 0x44 {
            return Err("unsupported SAI register");
        }
        if [8, 0x28].contains(&offset) && value & 7 > 4 {
            return Err("reserved SAI FIFO threshold");
        }
        if [4, 0x24].contains(&offset) && value & (3 << 16) != 0 && (value >> 5) & 7 != 6 {
            return Err("only 24-bit SAI PCM is modeled");
        }
        if [4, 0x24].contains(&offset) && value & (3 << 16) != 0 {
            let channel = usize::from(offset == 0x24);
            // RM0433 MODE[1] selects slave; MODE[0] selects receive.
            let expected_mode =
                (u32::from(channel == 1) << 1) | u32::from(channel == self.rx_channel);
            if value & 3 != expected_mode {
                return Err("SAI direction/master role does not match physical codec wiring");
            }
        }
        if [0x20, 0x40].contains(&offset) && usize::from(offset == 0x40) != self.rx_channel {
            if self.tx.len() >= 8 {
                return Err("SAI TX FIFO overflow");
            }
            self.tx.push_back(value);
            return Ok(());
        }
        if [0x18, 0x38, 0x20, 0x40].contains(&offset) {
            return Err("read-only SAI register");
        }
        if [8, 0x28].contains(&offset) && value & 8 != 0 {
            if usize::from(offset == 0x28) == self.rx_channel {
                self.rx.clear();
            } else {
                self.tx.clear();
            }
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
        if !self.drop_rx
            && self.registers[1 + self.rx_channel * 8] & (1 << 17) != 0
            && self.fifo_request(self.rx_channel)
        {
            flags |= 1 << self.rx_channel;
        }
        if self.registers[1 + (1 - self.rx_channel) * 8] & (1 << 17) != 0
            && self.fifo_request(1 - self.rx_channel)
        {
            flags |= 1 << (1 - self.rx_channel);
        }
        flags
    }
    #[cfg(test)]
    pub fn tick(&mut self, codec_active: bool) {
        self.tick_mask(if codec_active { 3 } else { 0 });
    }
    pub fn tick_mask(&mut self, codec_mask: u32) {
        if self.registers[1] & (1 << 16) == 0 {
            return;
        }
        if self.rx.len() > 6 {
            self.overruns += 1;
            self.rx.clear();
        }
        let sample = self.input.get(self.frames).copied().unwrap_or([0.0; 2]);
        for (channel, s) in sample.into_iter().enumerate() {
            self.rx.push_back(Self::packed(
                if codec_mask & 1 != 0 && codec_mask & (1 << (channel + 2)) == 0 {
                    s
                } else {
                    0.0
                },
            ));
        }
        let left = self.tx.pop_front();
        let right = self.tx.pop_front();
        if left.is_none() || right.is_none() {
            self.underruns += 1;
        }
        if self.output.len() < 1_000_000 {
            self.output.push(if codec_mask & 2 != 0 {
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
