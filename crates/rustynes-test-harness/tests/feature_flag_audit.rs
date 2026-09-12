// SPDX-License-Identifier: GPL-3.0-or-later
//! Pins the cargo feature flags this workspace DECLARES against the ones
//! `docs/STATUS.md` DOCUMENTS, in both directions.
//!
//! # Why this exists
//!
//! `docs/STATUS.md` carries a "Flag | Crate(s) | Default | Purpose" table that
//! is how anyone answers "which knobs exist, and is each one off for a reason?"
//! Nothing kept it honest, and it had drifted in the worse of the two possible
//! directions: it advertised **`cpu-implied-dummy-reads` as an available,
//! default-OFF flag** long after the gate was deleted and the behaviour became
//! unconditional. A row claiming a knob that does not exist is bad; a row
//! describing shipped default behaviour as *disabled* is worse, because it
//! invites someone to "enable" a fix that has been on for releases.
//!
//! The reverse direction is a live gap rather than a defect: the table has far
//! fewer rows than the workspace has flags. That is recorded as an explicit
//! [`UNTABLED`] list with a reason per entry rather than left silent, because
//! this project has already learned that an unexplained omission cannot be told
//! apart from work outstanding, work deliberately skipped, and work already
//! done. The list can only shrink; adding a flag forces a decision here.
//!
//! # What it does NOT assert
//!
//! Nothing about whether a default is *correct*. That is a measurement (see
//! `docs/performance.md`'s adoption bar), not a property of a table.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Flags declared in a manifest but not yet given a `docs/STATUS.md` row, each
/// with the reason it is acceptable for now. Shrinking this list is the point;
/// growing it needs a reason written next to the entry.
const UNTABLED: &[(&str, &str)] = &[
    // Frontend build/shape selectors, covered by the README and `AGENTS.md`
    // build block rather than the accuracy-status table.
    ("wasm-winit", "frontend wasm build selector"),
    ("wasm-canvas", "frontend lightweight wasm embed"),
    (
        "emu-thread",
        "frontend: dedicated emulator thread, default ON",
    ),
    ("help-tui", "frontend help viewer, default ON"),
    ("gpu-timing", "frontend GPU timing queries, default ON"),
    ("full", "frontend aggregate alias over the native opt-ins"),
    // Opt-in user features. Deliberately off; documented where the feature is.
    (
        "retroachievements",
        "opt-in user feature (FFI + network dep)",
    ),
    ("scripting", "opt-in user feature (mlua)"),
    ("script-ipc", "opt-in user feature (scripting + network)"),
    ("script-wasm", "opt-in wasm scripting backend (piccolo)"),
    ("script-sqlite", "opt-in scripting storage backend"),
    ("mlua-backend", "rustynes-script default backend selector"),
    ("hd-pack", "opt-in user feature (HD pack loader)"),
    ("av-record", "opt-in user feature (GIF/WAV capture)"),
    ("browser-cheevos", "wasm-only RetroAchievements path"),
    ("debug-hooks", "opt-in dev tooling; hot-path cost when on"),
    ("netplay-client", "netplay crate default; the client half"),
    (
        "signaling-server",
        "netplay crate: the server binary's deps",
    ),
    // Diagnostics and studies. Off by construction: several of them CHANGE
    // what is under test when enabled (v2.4.1's `irq-timing-trace` finding),
    // so being on by default would be a defect rather than an improvement.
    ("cpu-boot-trace", "diagnostic trace fixture"),
    ("cpu-instr-cycle-trace", "diagnostic trace fixture"),
    ("ppu-fetch-trace", "diagnostic trace fixture"),
    ("ppu-octal-trace", "diagnostic calibration ring"),
    ("phi2-write-sweep", "v2.6.18 write-commit study knob"),
    (
        "mmc3-m2-phase-irq",
        "open R1/R2 IRQ-timing experiment (ADR 0002)",
    ),
    ("mmc3-a12-phase-probe", "R1/R2 A12-phase probe"),
    ("cosim-interrupt-inject", "co-simulation testbench hook"),
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root is two levels above this crate")
        .to_path_buf()
}

/// Feature names declared in every `crates/*/Cargo.toml`, mapped to the crates
/// declaring them. `default`, `std` and `serde` are cargo/serde plumbing rather
/// than project knobs and are excluded by name.
fn declared_features(root: &Path) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let crates_dir = root.join("crates");
    let entries = std::fs::read_dir(&crates_dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", crates_dir.display()));
    for entry in entries {
        let dir = entry.expect("dir entry").path();
        let manifest = dir.join("Cargo.toml");
        if !manifest.is_file() {
            continue;
        }
        let krate = dir
            .file_name()
            .expect("crate dir name")
            .to_string_lossy()
            .into_owned();
        let text = std::fs::read_to_string(&manifest)
            .unwrap_or_else(|e| panic!("read {}: {e}", manifest.display()));
        let mut in_features = false;
        for line in text.lines() {
            let t = line.trim();
            if t.starts_with('[') {
                in_features = t == "[features]";
                continue;
            }
            if !in_features || t.starts_with('#') {
                continue;
            }
            let Some((name, _)) = t.split_once('=') else {
                continue;
            };
            let name = name.trim();
            if name.is_empty()
                || matches!(name, "default" | "std" | "serde")
                || !name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                continue;
            }
            out.entry(name.to_owned())
                .or_default()
                .insert(krate.clone());
        }
    }
    assert!(
        out.len() > 10,
        "parsed only {} features from crates/*/Cargo.toml -- the parser is \
         broken, and an empty result would pass every assertion below",
        out.len()
    );
    out
}

/// `(flag, row_is_marked_removed)` for each row of `docs/STATUS.md`'s feature
/// table. Fails closed if the table cannot be located: a heading rename must
/// break this test rather than silently make it check nothing.
fn tabled_flags(root: &Path) -> Vec<(String, bool)> {
    const HEADER: &str = "| Flag | Crate(s) | Default | Purpose |";
    let path = root.join("docs/STATUS.md");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let start = text.find(HEADER).unwrap_or_else(|| {
        panic!(
            "{HEADER:?} not found in docs/STATUS.md -- if the feature table \
             was renamed or moved, update this audit in the same change"
        )
    });
    let table = &text[start..];
    let end = table.find("\n\n").unwrap_or(table.len());
    let mut rows = Vec::new();
    for line in table[..end].lines().skip(2) {
        let Some(rest) = line.strip_prefix("| `") else {
            continue;
        };
        let Some((flag, tail)) = rest.split_once('`') else {
            continue;
        };
        // A retired flag is kept as a row for the historical record and marked.
        let removed = tail.contains("*(removed)*");
        rows.push((flag.to_owned(), removed));
    }
    assert!(
        rows.len() > 5,
        "parsed only {} rows from the feature table -- the parser is broken",
        rows.len()
    );
    rows
}

/// A row may only claim a live flag if a manifest declares it.
///
/// This is the direction that was actually wrong: `cpu-implied-dummy-reads` sat
/// in the table as an available default-OFF knob while the gate had been
/// deleted and the behaviour made unconditional.
#[test]
fn every_documented_flag_is_declared_by_some_crate() {
    let root = repo_root();
    let declared = declared_features(&root);
    let mut orphans = Vec::new();
    for (flag, removed) in tabled_flags(&root) {
        if removed || declared.contains_key(&flag) {
            continue;
        }
        orphans.push(flag);
    }
    assert!(
        orphans.is_empty(),
        "docs/STATUS.md's feature table lists {} flag(s) that no crates/*/Cargo.toml \
         declares: {orphans:?}. Either the flag was retired -- in which case mark the \
         row *(removed)* and say what replaced it, as the two v2.0.0-era rows do -- or \
         the table is advertising a knob that does not exist.",
        orphans.len()
    );
}

/// Every declared flag is either documented or on the explicit gap list.
#[test]
fn every_declared_flag_is_documented_or_explicitly_deferred() {
    let root = repo_root();
    let tabled: BTreeSet<String> = tabled_flags(&root)
        .into_iter()
        .map(|(flag, _)| flag)
        .collect();
    let deferred: BTreeSet<&str> = UNTABLED.iter().map(|(flag, _)| *flag).collect();
    let mut undocumented = Vec::new();
    for flag in declared_features(&root).keys() {
        if !tabled.contains(flag) && !deferred.contains(flag.as_str()) {
            undocumented.push(flag.clone());
        }
    }
    assert!(
        undocumented.is_empty(),
        "these cargo features are declared but neither in docs/STATUS.md's feature \
         table nor in this test's UNTABLED list: {undocumented:?}. Add a table row, \
         or an UNTABLED entry WITH a reason -- an omission with no reason cannot be \
         told apart from an oversight.",
    );
}

/// The gap list must not rot: an entry naming a flag nobody declares any more is
/// a stale deferral, and the whole point of the list is that it shrinks.
#[test]
fn the_untabled_gap_list_names_only_live_flags() {
    let root = repo_root();
    let declared = declared_features(&root);
    let stale: Vec<&str> = UNTABLED
        .iter()
        .map(|(flag, _)| *flag)
        .filter(|flag| !declared.contains_key(*flag))
        .collect();
    assert!(
        stale.is_empty(),
        "UNTABLED defers flags that no longer exist: {stale:?}. Remove the entries."
    );
    let tabled: BTreeSet<String> = tabled_flags(&root)
        .into_iter()
        .map(|(flag, _)| flag)
        .collect();
    let both: Vec<&str> = UNTABLED
        .iter()
        .map(|(flag, _)| *flag)
        .filter(|flag| tabled.contains(*flag))
        .collect();
    assert!(
        both.is_empty(),
        "these flags are BOTH documented and deferred: {both:?}. Once a flag has a \
         table row, drop its UNTABLED entry so the list keeps measuring the gap."
    );
    for (flag, reason) in UNTABLED {
        assert!(
            reason.len() >= 10,
            "UNTABLED entry {flag:?} has no usable reason ({reason:?})"
        );
    }
}
