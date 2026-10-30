//! STM32 DMA1 stream transfers: request-driven 32-bit stereo audio, circular HT/TC.
#[derive(Clone, Copy, Default)]
pub struct Stream {
    pub cr: u32,
    pub remaining: u32,
    pub initial: u32,
    pub par: u32,
    pub m0: u32,
    pub m1: u32,
    pub fcr: u32,
    pub cursor: u32,
}
#[derive(Default, Clone)]
pub struct Dma {
    pub streams: [Stream; 8],
    pub status: [u32; 2],
    pub transfers: u64,
    pub contested_grants: u64,
    pub grants: [u64; 8],
}

impl Dma {
    /// RM0433 15.3.6: four PL levels, then lower stream number.
    /// Single-beat/direct audio path; burst locking and independent port timing
    /// are separate from this functional request ordering.
    pub fn arbitrate(&mut self, requests: u32) -> Option<usize> {
        let mut eligible = 0;
        let mut winner = None;
        for (i, stream) in self.streams.iter().enumerate() {
            if requests & (1 << i) == 0 || stream.cr & 1 == 0 || stream.remaining == 0 {
                continue;
            }
            eligible += 1;
            let priority = (stream.cr >> 16) & 3;
            if winner
                .is_none_or(|previous: usize| priority > ((self.streams[previous].cr >> 16) & 3))
            {
                winner = Some(i);
            }
        }
        if let Some(i) = winner {
            self.grants[i] += 1;
            if eligible > 1 {
                self.contested_grants += 1;
            }
        }
        winner
    }
    pub fn stats(&self) -> String {
        format!("scope=DMA_SINGLE_BEAT_FUNCTIONAL_ARBITRATION\ncontested_grants={}\nstream_grants={:?}\ntransfers={}\naxi_ahb_stalls=UNMODELED\n",self.contested_grants,self.grants,self.transfers)
    }
    fn index(offset: u64) -> Result<(usize, u64), &'static str> {
        if !(0x10..0xd0).contains(&offset) || !offset.is_multiple_of(4) {
            return Err("unknown DMA register");
        }
        Ok((((offset - 0x10) / 0x18) as usize, (offset - 0x10) % 0x18))
    }
    pub fn read(&self, offset: u64) -> Result<u32, &'static str> {
        if offset < 16 {
            return match offset {
                0 => Ok(self.status[0]),
                4 => Ok(self.status[1]),
                8 | 12 => Ok(0),
                _ => Err("unaligned DMA register"),
            };
        }
        let (index, register) = Self::index(offset)?;
        let stream = self.streams[index];
        Ok(match register {
            0 => stream.cr,
            4 => stream.remaining,
            8 => stream.par,
            12 => stream.m0,
            16 => stream.m1,
            20 => stream.fcr | (4 << 3),
            _ => unreachable!(),
        })
    }
    pub fn write(&mut self, offset: u64, value: u32) -> Result<(), &'static str> {
        if offset < 16 {
            return match offset {
                8 | 12 => {
                    self.status[((offset - 8) / 4) as usize] &= !value;
                    Ok(())
                }
                _ => Err("DMA status is read-only"),
            };
        }
        let (index, register) = Self::index(offset)?;
        let stream = &mut self.streams[index];
        if stream.cr & 1 != 0 {
            if register == 0 {
                // EN and interrupt enables remain writable. The other stream
                // fields are protected until EN reads zero (RM0433 DMA_SxCR).
                stream.cr = (stream.cr & !0x1f) | (value & 0x1f);
                return Ok(());
            }
            if matches!(register, 4 | 8) {
                return Ok(());
            }
            if matches!(register, 12 | 16) {
                let inactive = stream.cr & (1 << 18) != 0
                    && ((register == 12) == (stream.cr & (1 << 19) != 0));
                if !inactive {
                    return Ok(());
                }
            }
            if register == 20 {
                return Ok(());
            }
        }
        match register {
            0 => {
                if value & 1 != 0 && stream.cr & 1 == 0 {
                    if stream.remaining == 0
                        || (value >> 11) & 3 != 2
                        || (value >> 13) & 3 != 2
                        || (value >> 6) & 3 > 1
                    {
                        return Err("unsupported DMA audio transfer format");
                    }
                    stream.initial = stream.remaining;
                    stream.cursor = 0;
                }
                stream.cr = value;
            }
            4 => stream.remaining = value & 0xffff,
            8 => stream.par = value,
            12 => stream.m0 = value,
            16 => stream.m1 = value,
            20 => stream.fcr = value & !(7 << 3),
            _ => unreachable!(),
        }
        Ok(())
    }
    pub fn request(&mut self, index: usize) -> Result<Option<(u32, u32)>, &'static str> {
        let stream = self
            .streams
            .get_mut(index)
            .ok_or("invalid DMA request stream")?;
        if stream.cr & 1 == 0 {
            return Ok(None);
        }
        if stream.remaining == 0 {
            return Err("DMA counter underflow");
        }
        let memory_base = if stream.cr & (1 << 19) != 0 {
            stream.m1
        } else {
            stream.m0
        };
        let memory = memory_base
            + if stream.cr & (1 << 10) != 0 {
                stream.cursor * 4
            } else {
                0
            };
        let peripheral = stream.par
            + if stream.cr & (1 << 9) != 0 {
                stream.cursor * 4
            } else {
                0
            };
        let transfer = if stream.cr & (1 << 6) != 0 {
            (memory, peripheral)
        } else {
            (peripheral, memory)
        };
        let readable_flash = (0x0800_0000..0x0802_0000).contains(&memory) && memory <= 0x0801_fffc;
        if !(crate::memory::SystemMemory::is_ram(memory, 4) || readable_flash) {
            self.status[index / 4] |= 1 << ([0, 6, 16, 22][index % 4] + 3);
            stream.cr &= !1;
            return Ok(None);
        }
        stream.remaining -= 1;
        stream.cursor += 1;
        self.transfers += 1;
        let shift = [0, 6, 16, 22][index % 4];
        if stream.remaining == stream.initial / 2 {
            self.status[index / 4] |= 1 << (shift + 4);
        }
        if stream.remaining == 0 {
            self.status[index / 4] |= 1 << (shift + 5);
            if stream.cr & ((1 << 8) | (1 << 18)) != 0 {
                stream.remaining = stream.initial;
                stream.cursor = 0;
                if stream.cr & (1 << 18) != 0 {
                    stream.cr ^= 1 << 19;
                }
            } else {
                stream.cr &= !1;
            }
        }
        Ok(Some(transfer))
    }
    pub fn irqs(&self) -> u32 {
        self.streams
            .iter()
            .enumerate()
            .fold(0, |mask, (i, stream)| {
                let flags = self.status[i / 4] >> [0, 6, 16, 22][i % 4];
                mask | if (flags & 0x10 != 0 && stream.cr & 8 != 0)
                    || (flags & 0x20 != 0 && stream.cr & 16 != 0)
                    || (flags & 8 != 0 && stream.cr & 4 != 0)
                {
                    1 << i
                } else {
                    0
                }
            })
    }
}
