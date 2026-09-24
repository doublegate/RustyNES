// SPDX-License-Identifier: GPL-3.0-or-later
//! Cartridge battery RAM, persisted to disk (v2.7.3 "Hearth", frontend ledger
//! FE-01).
//!
//! # What was missing
//!
//! A cartridge with a battery keeps its save RAM when the console is off. The
//! core exposes that RAM through [`rustynes_core::Nes::sram`]; v2.7.1 made every board that has
//! it expose it there. But until v2.7.3 **nothing in the desktop frontend read
//! it**: the libretro core handed it to RetroArch as the `.srm`, the test harness
//! checked it, and the desktop app wrote only the FDS writable-disk sidecar. An
//! in-game save on the desktop survived only inside a save state, and a player
//! who saved, quit and relaunched found the save gone.
//!
//! # The four rules (from review on #549)
//!
//! 1. **Gate on the battery bit, not on `sram()`.** Several boards expose work
//!    RAM through `sram()` whatever the header says: NROM returns 8 KiB for every
//!    image, and MMC1 / MMC3 allocate RAM by default. Keyed on a non-empty slice,
//!    a volatile cartridge would acquire a save it never had and restore stale RAM
//!    across power cycles. [`rustynes_core::Nes::has_battery`] is the header's flags-6 bit 1.
//! 2. **Key the file by [`rustynes_core::Nes::rom_sha256`]**, the hash save states already use:
//!    `<data_dir>/battery/<hex>.sav`. The hash includes the header, so correcting
//!    a header produces a new key; the old file is left in place, not deleted.
//! 3. **"Dirty" is equality with the last write.** The core has no save-RAM dirty
//!    latch, and adding one would be an API change for every mapper. The frontend
//!    does not need it: [`crate::battery_save::BatterySave::flush`] compares the live bytes with a copy
//!    of what it last wrote, and writes only on a difference. The comparison runs
//!    at most once per [`crate::battery_save::CHECK_PERIOD_FRAMES`] produced frames, so a game that
//!    treats its battery RAM as scratch memory costs one write a second, not
//!    sixty. Unload, ROM switch and exit flush unconditionally (`force`), which
//!    still skips the write when nothing changed.
//! 4. **Test first**: the tests below run a program that writes its save RAM,
//!    rebuild the console from the same ROM, and require the bytes back.
//!
//! # Never clobber a save that was not read
//!
//! The one outcome worse than no persistence is overwriting a good save with
//! power-on zeros. So [`crate::battery_save::BatterySave::attach`] refuses to arm itself when an
//! existing file cannot be used: an I/O error other than "not found", or a file
//! whose length is not the cartridge's save size (a different board revision, a
//! truncated copy, or another emulator's format). The file is left untouched and
//! the session simply is not persisted. The baseline copy is taken **after** the
//! load, so an attached session never rewrites the bytes it just read.
//!
//! # Threading
//!
//! Everything here runs under the `EmuCore` lock the caller already holds, for
//! the time of a slice comparison (and, on a change, one atomic file write).
//! [`crate::atomic_write::write_atomic`] supplies the durability: a crash mid-write
//! leaves the previous save, never a truncated one.

use std::io;
use std::path::{Path, PathBuf};

use rustynes_core::Nes;

/// How often, in produced frames, the periodic flush compares the live save
/// RAM with the last write. One second of NTSC play.
pub const CHECK_PERIOD_FRAMES: u32 = 60;

/// The directory under the frontend's data dir that holds `.sav` files.
pub const BATTERY_DIR: &str = "battery";

/// The `.sav` path for a ROM hash: `<data_dir>/battery/<hex>.sav`.
#[must_use]
pub fn sav_path(data_dir: &Path, rom_sha256: &[u8; 32]) -> PathBuf {
    data_dir
        .join(BATTERY_DIR)
        .join(format!("{}.sav", crate::save_state::hex_sha256(rom_sha256)))
}

/// Why a battery-backed cartridge is running without persistence this session.
/// Surfaced in the log so a missing save is explainable rather than silent.
#[derive(Debug)]
pub enum AttachError {
    /// The `.sav` exists but could not be read.
    Unreadable(PathBuf, io::Error),
    /// The `.sav` exists but its length is not this cartridge's save size.
    WrongSize {
        /// The file.
        path: PathBuf,
        /// Its length.
        found: usize,
        /// The cartridge's `sram()` length.
        expected: usize,
    },
}

impl core::fmt::Display for AttachError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unreadable(p, e) => write!(f, "{} is unreadable ({e})", p.display()),
            Self::WrongSize {
                path,
                found,
                expected,
            } => write!(
                f,
                "{} is {found} bytes, but this cartridge saves {expected}",
                path.display()
            ),
        }
    }
}

/// One cartridge's battery RAM, bound to its `.sav` file.
#[derive(Debug)]
pub struct BatterySave {
    path: PathBuf,
    /// The bytes last written to (or read from) `path`. "Clean" means the live
    /// save RAM equals this.
    last: Vec<u8>,
    /// Produced frames since the last periodic comparison.
    frames: u32,
}

impl BatterySave {
    /// Bind `nes`'s battery RAM to `<data_dir>/battery/<sha>.sav`, loading the
    /// file into the cartridge when one exists.
    ///
    /// Returns `Ok(None)` for a cartridge with nothing to persist: no battery
    /// bit, or an empty `sram()`. Returns `Err` when a file exists but cannot be
    /// used; in that case nothing is loaded and nothing will be written, so the
    /// file survives for the user to inspect.
    ///
    /// # Errors
    ///
    /// [`AttachError`] as described above.
    pub fn attach(nes: &mut Nes, data_dir: &Path) -> Result<Option<Self>, AttachError> {
        if !nes.has_battery() || nes.sram().is_empty() {
            return Ok(None);
        }
        let path = sav_path(data_dir, nes.rom_sha256());
        match std::fs::read(&path) {
            Ok(bytes) if bytes.len() == nes.sram().len() => {
                nes.sram_mut().copy_from_slice(&bytes);
            }
            Ok(bytes) => {
                return Err(AttachError::WrongSize {
                    path,
                    found: bytes.len(),
                    expected: nes.sram().len(),
                });
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(AttachError::Unreadable(path, e)),
        }
        Ok(Some(Self {
            path,
            last: nes.sram().to_vec(),
            frames: 0,
        }))
    }

    /// The file this session writes to.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Write the live save RAM if it differs from the last write.
    ///
    /// Without `force`, the comparison runs only every [`CHECK_PERIOD_FRAMES`]
    /// calls (one call per produced frame); with it, it runs now. Returns whether
    /// a file was written.
    ///
    /// # Errors
    ///
    /// The directory creation or the atomic write failed. The baseline is not
    /// advanced, so the next flush retries.
    pub fn flush(&mut self, nes: &Nes, force: bool) -> io::Result<bool> {
        if !force {
            self.frames += 1;
            if self.frames < CHECK_PERIOD_FRAMES {
                return Ok(false);
            }
        }
        self.frames = 0;
        let live = nes.sram();
        if live == self.last.as_slice() {
            return Ok(false);
        }
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::atomic_write::write_atomic(&self.path, live)?;
        self.last.clear();
        self.last.extend_from_slice(live);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// NROM, 16 KiB PRG, 8 KiB CHR, with or without the battery bit. The
    /// program writes `$A5` to `$6000`, then increments `$6001` forever, so the
    /// save RAM keeps changing:
    ///
    /// ```text
    /// C000: A9 A5     LDA #$A5
    ///       8D 00 60  STA $6000
    /// C005: EE 01 60  INC $6001
    ///       4C 05 C0  JMP $C005
    /// ```
    fn rom(battery: bool) -> Vec<u8> {
        let mut v = b"NES\x1A".to_vec();
        v.extend_from_slice(&[1, 1, if battery { 0x02 } else { 0 }, 0]);
        v.resize(16, 0);
        let mut prg = vec![0xEAu8; 0x4000];
        prg[..11].copy_from_slice(&[
            0xA9, 0xA5, 0x8D, 0x00, 0x60, 0xEE, 0x01, 0x60, 0x4C, 0x05, 0xC0,
        ]);
        prg[0x3FFC] = 0x00; // reset vector -> $C000
        prg[0x3FFD] = 0xC0;
        v.extend_from_slice(&prg);
        v.extend(core::iter::repeat_n(0u8, 0x2000));
        v
    }

    #[test]
    fn a_battery_save_survives_a_relaunch() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = rom(true);

        // Session 1: the game writes its save RAM; the session ends (unload).
        let mut nes = Nes::from_rom(&bytes).unwrap();
        let mut save = BatterySave::attach(&mut nes, dir.path())
            .unwrap()
            .expect("a battery cart is persisted");
        for _ in 0..3 {
            nes.run_frame();
        }
        assert_eq!(nes.sram()[0], 0xA5, "the program ran");
        let written = nes.sram()[..2].to_vec();
        assert!(save.flush(&nes, true).unwrap(), "unload writes the change");
        assert!(save.path().exists());

        // Session 2: a fresh console from the same ROM gets the bytes back,
        // before its first frame.
        let mut again = Nes::from_rom(&bytes).unwrap();
        assert_eq!(again.sram()[0], 0, "power-on RAM is blank");
        let _ = BatterySave::attach(&mut again, dir.path()).unwrap();
        assert_eq!(&again.sram()[..2], written.as_slice(), "the save came back");
    }

    #[test]
    fn a_cart_without_a_battery_leaves_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut nes = Nes::from_rom(&rom(false)).unwrap();
        assert!(!nes.sram().is_empty(), "NROM still exposes work RAM");
        assert!(BatterySave::attach(&mut nes, dir.path()).unwrap().is_none());
        assert!(!dir.path().join(BATTERY_DIR).exists(), "no .sav, no dir");
    }

    #[test]
    fn the_periodic_flush_writes_at_most_once_a_period() {
        let dir = tempfile::tempdir().unwrap();
        let mut nes = Nes::from_rom(&rom(true)).unwrap();
        let mut save = BatterySave::attach(&mut nes, dir.path()).unwrap().unwrap();
        let mut writes = 0;
        // The program increments $6001 every frame, so the RAM is always dirty.
        for _ in 0..(3 * CHECK_PERIOD_FRAMES) {
            nes.run_frame();
            if save.flush(&nes, false).unwrap() {
                writes += 1;
            }
        }
        assert_eq!(writes, 3, "one write per period, not one per frame");
    }

    #[test]
    fn an_unchanged_save_is_not_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let mut nes = Nes::from_rom(&rom(true)).unwrap();
        let mut save = BatterySave::attach(&mut nes, dir.path()).unwrap().unwrap();
        assert!(
            !save.flush(&nes, true).unwrap(),
            "power-on RAM is the baseline"
        );
        assert!(!save.path().exists(), "so nothing was written");
    }

    #[test]
    fn a_save_of_the_wrong_size_is_neither_loaded_nor_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = rom(true);
        let mut nes = Nes::from_rom(&bytes).unwrap();
        let path = sav_path(dir.path(), nes.rom_sha256());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, [0x11u8; 100]).unwrap();
        assert!(matches!(
            BatterySave::attach(&mut nes, dir.path()),
            Err(AttachError::WrongSize { found: 100, .. })
        ));
        assert_eq!(nes.sram()[0], 0, "nothing was loaded");
        assert_eq!(
            std::fs::read(&path).unwrap(),
            vec![0x11u8; 100],
            "untouched"
        );
    }
}
