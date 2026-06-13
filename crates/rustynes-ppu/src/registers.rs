//! Bitflag definitions for the four CPU-facing PPU control registers.
//!
//! Per `docs/ppu-2c02.md` §State and §Register quirks.

use bitflags::bitflags;

bitflags! {
    /// `$2000` PPUCTRL.
    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    pub struct PpuCtrl: u8 {
        /// Bits 1-0 — base nametable address (00=$2000, 01=$2400, ...).
        const NAMETABLE_LO         = 0b0000_0001;
        /// Second bit of the nametable selector.
        const NAMETABLE_HI         = 0b0000_0010;
        /// Bit 2 — VRAM address increment (0: +1, 1: +32).
        const VRAM_INCREMENT_32    = 0b0000_0100;
        /// Bit 3 — sprite pattern table address (0: $0000, 1: $1000)
        /// (ignored in 8x16 mode).
        const SPRITE_PATTERN_HIGH  = 0b0000_1000;
        /// Bit 4 — background pattern table address.
        const BG_PATTERN_HIGH      = 0b0001_0000;
        /// Bit 5 — sprite size (0: 8x8, 1: 8x16).
        const SPRITE_SIZE_16       = 0b0010_0000;
        /// Bit 6 — PPU master/slave (no-op on a stock NES).
        const MASTER_SLAVE         = 0b0100_0000;
        /// Bit 7 — generate NMI at start of VBL.
        const NMI_ENABLE           = 0b1000_0000;
    }
}

bitflags! {
    /// `$2001` PPUMASK.
    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    pub struct PpuMask: u8 {
        /// Bit 0 — greyscale (output ANDed with `$30`).
        const GREYSCALE         = 0b0000_0001;
        /// Bit 1 — show BG in leftmost 8 pixels.
        const SHOW_BG_LEFT      = 0b0000_0010;
        /// Bit 2 — show sprites in leftmost 8 pixels.
        const SHOW_SPRITE_LEFT  = 0b0000_0100;
        /// Bit 3 — render BG.
        const SHOW_BG           = 0b0000_1000;
        /// Bit 4 — render sprites.
        const SHOW_SPRITE       = 0b0001_0000;
        /// Bit 5 — emphasize red (NTSC) / green (PAL).
        const EMPHASIZE_RED     = 0b0010_0000;
        /// Bit 6 — emphasize green (NTSC) / red (PAL).
        const EMPHASIZE_GREEN   = 0b0100_0000;
        /// Bit 7 — emphasize blue.
        const EMPHASIZE_BLUE    = 0b1000_0000;
    }
}

bitflags! {
    /// `$2002` PPUSTATUS. Bits 4-0 are open-bus, not stored here.
    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    pub struct PpuStatus: u8 {
        /// Bit 5 — sprite overflow (set during sprite evaluation when more
        /// than 8 sprites are in range; cleared on pre-render dot 1).
        const SPRITE_OVERFLOW = 0b0010_0000;
        /// Bit 6 — sprite-zero hit.
        const SPRITE_ZERO_HIT = 0b0100_0000;
        /// Bit 7 — vertical blank started.
        const VBLANK = 0b1000_0000;
    }
}

impl PpuMask {
    /// `true` if either BG or sprites are enabled — i.e., rendering is on.
    #[must_use]
    pub const fn rendering_enabled(self) -> bool {
        self.intersects(Self::SHOW_BG.union(Self::SHOW_SPRITE))
    }
}
