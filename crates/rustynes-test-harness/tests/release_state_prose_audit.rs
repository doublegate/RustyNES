// SPDX-License-Identifier: GPL-3.0-or-later
//! Ordinary status prose must not describe a shipped release as unshipped.
//!
//! `release_anchor_audit` pins **15 fixed markers** and fails closed when one
//! goes missing. That is the right shape for *the* canonical current-release
//! statement in each document, and it is why a doubled bold marker on the
//! v2.4.2 cut failed loudly instead of silently checking 14 of 15.
//!
//! It cannot cover the failure this file exists for. When v2.4.2 moved every
//! anchor, review found three status claims that no anchor names:
//!
//! * `to-dos/ROADMAP.md` announced *"Next up -- v2.4.0 Concordance"* for a
//!   release that had shipped inside v2.4.1 and is deliberately never tagged --
//!   in the very change that documents why it has no tag,
//! * the same file still read *"In development -- the v2.0.0 tag itself"* for a
//!   tag pushed on 2026-07-03,
//! * and `to-dos/README.md` read *"In development -- v1.8.9"*, roughly fifteen
//!   releases stale, which nothing had flagged at all.
//!
//! The drift did not stop when the anchors were gated; it moved into the prose
//! beside them. So this is a **pattern** check rather than a marker list: a
//! status label naming a version at or below the workspace version is a
//! contradiction that needs no judgement to detect.
//!
//! # What this deliberately does NOT check
//!
//! A companion rule was drafted and **rejected on measurement**: "a `latest
//! release` or `the current line` claim must name the workspace version". It
//! fires on `docs/ios.md`, which correctly says *"The current line is v1.9.9
//! 'Workshop'"* -- scoped to the **iOS train**, not the project release. The
//! phrase legitimately scopes to a platform line, so the rule cannot separate a
//! stale claim from a correct one without judgement.
//!
//! A gate with false positives gets switched off, which is worse than no gate.
//! Recorded as a measured rejection rather than shipped and tuned.

use std::path::{Path, PathBuf};

/// Status labels that assert a version has NOT shipped yet.
///
/// Deliberately labels, not loose phrasing. An earlier draft included "next
/// release", which matched `to-dos/plans/v1.6.0-studio-plan.md` asking whether
/// to *defer* an item -- a question about a release, not a claim about its
/// state. Measured: with these four, the whole tree yields exactly one match.
const UNSHIPPED_LABELS: &[&str] = &["Next up", "In development", "Planned for", "Upcoming"];

/// Frozen trees are read from `.markdownlintignore` rather than duplicated here.
///
/// That file already encodes this repository's answer to "which prose is not
/// policed" -- immutable research, the two `archive/` trees, vendored upstream
/// content, and the release-plan copies kept exactly as authored. A second,
/// drifting copy of that list is the defect this whole file exists to catch, so
/// it is read at runtime instead.
const IGNORE_FILE: &str = ".markdownlintignore";

/// A line carrying this exact marker is exempt: it mentions a version beside a
/// status label without *asserting* that state.
///
/// Two legitimate cases, and the second was found the hard way. A deliberate
/// snapshot of what was true when written is one. The other is a QUOTATION --
/// documenting the defect requires reproducing it, and this gate duly failed on
/// its own CHANGELOG entry the moment that entry quoted `"Next up -- v2.4.0"`.
/// That is the same recursion that once made a commit body explaining the
/// `Closes #N` anti-pattern close an issue. Hence `not-a-claim` rather than
/// `historical`: the marker says the line is not making a claim, which is true
/// of both.
///
/// The marker is an HTML comment rather than a word, and that is not fussiness.
/// The first draft exempted any line containing "historical" -- and the very
/// first line written against it, `to-dos/README.md`, said "this paragraph is a
/// historical snapshot" for unrelated reasons. Mutation testing caught it: a
/// mutation reintroducing the exact defect this gate was built for came back
/// NOT CAUGHT, because the prose beside it had silently claimed the exemption.
///
/// An escape that can be claimed by accident is worse than no escape, because it
/// fails in the direction of silence. This one has to be typed on purpose.
/// Exemptions are printed and counted so growth stays visible.
const NOT_A_CLAIM: &str = "<!-- release-state: not-a-claim -->";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root is two levels above this crate")
        .to_path_buf()
}

/// The `version` under `[workspace.package]`, without any pre-release suffix.
fn workspace_version() -> String {
    let manifest = std::fs::read_to_string(repo_root().join("Cargo.toml"))
        .expect("read the workspace Cargo.toml");
    let mut in_table = false;
    for line in manifest.lines() {
        let t = line.split('#').next().unwrap_or("").trim();
        if t.starts_with('[') {
            in_table = t == "[workspace.package]";
            continue;
        }
        if in_table && let Some(rest) = t.strip_prefix("version") {
            let v = rest.trim_start().strip_prefix('=').unwrap_or("").trim();
            let v = v.trim_matches('"');
            return v.split(['-', '+']).next().unwrap_or(v).to_string();
        }
    }
    panic!("`[workspace.package]` has no `version` key");
}

/// `"2.4.3"` -> `(2, 4, 3)`. `None` for anything that is not three integers.
fn triple(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.split('.');
    let a = it.next()?.parse().ok()?;
    let b = it.next()?.parse().ok()?;
    let c = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some((a, b, c))
}

/// Every `vX.Y.Z` appearing within `window` characters after `at`.
///
/// Scans by CHARACTER, never by byte slice: these documents are full of
/// em-dashes and arrows, and `&s[a..b]` on a multi-byte boundary panics *while
/// formatting the diagnostic*, replacing the message that explains the failure
/// with a char-boundary error about the reporting code.
fn versions_near(line: &str, at: usize, window: usize) -> Vec<String> {
    let tail: String = line.chars().skip(at).take(window).collect();
    let mut out = Vec::new();
    let bytes: Vec<char> = tail.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 'v' && bytes.get(i + 1).is_some_and(char::is_ascii_digit) {
            let mut j = i + 1;
            let mut num = String::new();
            while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == '.') {
                num.push(bytes[j]);
                j += 1;
            }
            if triple(&num).is_some() {
                out.push(num);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

/// Directory prefixes and exact paths from `.markdownlintignore`.
fn frozen_prefixes() -> Vec<String> {
    let raw = std::fs::read_to_string(repo_root().join(IGNORE_FILE))
        .unwrap_or_else(|e| panic!("read {IGNORE_FILE}: {e}"));
    let v: Vec<String> = raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect();
    // Fail closed: an empty list would police the frozen trees and bury the
    // real findings under noise from content nobody maintains.
    assert!(
        !v.is_empty(),
        "{IGNORE_FILE} yielded no entries; the parse is wrong"
    );
    v
}

/// Markdown files this repository actually authors and maintains.
///
/// `git ls-files` rather than a directory walk, because the vendored clones
/// beside this checkout (`nesdev_wiki/` at 2,655 files, `RustyNES.wiki/`,
/// `salvaged/`) are untracked -- so "tracked" IS the definition of our content,
/// and needs no exclusion list that could drift.
fn tracked_markdown() -> Vec<String> {
    let out = std::process::Command::new("git")
        .args(["ls-files", "-z", "*.md"])
        .current_dir(repo_root())
        .output()
        .expect("run `git ls-files` (this audit needs the repository, not a vendored copy)");
    assert!(
        out.status.success(),
        "`git ls-files` failed: {:?}",
        out.status
    );

    let frozen = frozen_prefixes();
    String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|s| !s.is_empty())
        .filter(|s| !frozen.iter().any(|f| s.starts_with(f.as_str())))
        .map(str::to_string)
        .collect()
}

/// `**vX.Y.Z "Codename"** (current)` -- the label form, not any nearby word.
///
/// Deliberately narrow. A proximity match on `(current)` also catches prose
/// QUOTING the defect: `.github/release-notes/v2.3.9.md` and `CHANGELOG.md` both
/// say *"v2.3.5, still marked `(current)`"*, which is a correct historical
/// statement and must not be flagged. Requiring the bold version-and-codename
/// immediately before separates the two with no escape hatch needed.
fn current_labels(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let mut rest = line;
        while let Some(at) = rest.find("**v") {
            let tail = &rest[at..];
            // `**vX.Y.Z "Code"** (current)`
            let ok = tail
                .strip_prefix("**v")
                .and_then(|s| s.split_once(" \""))
                .filter(|(v, _)| {
                    let mut it = v.split('.');
                    (0..3).all(|_| it.next().is_some_and(|n| n.parse::<u64>().is_ok()))
                        && it.next().is_none()
                })
                .and_then(|(v, r)| r.split_once("\"** (current)").map(|_| v.to_string()));
            if let Some(v) = ok {
                out.push((i + 1, v));
            }
            rest = &rest[at + 3..];
        }
    }
    out
}

#[test]
fn exactly_one_release_is_labelled_current_and_it_is_this_one() {
    let root = repo_root();
    let current = workspace_version();
    let mut findings = Vec::new();
    let mut total = 0usize;

    for rel in &tracked_markdown() {
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        for (line, v) in current_labels(&text) {
            total += 1;
            if v != current {
                findings.push(format!(
                    "  {rel}:{line}  labels v{v} `(current)`, but the workspace is at {current}"
                ));
            }
        }
    }

    println!("release-state: {total} `(current)` label(s) found");
    assert!(
        findings.is_empty(),
        "stale `(current)` labels:\n{}\n\n\
         A release labelled current in prose is a claim about release state, and \
         `release_anchor_audit` does not see it -- it pins the anchors and the \
         VERSION-PLAN table, not the narrative beside them. This is where the \
         drift went after those were gated: v2.3.9 NOTICED `v2.3.5 ... still \
         marked (current)`, corrected the anchors, left the label, and added a \
         second one for itself.",
        findings.join("\n")
    );
    // Fail closed: the workspace release must be labelled somewhere.
    assert!(
        total > 0,
        "no `(current)` label found at all; the pattern is wrong"
    );
}

#[test]
fn the_current_label_scanner_ignores_prose_quoting_the_defect() {
    assert_eq!(
        current_labels(r#"| **v2.4.5 "Compass"** (current) | x |"#).len(),
        1
    );
    assert_eq!(
        current_labels(r#"**v2.3.5 "Manifest"** (current)."#)[0].1,
        "2.3.5"
    );
    // The historical quotation, which is correct prose and must not be flagged.
    assert!(current_labels("v2.3.5, still marked `(current)`").is_empty());
    assert!(current_labels("the table stopped at v2.3.5, still marked `(current)`").is_empty());
    // A version with no codename, or no label, is not a claim.
    assert!(current_labels("**v2.4.5** (current)").is_empty());
    assert!(current_labels(r#"**v2.4.5 "Compass"** shipped"#).is_empty());
}

/// Phrases that name a release as the one currently tagged.
///
/// Narrower than it could be, on purpose. `current release` appears 14 times and
/// `the latest release` 3, and most are generic prose with no version attached;
/// a rule over those would report findings nobody can act on, which is how a
/// check gets switched off. These two are the exact wording that drifted.
const TAG_PHRASES: &[&str] = &["current tag", "latest tag"];

// "current release" is DELIBERATELY not in that list, and extending it was tried
// and MEASURED on #448 before being rejected.
//
// CodeRabbit found a real instance the tag rule cannot see: `docs/STATUS.md`
// declared v2.5.2 current on line 3 and v2.2.0 "Capstone" current two thousand
// lines later. Both lines are fixed. But generalising the rule to the phrase
// produced **10 distinct findings across 7 files**, and inspection put nearly all
// of them in legitimate historical narrative -- a v2.3.9 release note correctly
// recording what was current in its own era, `AGENTS.md`'s "Built on ..." chain,
// `CHANGELOG-FULL.md`. Gating it would mean scattering not-a-claim markers
// through the corpus to silence correct prose, which is how a check earns being
// switched off.
//
// The narrow rule stays narrow. A stale "current release" in a LIVING status
// document is caught by review, as this one was; the phrase is too ambiguous to
// gate without more markers than it is worth. Recorded with the number so the
// next person does not re-derive it.

/// `... v2.3.9 "Crucible" (the current tag)` -- the version this claims is tagged.
///
/// The version precedes the phrase, so this scans BACKWARD and takes the
/// nearest one. A phrase with no version in reach is not a claim about any
/// particular release (`README.md` says a header "can lag ... the latest tag"
/// and names nothing), so it yields nothing rather than a finding.
fn tag_claims(text: &str) -> Vec<(usize, String)> {
    const LOOKBACK: usize = 220;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        // Prose QUOTING the defect is not the defect. The `(current)` rule solves
        // this structurally by matching only the bold label form; "the current tag"
        // has no such form, so this rule matches by proximity and therefore DOES
        // catch quotations -- reintroducing exactly the problem that rule's comment
        // warns about. It found this project's own release notes describing the
        // drift they fixed. So the established per-line marker applies here too.
        if line.contains(NOT_A_CLAIM) {
            continue;
        }
        for phrase in TAG_PHRASES {
            let mut from = 0usize;
            while let Some(off) = line[from..].find(phrase) {
                let at = from + off;
                // Character count, not byte offset: these lines carry em-dashes
                // and arrows, and a byte window would cut mid-character.
                // Byte index, but arrived at through `char_indices`, so it is
                // always a character boundary: these lines are full of em-dashes
                // and arrows, and `&s[a..b]` off a boundary panics WHILE
                // formatting the diagnostic -- replacing the message that
                // explains the failure with one about the reporting code.
                let start = line[..at]
                    .char_indices()
                    .rev()
                    .nth(LOOKBACK - 1)
                    .map_or(0, |(i, _)| i);
                let before = &line[start..at];
                if let Some(v) = versions_near(before, 0, before.chars().count()).pop() {
                    out.push((i + 1, v));
                }
                from = at + phrase.len();
            }
        }
    }
    out
}

#[test]
fn no_superseded_release_is_called_the_current_tag() {
    let root = repo_root();
    let current = workspace_version();
    let mut findings = Vec::new();
    let mut total = 0usize;

    for rel in &tracked_markdown() {
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        for (line, v) in tag_claims(&text) {
            total += 1;
            if v != current {
                findings.push(format!(
                    "  {rel}:{line}  calls v{v} the current tag, but the workspace is at {current}"
                ));
            }
        }
    }

    // Deliberately NOT fail-closed on `total == 0`, unlike the `(current)` rule
    // above. A `(current)` label MUST exist somewhere -- the workspace release has
    // to be marked -- but no document is obliged to phrase anything as "the
    // current tag", so requiring one would force the corpus to keep a wording it
    // may legitimately drop. The count is printed so a zero is VISIBLE rather than
    // silently reading as coverage: this rule guards phrasing that exists, it does
    // not assert that it exists.
    println!("release-state: {total} current-tag claim(s) found");
    assert!(
        findings.is_empty(),
        "stale current-tag claims:\n{}\n\n\
         This is the FOURTH place this project's release-state drift has moved to \
         after the previous one was gated: the anchors, then the `(current)` \
         labels, then the unshipped-status labels, and now the narrative sentence \
         that names a tag in different words. Extend the phrase list rather than \
         correcting the sentence alone. If a line QUOTES a past claim rather than \
         making one, append the not-a-claim marker to it.",
        findings.join("\n")
    );
}

#[test]
fn the_tag_claim_scanner_reads_backward_and_needs_a_version() {
    assert_eq!(
        tag_claims(r#"... -> **v2.4.6 "Abacus"**, the current tag)."#)[0].1,
        "2.4.6"
    );
    // The NEAREST version wins, not the first on the line.
    assert_eq!(
        tag_claims("v2.1.0 -> v2.2.0 -> v2.3.9 (the current tag).")[0].1,
        "2.3.9"
    );
    // No version in reach is not a claim about any release.
    assert!(tag_claims("its version can lag behind the latest tag").is_empty());
    // A version far outside the lookback is not the subject of the phrase.
    assert!(tag_claims(&format!("v2.3.9{} the current tag", " ".repeat(400))).is_empty());
    // Multibyte prose must not panic the backward scan.
    assert_eq!(
        tag_claims("— v2.4.6 → “Abacus” — the current tag")[0].1,
        "2.4.6"
    );
    // And the case that actually exercises the CUT: a line long enough that the
    // lookback truncates, with multi-byte characters straddling where it lands.
    //
    // A SINGLE long fixture is not enough, and assuming it was is how the first
    // TWO versions of this test passed a mutation replacing the boundary-safe
    // index with a raw `at - LOOKBACK`. Whether that byte offset falls inside a
    // character depends on the exact layout, and the first fixture landed on a
    // boundary by luck. The second swept the padding but placed it BEFORE the
    // repeats, which shifts the phrase and the repeats equally -- so the residue
    // never changed and the sweep was inert while looking thorough.
    //
    // `"— arrow "` is a 10-byte unit whose em-dash occupies bytes 0..2, and only
    // offsets 1 and 2 are inside it. Sweeping the padding AFTER the repeats
    // across 0..12 covers the whole period, so at least one cut must land there
    // whatever the surrounding lengths are -- which is the property that makes
    // this robust rather than a third lucky constant. (Today it is pad 6 and 7.)
    for pad in 0..12 {
        let long = format!(
            "v2.4.6 {}{} the current tag",
            "— arrow ".repeat(60),
            " ".repeat(pad)
        );
        assert!(
            long.chars().count() > 220,
            "the fixture must exceed LOOKBACK"
        );
        // A raw byte offset panics here rather than returning anything.
        assert!(tag_claims(&long).is_empty(), "pad={pad}");
    }
    let near = format!("v2.4.6 {} the current tag", "— ".repeat(20));
    assert_eq!(tag_claims(&near)[0].1, "2.4.6");
}

#[test]
fn no_shipped_release_is_described_as_unshipped() {
    let root = repo_root();
    let current = workspace_version();
    let cur = triple(&current).expect("workspace version is X.Y.Z");

    let mut files = tracked_markdown();
    files.sort();

    // Fail closed. A short list means the discovery is wrong, not that every
    // document is consistent -- the failure mode this project keeps finding.
    assert!(
        files.len() > 100,
        "only {} tracked markdown files examined; the discovery is wrong",
        files.len()
    );

    let mut findings = Vec::new();
    let mut exempt = Vec::new();

    for rel in &files {
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            for label in UNSHIPPED_LABELS {
                let Some(at) = line.find(label) else { continue };
                let at = line[..at].chars().count();
                for v in versions_near(line, at, 60) {
                    let Some(t) = triple(&v) else { continue };
                    if t > cur {
                        continue; // genuinely still ahead of us
                    }
                    if line.contains(NOT_A_CLAIM) {
                        exempt.push(format!("{rel}:{}  v{v}", n + 1));
                        continue;
                    }
                    findings.push(format!(
                        "  {rel}:{}  \"{label}\" names v{v}, but the workspace is at {current}\n    {}",
                        n + 1,
                        line.chars().take(120).collect::<String>().trim()
                    ));
                }
            }
        }
    }

    println!(
        "release-state prose: {} markdown files, {} exempt (labelled `{NOT_A_CLAIM}`)",
        files.len(),
        exempt.len()
    );
    for e in &exempt {
        println!("    exempt: {e}");
    }

    assert!(
        findings.is_empty(),
        "status prose describes shipped release(s) as unshipped:\n{}\n\n\
         A version at or below the workspace version has shipped, so a \
         \"Next up\"/\"In development\" label on it contradicts the release it \
         sits in. Either update the statement, or append `{NOT_A_CLAIM}` to the \
         line if it is deliberately a snapshot of what was true when written.",
        findings.join("\n")
    );
}

#[test]
fn the_version_parser_accepts_only_three_integers() {
    assert_eq!(triple("2.4.3"), Some((2, 4, 3)));
    assert_eq!(triple("10.0.1"), Some((10, 0, 1)));
    assert_eq!(triple("2.4"), None);
    assert_eq!(triple("2.4.3.1"), None);
    assert_eq!(triple("2.4.x"), None);
    assert_eq!(triple(""), None);
}

#[test]
fn versions_are_found_only_within_the_window() {
    let line = "In development — v1.8.9: the consolidation";
    assert_eq!(versions_near(line, 0, 60), vec!["1.8.9".to_string()]);
    // Outside the window, the same version is not attributed to the label.
    let far = format!("In development{} v1.8.9", " ".repeat(80));
    assert!(versions_near(&far, 0, 60).is_empty());
}

#[test]
fn a_multibyte_line_does_not_panic_the_scanner() {
    // Em-dashes and arrows are everywhere in these documents. A byte-slicing
    // implementation panics here while formatting its own diagnostic.
    let line = "Next up — v9.9.9 → the next thing — really";
    assert_eq!(versions_near(line, 0, 60), vec!["9.9.9".to_string()]);
}
