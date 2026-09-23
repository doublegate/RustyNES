// SPDX-License-Identifier: GPL-3.0-or-later
//! User data is written through `atomic_write::write_atomic`, never a bare
//! `fs::write`.
//!
//! `fs::write` truncates the target first and writes second, so a crash, a full
//! disk or a killed process between the two leaves the user holding a truncated
//! file: a movie they recorded, a `TAStudio` project, an FDS disk save. v2.3.9
//! built `write_atomic` for exactly that and moved config, save states, cheats
//! and per-game overlays onto it; the frontend audit (SEC-06) found the FDS
//! save, `RetroAchievements`, movie and export paths still on the bare call, and
//! v2.7.1 moved them. This test keeps the next one from arriving unnoticed.
//!
//! It scans `src/` (never this file) and stops reading each file at its
//! `mod tests`, because test fixtures legitimately seed files with `fs::write`.
//! A production write that is genuinely disposable goes in [`ALLOWED`] with the
//! reason -- the list is the place a reviewer sees the exception argued.

use std::fs;
use std::path::{Path, PathBuf};

/// `(path relative to src/, reason)` for production writes that stay bare.
const ALLOWED: &[(&str, &str)] = &[(
    "debugger/trace_panel.rs",
    "the CPU trace dump goes to the OS temp dir under a fixed name; it is \
     regenerated on demand and holds nothing the user authored",
)];

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
            if line.contains("fs::write(") {
                if ALLOWED.iter().any(|(p, _)| *p == rel) {
                    allowed_seen.push(rel.clone());
                } else {
                    offenders.push(format!("{rel}:{}: {}", n + 1, t));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "bare fs::write in production code -- use crate::atomic_write::write_atomic, \
         or add the site to ALLOWED with the reason it is disposable:\n  {}",
        offenders.join("\n  ")
    );
    // An allow-list entry that no longer matches anything is a stale exception
    // that would silently cover a future write in that file.
    for (p, _) in ALLOWED {
        assert!(
            allowed_seen.iter().any(|s| s == p),
            "ALLOWED entry {p} matches nothing; remove it"
        );
    }
}
