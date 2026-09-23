// SPDX-License-Identifier: GPL-3.0-or-later
//! User data is written through `atomic_write::write_atomic`, never by a call
//! that truncates the target in place.
//!
//! `fs::write` and `File::create` truncate the target first and writes second, so a crash, a full
//! disk or a killed process between the two leaves the user holding a truncated
//! file: a movie they recorded, a `TAStudio` project, an FDS disk save. v2.3.9
//! built `write_atomic` for exactly that and moved config, save states, cheats
//! and per-game overlays onto it; the frontend audit (SEC-06) found the FDS
//! save, `RetroAchievements`, movie and export paths still on the bare call, and
//! v2.7.1 moved them. This test keeps the next one from arriving unnoticed.
//!
//! It scans `src/` (never this file) for every call in [`WRITERS`] and stops
//! reading each file at its `mod tests`, because test fixtures legitimately
//! seed files directly. The first version matched `fs::write(` alone; review on
//! #548 showed that let `File::create` through, and the PPU pattern-table PNG
//! export was truncating an existing file that way while this test was green.
//! A production write that is genuinely not user data, or not a truncation, goes
//! in [`ALLOWED`] with the reason -- the list is where a reviewer sees each
//! exception argued. Every entry names the FILE and the CALL, so an exception
//! for one writer in a file does not excuse a different one beside it.
//!
//! A pattern match is a coarse instrument (`fs :: write (` would slip past); it
//! relies on rustfmt normalising call spacing, which the CI format gate enforces.

use std::fs;
use std::path::{Path, PathBuf};

/// Calls that create or truncate a file in place.
const WRITERS: &[&str] = &["fs::write(", "File::create(", "OpenOptions::new("];

/// `(path relative to src/, call, reason)` for production writers that stay.
const ALLOWED: &[(&str, &str, &str)] = &[
    (
        "atomic_write.rs",
        "OpenOptions::new(",
        "this IS the atomic writer: it opens its temp file with create_new before \
         the rename that publishes it",
    ),
    (
        "debugger/trace_panel.rs",
        "fs::write(",
        "the CPU trace dump goes to the OS temp dir under a fixed name; it is \
         regenerated on demand and holds nothing the user authored",
    ),
    (
        "av_record.rs",
        "File::create(",
        "the raw video/audio capture temps (`*.rustynes-avtmp`), created fresh per \
         recording and deleted after the mux; the user's output goes through a \
         staged encode that is renamed into place only on success",
    ),
    (
        "perf_log.rs",
        "File::create(",
        "the perf-overlay trace CSVs are diagnostic output streamed during a run, \
         under a per-run timestamped name, not user data",
    ),
    (
        "debugger/header_editor.rs",
        "OpenOptions::new(",
        "opens the ROM for writing WITHOUT truncate and overwrites only the 16-byte \
         header in place; the body is never touched and nothing is truncated",
    ),
];

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).expect("read src dir") {
        let p = e.expect("dir entry").path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn production_code_writes_user_data_atomically() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rs_files(&src, &mut files);
    assert!(
        files.len() > 20,
        "found only {} source files under {}",
        files.len(),
        src.display()
    );

    let mut offenders = Vec::new();
    let mut allowed_seen = Vec::new();
    for f in &files {
        let rel = f
            .strip_prefix(&src)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let text = fs::read_to_string(f).expect("read source");
        for (n, line) in text.lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with("mod tests") || t.starts_with("pub mod tests") {
                break; // fixtures below here may seed files with fs::write
            }
            if t.starts_with("//") {
                continue;
            }
            for w in WRITERS {
                if !line.contains(w) {
                    continue;
                }
                if ALLOWED.iter().any(|(p, c, _)| *p == rel && c == w) {
                    allowed_seen.push((rel.clone(), *w));
                } else {
                    offenders.push(format!("{rel}:{}: {}", n + 1, t));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "a file truncated in place in production code -- use \
         crate::atomic_write::write_atomic, or add (file, call, reason) to ALLOWED:\n  {}",
        offenders.join("\n  ")
    );
    // An allow-list entry that no longer matches anything is a stale exception
    // that would silently cover a future write in that file.
    for (p, c, _) in ALLOWED {
        assert!(
            allowed_seen.iter().any(|(s, w)| s == p && w == c),
            "ALLOWED entry ({p}, {c}) matches nothing; remove it"
        );
    }
}
