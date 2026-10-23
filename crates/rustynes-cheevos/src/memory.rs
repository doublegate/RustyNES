//! `RetroAchievements` address -> NES CPU-bus address mapping.
//!
//! `RetroAchievements` addresses NES memory as the CPU bus itself: the region
//! table in the vendored rcheevos (`consoleinfo.c`, `_rc_memory_regions_nes`)
//! puts system RAM at `0x0000..=0x07FF` with its mirrors to `0x1FFF`, the
//! PPU and APU registers above it, cartridge RAM at `0x6000..=0x7FFF` and
//! cartridge ROM at `0x8000..=0xFFFF`, each at its own CPU address. So the
//! mapping is the identity over `0x0000..=0xFFFF`, and anything above has no
//! NES-bus equivalent (`None`; the trampoline reports a short read, which
//! rcheevos treats as an invalid address).
//!
//! **Corrected in v3.5.0.** Until then `0x0800..=0x27FF` was mapped onto
//! cartridge RAM at `$6000..=$7FFF` and everything above `0x27FF` was
//! refused, so on the desktop and mobile apps a condition on a cartridge-RAM
//! address (`0x6xxx`) read nothing and one on a RAM mirror read cartridge RAM.
//! The browser bridge (`ra_glue.js`) and the libretro core's memory map
//! already used CPU addresses. Found by `CodeRabbit` on #624, against the
//! speedrun timer's copy of this mapping (`rustynes_core::speedrun`).
//!
//! The callers' peek is side-effect-free (`Nes::cpu_bus_peek`), so reading a
//! register or ROM address through this mapping changes nothing.
//!
//! This is kept here, pure and unit-tested, so the memory source stays
//! agnostic: callers supply a `FnMut(u16) -> u8` peeking the CPU bus and never
//! need to know the RA layout.

/// Translate a `RetroAchievements` address to a NES CPU-bus address.
///
/// Returns `None` for addresses above the 64 KiB CPU space.
#[must_use]
// The `as u16` cast is bounded by the range check before it, and
// `u16::try_from` is not available in a `const fn`.
#[allow(clippy::cast_possible_truncation)]
pub const fn ra_addr_to_nes(addr: u32) -> Option<u16> {
    if addr <= 0xFFFF {
        Some(addr as u16)
    } else {
        None
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

    /// The rcheevos NES regions, each at its own CPU address: a RAM mirror
    /// (the bus folds it onto `$0000..=$07FF`), cartridge RAM, cartridge ROM.
    /// Before v3.5.0, `0x0800` read `$6000` and `0x6000` read nothing.
    #[test]
    fn the_rcheevos_regions_are_cpu_addresses() {
        assert_eq!(ra_addr_to_nes(0x0800), Some(0x0800));
        assert_eq!(ra_addr_to_nes(0x6000), Some(0x6000));
        assert_eq!(ra_addr_to_nes(0x7FFF), Some(0x7FFF));
        assert_eq!(ra_addr_to_nes(0xFFFF), Some(0xFFFF));
    }

    #[test]
    fn out_of_range_is_none() {
        assert_eq!(ra_addr_to_nes(0x1_0000), None);
        assert_eq!(ra_addr_to_nes(0xFFFF_FFFF), None);
    }
}
