//! Data-driven commercial-ROM boot-coverage harness (auto-discovering).
//!
//! Where [`external_real_games`] and [`external_extended`] hand-write
//! ONE `#[test]` + `check(...)` per ROM (each carrying a curated INPUT
//! script tuned to reach that game's title / menu / gameplay state),
//! this harness takes the opposite tack: it **discovers every staged
//! ROM at runtime** and runs a single default boot/idle capture against
//! a per-ROM `insta` snapshot. New ROMs need NO code change — drop them
//! under `tests/roms/external/mapper-*/` and re-bless.
//!
//! This is the mechanism that lets per-mapper boot screenshots scale to
//! hundreds of ROMs (≥4-5 ROMs across all ~123 mapper families, per the
//! mapper-ROM-coverage policy) without an untenable hand-written test
//! count.
//!
//! ## Two assertions per ROM
//!
//! For every discovered ROM the harness runs the default boot capture
//! once and checks it two ways:
//!
//! 1. **Blank / few-colour health** — the SAME distinct-colour +
//!    dominant-fraction heuristic the `coverage_smoke` bin prints, shared
//!    via `rustynes_test_harness::coverage::frame_health` /
//!    `FrameHealth::looks_blank`. A crashed / hung / never-rendered boot
//!    collapses the frame to the backdrop colour (≤ 4 distinct colours,
//!    or one colour filling ≥ 99 % of the screen); a real title / menu
//!    draws dozens. A blank final frame fails the ROM. This catches a
//!    boot regression even before any baseline exists.
//! 2. **Baseline snapshot** — the `insta` `.snap` comparison (frame +
//!    audio + cycle hashes via [`snapshot_text`]), the regression net for
//!    a ROM that already has a committed baseline.
//!
//! ## Relationship to the curated harnesses (overlap)
//!
//! The two curated harnesses and this one DELIBERATELY overlap on the
//! ROM SET — a ROM staged for `external_real_games` is also discovered
//! here. They do NOT overlap on PURPOSE:
//!
//! - [`external_real_games`] / [`external_extended`]: hand-tuned input
//!   scripts (START taps, double-taps, long-intro waits, multi-stage
//!   menu navigation) so the captured frame lands on a MEANINGFUL,
//!   regression-sensitive screen. Keep these — they carry knowledge no
//!   auto-discovery can reconstruct.
//! - this file: a uniform [`DEFAULT_IDLE`] boot capture for EVERY staged
//!   ROM, so adding the 5th-Castlevania-clone to `mapper-002-UxROM/`
//!   gets a regression baseline for free. The snapshot id is derived
//!   purely from the relative path, so two harnesses snapshotting the
//!   same ROM produce DIFFERENT, non-colliding snapshot files (the test
//!   binary name + the derived id both differ).
//!
//! ## Honesty gate (ADR 0011) — reference-only, NOT a pass-gate
//!
//! This harness records boot output for Core / Curated / `BestEffort`
//! mappers ALIKE. It is a regression net + screenshot generator, **not**
//! an accuracy oracle: it never feeds the `AccuracyCoin` pass-gate and a
//! `BestEffort` ROM's baseline is reference-only (it locks in *current*
//! behavior, which for a `BestEffort` mapper may be imperfect by design).
//! `mapper_tier_honesty.rs` stays the authority on what counts as
//! accuracy-tested; this file does not touch that contract.
//!
//! ## Screenshot tier-split
//!
//! PNG dumps (when `RUSTYNES_DUMP_FRAMES=1`) all land flat under
//! `<DUMP_ROOT>/external/` here — this harness does not itself know a
//! mapper's tier. `scripts/screenshots/categorize_screenshots.py` runs
//! AFTERWARD and RELOCATES each `mapper-NNN-*` dir into
//! `screenshots/external/` (Core / Curated) or `screenshots/besteffort/`
//! (`BestEffort`) per the `rustynes-mappers` classifier. So the workflow
//! is: dump → categorize. The committed `.snap` baselines (emulator
//! output, never ROM bytes) are the assertion source of truth; the PNGs
//! are visual-verification aids.
//!
//! ## Feature gating
//!
//! ```text
//! cargo test -p rustynes-test-harness --features commercial-roms,test-roms \
//!     --test external_coverage -- --nocapture
//! ```
//!
//! Like the curated harnesses, `commercial-roms` is off by default so CI
//! never depends on non-distributable assets.
//!
//! ## Green on a fresh checkout (no staged ROMs)
//!
//! `tests/roms/external/` is gitignored, so a clean clone has no ROMs.
//! The discovery walk then finds zero `.nes` files and the single test
//! prints a SKIP line and returns `Ok` — it does NOT fail. The same is
//! true per-mapper: an empty `mapper-NNN-*/` dir contributes nothing.
//!
//! ## Blessing baselines for newly-staged ROMs
//!
//! ```bash
//! # Stage ROMs under tests/roms/external/mapper-NNN-Name/, then:
//! INSTA_UPDATE=auto RUSTYNES_DUMP_FRAMES=1 \
//!     cargo test -p rustynes-test-harness --features commercial-roms,test-roms \
//!     --test external_coverage -- --test-threads=1 --nocapture
//! # Inspect the PNGs at /tmp/rustynes-baseline-screenshots/external/,
//! # then sort them into the committed tree:
//! python3 scripts/screenshots/categorize_screenshots.py
//! # Accept the .snap.new files:
//! cargo insta accept
//! ```
//!
//! In `INSTA_UPDATE=auto` (or `always`) mode every missing / mismatched
//! baseline is written as a `.snap.new` file. insta still *reports* a
//! new/changed snapshot as a failed assertion (the run is non-zero), but
//! because this harness catches each per-ROM assertion panic and
//! aggregates (see below), the walk runs to completion and EVERY
//! `.snap.new` is produced in a single pass — so a bulk re-bless over
//! hundreds of newly-staged ROMs is one command + `cargo insta accept`.
//! In normal mode a mismatch is likewise caught per-ROM and aggregated
//! into one failure report instead of aborting on the first ROM.

#![cfg(feature = "commercial-roms")]
#![allow(clippy::doc_markdown)]
#![allow(clippy::too_many_lines)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::external::{InputScript, run_capture, snapshot_text};

/// Default boot/idle capture: 600 frames (10 s @ NTSC 60 Hz) with no
/// input. Long enough to clear the title-screen ramp-up + first
/// attract-mode tick for the overwhelming majority of commercial ROMs.
/// Mirrors the curated harnesses' `DEFAULT_IDLE` so a ROM that boots to
/// a stable title under both produces a directly comparable frame hash.
///
/// ROMs with a long pre-title intro (where 600 frames lands on a black
/// or animated frame) are better served by a hand-tuned entry in
/// `external_real_games` / `external_extended`; this harness's job is
/// breadth, not per-game perfection.
const DEFAULT_IDLE: InputScript = InputScript::IdleOnly { frames: 600 };

/// Walk `tests/roms/external/` and return every staged `.nes` ROM as a
/// path RELATIVE to that `external/` root (e.g.
/// `mapper-000-NROM/Donkey Kong.nes`), sorted for deterministic test
/// ordering + stable PNG-dump / snapshot iteration.
///
/// Only `mapper-*` (plus the special `fds` / `pc10` / `vs-system`)
/// sub-directories are walked, one level deep — the ROM corpus layout is
/// always `external/<dir>/<rom>.nes`. `.zip` is intentionally NOT
/// discovered: `run_capture` feeds raw bytes to `Nes::from_rom`, which
/// has no archive support, and the test-harness crate deliberately
/// carries no `zip` dependency. Stage ROMs as plain `.nes` (the
/// mapper-coverage policy stages `.nes` per-mapper); pre-extract any
/// archives first.
fn discover_external_roms() -> Vec<String> {
    let root = external_root();
    let mut out: Vec<String> = Vec::new();
    let Ok(entries) = fs::read_dir(&root) else {
        // No external/ tree at all (fresh checkout) — return empty so
        // the caller skips cleanly.
        return out;
    };
    let mut subdirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    subdirs.sort();
    for dir in subdirs {
        let Ok(files) = fs::read_dir(&dir) else {
            continue;
        };
        let dir_name = dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        let mut roms: Vec<String> = files
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nes")))
            .filter_map(|p| {
                p.file_name()
                    .and_then(|s| s.to_str())
                    .map(|name| format!("{dir_name}/{name}"))
            })
            .collect();
        roms.sort();
        out.extend(roms);
    }
    out
}

/// Resolve `<workspace>/tests/roms/external/`.
fn external_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("tests")
        .join("roms")
        .join("external")
}

/// Derive a deterministic, filesystem-safe `insta` snapshot id from a
/// ROM's `external/`-relative path. `mapper-000-NROM/Donkey Kong.nes`
/// becomes `mapper-000-NROM__Donkey_Kong` — the directory + rom-stem are
/// joined with `__` and every non-alphanumeric run is collapsed to a
/// single `_`. Stable across runs (no hashing of bytes), so the snapshot
/// file name is predictable from the ROM path alone.
fn snapshot_id(rom_rel: &str) -> String {
    let path = Path::new(rom_rel);
    let dir = path.parent().and_then(|p| p.to_str()).unwrap_or_default();
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(rom_rel);
    let joined = if dir.is_empty() {
        stem.to_string()
    } else {
        format!("{dir}__{stem}")
    };
    // Collapse every non-alphanumeric run to a single '_', trim edges.
    let mut id = String::with_capacity(joined.len());
    let mut prev_us = false;
    for c in joined.chars() {
        if c.is_ascii_alphanumeric() {
            id.push(c);
            prev_us = false;
        } else if !prev_us {
            id.push('_');
            prev_us = true;
        }
    }
    id.trim_matches('_').to_string()
}

/// Single auto-discovering coverage test.
///
/// Walks every staged ROM, runs the default boot capture, and applies
/// the two checks documented at the top of this file: (1) the shared
/// blank / few-colour health verdict, and (2) the derived `insta`
/// snapshot comparison. Per-ROM assertion panics are caught and
/// aggregated so one missing/mismatched baseline (or one blank boot)
/// does not hide the rest — the final panic message lists EVERY failing
/// ROM with its reason.
///
/// Skips cleanly (prints a SKIP line, passes) when no ROMs are staged,
/// so a fresh checkout without the gitignored dumps stays green.
#[test]
fn external_coverage_boot_smoke() {
    let roms = discover_external_roms();
    if roms.is_empty() {
        eprintln!(
            "[external_coverage] SKIP: no ROMs staged under {} — \
             stage commercial dumps per-mapper to populate this coverage net.",
            external_root().display()
        );
        return;
    }

    eprintln!(
        "[external_coverage] discovered {} staged ROM(s); running default \
         boot capture for each.",
        roms.len()
    );

    let mut failures: Vec<String> = Vec::new();
    for rom_rel in &roms {
        let id = snapshot_id(rom_rel);
        // Catch the per-ROM assertion panic (insta panics on a baseline
        // mismatch in normal mode; in INSTA_UPDATE=auto/always it writes
        // a .snap.new and does NOT panic) so the loop runs to completion
        // and we can report ALL failures at once. A ROM-read / parse
        // panic inside run_capture is caught here too and surfaces as a
        // clear per-ROM failure line.
        let rom = rom_rel.clone();
        let snap = id.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            move || -> Result<(), String> {
                let capture = run_capture(&rom, DEFAULT_IDLE);

                // (1) Blank / few-colour health — the shared coverage
                // heuristic. A real boot draws dozens of colours; a
                // crashed / never-rendered one collapses to the backdrop.
                // We do NOT panic on a blank frame (so the snapshot still
                // gets a chance to bless / compare); instead we record it
                // and surface it in the aggregated failure list.
                let health = capture.final_frame_health;
                let blank = if health.looks_blank() {
                    Some(format!(
                        "blank/few-colour boot: {} distinct colour(s), \
                         dominant {:.1}% of frame",
                        health.distinct_colors,
                        health.dominant_fraction * 100.0
                    ))
                } else {
                    None
                };

                // (2) Baseline snapshot comparison.
                let text = snapshot_text(&rom, DEFAULT_IDLE, &capture);
                insta::assert_snapshot!(snap.as_str(), text);

                // Snapshot passed; report the health verdict (if blank).
                blank.map_or(Ok(()), Err)
            },
        ));
        match result {
            // Snapshot passed AND frame not blank.
            Ok(Ok(())) => {}
            // Snapshot passed but the final frame was blank/few-colour.
            Ok(Err(reason)) => {
                failures.push(format!("{rom_rel}  (snapshot id: {id}) — {reason}"));
            }
            // run_capture panicked (read/parse) or insta panicked
            // (baseline mismatch / missing in normal mode).
            Err(_) => {
                failures.push(format!(
                    "{rom_rel}  (snapshot id: {id}) — snapshot mismatch or boot panic"
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "external_coverage: {} of {} staged ROM(s) failed their boot coverage \
         check (blank frame and/or baseline mismatch; re-bless baselines with \
         INSTA_UPDATE=auto … --test external_coverage, then `cargo insta \
         accept`):\n  {}",
        failures.len(),
        roms.len(),
        failures.join("\n  "),
    );
}
