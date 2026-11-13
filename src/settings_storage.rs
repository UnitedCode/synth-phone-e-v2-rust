//! Power-loss-safe storage for the master volume and every menu setting in
//! onboard QSPI flash.

use crate::state_machine::{MenuValues, MENU_ITEMS};

/// Number of menu values in a saved record — one per entry in `MENU_ITEMS`.
pub const MENU_VALUE_COUNT: usize = MENU_ITEMS.len();
pub const RECORD_SIZE: usize = 64;
const MAGIC: [u8; 4] = *b"SPHV";
const VERSION: u8 = 2;
const MASTER_OFFSET: usize = 12;
const MENU_OFFSET: usize = 13;
const CRC_OFFSET: usize = RECORD_SIZE - 4;
const _: () = assert!(MENU_OFFSET + MENU_VALUE_COUNT <= CRC_OFFSET);

/// Quiet-time debounce driven by elapsed core cycles, not serviced UI ticks.
/// The two-second delay is below one counter wrap at the supported core clock.
pub struct SaveDebounce {
    delay_cycles: u32,
    changed_at: Option<u32>,
}

impl SaveDebounce {
    pub const fn new(delay_cycles: u32) -> Self {
        Self {
            delay_cycles,
            changed_at: None,
        }
    }
    pub fn changed(&mut self, now: u32) {
        self.changed_at = Some(now);
    }
    pub fn ready(&self, now: u32) -> bool {
        self.changed_at
            .is_some_and(|start| now.wrapping_sub(start) >= self.delay_cycles)
    }
    pub fn submitted(&mut self) {
        self.changed_at = None;
    }
}

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

impl SavedSettings {
    /// Encode the fixed-size, checksummed QSPI record, without accessing flash.
    pub fn encode_record(self, generation: u32) -> [u8; RECORD_SIZE] {
        let mut bytes = [0xFF; RECORD_SIZE];
        bytes[..4].copy_from_slice(&MAGIC);
        bytes[4] = VERSION;
        bytes[5] = MENU_VALUE_COUNT as u8;
        bytes[8..12].copy_from_slice(&generation.to_le_bytes());
        bytes[MASTER_OFFSET] = self.master as u8;
        for (i, &value) in self.menu.iter().enumerate() {
            bytes[MENU_OFFSET + i] = value as u8;
        }
        let crc = crc32(&bytes[..CRC_OFFSET]);
        bytes[CRC_OFFSET..].copy_from_slice(&crc.to_le_bytes());
        bytes
    }

    /// Decode a QSPI record. Menu ranges are clamped when applying settings.
    pub fn decode_record(bytes: &[u8; RECORD_SIZE]) -> Option<(u32, Self)> {
        let count = bytes[5] as usize;
        // New controls are appended after the original 21 v2 entries. Load
        // known old prefixes and keep newly added controls at their defaults.
        if bytes[..4] != MAGIC || bytes[4] != VERSION || !(21..=MENU_VALUE_COUNT).contains(&count) {
            return None;
        }
        let expected = u32::from_le_bytes(bytes[CRC_OFFSET..].try_into().ok()?);
        if crc32(&bytes[..CRC_OFFSET]) != expected {
            return None;
        }
        let mut settings = Self {
            master: bytes[MASTER_OFFSET] as i8,
            ..Self::default()
        };
        for (i, value) in settings.menu[..count].iter_mut().enumerate() {
            *value = bytes[MENU_OFFSET + i] as i8;
        }
        Some((u32::from_le_bytes(bytes[8..12].try_into().ok()?), settings))
    }
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

// The flash I/O below talks to QSPI via `libdaisy`, so it only exists on the
// firmware target. `cargo test --lib` (host) compiles just `SavedSettings` above,
// keeping `libdaisy`'s embedded panic handler out of the host test binary.
#[cfg(target_os = "none")]
pub use flash_io::{load, save};

#[cfg(target_os = "none")]
mod flash_io {
    use super::{SavedSettings, RECORD_SIZE};
    use libdaisy::flash::{Flash, FlashErase};

    const SLOT_ADDRESSES: [u32; 2] = [0x7F_C000, 0x7F_D000];

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
        let bytes = settings.encode_record(generation);
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
        SavedSettings::decode_record(&bytes).map(|(generation, settings)| Record {
            generation,
            settings,
        })
    }
}
