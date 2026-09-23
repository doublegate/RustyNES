//! Standing audit — every in-file `// Provenance:` header has a row in
//! `docs/originality-and-provenance.md` §1, and every §1 row names a file that
//! exists and carries a header.
//!
//! **Why this exists.** `RustyNES`'s derivation record lives in three places: the
//! header at the top of each derived file, the §1 table, and `NOTICE`. They are
//! written by hand, at different times, and nothing compared them. v2.7.1
//! classified three more files as derived (`m085_vrc7.rs`, `m099_vs_system.rs`,
//! `m244_cne_decathlon.rs`, core audit §6.2) and had to update all three places
//! by hand; a header added without its row, or a row whose file was renamed
//! away, would have been invisible until an outside reviewer tripped over it —
//! which is how the original provenance failure was found.
//!
//! It also corrects a snapshot. `AGENTS.md` said to regenerate the header list
//! with `grep -rn "^// Provenance:" crates/*/src/*.rs`, and that glob does not
//! descend: it missed `debugger/source_map.rs` and `bin/pgo_trainer.rs`, both
//! derived and both recorded in §1. This test walks the whole tree.
//!
//! **What it does not do.** It checks that the record is *consistent*, not that
//! it is *complete* or *correct*: a derived region nobody has disclosed has no
//! header and no row, and passes. Whether code is derived is a human judgement
//! (the project's rule is never to self-certify it); this only stops the
//! disclosures that do exist from drifting apart.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Workspace root, derived from this crate's manifest dir rather than the CWD.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/<crate>/ is two levels below the workspace root")
        .to_path_buf()
}

/// §1 rows that name a file with no header of its own, each with the reason.
/// The list is where a reviewer sees the exception argued.
const ROW_WITHOUT_HEADER: &[(&str, &str)] = &[(
    "crates/rustynes-gfx-shaders/src/lib.rs",
    "named in the crt_stack.rs row because it re-exports CRT_ROYALE_WGSL / CRT_GUEST_WGSL / \
     MEGATRON_WGSL; the reimplemented shaders themselves live in crt_stack.rs, which carries \
     the header",
)];

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    // An unreadable directory fails the test rather than being skipped: a
    // skipped directory is a set of files whose headers were never checked,
    // and the test would still report a pass (review finding on #548).
    let rd = fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
    for e in rd {
        let p = e.expect("dir entry").path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if p.is_dir() {
            // Vendored third-party trees carry their own notices, not ours.
            if name != "target" && name != "vendor" {
                rs_files(&p, out);
            }
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Files whose leading comment block carries a `// Provenance:` header.
fn files_with_header(root: &Path) -> BTreeSet<String> {
    let mut files = Vec::new();
    rs_files(&root.join("crates"), &mut files);
    assert!(
        files.len() > 300,
        "found only {} .rs files; the walk is wrong",
        files.len()
    );
    files
        .iter()
        .filter(|f| {
            let text = fs::read_to_string(f).expect("read source");
            // The header sits in the first comment block, after the SPDX line.
            text.lines()
                .take(8)
                .any(|l| l.starts_with("// Provenance:"))
        })
        .map(|f| {
            f.strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

/// Paths named in the first column of the §1 table. A cell may name several
/// files (`` `crates/x/src/a.rs`, `src/b.rs` ``); a bare `src/...` is relative
/// to the crate of the first path in the same cell.
fn section1_paths(root: &Path) -> BTreeSet<String> {
    let doc = fs::read_to_string(root.join("docs/originality-and-provenance.md"))
        .expect("read docs/originality-and-provenance.md");
    let start = doc
        .find("\n## 1. ")
        .expect("docs/originality-and-provenance.md has lost its section 1 heading");
    let end = doc[start + 1..]
        .find("\n## ")
        .map_or(doc.len(), |i| start + 1 + i);
    let mut out = BTreeSet::new();
    for line in doc[start..end].lines() {
        let Some(cell) = line.strip_prefix("| ").and_then(|l| l.split(" | ").next()) else {
            continue;
        };
        let mut crate_dir: Option<String> = None;
        for (i, piece) in cell.split('`').enumerate() {
            if i % 2 == 0 || Path::new(piece).extension().is_none_or(|x| x != "rs") {
                continue; // outside backticks, or not a path
            }
            let path = if piece.starts_with("crates/") {
                crate_dir = piece.split("/src/").next().map(str::to_owned);
                piece.to_owned()
            } else {
                let base = crate_dir.as_deref().unwrap_or_else(|| {
                    panic!("section 1 cell `{cell}` names `{piece}` before any crate path")
                });
                format!("{base}/{piece}")
            };
            out.insert(path);
        }
    }
    assert!(
        out.len() > 20,
        "parsed only {} paths from section 1; the parser is wrong",
        out.len()
    );
    out
}

#[test]
fn every_provenance_header_has_a_section_1_row() {
    let root = workspace_root();
    let headers = files_with_header(&root);
    let rows = section1_paths(&root);
    let missing: Vec<_> = headers.difference(&rows).collect();
    assert!(
        missing.is_empty(),
        "these files disclose a derivation in their header but have no row in \
         docs/originality-and-provenance.md section 1 (add the row, and the NOTICE entry):\n  {}",
        missing
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

#[test]
fn every_section_1_row_names_a_file_that_carries_a_header() {
    let root = workspace_root();
    let headers = files_with_header(&root);
    let rows = section1_paths(&root);
    let mut problems = Vec::new();
    for row in &rows {
        if !root.join(row).is_file() {
            problems.push(format!("{row}: no such file (renamed or removed?)"));
        } else if !headers.contains(row) && !ROW_WITHOUT_HEADER.iter().any(|(p, _)| p == row) {
            problems.push(format!(
                "{row}: listed as derived but has no `// Provenance:` header"
            ));
        }
    }
    // An exception that no longer matches a row is stale, and would silently
    // excuse a future file of the same name.
    for (p, _) in ROW_WITHOUT_HEADER {
        if !rows.contains(*p) {
            problems.push(format!(
                "ROW_WITHOUT_HEADER entry {p} matches no section 1 row; remove it"
            ));
        } else if headers.contains(*p) {
            problems.push(format!(
                "ROW_WITHOUT_HEADER entry {p} now has a header; remove the exception"
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "section 1 and the headers disagree:\n  {}",
        problems.join("\n  ")
    );
}
