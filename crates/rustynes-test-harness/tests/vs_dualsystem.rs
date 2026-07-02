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
//! **The four ORIGINAL 32 KiB-PRG boot tests stay `#[ignore]`d — those
//! specific dumps cannot pass them on ANY emulator.** The circulating 32 KiB
//! "GVS" dumps are the MAME `maincpu` region ONLY (verified byte-for-byte:
//! GVS Balloon Fight's four 8 KiB PRG chunks CRC32-match MAME `balonfgt`'s
//! `mds-bf4 a-3.1d/1c/1b/1a` exactly; the cabinet's SUB CPU runs the
//! different `.6d`/`.6a` ROMs, which the dumps omit — GVS Tennis likewise
//! matches `vstennisa`'s main region). The main program's boot handshake
//! waits forever for a sub-side answer that only the missing sub program can
//! write. The cabinet model itself is CI-verified by the synthetic-cart
//! suite in `vs_dualsystem_synth.rs`.
//!
//! **v2.0.0 beta.5 update (2026-07-02):** a combined 64 KiB-PRG dual dump
//! (main half + sub half, the Mesen2 `prgOuter` layout) was assembled for
//! TWO of the four titles — Balloon Fight and Wrecking Crew — once their
//! sub-CPU program ROMs were located (Tennis and Mahjong remain 32 KiB-only;
//! no sub-CPU dump has been located for either, so their tests above are
//! unchanged). See `docs/audit/vs-dualsystem-combined-dumps-2026-07-02.md`
//! for the full assembly/verification record. Outcome, confirmed by eye
//! against the dumped PNGs:
//!
//! - **Balloon Fight boots for real** on the combined dump: the boot
//!   handshake completes (unlike the previous total deadlock), both
//!   consoles render an identical, legible attract-mode menu screen
//!   ("1PLAYER VS. COMPUTER" / "2PLAYERS MUST USE BOTH SCREENS" / a credit
//!   counter that correctly tracks simulated coin insertions), and neither
//!   CPU jams over 1200 frames (20 simulated seconds). Promoted to a real,
//!   un-ignored test below (`gvs_balloon_fight_dual_combined_boots`) with an
//!   `insta` snapshot pin instead of the generic `assert_dual_alive`
//!   heuristic — see that test's doc comment for why.
//! - **Wrecking Crew is inconclusive** on the combined dump: neither CPU
//!   jams, and the wrapper's cross-wiring is demonstrably ACTIVE (bit-1
//!   `/IRQ` toggling both directions, shared-WRAM writes observed in a dense
//!   trace) — a genuine improvement over the prior flat-grey deadlock. But
//!   over 3600 frames (60 simulated seconds), with or without simulated coin
//!   insertions, the framebuffer never exceeds 3 distinct colours; it
//!   oscillates between 1 and 3 colours on a stable ~600-frame period (a
//!   blinking two-sprite animation, confirmed in the dumped PNGs — NOT
//!   garbage, but nowhere near a rendered title/attract screen). This is
//!   left as an `#[ignore]`d diagnostic below
//!   (`diag_gvs_wrecking_crew_dual_combined`) rather than promoted to a
//!   passing test — forcing an assertion around "3 colours forever" would
//!   be a false positive, and the data does not yet distinguish "needs a
//!   different input sequence this harness doesn't provide" from "a real
//!   residual bug in the cross-wiring model for this specific title."
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
use std::path::PathBuf;

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
    let dir = std::env::temp_dir().join("RustyNES/vs-dualsystem");
    if std::fs::create_dir_all(&dir).is_err() {
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

/// The combined-dump Balloon Fight boot, verified for real (v2.0.0 beta.5,
/// 2026-07-02). See the module docs for the full finding; this test pins it.
///
/// Deliberately does NOT reuse [`assert_dual_alive`]: that helper's
/// `> 4 distinct colours` heuristic and `main != sub` framebuffer check are
/// tuned for typical multi-colour commercial titles and gameplay where the
/// two cabinet halves have diverged into separate player views. Balloon
/// Fight's attract-mode menu is a genuine, correctly-rendering, but
/// deliberately two-colour (green backdrop + magenta text) screen shown
/// IDENTICALLY on both consoles before a game starts — both properties this
/// specific screen legitimately violates. Visually confirmed against
/// `/tmp/RustyNES/vs-dualsystem-real-boot/GVS_Balloon_Fight__Dual__{main,sub}.png`
/// before pinning this snapshot: real, legible text ("1PLAYER VS. COMPUTER",
/// "2PLAYERS MUST USE BOTH SCREENS", "CREDIT 10" — the credit count exactly
/// matches the 10 simulated coin pulses [`boot_dual`] performs over
/// 1200 frames), not garbage and not the flat-grey deadlocked screen the
/// 32 KiB-only dump produces.
#[test]
fn gvs_balloon_fight_dual_combined_boots() {
    let dual = boot_dual("vs-system/GVS Balloon Fight (Dual).nes", 1200);
    dump_pngs("GVS Balloon Fight (Dual)", &dual);

    assert!(!dual.main().is_jammed(), "MAIN console CPU jammed");
    assert!(!dual.sub().is_jammed(), "SUB console CPU jammed");

    let main_health = rustynes_test_harness::coverage::frame_health(dual.main_framebuffer());
    let sub_health = rustynes_test_harness::coverage::frame_health(dual.sub_framebuffer());

    // Hash-based regression pin (this crate's `insta` convention — see
    // `common::snapshot_text` / `tests/external_coverage.rs`): a multi-line
    // text summary rather than a raw pixel dump, so a genuine future
    // rendering change produces a small, reviewable diff instead of an
    // opaque binary blob.
    let snap = format!(
        "rom=vs-system/GVS Balloon Fight (Dual).nes\n\
         frames=1200\n\
         main_jammed={}\n\
         sub_jammed={}\n\
         main_cycle={}\n\
         sub_cycle={}\n\
         main_distinct_colors={}\n\
         sub_distinct_colors={}\n\
         main_fb_fnv1a64={:016x}\n\
         sub_fb_fnv1a64={:016x}",
        dual.main().is_jammed(),
        dual.sub().is_jammed(),
        dual.main().cycle(),
        dual.sub().cycle(),
        main_health.distinct_colors,
        sub_health.distinct_colors,
        common::fnv1a64(dual.main_framebuffer()),
        common::fnv1a64(dual.sub_framebuffer()),
    );
    insta::assert_snapshot!("gvs_balloon_fight_dual_combined_boots", snap);
}

/// Diagnostic (NOT promoted to a passing assertion — see the module docs'
/// 2026-07-02 update for why): boots the combined-dump Wrecking Crew for
/// 1200 frames and records its framebuffer health so a future session can
/// pick up the investigation without re-deriving the combined dump. Neither
/// CPU jams, but the framebuffer never exceeds a handful of colours in this
/// harness's 1200-frame window (see the longer 3600-frame + no-coin
/// experiments recorded in `docs/audit/vs-dualsystem-combined-dumps-2026-07-02.md`,
/// which rule out the simulated coin-insert timing as the cause).
#[test]
#[ignore = "combined 64 KiB dual dump: cross-wiring demonstrably active (non-jammed, bidirectional bit-1 IRQ + shared-WRAM writes observed) but the framebuffer does not reach a real title/attract screen within 60 simulated seconds - see docs/audit/vs-dualsystem-combined-dumps-2026-07-02.md before spending more time here"]
fn diag_gvs_wrecking_crew_dual_combined() {
    let dual = boot_dual("vs-system/GVS Wrecking Crew (Dual).nes", 1200);
    dump_pngs("GVS Wrecking Crew (Dual)", &dual);
    assert!(!dual.main().is_jammed(), "MAIN console CPU jammed");
    assert!(!dual.sub().is_jammed(), "SUB console CPU jammed");
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
