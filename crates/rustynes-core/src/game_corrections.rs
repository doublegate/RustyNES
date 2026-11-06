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
//! The dumps are matched by the CRC32 of the ROM's PRG and CHR data (after
//! the 16-byte header and any 512-byte trainer, and no further than the sizes
//! the header declares), the key the vendored game database
//! (`rustynes-gamedb`'s `rom_crc32`) uses for the same three entries. Hashing
//! everything after the header instead would miss a dump that carries a
//! trainer or trailing bytes, which the cartridge parser accepts (review round
//! 8 on #640). Only the USA dump was available to measure; the other two
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
    prg_chr(rom).is_some_and(|data| {
        let crc = crc32(data);
        COHERENT_MIDLINE_COPY_DUMPS.iter().any(|&(c, _)| c == crc)
    })
}

/// The PRG and CHR data of an iNES image, the bytes the database's CRC
/// covers: after the header and any trainer (header byte 6, bit 2), and
/// `16 KiB x byte 4 + 8 KiB x byte 5` long. `None` for a file without the
/// `NES\x1A` magic, with no PRG, or shorter than the header says. The
/// bounds are `rustynes_gamedb::rom_crc32`'s, so a listed CRC means the
/// same bytes in both places.
fn prg_chr(rom: &[u8]) -> Option<&[u8]> {
    if rom.len() < 16 || &rom[0..4] != b"NES\x1A" {
        return None;
    }
    let prg = usize::from(rom[4]) * 16 * 1024;
    let chr = usize::from(rom[5]) * 8 * 1024;
    let start: usize = 16 + if rom[6] & 0x04 != 0 { 512 } else { 0 };
    let end = start.checked_add(prg)?.checked_add(chr)?;
    if prg == 0 || end > rom.len() {
        return None;
    }
    Some(&rom[start..end])
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

    /// The hashed bytes are the PRG and CHR data, wherever the file puts
    /// them: a trainer before them and padding after them do not move the
    /// key (review round 8 on #640), and a truncated or unmarked file has
    /// none.
    #[test]
    fn the_key_covers_prg_and_chr_only() {
        let mut plain = alloc::vec![0x4Eu8, 0x45, 0x53, 0x1A, 1, 1, 0, 0];
        plain.resize(16, 0);
        let body: alloc::vec::Vec<u8> = (0..24 * 1024u32)
            .map(|i| (i * 7 + 3).to_le_bytes()[0])
            .collect();
        plain.extend_from_slice(&body);
        assert_eq!(prg_chr(&plain), Some(&body[..]));

        let mut trainer = plain[..16].to_vec();
        trainer[6] |= 0x04;
        trainer.extend(core::iter::repeat_n(0xAAu8, 512));
        trainer.extend_from_slice(&body);
        assert_eq!(prg_chr(&trainer), Some(&body[..]), "a trainer is skipped");

        let mut padded = plain.clone();
        padded.extend_from_slice(&[0xFF; 128]);
        assert_eq!(
            prg_chr(&padded),
            Some(&body[..]),
            "trailing bytes are ignored"
        );

        assert_eq!(
            prg_chr(&plain[..plain.len() - 1]),
            None,
            "short of the header's sizes"
        );
        let mut unmarked = plain.clone();
        unmarked[3] = 0;
        assert_eq!(prg_chr(&unmarked), None, "no NES magic");
    }

    #[test]
    fn only_a_listed_dump_gets_the_correction() {
        assert!(!coherent_midline_copy_by_default(&[0u8; 16]));
        let mut rom = alloc::vec![0x4Eu8, 0x45, 0x53, 0x1A];
        rom.resize(16 + 1024, 0);
        assert!(!coherent_midline_copy_by_default(&rom));
    }
}
