// SPDX-License-Identifier: GPL-3.0-or-later
//! v2.6.18 condition 1 — which SUB-TEST fails, under which access placement.
//!
//! # Why the battery cannot answer this
//!
//! The 7000-frame battery reports one status byte per catalog entry, so a
//! placement that breaks `Stale Sprite Shift Regs` tells you the entry moved
//! and nothing about WHICH of its six assertions moved. The sub-test corpus in
//! `tests/roms/AccuracyCoin/sub-tests/` boots straight into one entry, so a
//! standalone run yields `Fail(N)` with N naming the assertion — and it costs a
//! few hundred frames instead of seven thousand.
//!
//! # The result address is read, never assumed
//!
//! `AGENTS.md` records that a sub-test's result address and the catalog's can
//! differ, so this probe SCANS `$0400-$04FF` for whatever the ROM actually
//! wrote rather than indexing a constant. That also makes it work for a
//! sub-test nobody has characterised yet.
//!
//! # Encoding
//!
//! `TEST_Fail` stores `(ErrorCode << 2) | 2` and the runner sets `ErrorCode` to
//! **1** before every routine, so `Fail(N)` names assertion N **one-based**.
//! `$01` is a clean pass and `$00` means the entry never wrote a result.
//!
//! Diagnostic, not a gate. Run:
//! `cargo test -p rustynes-test-harness --release --features test-roms,phi2-write-sweep --test subtest_placement_probe -- --nocapture`
#![cfg(all(feature = "test-roms", feature = "phi2-write-sweep"))]
use core::sync::atomic::Ordering::Relaxed;
use rustynes_core::{Buttons, Nes};
use rustynes_test_harness::accuracy_coin_catalog as cat;
use std::path::PathBuf;

/// NTSC: 4 master clocks per dot; the shipped effective advance is 4 mc for a
/// read and 6 for a write, i.e. both land in the CPU cycle's dot 1.
const MC_PER_DOT: i32 = 4;
const READ_BASE_MC: i32 = 4;
const WRITE_BASE_MC: i32 = 6;

fn place(base_mc: i32, dot: i32) -> (u8, u8) {
    let delta = dot * MC_PER_DOT - base_mc;
    if delta >= 0 {
        (u8::try_from(delta).expect("offset fits"), 0)
    } else {
        (0, u8::try_from(-delta).expect("backoff fits"))
    }
}

fn apply(read_dot: i32, write_dot: i32) {
    let (ro, rb) = place(READ_BASE_MC, read_dot);
    let (wo, wb) = place(WRITE_BASE_MC, write_dot);
    rustynes_core::rustynes_cpu::READ_PHI_OFFSET.store(ro, Relaxed);
    rustynes_core::rustynes_cpu::READ_PHI_BACKOFF.store(rb, Relaxed);
    rustynes_core::rustynes_cpu::WRITE_PHI_OFFSET.store(wo, Relaxed);
    rustynes_core::rustynes_cpu::WRITE_PHI_BACKOFF.store(wb, Relaxed);
}

fn subtest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root")
        .join("tests/roms/AccuracyCoin/sub-tests")
}

/// Run one sub-test ROM standalone and return every non-zero byte it left on
/// the result page, decoded.
fn run_subtest(rom: &str, frames: u64, press_start: bool) -> Vec<(u16, u8, cat::TestStatus)> {
    let path = subtest_dir().join(rom);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("load {rom}: {e:?}"));
    if press_start {
        for _ in 0..120 {
            nes.run_frame();
        }
        nes.set_buttons(0, Buttons::START);
        for _ in 0..6 {
            nes.run_frame();
        }
        nes.set_buttons(0, Buttons::empty());
    }
    for _ in 0..frames {
        nes.run_frame();
    }
    let ram = nes.bus().ram_bytes().to_vec();
    (0x0400..0x0500)
        .filter_map(|addr| {
            let b = ram[addr as usize];
            (b != 0).then(|| (addr, b, cat::TestStatus::from_byte(b)))
        })
        .collect()
}

/// The two entries the write placement trades against, plus the one it buys.
const SUBTESTS: [&str; 3] = [
    "ppu-misc-stale-sprite-shift-regs.nes",
    "sprite-eval-arbitrary-sprite-zero.nes",
    "sprite-eval-misaligned-oam.nes",
];

#[test]
fn which_subtest_assertion_moves_with_the_access_dot() {
    for rom in SUBTESTS {
        eprintln!("=== {rom} ===");
        for (read_dot, write_dot) in [(1, 1), (1, 2), (2, 2), (0, 1), (0, 0)] {
            apply(read_dot, write_dot);
            let found = run_subtest(rom, 900, false);
            let rendered: Vec<String> = found
                .iter()
                .map(|(a, b, st)| format!("${a:04X}=${b:02X} {st:?}"))
                .collect();
            eprintln!(
                "  read=dot{read_dot} write=dot{write_dot}  {}",
                if rendered.is_empty() {
                    "(result page empty -- the ROM wrote nothing)".to_owned()
                } else {
                    rendered.join("  ")
                }
            );
        }
    }
    apply(1, 1);
}

/// `sprite-eval-arbitrary-sprite-zero` reports `Fail(1)` standalone while the
/// battery passes the entry, which is why the sibling's `regress.sh` leaves it
/// deliberately unregistered. Assertion 1 is "Sprite 0 should trigger a sprite
/// zero hit. No other sprite should." — a PRECONDITION every entry on that page
/// shares, so failing it standalone points at the ROM's entry conditions rather
/// than at sprite evaluation.
///
/// This asks the cheap question first: does it need the menu press, or longer?
#[test]
fn does_arbitrary_sprite_zero_stand_alone() {
    apply(1, 1);
    for (press, frames) in [(false, 300u64), (false, 900), (false, 2400), (true, 900)] {
        let found = run_subtest("sprite-eval-arbitrary-sprite-zero.nes", frames, press);
        let rendered: Vec<String> = found
            .iter()
            .map(|(a, b, st)| format!("${a:04X}=${b:02X} {st:?}"))
            .collect();
        eprintln!(
            "  press_start={press} frames={frames}  {}",
            if rendered.is_empty() {
                "(empty)".to_owned()
            } else {
                rendered.join("  ")
            }
        );
    }
}
