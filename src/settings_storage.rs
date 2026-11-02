//! Power-loss-safe storage for the master volume and every menu setting in
//! onboard QSPI flash.

use crate::state_machine::{MenuValues, MENU_ITEMS};

/// Number of menu values in a saved record — one per entry in `MENU_ITEMS`.
pub const MENU_VALUE_COUNT: usize = MENU_ITEMS.len();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SavedSettings {
    pub master: i8,
    /// Every menu value, in `MENU_ITEMS` order.
    pub menu: [i8; MENU_VALUE_COUNT],
}

impl Default for SavedSettings {
    fn default() -> Self {
        let defaults = MenuValues::default();
        Self {
            master: 10,
            menu: MENU_ITEMS.map(|item| defaults.get(item)),
        }
    }
}

// The flash I/O below talks to QSPI via `libdaisy`, so it only exists on the
// firmware target. `cargo test --lib` (host) compiles just `SavedSettings` above,
// keeping `libdaisy`'s embedded panic handler out of the host test binary.
#[cfg(target_os = "none")]
pub use flash_io::{load, save};

#[cfg(target_os = "none")]
mod flash_io {
    use super::{SavedSettings, MENU_VALUE_COUNT};
    use libdaisy::flash::{Flash, FlashErase};

    const SLOT_ADDRESSES: [u32; 2] = [0x7F_C000, 0x7F_D000];
    const RECORD_SIZE: usize = 64;
    const MAGIC: [u8; 4] = *b"SPHV";
    // v2 stores every menu value; v1 (mixer levels + mode bitmask) is ignored.
    const VERSION: u8 = 2;
    const MASTER_OFFSET: usize = 12;
    const MENU_OFFSET: usize = 13;
    const CRC_OFFSET: usize = RECORD_SIZE - 4;
    const _: () = assert!(MENU_OFFSET + MENU_VALUE_COUNT <= CRC_OFFSET);

    #[derive(Clone, Copy)]
    struct Record {
        generation: u32,
        settings: SavedSettings,
    }

    pub fn load(flash: &mut Flash) -> Option<SavedSettings> {
        newest(flash).map(|(_, record)| record.settings)
    }

    pub fn save(flash: &mut Flash, settings: SavedSettings) -> bool {
        let (slot, generation) = match newest(flash) {
            Some((current, record)) => (1 - current, record.generation.wrapping_add(1)),
            None => (0, 0),
        };
        let bytes = encode(Record {
            generation,
            settings,
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
        bytes[5] = MENU_VALUE_COUNT as u8;
        bytes[8..12].copy_from_slice(&record.generation.to_le_bytes());
        bytes[MASTER_OFFSET] = record.settings.master as u8;
        for (i, &value) in record.settings.menu.iter().enumerate() {
            bytes[MENU_OFFSET + i] = value as u8;
        }
        let crc = crc32(&bytes[..CRC_OFFSET]);
        bytes[CRC_OFFSET..].copy_from_slice(&crc.to_le_bytes());
        bytes
    }

    /// Value ranges aren't checked here — `apply_saved_settings` clamps each
    /// value to its menu range. A record whose menu count differs (items were
    /// added or removed) is rejected, since its values would land on the wrong
    /// items.
    fn decode(bytes: &[u8; RECORD_SIZE]) -> Option<Record> {
        if bytes[..4] != MAGIC || bytes[4] != VERSION || bytes[5] != MENU_VALUE_COUNT as u8 {
            return None;
        }
        let expected = u32::from_le_bytes(bytes[CRC_OFFSET..].try_into().ok()?);
        if crc32(&bytes[..CRC_OFFSET]) != expected {
            return None;
        }
        let mut menu = [0i8; MENU_VALUE_COUNT];
        for (i, value) in menu.iter_mut().enumerate() {
            *value = bytes[MENU_OFFSET + i] as i8;
        }
        Some(Record {
            generation: u32::from_le_bytes(bytes[8..12].try_into().ok()?),
            settings: SavedSettings {
                master: bytes[MASTER_OFFSET] as i8,
                menu,
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
