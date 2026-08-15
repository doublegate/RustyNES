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
//! - this file: a uniform [`DEFAULT_CAPTURE`] boot capture for EVERY staged
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
//! # Stage ROMs under tests/roms/external/mapper-NNN-Name/, then use the ONE
//! # lock-guarded bless entry point (NEVER run two blesses at once / nohup it —
//! # they race the Cargo target lock; see the script header for the postmortem):
//! scripts/coverage/bless.sh                 # full sweep, single-threaded, flock'd
//! # Inspect the PNGs at /tmp/rustynes-baseline-screenshots/external/, then:
//! cargo insta accept
//! python3 scripts/coverage/coverage.py categorize
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

use common::external::{InputScript, run_capture_opt, snapshot_text};

/// Default boot capture for breadth coverage. A discovered ROM's intro
/// structure is unknown, so a passive idle frequently lands on a black
/// title / publisher splash; instead clear the initial ramp-up, then tap
/// START every `period` frames to press through multi-stage intros
/// (publisher -> story -> title -> menu), then free-run to a settled late
/// frame so the captured screen is gameplay / menu rather than a black
/// boot frame. `RepeatStartTap` is what the curated harnesses use for
/// intro-heavy games (Mega Man, Bandit Kings); here it is the default
/// because the structure is unknown per-ROM. ROMs that still land blank
/// get a hand-tuned entry in `external_real_games` / `external_extended`,
/// or indicate a genuine mapper-decode bug to fix.
const DEFAULT_CAPTURE: InputScript = InputScript::RepeatStartTap {
    warmup: 240,
    period: 150,
    taps: 5,
    free_run: 300,
    checkpoints: &[900, 1100],
};

/// Per-ROM capture override.
///
/// The `RepeatStartTap` [`DEFAULT_CAPTURE`] is right for intro-heavy games, but
/// for a sizeable class of titles a START tap ADVANCES the title screen into a
/// black transition / blank menu, so the captured frame collapses. Those games
/// render a clean, regression-sensitive title screen with a passive idle (no
/// input) settled at a late frame instead. A handful need a longer idle to
/// clear a slow fade. This map keys the `external/`-relative ROM path to a
/// tailored [`InputScript`]; everything not listed uses [`DEFAULT_CAPTURE`].
///
/// These are accuracy-neutral capture-timing choices (the coverage harness is a
/// screenshot generator + boot-smoke net, not the `AccuracyCoin` oracle). Each
/// entry was verified to land on a meaningful rendered frame via the
/// `coverage_smoke` bin.
fn capture_override(rom_rel: &str) -> Option<InputScript> {
    // Titles that render a title/menu with a passive idle; a START tap would
    // advance past it into a blank transition.
    const IDLE: InputScript = InputScript::IdleOnly { frames: 700 };
    // A few titles need a longer fade to settle on the title screen.
    const IDLE_LONG: InputScript = InputScript::IdleOnly { frames: 1200 };
    let idle = [
        "mapper-000-NROM/Gyromite.nes",
        "mapper-001-MMC1/Dr. Mario.nes",
        "mapper-001-MMC1/Dragon Warrior.nes",
        "mapper-001-MMC1/Metroid.nes",
        "mapper-022-VRC2a/Ganbare Pennant Race! (J) [!].nes",
        "mapper-025-VRC2-VRC4/Ganbare Goemon Gaiden - Kieta Ougon Kiseru (Japan) (En) (0.99c).nes",
        "mapper-048-TaitoTC0690/Bakushou!! Jinsei Gekijou 3 (Japan).nes",
        "mapper-082-TaitoX1-017/Kyuukyoku Harikiri Koushien (Japan).nes",
        "mapper-082-TaitoX1-017/Kyuukyoku Harikiri Stadium III (Japan).nes",
        "mapper-085-VRC7/Lagrange Point (J) [!].nes",
        "mapper-085-VRC7/Lagrange Point (Japan) (En) (1.01).nes",
        "mapper-119-TQROM/High Speed (E) [!].nes",
        "mapper-119-TQROM/Pin Bot (E) [!].nes",
    ];
    let idle_long = ["mapper-001-MMC1/Tecmo Bowl.nes"];
    if idle.contains(&rom_rel) {
        Some(IDLE)
    } else if idle_long.contains(&rom_rel) {
        Some(IDLE_LONG)
    } else {
        None
    }
}

/// Walk `tests/roms/external/` and return every staged `.nes` ROM as a
/// path RELATIVE to that `external/` root (e.g.
/// `mapper-000-NROM/Donkey Kong.nes`), sorted for deterministic test
/// ordering + stable PNG-dump / snapshot iteration.
///
/// Only `mapper-*` (plus the special `fds` / `pc10` / `vs-system`)
/// sub-directories are walked, one level deep — the ROM corpus layout is
/// always `external/<dir>/<rom>.<ext>`. Every loadable form the frontend
/// accepts is discovered (T-PS-059): iNES (`.nes`), UNIF (`.unf` / `.unif`),
/// FDS disk images (`.fds`), and `.zip` / `.7z` archives (the No-Intro
/// distribution form) — `run_capture_opt` (via `common::external::load_nes`)
/// mirrors the frontend's load dispatch, unwrapping an archive to its first
/// NES/FDS/UNIF entry and routing an FDS disk through `Nes::from_disk` with a
/// resolved BIOS. So a ROM left zipped, or an `.fds` disk, gets a boot
/// screenshot just like a loose `.nes`.
fn discover_external_roms_raw() -> Vec<String> {
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
            .filter(|p| {
                p.is_file()
                    && p.extension().is_some_and(|e| {
                        // v1.6.0 (E2): UNIF (.unf/.unif) dumps are boot-captured
                        // alongside iNES (.nes) ones. T-PS-059: also FDS disk
                        // images (.fds) and .zip / .7z archives, so a ROM left
                        // in any loadable form gets a boot screenshot. load_nes
                        // unwraps the archive / routes the FDS disk.
                        e.eq_ignore_ascii_case("nes")
                            || e.eq_ignore_ascii_case("unf")
                            || e.eq_ignore_ascii_case("unif")
                            || e.eq_ignore_ascii_case("fds")
                            || e.eq_ignore_ascii_case("zip")
                            || e.eq_ignore_ascii_case("7z")
                    })
            })
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

/// Every staged ROM, with snapshot-id collisions resolved. This is what the
/// sweep runs; [`discover_external_roms_raw`] is what it found before
/// [`dedupe_colliding_ids`] had a say, and the two are kept separate so a test
/// can assert something about the raw corpus that deduplication would otherwise
/// have made true by construction.
fn discover_external_roms() -> Vec<String> {
    dedupe_colliding_ids(discover_external_roms_raw())
}

/// Collapse ROMs that share a [`snapshot_id`], or fail loudly if they cannot be.
///
/// Two staged files can normalise to one id even after the extension is folded
/// in, because the id sanitiser maps every non-alphanumeric run to a single `_`:
/// `Magic Dragon (Unl).nes` and `Magic Dragon _Unl_.nes` are distinct files with
/// the same id. Left alone that is *permanent* -- both write the same snapshot
/// with a different `rom=` line, so exactly one of the pair mismatches on every
/// run and blessing one breaks the other. That is the same failure the extension
/// suffix was added to fix, arriving by a second route, and it is invisible in
/// the output because a mismatch looks like ordinary baseline drift.
///
/// When the colliding files are **byte-identical** -- which is what a sanitised
/// duplicate of an existing dump is -- one baseline is the correct answer for
/// both, so the lexicographically-first path is kept and the rest are dropped
/// with a note. When they **differ**, no single baseline can be right and the
/// sweep aborts naming the paths, because that needs a human to rename or
/// remove one, not a silent choice made here.
fn dedupe_colliding_ids(roms: Vec<String>) -> Vec<String> {
    dedupe_colliding_ids_in(&external_root(), roms)
}

/// [`dedupe_colliding_ids`] against an explicit corpus root, so the collision
/// handling is testable without staging colliding ROMs in the real corpus. The
/// staged corpus is currently 1:1 (691 files, 691 ids), which is exactly why
/// this needs its own test: a safety net nothing exercises is decoration.
fn dedupe_colliding_ids_in(root: &Path, roms: Vec<String>) -> Vec<String> {
    use std::collections::BTreeMap;

    let mut by_id: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for rom in roms {
        by_id.entry(snapshot_id(&rom)).or_default().push(rom);
    }

    let mut kept = Vec::new();
    let mut conflicts = Vec::new();
    for (id, mut group) in by_id {
        if group.len() > 1 {
            group.sort();
            let digests: Vec<Option<[u8; 32]>> =
                group.iter().map(|r| file_digest(&root.join(r))).collect();
            let all_same = digests.windows(2).all(|w| w[0].is_some() && w[0] == w[1]);
            if all_same {
                eprintln!(
                    "[external_coverage] snapshot id `{id}` is shared by {} byte-identical \
                     staged files; keeping `{}` and skipping the rest: {:?}",
                    group.len(),
                    group[0],
                    &group[1..]
                );
            } else {
                conflicts.push(format!("  id `{id}` <- {group:?}"));
                continue;
            }
        }
        kept.push(group.swap_remove(0));
    }

    assert!(
        conflicts.is_empty(),
        "external_coverage: {} snapshot id(s) are shared by staged ROMs whose CONTENTS differ, \
         so no single baseline can be correct for them and each run would report a spurious \
         mismatch. Rename or remove one of each set:\n{}",
        conflicts.len(),
        conflicts.join("\n")
    );

    kept.sort();
    kept
}

/// SHA-256 of a file, or `None` if it cannot be read.
fn file_digest(path: &Path) -> Option<[u8; 32]> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).ok()?;
    Some(Sha256::digest(bytes).into())
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
    // v2.3.4 — the EXTENSION has to take part, or a game staged in two forms
    // collides on one id.
    //
    // The corpus deliberately stages some titles both loose and archived, so
    // the archive load path gets boot coverage too (T-PS-059). Because the id
    // was built from `file_stem()`, `Foo.nes` and `Foo.zip` produced the SAME
    // id, wrote the same snapshot with a different `rom=` line, and exactly one
    // of every pair mismatched on every run -- permanently, and un-fixable by
    // blessing, because blessing one broke the other. Measured on the corpus:
    // 54 colliding ids over 108 ROMs, which is exactly the number of otherwise
    // unexplained persistent mismatches.
    //
    // `.nes` keeps the bare id and every other form is suffixed, rather than
    // appending unconditionally, because that would have orphaned every
    // committed baseline at once.
    //
    // It does NOT make the change free, and an earlier version of this comment
    // claimed it did. The bare id only survives for a title that has a `.nes`
    // form; an archive-only ROM previously owned the bare id outright, so its
    // baseline IS orphaned by the suffix. Measured on this corpus: 283 of the
    // 691 staged ROMs, which were re-blessed under their new ids and whose dead
    // baselines were removed in the same change. The remaining orphaned
    // baselines belong to ROMs not staged on this machine and were deliberately
    // left alone -- `tests/roms/external/` is gitignored, so a baseline with no
    // local ROM means someone else's corpus is larger, not that it is stale.
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| !e.eq_ignore_ascii_case("nes"))
        .unwrap_or_default();
    let stem_ext = if ext.is_empty() {
        stem.to_string()
    } else {
        format!("{stem}_{ext}")
    };
    let joined = if dir.is_empty() {
        stem_ext
    } else {
        format!("{dir}__{stem_ext}")
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
/// v2.3.4 — narrow the sweep to the ROMs whose paths contain any of the
/// comma-separated needles in `RUSTYNES_COVERAGE_FILTER` (case-insensitive).
///
/// The full sweep is **699 ROMs x ~1,290 boot frames == ~900k emulated frames,
/// about 70 minutes single-threaded**. That is the right cost for a release
/// gate and the wrong cost for the loop you actually work in — diagnosing one
/// drifted baseline should not require re-booting the entire corpus, and
/// before this the only way to re-check one ROM was to run all of them.
///
/// Matching is a plain substring over the ROM's path relative to
/// `tests/roms/external/`, so the useful granularities all fall out of one
/// mechanism: a family (`mapper-250`), a directory (`pc10`), a specific title
/// (`Power Blade`), or several at once (`pc10,mapper-250`).
///
/// ```text
/// RUSTYNES_COVERAGE_FILTER=pc10 cargo test -p rustynes-test-harness \
///     --features test-roms,commercial-roms --test external_coverage
/// ```
///
/// Unset (the default) runs everything, so the gate is unchanged. An
/// unmatched filter is a hard failure rather than a silent pass: a typo that
/// quietly ran zero ROMs and reported green is exactly the "the suite is fine"
/// answer that hides a red net.
fn apply_filter(all: Vec<String>) -> Vec<String> {
    let Ok(raw) = std::env::var("RUSTYNES_COVERAGE_FILTER") else {
        return all;
    };
    let needles: Vec<String> = raw
        .split(',')
        .map(|n| n.trim().to_ascii_lowercase())
        .filter(|n| !n.is_empty())
        .collect();
    if needles.is_empty() {
        return all;
    }
    let total = all.len();
    let kept: Vec<String> = all
        .into_iter()
        .filter(|rom| {
            let hay = rom.to_ascii_lowercase();
            needles.iter().any(|n| hay.contains(n.as_str()))
        })
        .collect();
    assert!(
        !kept.is_empty(),
        "RUSTYNES_COVERAGE_FILTER={raw:?} matched none of the {total} staged ROMs. \
         Refusing to report a green run over an empty set — check the spelling, or \
         unset the variable to sweep everything."
    );
    eprintln!(
        "[external_coverage] filter {raw:?} -> {} of {total} staged ROM(s)",
        kept.len()
    );
    kept
}

/// does not hide the rest — the final panic message lists EVERY failing
/// ROM with its reason.
///
/// Skips cleanly (prints a SKIP line, passes) when no ROMs are staged,
/// so a fresh checkout without the gitignored dumps stays green.
#[test]
fn external_coverage_boot_smoke() {
    let all = discover_external_roms();
    let roms = apply_filter(all);
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
        let capture_script = capture_override(rom_rel).unwrap_or(DEFAULT_CAPTURE);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            move || -> Result<Option<()>, String> {
                // FDS disk on a BIOS-less checkout -> clean skip (Ok(None)).
                let Some(capture) = run_capture_opt(&rom, capture_script) else {
                    return Ok(None);
                };

                // (1) Blank / few-colour health — the shared coverage
                // heuristic. A real boot draws dozens of colours; a
                // crashed / never-rendered one collapses to the backdrop.
                // We do NOT panic on a blank frame (so the snapshot still
                // gets a chance to bless / compare); instead we record it
                // and surface it in the aggregated failure list.
                // v2.3.4 — judge the BEST frame the capture saw, not the one it
                // happened to stop on. The check exists to catch a boot that
                // never rendered; a boot that crashed or hung shows nothing at
                // every checkpoint, while a healthy ROM whose script ends on a
                // dark transition (Solstice: full title at f1100, uniform black
                // at the final frame) is not a failure and was being reported as
                // one.
                let health = capture.best_frame_health;
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
                let text = snapshot_text(&rom, capture_script, &capture);
                insta::assert_snapshot!(snap.as_str(), text);

                // Snapshot passed; report the health verdict (if blank).
                blank.map_or(Ok(Some(())), Err)
            },
        ));
        match result {
            // Snapshot passed AND frame not blank.
            Ok(Ok(Some(()))) => {}
            // ROM was an FDS disk with no resolvable BIOS — clean skip.
            Ok(Ok(None)) => {
                eprintln!("[external_coverage] SKIP {rom_rel}: FDS disk, no BIOS resolved.");
            }
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

// ---------------------------------------------------------------------------
// Snapshot-id collision handling.
//
// These run without any staged ROM, against a temp corpus, because the real
// corpus is deliberately 1:1 (691 files, 691 ids) after the duplicate cleanup --
// so nothing in the sweep exercises this path any more. It is a regression net
// for a failure that has now arrived twice by two different routes (archive
// forms sharing a stem, then sanitized filename copies), and both times it
// presented as ordinary baseline drift rather than as an error.
// ---------------------------------------------------------------------------

/// Build a throwaway corpus root and return its path plus the relative names.
fn stage_tmp_corpus(tag: &str, files: &[(&str, &[u8])]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("rustynes-coverage-collide-{tag}"));
    let _ = std::fs::remove_dir_all(&root);
    for (rel, bytes) in files {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, bytes).expect("write");
    }
    root
}

#[test]
fn byte_identical_files_sharing_a_snapshot_id_collapse_to_one() {
    // `(Unl)` and `_Unl_` normalise to the same id, and both are `.nes`, so the
    // extension suffix cannot separate them. Same bytes => one baseline is the
    // right answer for both.
    let root = stage_tmp_corpus(
        "identical",
        &[
            ("m107/Magic Dragon (Unl).nes", b"ROMBYTES"),
            ("m107/Magic Dragon _Unl_.nes", b"ROMBYTES"),
            ("m107/Other Game.nes", b"DIFFERENT"),
        ],
    );
    let roms = vec![
        "m107/Magic Dragon (Unl).nes".to_string(),
        "m107/Magic Dragon _Unl_.nes".to_string(),
        "m107/Other Game.nes".to_string(),
    ];
    assert_eq!(snapshot_id(&roms[0]), snapshot_id(&roms[1]), "premise");

    let kept = dedupe_colliding_ids_in(&root, roms);
    assert_eq!(
        kept.len(),
        2,
        "the colliding pair must collapse to one entry"
    );
    assert!(
        kept.contains(&"m107/Magic Dragon (Unl).nes".to_string()),
        "the lexicographically-first path is the one kept, so the `rom=` line in \
         an already-blessed baseline stays correct: {kept:?}"
    );
    assert!(kept.contains(&"m107/Other Game.nes".to_string()));
}

#[test]
#[should_panic(expected = "whose CONTENTS differ")]
fn differing_files_sharing_a_snapshot_id_abort_the_sweep() {
    // Different bytes cannot share a baseline, so this must be loud rather than
    // silently picking one -- picking one is what produced a permanent,
    // un-blessable mismatch on every run the last two times.
    let root = stage_tmp_corpus(
        "differing",
        &[
            ("m107/Magic Dragon (Unl).nes", b"ROM-A"),
            ("m107/Magic Dragon _Unl_.nes", b"ROM-B"),
        ],
    );
    let roms = vec![
        "m107/Magic Dragon (Unl).nes".to_string(),
        "m107/Magic Dragon _Unl_.nes".to_string(),
    ];
    let _ = dedupe_colliding_ids_in(&root, roms);
}

#[test]
fn a_corpus_without_collisions_passes_through_unchanged() {
    let root = stage_tmp_corpus("clean", &[("m000/A.nes", b"A"), ("m000/B.nes", b"B")]);
    let roms = vec!["m000/A.nes".to_string(), "m000/B.nes".to_string()];
    assert_eq!(dedupe_colliding_ids_in(&root, roms.clone()), roms);
}

#[test]
fn the_staged_corpus_has_no_snapshot_id_collisions() {
    // Standing assertion on the real corpus: after the v2.3.4 cleanup it is 1:1,
    // and a future staging mistake should surface here rather than as drift.
    // RAW, deliberately: `discover_external_roms()` runs the deduper, so
    // asserting uniqueness on its output only proves the deduper returned unique
    // ids -- true by construction, and a test that cannot fail.
    let roms = discover_external_roms_raw();
    let mut ids: Vec<String> = roms.iter().map(|r| snapshot_id(r)).collect();
    ids.sort();
    let before = ids.len();
    ids.dedup();
    assert_eq!(
        before,
        ids.len(),
        "{} staged ROM(s) share a snapshot id BEFORE deduplication. The deduper \
             will collapse byte-identical ones, but the corpus is meant to be 1:1 -- \
             a new collision means a duplicate or a near-duplicate filename was staged.",
        before - ids.len()
    );
}
