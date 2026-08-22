// SPDX-License-Identifier: GPL-3.0-or-later
//! Release notes must not be hard-wrapped, because GitHub renders them with
//! hard line breaks.
//!
//! # The regression this exists to prevent
//!
//! GitHub renders release bodies (like issue and PR bodies) with GFM's
//! *hard-line-break* extension: a single newline inside a paragraph becomes a
//! `<br>`. It is not `CommonMark`, where such a newline is a space.
//!
//! So a paragraph hard-wrapped at 80 columns does not render as a paragraph. It
//! renders as a ragged column of short lines, broken mid-sentence at whatever
//! width the author's editor happened to use. Every release from v1.10.0 to
//! v2.4.1 was written as one long line per paragraph and rendered correctly;
//! v2.4.2, v2.4.3 and v2.4.4 were hard-wrapped and did not.
//!
//! Nothing caught it. `markdownlint` cannot: `MD013` (line length) is disabled
//! in this repository by design, for the long technical tables — and even
//! enabled it would have demanded the *opposite* of what GitHub needs.
//!
//! # Why a paragraph check rather than a line-length check
//!
//! "Lines must be long" is the wrong rule: a short paragraph is legitimately a
//! short line, and a table row or list item is legitimately short. The property
//! that actually matters is structural — **a paragraph must occupy exactly one
//! line** — and it is exact rather than a heuristic threshold.
//!
//! Fix a failure with the tool this repository already has:
//!
//! ```console
//! $ python3 scripts/release-automation/reflow.py < FILE > FILE.new && mv FILE.new FILE
//! ```

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root is two levels above this crate")
        .to_path_buf()
}

/// Lines that begin a structure of their own, and therefore end a paragraph.
///
/// Deliberately conservative: anything that might not be flowing prose ends the
/// run rather than joining it, so the check reports only genuine wrapped
/// paragraphs.
fn is_structural(line: &str) -> bool {
    let t = line.trim_start();
    t.is_empty()
        || t.starts_with('#')
        || t.starts_with('|')
        || t.starts_with('>')
        || t.starts_with("- ")
        || t.starts_with("* ")
        || t.starts_with("---")
        || t.starts_with("<!--")
        || t.starts_with('<')
        || t.chars().next().is_some_and(|c| c.is_ascii_digit())
            && t.contains(". ")
            && t.split_once(". ")
                .is_some_and(|(n, _)| n.chars().all(|c| c.is_ascii_digit()))
}

/// Every multi-line prose paragraph in `text`, as (first line number, lines).
fn wrapped_paragraphs(text: &str) -> Vec<(usize, Vec<String>)> {
    let mut out = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    let mut start = 0usize;
    let mut in_fence = false;

    for (i, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            in_fence = !in_fence;
            if cur.len() > 1 {
                out.push((start, std::mem::take(&mut cur)));
            } else {
                cur.clear();
            }
            continue;
        }
        if in_fence {
            continue;
        }
        // A markdown hard break -- two trailing spaces -- MEANS "break here",
        // so it legitimately ends a line without ending the paragraph. Treat it
        // as structural rather than reporting a deliberate break as a defect.
        if is_structural(line) || line.ends_with("  ") {
            if cur.len() > 1 {
                out.push((start, std::mem::take(&mut cur)));
            } else {
                cur.clear();
            }
            continue;
        }
        if cur.is_empty() {
            start = i + 1;
        }
        cur.push(line.to_string());
    }
    if cur.len() > 1 {
        out.push((start, cur));
    }
    out
}

#[test]
fn release_notes_are_not_hard_wrapped() {
    let dir = repo_root().join(".github/release-notes");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|x| x == "md")
                && p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with('v'))
        })
        .collect();
    files.sort();

    // Fail closed. Zero files examined means the directory moved, not that
    // every release renders correctly.
    assert!(
        files.len() > 20,
        "only {} release-notes files found in {}; the discovery is wrong",
        files.len(),
        dir.display()
    );

    let mut findings = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap_or_default();
        let name = f
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        for (line_no, para) in wrapped_paragraphs(&text) {
            findings.push(format!(
                "  {name}:{line_no}  a paragraph spans {} lines; the first is:\n    {}",
                para.len(),
                para[0].chars().take(96).collect::<String>().trim()
            ));
        }
    }

    println!("release-notes render: {} files checked", files.len());
    assert!(
        findings.is_empty(),
        "hard-wrapped paragraphs in release notes:\n{}\n\n\
         GitHub renders release bodies with GFM hard line breaks, so each of \
         these newlines becomes a `<br>` and the paragraph displays as a ragged \
         column broken mid-sentence rather than as flowing prose.\n\n\
         Fix with the tool this repository already has:\n  \
         python3 scripts/release-automation/reflow.py < FILE > FILE.new && mv FILE.new FILE",
        findings.join("\n")
    );
}

#[test]
fn the_paragraph_scanner_recognises_the_shapes_release_notes_use() {
    // One line per paragraph -- the required form.
    assert!(wrapped_paragraphs("A single long paragraph line.\n\nAnother one.\n").is_empty());
    // Two lines of one paragraph -- the defect.
    assert_eq!(wrapped_paragraphs("wrapped here\nand continued\n").len(), 1);
    // Structure that legitimately occupies several short lines.
    assert!(wrapped_paragraphs("| a | b |\n| - | - |\n").is_empty());
    assert!(wrapped_paragraphs("- one\n- two\n").is_empty());
    assert!(wrapped_paragraphs("> quoted\n> more\n").is_empty());
    assert!(wrapped_paragraphs("# head\n## head2\n").is_empty());
    // Fenced code keeps its own line structure.
    assert!(wrapped_paragraphs("```text\nline one\nline two\n```\n").is_empty());
    // A deliberate hard break (two trailing spaces) is not a wrap.
    assert!(wrapped_paragraphs("line one  \nline two\n").is_empty());
}
