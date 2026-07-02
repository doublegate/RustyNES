//! Nintendo Vs. `DualSystem` dual-console boot verification (v2.0.0 beta.5).
//!
//! The four `DualSystem` boards (Vs. Balloon Fight / Mahjong / Tennis /
//! Wrecking Crew) run TWO complete consoles — two 2A03 CPUs and two RGB PPUs
//! — cross-wired through the `$4016` comms protocol and a shared 2 KiB WRAM
//! window at `$6000-$7FFF`. `rustynes_core::VsDualSystem` models the cabinet
//! as two full `Nes` instances plus the wrapper-owned cross-wiring (the
//! bit-1 /IRQ lines, the converged shared WRAM, and the 5-CPU-cycle
//! soft-lockstep); see `docs/audit/vs-dualsystem-design-2026-06-11.md`, the
//! Mesen2 `VsControlManager` prior art, and MAME `vsnes.cpp` (the memory
//! authority).
//!
//! These tests boot the user-supplied `DualSystem` dumps at
//! `tests/roms/external/vs-system/` (gitignored; never committed) through the
//! [`Emu`] front door — proving the SHA-keyed `vs_db` dual-system detection
//! routes to the dual constructor — then run ~600 frames and assert the
//! cabinet is genuinely alive on BOTH consoles:
//!
//! - neither CPU jammed (a wiring fault — e.g. a stuck /IRQ with no handler,
//!   or garbage over the shared WRAM — typically crashes one side into a
//!   JAM/BRK loop within the first few hundred frames);
//! - both framebuffers are non-blank (> 4 distinct colours, the same
//!   crashed-boot heuristic `vs_system_rgb.rs` uses);
//! - the two framebuffers DIFFER — each side of a `DualSystem` cabinet renders
//!   its own player's view, so byte-identical outputs would mean the sub
//!   console is a mirror rather than an independent machine.
//!
//! **The four boot tests are `#[ignore]`d — the staged dumps cannot pass
//! them on ANY emulator.** The circulating 32 KiB "GVS" dumps are the MAME
//! `maincpu` region ONLY (verified byte-for-byte: GVS Balloon Fight's four
//! 8 KiB PRG chunks CRC32-match MAME `balonfgt`'s `mds-bf4 a-3.1d/1c/1b/1a`
//! exactly; the cabinet's SUB CPU runs the different `.6d`/`.6a` ROMs,
//! which the dumps omit — GVS Tennis likewise matches `vstennisa`'s main
//! region). The main program's boot handshake waits forever for a sub-side
//! answer (`$AA` at shared-WRAM offset `$220`, the `$81CF` exchange) that
//! only the missing sub program can write. Re-enable when a combined
//! 64 KiB-PRG dual dump (main half + sub half, the Mesen2 `prgOuter`
//! layout) is staged. The cabinet model itself is CI-verified by the
//! synthetic-cart suite in `vs_dualsystem_synth.rs`.
//!
//! Each console's final frame is also dumped as a PNG under
//! `/tmp/RustyNES/vs-dualsystem/` for eyeball verification (best-effort;
//! failures to write are non-fatal).
//!
//! Gated on `commercial-roms` so CI never depends on non-distributable
//! assets.

#![cfg(feature = "commercial-roms")]

mod common;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use common::external::external_rom_path;
use common::write_png;
use rustynes_core::{Emu, VsDualSystem};

/// Boot a `DualSystem` dump via the [`Emu`] front door and run it with coins
/// pulsed into both sides' acceptors (acceptor 0 = main, acceptor 2 = sub)
/// plus an occasional Start tap, mirroring the single-console Vs. harness.
fn boot_dual(rel: &str, frames: u64) -> VsDualSystem {
    let path: PathBuf = external_rom_path(rel);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let emu = Emu::from_rom(&bytes).expect("DualSystem ROM must parse");
    let mut dual = match emu {
        Emu::Dual(d) => *d,
        Emu::Single(_) => panic!("{rel}: vs_db must flag this dump as a DualSystem board"),
    };
    for f in 0..frames {
        // Pulse a coin every ~120 frames on both sides (the acceptor reads
        // true for a short hardware window; cleared a few frames later).
        if f % 120 == 30 {
            dual.insert_coin(0); // main acceptor #1
            dual.insert_coin(2); // sub acceptor #1
        }
        if f % 120 == 34 {
            dual.clear_coin();
        }
        // Tap Start on the main console's P1 occasionally.
        let btn = if f % 120 == 60 {
            rustynes_core::Buttons::START
        } else {
            rustynes_core::Buttons::empty()
        };
        dual.set_buttons(0, btn);
        dual.run_frame();
    }
    dual
}

/// Count distinct RGBA colours in a framebuffer (blank/crash heuristic).
fn colour_count(fb: &[u8]) -> usize {
    fb.chunks_exact(4)
        .map(|c| [c[0], c[1], c[2], c[3]])
        .collect::<HashSet<[u8; 4]>>()
        .len()
}

/// Best-effort PNG dump of both consoles' final frames for eyeballing.
fn dump_pngs(label: &str, dual: &VsDualSystem) {
    let dir = Path::new("/tmp/RustyNES/vs-dualsystem");
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let safe: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    for (side, fb) in [
        ("main", dual.main_framebuffer()),
        ("sub", dual.sub_framebuffer()),
    ] {
        let path = dir.join(format!("{safe}_{side}.png"));
        if let Err(e) = write_png(&path, fb) {
            eprintln!(
                "[vs_dualsystem] png write failed for {}: {e}",
                path.display()
            );
        }
    }
}

/// The shared assertion battery for a booted `DualSystem` cabinet.
fn assert_dual_alive(label: &str, dual: &VsDualSystem) {
    dump_pngs(label, dual);

    // Neither CPU may be jammed — a cross-wiring fault crashes a side fast.
    assert!(!dual.main().is_jammed(), "{label}: MAIN console CPU jammed");
    assert!(!dual.sub().is_jammed(), "{label}: SUB console CPU jammed");

    // Both consoles must be rendering something real.
    let main_colours = colour_count(dual.main_framebuffer());
    let sub_colours = colour_count(dual.sub_framebuffer());
    assert!(
        main_colours > 4,
        "{label}: MAIN framebuffer is blank/crashed ({main_colours} colours)"
    );
    assert!(
        sub_colours > 4,
        "{label}: SUB framebuffer is blank/crashed ({sub_colours} colours)"
    );

    // Each side renders its own player's view; identical outputs would mean
    // the sub console is a mirror, not an independent machine.
    assert_ne!(
        dual.main_framebuffer(),
        dual.sub_framebuffer(),
        "{label}: main and sub framebuffers are byte-identical"
    );
}

#[test]
#[ignore = "staged GVS dumps are the MAME maincpu half only (sub-CPU PRG absent) - boot cannot complete; needs a combined 64 KiB dual dump"]
fn gvs_balloon_fight_dual_boots_on_both_consoles() {
    let dual = boot_dual("vs-system/GVS Balloon Fight.nes", 600);
    assert_dual_alive("GVS Balloon Fight", &dual);
}

#[test]
#[ignore = "staged GVS dumps are the MAME maincpu half only (sub-CPU PRG absent) - boot cannot complete; needs a combined 64 KiB dual dump"]
fn gvs_mahjong_dual_boots_on_both_consoles() {
    let dual = boot_dual("vs-system/GVS Mahjong.nes", 600);
    assert_dual_alive("GVS Mahjong", &dual);
}

#[test]
#[ignore = "staged GVS dumps are the MAME maincpu half only (sub-CPU PRG absent) - boot cannot complete; needs a combined 64 KiB dual dump"]
fn gvs_tennis_dual_boots_on_both_consoles() {
    let dual = boot_dual("vs-system/GVS Tennis.nes", 600);
    assert_dual_alive("GVS Tennis", &dual);
}

#[test]
#[ignore = "staged GVS dumps are the MAME maincpu half only (sub-CPU PRG absent) - boot cannot complete; needs a combined 64 KiB dual dump"]
fn gvs_wrecking_crew_dual_boots_on_both_consoles() {
    let dual = boot_dual("vs-system/GVS Wrecking Crew.nes", 600);
    assert_dual_alive("GVS Wrecking Crew", &dual);
}

/// The dual snapshot container must round-trip: snapshot → restore into a
/// fresh cabinet → both sides' framebuffers and cycle counters continue
/// byte-identically for another 60 frames.
#[test]
fn dual_snapshot_round_trips() {
    let mut a = boot_dual("vs-system/GVS Balloon Fight.nes", 300);
    let snap = a.snapshot();

    let bytes = std::fs::read(external_rom_path("vs-system/GVS Balloon Fight.nes")).unwrap();
    let mut b = VsDualSystem::from_rom(&bytes).expect("fresh cabinet must construct");
    b.restore(&snap).expect("dual snapshot must restore");

    for _ in 0..60 {
        a.run_frame();
        b.run_frame();
    }
    assert_eq!(
        a.main_framebuffer(),
        b.main_framebuffer(),
        "main framebuffers diverged after snapshot round-trip"
    );
    assert_eq!(
        a.sub_framebuffer(),
        b.sub_framebuffer(),
        "sub framebuffers diverged after snapshot round-trip"
    );
    assert_eq!(a.main().cycle(), b.main().cycle(), "main cycle diverged");
    assert_eq!(a.sub().cycle(), b.sub().cycle(), "sub cycle diverged");
}
