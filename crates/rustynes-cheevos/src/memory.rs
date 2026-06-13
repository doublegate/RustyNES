//! RetroAchievements flat-address -> NES CPU-bus address mapping.
//!
//! RetroAchievements addresses NES memory as a flat space:
//!   - `0x0000..=0x07FF` -> the 2 KiB system RAM at CPU `$0000..=$07FF`
//!   - `0x0800..`        -> cartridge save/work RAM, mapped to CPU `$6000..`
//!     (the 8 KiB WRAM window `$6000..=$7FFF`)
//!
//! Anything outside those two windows has no NES-bus equivalent for
//! achievement purposes and maps to `None` (the trampoline reports 0 bytes
//! read, which rcheevos treats as an invalid address).
//!
//! This is kept here, pure and unit-tested, so the memory source stays
//! agnostic: callers supply a `FnMut(u16) -> u8` peeking the CPU bus and never
//! need to know the RA layout.

/// Top of the NES system-RAM window in the RA flat space (exclusive).
const RA_SYSTEM_RAM_END: u32 = 0x0800;
/// Size of the cartridge WRAM window (`$6000..=$7FFF`).
const NES_WRAM_LEN: u32 = 0x2000;
/// Base of the cartridge WRAM window on the NES CPU bus.
const NES_WRAM_BASE: u16 = 0x6000;

/// Translate a RetroAchievements flat address to a NES CPU-bus address.
///
/// Returns `None` for addresses that have no NES-bus equivalent.
#[must_use]
pub fn ra_addr_to_nes(addr: u32) -> Option<u16> {
    if addr < RA_SYSTEM_RAM_END {
        // System RAM: identity map into $0000..=$07FF.
        Some(addr as u16)
    } else {
        // Cartridge WRAM window: RA 0x0800 -> NES $6000, clamped to the 8 KiB
        // $6000..=$7FFF region. Addresses beyond the window are unmapped.
        let offset = addr - RA_SYSTEM_RAM_END;
        if offset < NES_WRAM_LEN {
            Some(NES_WRAM_BASE + offset as u16)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_ram_identity() {
        assert_eq!(ra_addr_to_nes(0x0000), Some(0x0000));
        assert_eq!(ra_addr_to_nes(0x0001), Some(0x0001));
        assert_eq!(ra_addr_to_nes(0x07FF), Some(0x07FF));
    }

    #[test]
    fn wram_window_boundaries() {
        // First WRAM byte.
        assert_eq!(ra_addr_to_nes(0x0800), Some(0x6000));
        // Last WRAM byte: 0x0800 + 0x1FFF -> $7FFF.
        assert_eq!(ra_addr_to_nes(0x0800 + 0x1FFF), Some(0x7FFF));
    }

    #[test]
    fn out_of_range_is_none() {
        // One past the WRAM window.
        assert_eq!(ra_addr_to_nes(0x0800 + 0x2000), None);
        assert_eq!(ra_addr_to_nes(0xFFFF_FFFF), None);
    }
}
