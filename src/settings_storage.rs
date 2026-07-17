//! Power-loss-safe storage for phone mixer levels in onboard QSPI flash.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VolumeSettings {
    pub master: i8,
    pub voice: i8,
    pub melody: i8,
    pub drums: i8,
}

impl Default for VolumeSettings {
    fn default() -> Self {
        Self {
            master: 10,
            voice: 10,
            melody: 10,
            drums: 10,
        }
    }
}

// The flash I/O below talks to QSPI via `libdaisy`, so it only exists on the
// firmware target. `cargo test --lib` (host) compiles just `VolumeSettings` above,
// keeping `libdaisy`'s embedded panic handler out of the host test binary.
#[cfg(target_os = "none")]
pub use flash_io::{load, save};

#[cfg(target_os = "none")]
mod flash_io {
    use super::VolumeSettings;
    use libdaisy::flash::{Flash, FlashErase};

    const SLOT_ADDRESSES: [u32; 2] = [0x7F_C000, 0x7F_D000];
    const RECORD_SIZE: usize = 32;
    const MAGIC: [u8; 4] = *b"SPHV";
    const VERSION: u8 = 1;

    #[derive(Clone, Copy)]
    struct Record {
        generation: u32,
        volumes: VolumeSettings,
    }

    pub fn load(flash: &mut Flash) -> Option<VolumeSettings> {
        newest(flash).map(|(_, record)| record.volumes)
    }

    pub fn save(flash: &mut Flash, volumes: VolumeSettings) -> bool {
        let (slot, generation) = match newest(flash) {
            Some((current, record)) => (1 - current, record.generation.wrapping_add(1)),
            None => (0, 0),
        };
        let bytes = encode(Record {
            generation,
            volumes,
        });
        if stm32h7xx_hal::nb::block!(flash.erase(FlashErase::Sector4K(SLOT_ADDRESSES[slot])))
            .is_err()
        {
            return false;
        }
        stm32h7xx_hal::nb::block!(flash.program(SLOT_ADDRESSES[slot], &bytes)).is_ok()
    }

    fn newest(flash: &mut Flash) -> Option<(usize, Record)> {
        match (read_slot(flash, 0), read_slot(flash, 1)) {
            (Some(a), Some(b)) if b.generation.wrapping_sub(a.generation) < 0x8000_0000 => {
                Some((1, b))
            }
            (Some(a), Some(_)) => Some((0, a)),
            (Some(a), None) => Some((0, a)),
            (None, Some(b)) => Some((1, b)),
            (None, None) => None,
        }
    }

    fn read_slot(flash: &mut Flash, slot: usize) -> Option<Record> {
        let mut bytes = [0u8; RECORD_SIZE];
        flash.read(SLOT_ADDRESSES[slot], &mut bytes).ok()?;
        decode(&bytes)
    }

    fn encode(record: Record) -> [u8; RECORD_SIZE] {
        let mut bytes = [0xFF; RECORD_SIZE];
        bytes[..4].copy_from_slice(&MAGIC);
        bytes[4] = VERSION;
        bytes[5] = 4;
        bytes[8..12].copy_from_slice(&record.generation.to_le_bytes());
        bytes[12] = record.volumes.master as u8;
        bytes[13] = record.volumes.voice as u8;
        bytes[14] = record.volumes.melody as u8;
        bytes[15] = record.volumes.drums as u8;
        let crc = crc32(&bytes[..28]);
        bytes[28..].copy_from_slice(&crc.to_le_bytes());
        bytes
    }

    fn decode(bytes: &[u8; RECORD_SIZE]) -> Option<Record> {
        if bytes[..4] != MAGIC || bytes[4] != VERSION || bytes[5] != 4 {
            return None;
        }
        let expected = u32::from_le_bytes(bytes[28..32].try_into().ok()?);
        if crc32(&bytes[..28]) != expected || bytes[12..16].iter().any(|&v| v > 20) {
            return None;
        }
        Some(Record {
            generation: u32::from_le_bytes(bytes[8..12].try_into().ok()?),
            volumes: VolumeSettings {
                master: bytes[12] as i8,
                voice: bytes[13] as i8,
                melody: bytes[14] as i8,
                drums: bytes[15] as i8,
            },
        })
    }

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &byte in bytes {
            crc ^= byte as u32;
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xEDB8_8320 & 0u32.wrapping_sub(crc & 1));
            }
        }
        !crc
    }
}
