//! v3.5.0 (`FE-10`) — the Vs. `DualSystem` cabinet in the `wasm-canvas` embed.
//!
//! The full web build (`wasm-winit`) has run the two-console cabinet since
//! v2.9.7 through the same `EmuCore` as the desktop. The canvas embed has no
//! `EmuCore`: it drives one `Nes` from a `requestAnimationFrame` loop and blits
//! its framebuffer with `put_image_data`, so until v3.5.0 a `DualSystem` title
//! ran its main console alone (with a console warning) and never got past the
//! attract screen. This module is the part of the embed's cabinet support that
//! does not touch the DOM, so the native unit tests can drive it:
//!
//! - [`build_cabinet`]: the load decision. A `DualSystem` image (by its NES 2.0
//!   header or the Vs. database, `crate::app::is_dual_system`) becomes a
//!   [`VsDualSystem`]; both consoles get the game database's mirroring
//!   correction and the Vs. database's PPU palette and DIP preset, which is
//!   what the embed's single-console path gives a Vs. game.
//! - [`CabinetFrame::step`]: one frame. Latches input (P1 / P2 on the main
//!   console, P3 / P4 on the sub, `VsDualSystem::set_buttons`), runs the coin
//!   hold, steps both consoles, and composes the two screens side by side into
//!   one 512x240 RGBA image with `gfx::compose_dual_into`, the desktop's
//!   compositor, so the embed shows exactly the pixels the desktop shows.
//! - [`drain_audio`]: the MAIN console's audio, which is the one stream the
//!   frontends play (as on the desktop); the sub console's is drained and
//!   discarded so its sample buffer stays bounded.
//! - [`CoinLatch`]: a Vs. coin insertion held, then cleared, by the
//!   desktop's countdown ([`VS_COIN_HOLD_FRAMES`]). The embed also gains the
//!   desktop's default coin key (`F10`) for single Vs. games, which could not
//!   be coined in it at all before.
//!
//! What the embed's cabinet does not do, as on the desktop (ADR 0032): TAS
//! movies, which are refused with a console note. It also has no rewind,
//! run-ahead or netplay, which the embed lacks for a single console too. Save
//! states (F1 / F4) store the cabinet's own two-console snapshot
//! (`VsDualSystem::snapshot`, the "RVSD" container) under the ROM's hash.

use rustynes_core::{Buttons, Nes, VsDualSystem};

use crate::gfx::{DualLayout, compose_dual_into};

/// The coin hold counter, the desktop's value (`app.rs`).
///
/// As there, the latch is cleared at the top of the frame on which the counter reaches 0,
/// so an insertion reads on `$4016` through two emulated frames (about
/// 33 ms at 60 fps) before it is released.
pub const VS_COIN_HOLD_FRAMES: u8 = 3;

/// The coin acceptor's hold-then-release timer.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CoinLatch {
    frames: u8,
}

impl CoinLatch {
    /// A coin went in: start the [`VS_COIN_HOLD_FRAMES`] countdown (a second
    /// coin while held restarts it).
    pub const fn insert(&mut self) {
        self.frames = VS_COIN_HOLD_FRAMES;
    }

    /// Called once per frame, before it runs: `true` on the frame the hold
    /// expires, when the caller clears the acceptor.
    pub const fn tick(&mut self) -> bool {
        if self.frames == 0 {
            return false;
        }
        self.frames -= 1;
        self.frames == 0
    }
}

/// Apply the Vs. database's PPU palette and DIP preset to `nes`, as the
/// embed's single-console path does for a Vs. game. A no-op for a cartridge
/// the database does not list.
pub fn apply_vs_db_defaults(nes: &mut Nes) {
    if let Some(entry) = rustynes_core::vs_db::lookup(nes) {
        nes.set_vs_ppu_type(entry.vs_ppu_type);
        nes.set_vs_dip(entry.vs_dip);
    }
}

/// Build the two-console cabinet for `bytes` when its probe console `probe`
/// is a Vs. `DualSystem` board.
///
/// `None` for any other cartridge (the caller
/// keeps `probe` as its single console). `crc` is the game database key the
/// header-correction stage returned, for the mirroring correction both
/// consoles get. `Some(Err(reason))` is a cabinet that failed to build; the caller
/// falls back to the probe with the usual "main console only" warning.
#[must_use]
pub fn build_cabinet(
    probe: &Nes,
    bytes: &[u8],
    crc: Option<u32>,
    sample_rate: Option<u32>,
) -> Option<Result<Box<VsDualSystem>, String>> {
    if !crate::app::is_dual_system(probe) {
        return None;
    }
    let built = sample_rate.map_or_else(
        || VsDualSystem::from_rom(bytes),
        |sr| VsDualSystem::from_rom_with_sample_rate(bytes, sr),
    );
    Some(built.map_err(|e| e.to_string()).map(|mut vs| {
        let pair: [&mut Nes; 2] = vs.split_mut().into();
        for console in pair {
            if let Some(crc) = crc {
                crate::game_db::correct_console(console, crc);
            }
            apply_vs_db_defaults(console);
        }
        Box::new(vs)
    }))
}

/// Per-frame state the embed keeps beside its cabinet: the coin hold and the
/// reusable composed-image buffer (allocated once, at 512x240x4).
#[derive(Debug, Default)]
pub struct CabinetFrame {
    /// The coin acceptor's hold timer.
    pub coin: CoinLatch,
    composed: Vec<u8>,
}

impl CabinetFrame {
    /// Insert a coin into the MAIN cabinet's acceptor #1 (the desktop's
    /// Insert Coin action).
    pub const fn insert_coin(&mut self, dual: &mut VsDualSystem) {
        dual.insert_coin(0);
        self.coin.insert();
    }

    /// Run one cabinet frame with `ports` (P1..P4) latched, and return the two
    /// screens composed side by side: `(rgba, width, height)`, 512x240.
    pub fn step(&mut self, dual: &mut VsDualSystem, ports: [Buttons; 4]) -> (&[u8], u32, u32) {
        if self.coin.tick() {
            dual.clear_coin();
        }
        for (port, buttons) in ports.into_iter().enumerate() {
            dual.set_buttons(port, buttons);
        }
        dual.run_frame();
        let (w, h) = compose_dual_into(
            &mut self.composed,
            dual.main_framebuffer(),
            dual.sub_framebuffer(),
            DualLayout::SideBySide,
        );
        (&self.composed, w, h)
    }
}

/// This frame's audio to play: the MAIN console's samples. The sub console's
/// are drained and dropped (only one stream is presented), so its APU buffer
/// cannot grow without bound across a long session.
pub fn drain_audio(dual: &mut VsDualSystem) -> Vec<f32> {
    drop(dual.sub_mut().drain_audio());
    dual.main_mut().drain_audio()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::{NES_H, NES_W};

    fn cabinet_rom() -> Vec<u8> {
        crate::runahead::tests::flashing_cabinet()
    }

    /// A plain NROM cartridge: 16 KiB PRG looping at `$8000`, 8 KiB CHR.
    fn plain_nrom() -> Vec<u8> {
        let mut prg = vec![0u8; 0x4000];
        prg[..3].copy_from_slice(&[0x4C, 0x00, 0x80]);
        for v in [0x3FFA, 0x3FFC, 0x3FFE] {
            prg[v + 1] = 0x80;
        }
        let mut rom = b"NES\x1a\x01\x01\x00\x00".to_vec();
        rom.resize(16, 0);
        rom.extend_from_slice(&prg);
        rom.resize(16 + 0x4000 + 0x2000, 0);
        rom
    }

    #[test]
    fn a_dual_system_image_builds_a_cabinet_and_a_cartridge_does_not() {
        let rom = cabinet_rom();
        let probe = Nes::from_rom(&rom).unwrap();
        assert!(
            matches!(build_cabinet(&probe, &rom, None, Some(48_000)), Some(Ok(_))),
            "a DualSystem image becomes a cabinet"
        );
        let cart = plain_nrom();
        let probe = Nes::from_rom(&cart).unwrap();
        assert!(
            build_cabinet(&probe, &cart, None, Some(48_000)).is_none(),
            "a plain cartridge stays a single console"
        );
    }

    /// One `step` is one cabinet frame, and the image it returns is the two
    /// consoles' screens side by side, pixel for pixel.
    #[test]
    fn a_step_runs_one_frame_and_composes_both_screens() {
        let rom = cabinet_rom();
        let probe = Nes::from_rom(&rom).unwrap();
        let mut dual = build_cabinet(&probe, &rom, None, None).unwrap().unwrap();
        let mut reference = build_cabinet(&probe, &rom, None, None).unwrap().unwrap();
        let mut frame = CabinetFrame::default();
        for _ in 0..8 {
            let (img, w, h) = frame.step(&mut dual, [Buttons::empty(); 4]);
            reference.run_frame();
            assert_eq!((w, h), (NES_W * 2, NES_H));
            let row = NES_W as usize * 4;
            for y in [0usize, 100, 239] {
                let line = &img[y * row * 2..(y + 1) * row * 2];
                assert_eq!(
                    &line[..row],
                    &reference.main_framebuffer()[y * row..(y + 1) * row]
                );
                assert_eq!(
                    &line[row..],
                    &reference.sub_framebuffer()[y * row..(y + 1) * row]
                );
            }
        }
        assert_eq!(dual.snapshot(), reference.snapshot(), "frame for frame");
        // The stimulus moves, so the comparison above saw changing screens.
        let first = dual.main_framebuffer().to_vec();
        frame.step(&mut dual, [Buttons::empty(); 4]);
        assert_ne!(dual.main_framebuffer(), first.as_slice());
    }

    /// Input reaches the console the desktop routes it to: P1 / P2 to the main
    /// console's ports, P3 / P4 to the sub's.
    #[test]
    fn a_step_latches_each_port_where_the_desktop_does() {
        let rom = cabinet_rom();
        let probe = Nes::from_rom(&rom).unwrap();
        let mut dual = build_cabinet(&probe, &rom, None, None).unwrap().unwrap();
        let mut reference = build_cabinet(&probe, &rom, None, None).unwrap().unwrap();
        let ports = [Buttons::A, Buttons::START, Buttons::B, Buttons::SELECT];
        let mut frame = CabinetFrame::default();
        frame.step(&mut dual, ports);
        for (port, b) in ports.into_iter().enumerate() {
            reference.set_buttons(port, b);
        }
        reference.run_frame();
        assert_eq!(dual.snapshot(), reference.snapshot());
    }

    /// A coin reads on `$4016` through two emulated frames and is cleared at
    /// the top of the third, the third `tick` after the insertion: the
    /// desktop's `produce_dual_frame` rule exactly (its comment calls it a
    /// three-frame hold; the counter is 3, the latch is seen for two frames).
    #[test]
    fn a_coin_reads_for_two_frames_then_is_released() {
        let rom = cabinet_rom();
        let probe = Nes::from_rom(&rom).unwrap();
        let mut dual = build_cabinet(&probe, &rom, None, None).unwrap().unwrap();
        let mut reference = build_cabinet(&probe, &rom, None, None).unwrap().unwrap();
        // Acceptor #1 reads on `$4016` bit 5 of the MAIN console. The
        // save-state snapshot does not carry the coin latch, so the bit is what
        // the test watches. The read is a real one (it clocks the controller
        // shift register), so both cabinets are read alike to keep the
        // snapshot comparison fair.
        let coin_bit = |d: &mut VsDualSystem, r: &mut VsDualSystem| {
            let _ = r.main_mut().bus_mut().peek_cpu(0x4016);
            d.main_mut().bus_mut().peek_cpu(0x4016) & 0x20 != 0
        };
        let mut frame = CabinetFrame::default();
        assert!(
            !coin_bit(&mut dual, &mut reference),
            "no coin before one is inserted"
        );
        frame.insert_coin(&mut dual);
        reference.insert_coin(0);
        for n in 0..2 {
            assert!(coin_bit(&mut dual, &mut reference), "held before frame {n}");
            frame.step(&mut dual, [Buttons::empty(); 4]);
            reference.run_frame();
            assert_eq!(dual.snapshot(), reference.snapshot(), "held");
        }
        assert!(
            coin_bit(&mut dual, &mut reference),
            "still held after the second frame"
        );
        frame.step(&mut dual, [Buttons::empty(); 4]);
        reference.clear_coin();
        reference.run_frame();
        assert_eq!(dual.snapshot(), reference.snapshot(), "released");
        assert!(
            !coin_bit(&mut dual, &mut reference),
            "cleared before the third frame ran"
        );
        assert_eq!(
            dual.sub_mut().bus_mut().peek_cpu(0x4016) & 0x60,
            0,
            "the sub's acceptors are untouched"
        );

        let mut latch = CoinLatch::default();
        assert!(!latch.tick(), "nothing held, nothing to clear");
        latch.insert();
        assert_eq!(
            [latch.tick(), latch.tick(), latch.tick()],
            [false, false, true]
        );
        assert!(!latch.tick());
    }

    /// The main console's audio is returned and the sub's is emptied.
    #[test]
    fn the_main_consoles_audio_plays_and_the_subs_is_drained() {
        let rom = cabinet_rom();
        let probe = Nes::from_rom(&rom).unwrap();
        let mut dual = build_cabinet(&probe, &rom, None, Some(48_000))
            .unwrap()
            .unwrap();
        let mut frame = CabinetFrame::default();
        frame.step(&mut dual, [Buttons::empty(); 4]);
        let played = drain_audio(&mut dual);
        assert!(
            (700..=900).contains(&played.len()),
            "about one 60 Hz frame at 48 kHz: {}",
            played.len()
        );
        assert!(
            dual.sub_mut().drain_audio().is_empty(),
            "the sub was drained"
        );
        assert!(
            dual.main_mut().drain_audio().is_empty(),
            "nothing left over"
        );
    }
}
