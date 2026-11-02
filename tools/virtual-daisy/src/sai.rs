//! Functional stereo SAI FIFO/PCM boundary; clock and analog timing are not calibrated.
use std::collections::VecDeque;

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
    pub sample_rate: f64,
    input_position: f64,
    rx_resume: Option<usize>,
    tx_resume: Option<usize>,
    last_transmitted: [u32; 2],
    mute_frames: u64,
}
impl Default for Sai {
    fn default() -> Self {
        let mut registers = [0; 18];
        registers[1] = 0x40;
        registers[9] = 0x40;
        Self {
            registers,
            rx: VecDeque::new(),
            tx: VecDeque::new(),
            input: Vec::new(),
            output: Vec::new(),
            frames: 0,
            underruns: 0,
            overruns: 0,
            drop_rx: false,
            rx_channel: 0,
            sample_rate: 0.0,
            input_position: 0.0,
            rx_resume: None,
            tx_resume: None,
            last_transmitted: [0; 2],
            mute_frames: 0,
        }
    }
}
impl Sai {
    fn mono(&self, channel: usize) -> bool {
        self.registers[1 + channel * 8] & (1 << 12) != 0
            && (self.registers[4 + channel * 8] >> 8) & 15 == 1
    }
    pub fn irq(&self) -> bool {
        (0..2).any(|channel| {
            let status =
                self.registers[6 + channel * 8] | if self.fifo_request(channel) { 8 } else { 0 };
            status & self.registers[5 + channel * 8] & 0x7f != 0
        })
    }
    fn level(words: usize, receive: bool) -> u32 {
        if words == 0 {
            0
        } else if words == 8 {
            5
        } else {
            ((words - usize::from(!receive)) / 2 + 1) as u32
        }
    }
    fn fifo_request(&self, channel: usize) -> bool {
        let threshold = [1, 2, 4, 6, 8][(self.registers[2 + channel * 8] & 7) as usize];
        let words = if channel == self.rx_channel {
            self.rx.len()
        } else {
            self.tx.len()
        };
        if self.registers[1 + channel * 8] & 1 != 0 {
            words >= threshold
        } else {
            words < threshold
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
                let receive = self.registers[1 + usize::from(offset == 0x38) * 8] & 1 != 0;
                let words = if usize::from(offset == 0x38) == self.rx_channel {
                    self.rx.len()
                } else {
                    self.tx.len()
                };
                let request = self.fifo_request(usize::from(offset == 0x38));
                Ok((Self::level(words, receive) << 16)
                    | self.registers[offset as usize / 4]
                    | if request { 1 << 3 } else { 0 })
            }
            0x1c | 0x3c => Ok(0),
            0x20 | 0x40 if usize::from(offset == 0x40) == self.rx_channel => {
                self.rx.pop_front().ok_or("SAI RX FIFO underflow")
            }
            0x20 | 0x40 => Err("cannot read transmit-only SAI data"),
            _ => Ok(self.registers[offset as usize / 4]),
        }
    }
    pub fn write(&mut self, offset: u64, mut value: u32) -> Result<(), &'static str> {
        if !offset.is_multiple_of(4) || offset > 0x44 {
            return Err("unsupported SAI register");
        }
        if [0x1c, 0x3c].contains(&offset) {
            self.registers[offset as usize / 4 - 1] &= !(value & 0x77);
            return Ok(());
        }
        if [8, 0x28].contains(&offset) && value & 7 > 4 {
            return Err("reserved SAI FIFO threshold");
        }
        if offset == 0x44 && value & 1 != 0 {
            return Err("SAI PDM bitstream input is not implemented");
        }
        if [8, 0x28].contains(&offset)
            && value & (3 << 14) != 0
            && self.registers[1 + usize::from(offset == 0x28) * 8] & (1 << 16) != 0
        {
            return Err("SAI companding is not implemented");
        }
        if [4, 0x24].contains(&offset) && value & (3 << 16) != 0 && (value >> 5) & 7 != 6 {
            return Err("only 24-bit SAI PCM is modeled");
        }
        if [4, 0x24].contains(&offset) && value & (3 << 16) != 0 {
            let channel = usize::from(offset == 0x24);
            if value & ((3 << 2) | (1 << 8)) != 0
                || (value >> 10) & 3 >= 2
                || self.registers[2 + channel * 8] & (3 << 14) != 0
            {
                return Err(
                    "unsupported SAI serial protocol, bit order, external sync or companding",
                );
            }
            // RM0433 MODE[1] selects slave; MODE[0] selects receive.
            let expected_mode =
                (u32::from(channel == 1) << 1) | u32::from(channel == self.rx_channel);
            if value & 3 != expected_mode {
                return Err("SAI direction/master role does not match physical codec wiring");
            }
            let frame = (self.registers[3 + channel * 8] & 255) + 1;
            if value & (1 << 16) != 0
                && self.registers[1 + channel * 8] & (1 << 16) == 0
                && value & ((1 << 19) | 2) == 0
                && (!(8..=256).contains(&frame) || !frame.is_power_of_two())
            {
                self.registers[6 + channel * 8] |= 4;
                value &= !(1 << 16);
            }
        }
        if [4, 0x24].contains(&offset) && value & (1 << 16) == 0 {
            if usize::from(offset == 0x24) == self.rx_channel {
                self.rx_resume = None;
                self.mute_frames = 0;
            } else {
                self.tx_resume = None;
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
        let sample = self.input_sample();
        self.tick_sample(codec_mask, sample);
    }
    pub fn input_sample(&self) -> [f32; 2] {
        let position = if self.sample_rate > 0.0 {
            self.input_position
        } else {
            self.frames as f64
        };
        let first = self
            .input
            .get(position as usize)
            .copied()
            .unwrap_or([0.0; 2]);
        let next = self
            .input
            .get(position as usize + 1)
            .copied()
            .unwrap_or(first);
        let fraction = position.fract() as f32;
        std::array::from_fn(|channel| first[channel] + (next[channel] - first[channel]) * fraction)
    }
    pub fn tick_sample(&mut self, codec_mask: u32, sample: [f32; 2]) {
        if self.registers[1] & (1 << 16) == 0 {
            return;
        }
        // Input WAV/raw PCM is timestamped at 48014 Hz. Interpolate at the
        // derived SAI frame clock; never relabel input to change its pitch.
        self.input_position += if self.sample_rate > 0.0 {
            48014.0 / self.sample_rate
        } else {
            1.0
        };
        let incoming = std::array::from_fn::<_, 2, _>(|channel| {
            Self::packed(
                if codec_mask & 1 != 0 && codec_mask & (1 << (channel + 2)) == 0 {
                    sample[channel]
                } else {
                    0.0
                },
            )
        });
        if self.registers[1 + self.rx_channel * 8] & (1 << 16) != 0 {
            if incoming.iter().all(|&word| word & 0xffffff == 0) {
                self.mute_frames = self.mute_frames.saturating_add(1);
                let limit = (self.registers[2 + self.rx_channel * 8] >> 7) & 63;
                if limit != 0 && self.mute_frames == u64::from(limit) {
                    self.registers[6 + self.rx_channel * 8] |= 2;
                }
            } else {
                self.mute_frames = 0;
            }
        }
        let mut overrun = false;
        for (channel, word) in incoming.into_iter().enumerate() {
            if self.registers[1 + self.rx_channel * 8] & (1 << 16) == 0 {
                break;
            }
            if channel == 1 && self.mono(self.rx_channel) {
                continue;
            }
            // RM0433 51.4.14: retain queued samples and restart only at the
            // slot that failed, even if space becomes free at an earlier slot.
            if self.rx_resume.is_some_and(|slot| slot != channel) {
                continue;
            }
            if self.rx.len() == 8 {
                self.rx_resume = Some(channel);
                overrun = true;
                continue;
            }
            self.rx_resume = None;
            self.rx.push_back(word);
        }
        if overrun {
            self.overruns += 1;
            self.registers[6 + self.rx_channel * 8] |= 1;
        }
        let mut transmitted = [0; 2];
        let mut underrun = false;
        for slot in 0..2 {
            if self.registers[1 + (1 - self.rx_channel) * 8] & (1 << 16) == 0 {
                break;
            }
            if slot == 1 && self.mono(1 - self.rx_channel) {
                transmitted[1] = transmitted[0];
                continue;
            }
            if self.tx_resume.is_some_and(|failed| failed != slot) {
                continue;
            }
            if let Some(value) = self.tx.pop_front() {
                transmitted[slot] = value;
                self.tx_resume = None;
            } else {
                self.tx_resume = Some(slot);
                underrun = true;
            }
        }
        if underrun {
            self.underruns += 1;
            self.registers[6 + (1 - self.rx_channel) * 8] |= 1;
        }
        let tx_control = self.registers[2 + (1 - self.rx_channel) * 8];
        if self.registers[1 + (1 - self.rx_channel) * 8] & (1 << 16) != 0 && tx_control & 32 != 0 {
            transmitted = if tx_control & 64 != 0 {
                self.last_transmitted
            } else {
                [0; 2]
            };
        }
        self.last_transmitted = transmitted;
        if self.output.len() < 1_000_000 {
            self.output.push(if codec_mask & 2 != 0 {
                [
                    Self::unpacked(transmitted[0]),
                    Self::unpacked(transmitted[1]),
                ]
            } else {
                [0.0; 2]
            });
        }
        self.frames += 1;
    }
}
