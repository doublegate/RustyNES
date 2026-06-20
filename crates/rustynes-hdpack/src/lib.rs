//! HD-pack loader + compositor (and HD audio) for `RustyNES`.
//!
//! This crate holds the Mesen-style HD-pack subsystem — the `hires.txt` loader, the
//! per-frame tile compositor, and the `<bgm>`/`<sfx>` HD-audio decode/mix — extracted
//! from the desktop frontend (v1.8.5) so the **mobile bridge** can reach it too. It
//! is a plain `std` crate (the emulation core is `#![no_std]`, so the loader, which
//! uses `std::io`/`std::path`/`zip`/`png`, cannot live there).
//!
//! It depends only on `rustynes-ppu` (for the `HdTileSource` telemetry type); the
//! compositor takes the framebuffer, the watched-memory snapshot, and a CHR-peek
//! closure as arguments, so it never needs the `Nes`/`rustynes-core`. The desktop
//! frontend re-exports these modules, so existing `crate::hdpack` / `crate::hd_audio`
//! paths keep working.
//!
//! Presentation-only — nothing here touches emulation or the determinism contract.

pub mod hd_audio;
pub mod hdpack;
