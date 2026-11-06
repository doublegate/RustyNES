//! Per-game emulation corrections that depart from hardware ON PURPOSE.
//!
//! The core emulates the hardware, including the hardware's own glitches. A
//! correction here exists only where the maintainer decided that a specific
//! game's authentic glitch should not be drawn by default, and each one is a
//! [`crate::HardwareOptions`] setting: on by default for the listed dumps
//! only, switchable off by the host, and recorded in movies and in the
//! netplay configuration digest, so a replay or a peer runs the same picture.
//!
//! # `coherent_midline_copy` (v3.6.2, `T-RADRACER-COHERENT-COPY`)
//!
//! *Rad Racer* writes `$2006` in the middle of several scanlines from
//! sprite-0-timed loops whose timing varies by a few dots from frame to frame.
//! When the delayed `t -> v` copy of such a write lands inside an 8-dot
//! background fetch group, that group's tile is fetched from two addresses
//! (the nametable "Hybrid Addresses" splice, or a pattern row from the new
//! fine Y), and about one frame in eleven draws an 8-pixel wrong tile in the
//! sand right of the road (line 139) and at the horizon (line 114). A 2012
//! nesdev report describes a "very noticable glitch" on the right at that line
//! on a real NES, so the glitch is the hardware's. The correction holds such a
//! copy to the next fetch-group boundary (`rustynes_ppu::Ppu`'s
//! `coherent_midline_copy`), which draws every tile from one address. Decided
//! by the maintainer on 2026-11-06; see the ADR 0030 withdrawal note.
//!
//! The dumps are matched by the CRC32 of the ROM after its 16-byte header,
//! the key the vendored game database (`rustynes-gamedb`) uses for the same
//! three entries. Only the USA dump was available to measure; the other two
//! CRCs are the database's, and their games are the same program.

/// The headerless CRC32s of the dumps that get the coherent mid-line copy
/// correction by default, with the game database's names.
pub const COHERENT_MIDLINE_COPY_DUMPS: [(u32, &str); 3] = [
    (0x8B9D_3E9C, "Rad Racer (USA)"),
    (0x654F_4E90, "Rad Racer (Europe)"),
    (0x0258_9598, "Highway Star (Japan)"),
];

/// Whether `rom` (a whole iNES / NES 2.0 image, header included) is one of
/// [`COHERENT_MIDLINE_COPY_DUMPS`].
#[must_use]
pub fn coherent_midline_copy_by_default(rom: &[u8]) -> bool {
    rom.len() > 16 && {
        let crc = crc32(&rom[16..]);
        COHERENT_MIDLINE_COPY_DUMPS.iter().any(|&(c, _)| c == crc)
    }
}

/// CRC-32 (IEEE 802.3, reflected, `0xEDB88320`), as `rustynes-gamedb` and
/// zlib compute it. Bitwise: it runs once per ROM load, so a table would buy
/// nothing, and `rustynes-core` stays free of a dependency on the database.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_the_standard_check_value() {
        // The CRC-32 check value: crc32("123456789") = 0xCBF43926.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn only_a_listed_dump_gets_the_correction() {
        assert!(!coherent_midline_copy_by_default(&[0u8; 16]));
        let mut rom = alloc::vec![0x4Eu8, 0x45, 0x53, 0x1A];
        rom.resize(16 + 1024, 0);
        assert!(!coherent_midline_copy_by_default(&rom));
    }
}
