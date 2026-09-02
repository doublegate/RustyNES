//! Standing audit — every document that names the current release must name the
//! release this tree actually is.
//!
//! **Why this exists.** On 2026-08-20, cutting v2.3.9, the current-release
//! version was found written down in **eight** documents with nothing asserting
//! they agree. They had drifted to **six different values**:
//!
//! | document | said | actual |
//! |---|---|---|
//! | `README.md` badge + Current Release | v2.3.7 | v2.3.9 |
//! | `docs/STATUS.md` | v2.3.7 | |
//! | `AGENTS.md` (both anchors) | v2.3.7 | |
//! | `VERSION-PLAN.md` | v2.3.6 (table stopped at v2.3.5, still `(current)`) | |
//! | `to-dos/ROADMAP.md` | v2.3.3 **and** v2.2.5, in two places | |
//! | `SUPPORT.md` | v2.3.0 | |
//! | `ROADMAP.md` (root) | v2.0.4 | |
//!
//! Drift is the default outcome when one fact lives in eight places and a human
//! is the only thing keeping them in step. Two of those documents were
//! additionally wrong about more than the number — `SECURITY.md` was still
//! offering support for `1.0.x`, and the root `ARCHITECTURE.md` was presenting
//! the dot-lockstep scheduler retired in v2.0.0 as the current design — but only
//! the version half is mechanically checkable, and that is what this file checks.
//!
//! **The precedent is deliberate.** `libretro_info_audit.rs` exists because the
//! libretro `.info` `display_version` drifted from the workspace and shipped a
//! wrong licence to users for eleven days. This is the same audit applied to the
//! prose surfaces, for the same reason, in the same shape: the workspace manifest
//! is the single source of truth, and every other statement of the fact is
//! compared against it rather than maintained beside it.
//!
//! **It fails closed.** A marker that matches nothing is a FAILURE, never a pass.
//! That is the load-bearing property: an audit that quietly finds zero anchors
//! and reports success is indistinguishable from one that found them all correct,
//! which is precisely the class of defect the v2.3.9 release was about. If an
//! anchor is reworded or moved, this test fails and asks to be taught the new
//! wording — it does not shrug.
//!
//! **What it deliberately does not check.** Whether the prose around the version
//! is *true* — that the codename fits, that the summary describes the release, or
//! that a scheduler description matches the scheduler. No test can do that. This
//! narrows the human review surface to the claims that need judgement by taking
//! the mechanical ones away from it.
//!
//! The small manifest-parsing helpers are duplicated from
//! `libretro_info_audit.rs` rather than shared. Cargo compiles each
//! `tests/<name>.rs` as its own crate, and the alternative — `tests/common/mod.rs`
//! — pulls in the 46 KB framebuffer/ROM harness this audit has no use for. The
//! duplicated thing is a *parser*; the fact itself still has exactly one home, in
//! `[workspace.package]`.

use std::path::{Path, PathBuf};

/// Workspace root, derived from this crate's manifest dir rather than the CWD
/// (which differs between `cargo test` and a direct binary invocation).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/<crate>/ is two levels below the workspace root")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let p = workspace_root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// Pull a `key = "value"` out of the workspace `[workspace.package]` table.
///
/// Table-scoped rather than a first-match scan, for the reason recorded in
/// `libretro_info_audit.rs`: the manifest carries `version` keys in more than one
/// place, and the first one is not necessarily the one that governs the crates.
/// The CI toolchain resolver was bitten by exactly this shape when it matched the
/// first `channel` key anywhere in `rust-toolchain.toml`.
fn workspace_package_field(key: &str) -> String {
    let manifest = read("Cargo.toml");
    let mut in_table = false;
    for line in manifest.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            // Strip an inline comment, THEN compare exactly.
            //
            // An exact `==` against the RAW line silently skips a header carrying
            // a trailing comment, and does so confusingly: `in_table` stays false
            // and the panic at the bottom blames a missing key rather than the
            // header. That is the hole this closes. (Review on #427.)
            //
            // Review also raised a second concern against the `starts_with` form
            // that preceded this -- that it would match SUB-TABLES like
            // `[workspace.package.metadata]` and read `version` from the wrong
            // section. **That one does not hold, and it was checked rather than
            // assumed:** the literal ends with `]`, and the sub-table has `.`
            // at that position, so `"[workspace.package.metadata]"
            // .starts_with("[workspace.package]")` is `false`. Injecting such a
            // sub-table above the real one and running the audit confirms it --
            // both forms read `2.3.9`.
            //
            // The exact form is kept anyway, because it is correct without
            // requiring the reader to notice that the closing bracket is doing
            // the work. `sub_table_headers_do_not_match_the_workspace_package_table`
            // pins the property so the reasoning cannot be lost.
            in_table = t.split('#').next().unwrap_or(t).trim() == "[workspace.package]";
            continue;
        }
        if !in_table {
            continue;
        }
        if let Some((k, v)) = t.split_once('=')
            && k.trim() == key
        {
            // Strip a trailing inline comment before unquoting, so
            // `version = "2.3.9" # pinned` yields `2.3.9` rather than
            // `2.3.9" # pinned`. (Review on #427.)
            //
            // Naive for a general TOML value -- it would truncate a string that
            // legitimately contains `#`. Correct for every key this helper is
            // asked for (`version`, `license`), and the function is private to
            // this audit. A future caller wanting a `#`-bearing value needs a real
            // parser, not a patch here; the reason a `toml` dependency is not
            // pulled in for a documentation audit is that it would be the only
            // thing this test target needs it for.
            let v = v.split('#').next().unwrap_or(v);
            return v.trim().trim_matches('"').to_owned();
        }
    }
    panic!("`[workspace.package]` has no `{key}` key");
}

/// The `MAJOR.MINOR.PATCH` core of a version, dropping any `SemVer` pre-release or
/// build suffix.
///
/// The audit compares release *lines*, not exact `SemVer` strings, and that is a
/// decision rather than a shortcut. Review on #427 found the reason: with a
/// pre-release workspace version such as `2.4.0-rc.1`, comparing the raw strings
/// fails every anchor even when all of them are correct, because `v2.4.0-rc.1` in
/// prose parses back as `2.4.0`. Worse, the README badge is
/// `badge/version-v2.3.9-blue.svg`, where the hyphen is a URL delimiter and not a
/// pre-release marker at all — so "just parse the suffix too" turns the badge into
/// version `2.3.9-blue.svg`.
///
/// Comparing the numeric core sidesteps both. The cost is that an anchor reading
/// `v2.4.0` while the tree is at `2.4.0-rc.1` passes, which is the right call:
/// whether prose names a pre-release suffix is a maintainer style choice, while
/// naming the wrong release line is the error this audit exists to catch.
fn version_core(v: &str) -> &str {
    v.split(['-', '+']).next().unwrap_or(v)
}

/// `version_core` is tested directly, because the situation it exists for cannot
/// be reached through `Cargo.toml`.
///
/// Setting `[workspace.package] version = "2.3.9-rc.1"` to reproduce the reported
/// failure does not reach this audit at all — **cargo** rejects it first:
///
/// ```text
/// error: failed to select a version for the requirement `rustynes-apu = "^2.0.0"`
/// candidate versions found which didn't match: 2.3.9-rc.1
/// ```
///
/// A caret requirement does not match a pre-release, so every intra-workspace
/// dependency would have to be rewritten before the workspace could carry a
/// pre-release version. That makes the scenario unreachable *today* rather than
/// impossible — the guard stays, because the day someone does that work this
/// audit should not be the thing that then blocks the release for a reason
/// unrelated to the anchors.
/// A TOML sub-table header must not be mistaken for the table itself.
///
/// Review on #427 raised this as a blocking defect against the `starts_with`
/// form. It is not one — the literal ends with `]` and a sub-table has `.` there
/// — but the property is worth pinning rather than left to a reader spotting a
/// closing bracket, and the parser now states it structurally.
/// Markdown emphasis between the version and its codename must not skip the
/// codename check, and a version-only anchor must still be skipped.
///
/// Latent rather than live: no anchor is written `**v2.3.9** "Crucible"` today.
/// Pinned so a reformat cannot quietly reopen the hole. (Review on #427.)
#[test]
fn emphasis_between_version_and_codename_does_not_skip_the_check() {
    // Would have been skipped before: emphasis, then the codename.
    assert!(skip_to_codename("** \"Crucible\"").starts_with('"'));
    assert!(skip_to_codename("  \"Crucible\"").starts_with('"'));
    // The shapes that legitimately have no codename must STILL be skipped.
    assert!(!skip_to_codename("** (2026-08-20)").starts_with('"'));
    assert!(!skip_to_codename(" (the scheduling model is v2.0.0)").starts_with('"'));
    assert!(!skip_to_codename("-blue.svg").starts_with('"'));
}

/// A version at the end of a sentence must parse, not panic.
///
/// `parse_version_prefix` consumed contiguous dots, so `v2.3.9.` yielded four
/// parts and returned `None` — which the caller turns into a panic. Fail-closed
/// is right for a missing marker; a full stop is not a missing marker.
#[test]
fn trailing_prose_punctuation_does_not_break_version_parsing() {
    assert_eq!(parse_version_prefix("2.3.9.").as_deref(), Some("2.3.9"));
    assert_eq!(parse_version_prefix("2.3.9...").as_deref(), Some("2.3.9"));
    assert_eq!(parse_version_prefix("2.3.9").as_deref(), Some("2.3.9"));
    // Still rejects things that are genuinely not a version.
    assert_eq!(parse_version_prefix("2.3").as_deref(), None);
    assert_eq!(parse_version_prefix("2.3.9.4").as_deref(), None);
}

#[test]
fn sub_table_headers_do_not_match_the_workspace_package_table() {
    // The claim that was checked rather than accepted.
    assert!(!"[workspace.package.metadata]".starts_with("[workspace.package]"));
    // What the parser actually does, in both shapes it must handle.
    let exact = |t: &str| t.split('#').next().unwrap_or(t).trim() == "[workspace.package]";
    assert!(exact("[workspace.package]"));
    assert!(exact("[workspace.package] # pinned"));
    assert!(!exact("[workspace.package.metadata]"));
    assert!(!exact("[workspace.dependencies]"));
}

#[test]
fn version_core_drops_a_prerelease_or_build_suffix() {
    assert_eq!(version_core("2.3.9"), "2.3.9");
    assert_eq!(version_core("2.4.0-rc.1"), "2.4.0");
    assert_eq!(version_core("2.4.0-beta.5"), "2.4.0");
    assert_eq!(version_core("2.4.0+build.7"), "2.4.0");
    // The README badge is `badge/version-v2.3.9-blue.svg`, where the hyphen is a
    // URL delimiter. `parse_version_prefix` already stops at it, so the badge
    // never reaches here carrying a suffix -- pinned so a future "parse the
    // suffix too" change cannot silently turn the badge into `2.3.9-blue.svg`.
    assert_eq!(
        parse_version_prefix("2.3.9-blue.svg").as_deref(),
        Some("2.3.9")
    );
}

/// One place a document states the current release version.
///
/// `marker` is the literal text immediately preceding the version, so the audit
/// pins the *claim* rather than merely the presence of a version string
/// somewhere in the file. Every one of these documents also carries a historical
/// trail naming older releases; a looser match would either fire on those or,
/// worse, be satisfied by them.
struct Anchor {
    path: &'static str,
    /// What the anchor is, phrased for the failure message.
    what: &'static str,
    marker: &'static str,
}

/// Every document that states which release this tree is.
///
/// Adding a new such statement without adding it here is the failure mode this
/// audit exists to prevent, so the list is the deliverable, not the plumbing.
const ANCHORS: &[Anchor] = &[
    Anchor {
        path: "README.md",
        what: "the version badge",
        marker: "badge/version-v",
    },
    Anchor {
        path: "README.md",
        what: "the Current Release section",
        marker: "RustyNES's current release is **v",
    },
    Anchor {
        path: "docs/STATUS.md",
        what: "the status-matrix header (the single source of truth for current state)",
        marker: "> **Current release: v",
    },
    Anchor {
        path: "AGENTS.md",
        what: "the \"What this is\" current-release block",
        marker: "**Current release: v",
    },
    Anchor {
        path: "AGENTS.md",
        what: "the operating-notes current-release bullet",
        marker: "The current release is **v",
    },
    Anchor {
        path: "AGENTS.md",
        what: "the \"never claim a later version\" guard",
        marker: "**Never claim any version *later* than v",
    },
    Anchor {
        path: "VERSION-PLAN.md",
        what: "the plan header",
        marker: "**Current release: v",
    },
    Anchor {
        path: "to-dos/ROADMAP.md",
        what: "the Status section",
        marker: "- **Current release:** **RustyNES v",
    },
    Anchor {
        path: "SUPPORT.md",
        what: "the \"can I use RustyNES now\" answer",
        marker: "the current release is **v",
    },
    Anchor {
        path: "SECURITY.md",
        what: "the supported-versions preamble",
        marker: "The current release is **v",
    },
    Anchor {
        path: "ROADMAP.md",
        what: "the project-status line",
        marker: "**Project Status:** v",
    },
    Anchor {
        path: "ROADMAP.md",
        what: "the current-release paragraph",
        marker: "The current release is **v",
    },
    Anchor {
        path: "OVERVIEW.md",
        what: "the document's Applies-to header",
        marker: "**Applies to:** RustyNES v",
    },
    Anchor {
        path: "OVERVIEW.md",
        what: "the current-release statements",
        marker: "The current release is **v",
    },
    Anchor {
        path: "ARCHITECTURE.md",
        what: "the document's Applies-to header",
        marker: "**Applies to:** RustyNES v",
    },
];

/// Skip whatever sits between a version and a codename that follows it.
///
/// A named function rather than an inline `trim_start_matches`, so the test can
/// exercise THIS code instead of a copy of it. The first attempt asserted the
/// property against a local closure duplicating the same call — and a mutation
/// removing the production stripping went uncaught, because the test was never
/// looking at it. A test that reimplements what it checks is testing itself.
///
/// Strips spaces and markdown emphasis. Both were fail-opens found by review on
/// #427: a second space, or `**v2.3.9** "Crucible"`, sent the codename check down
/// its legitimate "no codename here" path and silently skipped that anchor.
fn skip_to_codename(after_version: &str) -> &str {
    after_version.trim_start_matches([' ', '*'])
}

/// Read a `MAJOR.MINOR.PATCH` starting at `s[0]`, stopping at the first
/// character that cannot be part of one.
///
/// Returns `None` rather than a partial parse, so a marker followed by something
/// that is not a version fails the audit instead of silently comparing garbage.
fn parse_version_prefix(s: &str) -> Option<String> {
    let end = s
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(s.len());
    // Trailing dots are prose punctuation, not version components. A sentence
    // ending "...is at v2.3.9." would otherwise yield four parts, return `None`,
    // and PANIC -- a false failure caused by a full stop. Fail-closed is right for
    // a missing marker; it is not right for a period. (Review on #427.)
    let v = s[..end].trim_end_matches('.');
    let parts: Vec<&str> = v.split('.').collect();
    (parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())))
    .then(|| v.to_owned())
}

/// Every version stated at `marker` in `text`, with the byte offset of each.
///
/// Panics when the marker is absent. That is the fail-closed contract: a
/// reworded anchor must break this test loudly, because the alternative is an
/// audit that reports success while checking nothing.
///
/// The excerpts in the panic messages are built with `chars().take(n)`, never a
/// byte slice. These documents are full of em-dashes and arrows, so
/// `&text[at..at + 24]` can land inside a multi-byte character and panic **while
/// formatting the diagnostic** -- destroying the message that explains the real
/// failure and replacing it with a byte-index error about the reporting code.
/// A diagnostic that can crash the diagnosis is worse than no diagnostic.
/// (Review on #427.)
fn versions_at(text: &str, anchor: &Anchor) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(i) = text[from..].find(anchor.marker) {
        let at = from + i + anchor.marker.len();
        let v = parse_version_prefix(&text[at..]).unwrap_or_else(|| {
            panic!(
                "{}: found `{}` ({}) but what follows is not a MAJOR.MINOR.PATCH version: {:?}",
                anchor.path,
                anchor.marker,
                anchor.what,
                text[at..].chars().take(24).collect::<String>()
            )
        });
        out.push((at, v));
        // Resume past the parsed version rather than at its start. `at` alone is
        // safe today because every marker is longer than zero, but resuming after
        // what was just consumed is what the loop means. (Review on #427.)
        from = at + out.last().expect("just pushed").1.len();
    }
    assert!(
        !out.is_empty(),
        "{}: no occurrence of `{}` ({}).\n\
         \n\
         The anchor was reworded, moved, or deleted. This audit fails closed on \
         purpose -- an anchor it cannot find is one it is not checking, and a \
         silent zero-anchor pass is the exact defect this file was written \
         after. Update the marker in ANCHORS to the new wording (or drop the \
         entry if the statement is genuinely gone).",
        anchor.path,
        anchor.marker,
        anchor.what
    );
    out
}

/// The `## [X.Y.Z] - <date> - "<Codename>" (<theme>)` header for a version.
///
/// `release-auto.yml` parses this same line twice — once for the release-body
/// fallback when no `.github/release-notes/vX.Y.Z.md` override exists, and once
/// to derive the release title's codename. Pinning it here means a malformed
/// header fails in CI rather than at publish time, which is when it failed for
/// v2.1.7.
fn changelog_header(version: &str) -> String {
    let changelog = read("CHANGELOG.md");
    let needle = format!("## [{version}]");
    changelog
        .lines()
        .find(|l| l.starts_with(&needle))
        .unwrap_or_else(|| {
            panic!(
                "CHANGELOG.md has no `{needle}` section.\n\
                 \n\
                 The workspace is at {version}, so either the release entry was \
                 not written or the version was bumped early. `release-auto.yml` \
                 treats \"version has no matching tag\" as ready-to-release and \
                 will fail closed for want of notes -- see the comment above \
                 `[workspace.package] version`."
            )
        })
        .to_owned()
}

/// The quoted codename out of a CHANGELOG header line.
fn codename_of(header: &str) -> String {
    let open = header
        .find('"')
        .unwrap_or_else(|| panic!("CHANGELOG header has no quoted codename: {header:?}"));
    let rest = &header[open + 1..];
    let close = rest
        .find('"')
        .unwrap_or_else(|| panic!("CHANGELOG header has an unterminated codename: {header:?}"));
    rest[..close].to_owned()
}

/// Every anchor must state the version this tree actually is.
///
/// This is the assertion whose absence let eight documents reach six different
/// answers.
#[test]
fn every_release_anchor_names_the_workspace_version() {
    let version = workspace_package_field("version");
    let expected = version_core(&version);
    let mut wrong: Vec<String> = Vec::new();

    for anchor in ANCHORS {
        let text = read(anchor.path);
        for (_, found) in versions_at(&text, anchor) {
            if found != expected {
                wrong.push(format!(
                    "  {} -- {} says v{found}, workspace is {expected}",
                    anchor.path, anchor.what
                ));
            }
        }
    }

    assert!(
        wrong.is_empty(),
        "{} release anchor(s) disagree with `[workspace.package] version` = {version}:\n{}\n\n\
         Every one of these is a statement to a reader about which release they \
         are looking at. Update them in the same change as the version bump; the \
         release cut is the moment they are all correct at once.",
        wrong.len(),
        wrong.join("\n")
    );
}

/// The CHANGELOG must retain an `[Unreleased]` section after a release cut.
///
/// **Why this exists.** Cutting v2.6.3 renamed `## [Unreleased]` into
/// `## [2.6.3] - ...` instead of inserting the new section *below* a retained
/// `[Unreleased]`, so the file shipped with no `[Unreleased]` heading at all.
/// Every prior tag has one — `v2.6.0`, `v2.6.1` and `v2.6.2` each carry an empty
/// `## [Unreleased]` immediately above the newest release — and the Keep a
/// Changelog convention the file declares requires it.
///
/// It was caught, but three crates away and by accident: `rustynes-frontend`'s
/// in-app documentation panel parses this file, and three of its tests assert an
/// `[Unreleased]` section exists and sorts last. So a CHANGELOG defect surfaced
/// as a frontend unit-test failure on the full-workspace matrix leg, which is
/// both a long way from the edit and a gate that does not run on every PR.
///
/// The release audit already parses this file for the header shape, so the
/// assertion belongs here, next to the other claims the cut has to satisfy —
/// where it names the CHANGELOG by name at the moment the version is bumped.
///
/// Deliberately checks only that the heading EXISTS, not that it is empty:
/// carrying an entry destined for the next release is legitimate, and asserting
/// emptiness would fail a tree that is merely ahead.
#[test]
fn the_changelog_keeps_an_unreleased_section() {
    let changelog = read("CHANGELOG.md");
    let count = changelog
        .lines()
        .filter(|l| l.trim_end() == "## [Unreleased]")
        .count();

    assert!(
        count > 0,
        "CHANGELOG.md has no `## [Unreleased]` heading.\n\n\
         A release cut inserts the new version section BELOW a retained\n\
         `## [Unreleased]`; it does not rename that heading into the new\n\
         version. Every tag from v2.6.0 onward carries an empty one.\n\n\
         Without it, `rustynes-frontend`'s documentation panel fails three\n\
         tests -- but only on the full-workspace matrix leg, so this is the\n\
         cheaper place to find out."
    );

    assert_eq!(
        count, 1,
        "CHANGELOG.md has {count} `## [Unreleased]` headings; exactly one is \
         expected. More than one means a previous cut left its heading behind."
    );

    // `[Unreleased]` must come FIRST. A heading that has drifted below a
    // released section still satisfies the existence check above while telling
    // a reader the opposite of the truth about where new entries go.
    let first_section = changelog
        .lines()
        .find(|l| l.starts_with("## ["))
        .expect("CHANGELOG.md has no `## [` section heading at all");
    assert_eq!(
        first_section.trim_end(),
        "## [Unreleased]",
        "the first `## [` section in CHANGELOG.md is {first_section:?}, but \
         `## [Unreleased]` must lead the file so new entries have an unambiguous \
         home."
    );
}

/// The CHANGELOG must carry a section for the version the workspace claims.
#[test]
fn the_changelog_has_a_section_for_the_workspace_version() {
    let version = workspace_package_field("version");
    let header = changelog_header(&version);

    // The header must match what `release-auto.yml` actually parses, component by
    // component:
    //
    //     ## [X.Y.Z] - YYYY-MM-DD - "Codename" (theme)
    //
    // The first version asserted only that the tail contained " - " and a
    // non-empty quoted string. Review on #427 caught that as too loose, and it
    // was: `## [2.3.9] - "Crucible"` satisfies both and would still have degraded
    // the published title, because the workflow's sed strips `- <date> -` and
    // finds nothing to strip. An audit that passes the malformed input it exists
    // to reject is worse than no audit, so each component is checked separately.
    let after_version = header
        .split_once(']')
        .expect("CHANGELOG header has no closing `]`")
        .1;

    // ` - YYYY-MM-DD - ` -- the shape the workflow's
    // `s/^## \[[^]]*\][[:space:]]*-[[:space:]]*[0-9-]+[[:space:]]*-[[:space:]]*//`
    // removes. A date it cannot match leaves the whole prefix in the title.
    let tail = after_version.strip_prefix(" - ").unwrap_or_else(|| {
        panic!(
            "CHANGELOG header for {version} does not continue ` - <date> - ...`, \
             which `release-auto.yml` strips to build the release title:\n  {header}"
        )
    });
    let (date, rest) = tail.split_once(" - ").unwrap_or_else(|| {
        panic!(
            "CHANGELOG header for {version} has no ` - ` after the date, so the \
             title would keep the date prefix:\n  {header}"
        )
    });
    let date_parts: Vec<&str> = date.split('-').collect();
    assert!(
        date_parts.len() == 3
            && date_parts[0].len() == 4
            && date_parts[1].len() == 2
            && date_parts[2].len() == 2
            && date_parts
                .iter()
                .all(|p| p.chars().all(|c| c.is_ascii_digit())),
        "CHANGELOG header for {version} has `{date}` where an ISO YYYY-MM-DD date \
         belongs. `release-auto.yml` matches `[0-9-]+`, so a malformed date is \
         either left in the title or swallows part of the codename:\n  {header}"
    );

    // `"Codename" (theme)` -- what becomes the title's suffix.
    let codename = codename_of(&header);
    assert!(
        !codename.is_empty(),
        "CHANGELOG header for {version} has an empty codename:\n  {header}"
    );
    assert!(
        rest.starts_with('"'),
        "CHANGELOG header for {version} does not begin its theme with a quoted \
         codename:\n  {header}"
    );
    let after_codename = rest[1..]
        .split_once('"')
        .expect("codename_of already proved the closing quote exists")
        .1;
    let theme = after_codename.trim();
    assert!(
        theme.starts_with('(') && theme.ends_with(')') && theme.len() > 2,
        "CHANGELOG header for {version} has no parenthesised theme after the \
         codename. Every release in this file carries one, and it is half of what \
         the published title says:\n  {header}"
    );

    // The theme becomes the published release TITLE, and a GitHub release title
    // is PLAIN TEXT -- it is not markdown-rendered. So emphasis markers that
    // read correctly everywhere else in this file appear literally there.
    //
    // v2.6.0 shipped with `**in the MiSTer co-simulation DUT**` in its title,
    // asterisks and all. The markers were added for a good reason -- a review
    // asked for the claim to be scoped to the sibling DUT -- and they were
    // correct in the eight anchor documents that DO render markdown. Only this
    // one string crosses into a plain-text surface, which is exactly why
    // nothing caught it: every other consumer of the same words was fine.
    for marker in ["**", "__", "`"] {
        assert!(
            !theme.contains(marker),
            "CHANGELOG header for {version} carries the markdown marker {marker:?} \
             in its theme. That theme is parsed by release-auto.yml into the \
             GitHub release title, which is NOT markdown-rendered, so the marker \
             appears literally to every reader:\n  {header}"
        );
    }
}

/// Anchors that quote a codename must quote the CHANGELOG's codename.
///
/// A correct version beside the previous release's codename is still a wrong
/// claim, and it is the more confusing kind: the number looks right, so a reader
/// trusts the sentence around it.
#[test]
fn every_anchor_that_quotes_a_codename_quotes_the_changelog_codename() {
    let version = workspace_package_field("version");
    let expected = codename_of(&changelog_header(&version));
    let mut wrong: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for anchor in ANCHORS {
        let text = read(anchor.path);
        for (at, found) in versions_at(&text, anchor) {
            if found != version_core(&version) {
                continue; // reported by the version test; do not double-report
            }
            // A codename, when present, follows as ` "Name"` -- but the exact
            // punctuation between the version and the quote is a prose detail,
            // not a contract. Two separate fail-opens were found here by review
            // on #427, both the same shape: something the check did not expect
            // sent it down `continue`, silently skipping that anchor's codename
            // while another anchor kept `checked > 0`.
            //
            //   * two spaces instead of one;
            //   * markdown emphasis, e.g. `**v2.3.9** "Crucible"`.
            //
            // NEITHER is live today -- every anchor currently reads
            // `v2.3.9 "Crucible"` with a single space, and the emphasis case was
            // checked against all ten documents rather than assumed. Both are
            // latent, and a fail-open that waits for a reformat is exactly the
            // kind this audit exists to remove.
            //
            // Stripping `*` as well as spaces keeps the version-only anchors
            // working: `**v2.3.9** (2026-08-20)` strips to `(2026-08-20`, which
            // is not a quote, so it still takes the legitimate `continue`.
            let tail = skip_to_codename(&text[at + found.len()..]);
            let Some(rest) = tail.strip_prefix('"') else {
                continue; // this anchor states a version only -- legitimate
            };
            // An OPENING quote with no closing one is malformed, not
            // "version-only". The first version `continue`d here, so a broken
            // release claim slipped through as long as some OTHER anchor kept
            // `checked > 0` — a fail-OPEN inside the test whose stated property is
            // failing closed. Review on #427 caught it. Reaching the `continue`
            // above is legitimate; reaching this point is not.
            let Some(close) = rest.find('"') else {
                panic!(
                    "{}: {} opens a codename after v{found} and never closes it: {:?}",
                    anchor.path,
                    anchor.what,
                    rest.chars().take(40).collect::<String>()
                )
            };
            checked += 1;
            let name = &rest[..close];
            if name != expected {
                wrong.push(format!(
                    "  {} -- {} says \"{name}\", CHANGELOG says \"{expected}\"",
                    anchor.path, anchor.what
                ));
            }
        }
    }

    assert!(
        checked > 0,
        "no anchor was found quoting a codename. Either every anchor was \
         reworded to drop the codename, or the ` \"Name\"` shape this test looks \
         for changed. Fail-closed: teach the test the new shape rather than \
         leaving it asserting nothing."
    );
    assert!(
        wrong.is_empty(),
        "{} anchor(s) name the right version with the wrong codename:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

/// `VERSION-PLAN.md`'s release table must mark the current release `(current)`,
/// and mark only it.
///
/// The table is the one anchor that carries a per-release row, so the drift it
/// suffers is different in kind: at v2.3.9 the header said v2.3.6 while the table
/// stopped at v2.3.5 and still marked that row `(current)` — four releases
/// unlisted, and the marker three releases stale.
#[test]
fn the_version_plan_table_marks_exactly_the_current_release() {
    let version = workspace_package_field("version");
    let plan = read("VERSION-PLAN.md");

    let marked: Vec<&str> = plan
        .lines()
        .filter(|l| l.contains("(current)") && l.trim_start().starts_with("| **v"))
        .collect();

    assert_eq!(
        marked.len(),
        1,
        "VERSION-PLAN.md's release table has {} rows marked `(current)`, expected \
         exactly 1:\n{}",
        marked.len(),
        marked
            .iter()
            .map(|l| format!("  {}", l.chars().take(90).collect::<String>()))
            .collect::<Vec<_>>()
            .join("\n")
    );

    let row = marked[0];
    let at = row.find("| **v").expect("checked by the filter above") + "| **v".len();
    let found = parse_version_prefix(&row[at..]).unwrap_or_else(|| {
        panic!("VERSION-PLAN.md `(current)` row does not start with a version: {row:?}")
    });

    assert_eq!(
        found, version,
        "VERSION-PLAN.md marks v{found} as `(current)` but the workspace is at \
         {version}. Add the missing row(s) and move the marker; a release table \
         that stops short is how this one ended up three releases behind its own \
         header."
    );
}

/// The version token immediately preceding `before`'s end, if there is one.
///
/// Extracted rather than inlined into the audit so a test can call the REAL
/// decision. A first version of the test restated the scan as a local closure
/// and would have agreed with itself forever -- the exact shape this project
/// recorded in v2.4.0, three times in one release.
///
/// The `v` must START a token: without that, the scan accepts the `v` inside an
/// identifier, and `rev2.5.9, the current release` parses as version 2.5.9
/// while containing no version token at all.
fn version_before(before: &str) -> Option<String> {
    before
        .char_indices()
        .rev()
        .filter(|&(_, c)| c == 'v')
        .filter(|&(at, _)| {
            before[..at]
                .chars()
                .next_back()
                .is_none_or(|p| !p.is_alphanumeric() && p != '_')
        })
        .find_map(|(at, _)| parse_version_prefix(&before[at + 1..]))
}

/// The reverse `v` scan must not accept a `v` inside a word.
///
/// Two live inputs make this concrete rather than theoretical, and BOTH were
/// found after the check shipped: a codename containing the letter (`"Overture"`
/// broke the first version outright), and an identifier ending in a version
/// (`rev2.5.9` parses as `2.5.9` and would satisfy the audit while containing no
/// version token at all).
#[test]
fn the_version_scan_requires_a_token_boundary() {
    let scan = version_before;

    assert_eq!(scan("**v2.5.9 \"Overture\"**").as_deref(), Some("2.5.9"));
    assert_eq!(scan("v2.5.9").as_deref(), Some("2.5.9"));
    // A `v` that begins no token is not a version marker.
    assert_eq!(scan("rev2.5.9"), None);
    assert_eq!(scan("_v2.5.9"), None);
    // ...and the surrounding prose must not defeat a real one.
    assert_eq!(
        scan("shipped v2.5.9 (rev2 of the plan)").as_deref(),
        Some("2.5.9")
    );
}

/// A release-line CHAIN that ends by naming its last entry "the current
/// release" must name the CURRENT one.
///
/// This is a different shape from [`ANCHORS`] and needs its own check because
/// of it: an `Anchor` marker is text immediately FOLLOWED by the version, and
/// here the version comes first — `**v2.5.8 "Blanking"**, the current release`
/// — so no marker in that table can reach it.
///
/// It earns a test by having drifted three releases running: the phrase named
/// v2.5.6 at the v2.5.7 cut, was corrected to v2.5.7 by review, and named
/// v2.5.7 again at the v2.5.8 cut. Three consecutive releases is not a lapse,
/// it is an unguarded anchor — and the bump script cannot help, because it
/// rewrites `marker`-shaped sites only.
#[test]
fn a_chain_that_names_its_tail_the_current_release_names_the_current_one() {
    const PHRASE: &str = ", the current release";
    // Every document that carries a release-line chain. Listed rather than
    // globbed: a glob would silently start covering new files, and a check
    // whose scope moves on its own cannot be reasoned about at a release cut.
    const DOCS: &[&str] = &[
        "to-dos/ROADMAP.md",
        "VERSION-PLAN.md",
        "README.md",
        "AGENTS.md",
        "docs/STATUS.md",
        "ROADMAP.md",
    ];

    let version = workspace_package_field("version");
    let expected = version_core(&version);
    let mut wrong: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for doc in DOCS {
        let text = read(doc);
        for (idx, _) in text.match_indices(PHRASE) {
            checked += 1;
            // Walk back to the nearest `v` that actually BEGINS A VERSION, not
            // merely the nearest `v`. The tail looks like
            // `**v2.5.9 "Overture"**, the current release` -- and "Overture"
            // contains a `v`, which is what the first version of this check
            // found. It reported the document as unparseable when the document
            // was fine and the parser was not, on the very next release after
            // the check was written. A codename is free text; only a `v`
            // followed by a version is a version.
            let before = &text[..idx];
            let found = version_before(before);
            match found {
                Some(f) if f == expected => {}
                Some(f) => wrong.push(format!(
                    "  {doc} -- a chain ends \"v{f}{PHRASE}\", workspace is {expected}"
                )),
                None => wrong.push(format!(
                    "  {doc} -- \"{PHRASE}\" is not preceded by any parseable version"
                )),
            }
        }
    }

    // Fail closed. A corpus where the phrase appears nowhere would pass this
    // test while checking nothing, which is the failure this project has
    // recorded more than once.
    assert!(
        checked > 0,
        "no document contains \"{PHRASE}\" -- either the release-line chains were \
         reworded (update DOCS and this test's premise) or the check is now \
         inert, and an inert check reports a pass it has not earned"
    );

    assert!(
        wrong.is_empty(),
        "{} release-line chain(s) end by calling an OLD release current \
         (workspace = {version}):\n{}\n\n\
         The chain's last entry and the \"current release\" label are one claim, \
         so extending the chain and moving the label are one edit.",
        wrong.len(),
        wrong.join("\n")
    );
}

/// Every released version, newest first, as the CHANGELOG orders them.
///
/// `[Unreleased]` is deliberately excluded: it is not a release, and including
/// it would make the chain checks below expect a version no document can name.
fn changelog_released_versions() -> Vec<String> {
    let changelog = read("CHANGELOG.md");
    let mut out = Vec::new();
    for line in changelog.lines() {
        let Some(rest) = line.strip_prefix("## [") else {
            continue;
        };
        let Some(close) = rest.find(']') else {
            continue;
        };
        let v = &rest[..close];
        if parse_version_prefix(v).is_some_and(|p| p == v) {
            out.push(v.to_owned());
        }
    }
    assert!(
        out.len() >= 2,
        "CHANGELOG.md yielded {} released version header(s); the chain checks \
         need at least two. Either the `## [X.Y.Z]` shape changed or this parse \
         is now inert.",
        out.len()
    );
    out
}

/// Every document that carries a release claim, DERIVED from [`ANCHORS`].
///
/// Not a second hand-written list. The first version of this check listed six
/// documents by hand and was wrong on its first use: the v2.6.10 drift reached
/// **eight**, and `SUPPORT.md` and `SECURITY.md` were not on it. A hand list
/// silently OMITS exactly as a glob silently widens, and `ANCHORS` is already
/// this file's single answer to "where does a release claim live" -- the bump
/// script reads the same table for the same reason.
fn chain_docs() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for a in ANCHORS {
        if !out.contains(&a.path) {
            out.push(a.path);
        }
    }
    out
}

/// A release-line chain must not SKIP a release.
///
/// This is the drift the rest of this file cannot see, and it went out in
/// **six documents at once** at the v2.6.10 cut. The bump rewrote the version
/// token and left the summary prose that followed it, so every lead read
/// `v2.6.10 "Inference" -- <v2.6.9's summary> ... Built on **v2.6.8`. Two wrong
/// claims from one edit: v2.6.10 described as the previous release's work, and
/// v2.6.9 gone from the lineage entirely.
///
/// Every anchor check above passed, correctly -- each one pins the version
/// TOKEN, and the token was right. What is mechanically checkable is the
/// SEQUENCE: the first release a chain names after the current one must be the
/// release immediately before it in the CHANGELOG. Prose cannot be audited;
/// an ordering can.
#[test]
fn a_release_line_chain_does_not_skip_a_release() {
    // A chain link is written TWO ways in this corpus, and a marker that knows
    // only one reports a false failure on the other. `bump_release.py` demotes
    // a "bare"/"dash" anchor as `Built on **vX.Y.Z` and a "paren" one as
    // `..., on **vX.Y.Z` -- 22 occurrences of the second form in `AGENTS.md`
    // alone. Both are accepted; anything else spelling `on **v` is not.
    const MARKER: &str = "on **v";
    const LEAD_INS: [&str; 2] = ["Built ", ", "];

    let released = changelog_released_versions();
    let version = workspace_package_field("version");
    let current = version_core(&version).to_owned();

    // The chain's first `Built on` must name whichever release precedes the
    // workspace version. Derived from the CHANGELOG rather than written down,
    // because a literal is how two other gates in this project went stale.
    let idx = released
        .iter()
        .position(|v| *v == current)
        .unwrap_or_else(|| {
            panic!(
                "CHANGELOG.md has no `## [{current}]` section, so the previous \
                 release cannot be derived. Newest headers found: {:?}",
                released.iter().take(3).collect::<Vec<_>>()
            )
        });
    let previous = released.get(idx + 1).unwrap_or_else(|| {
        panic!("`{current}` is the oldest release in CHANGELOG.md; a chain cannot be checked")
    });

    let mut wrong: Vec<String> = Vec::new();
    let mut checked = 0usize;
    let mut docs_with_chains = 0usize;

    for doc in chain_docs() {
        let text = read(doc);
        // Each chain in the document, not merely the first: `ROADMAP.md` carries
        // two, and the v2.6.10 drift reached BOTH. Checking one would have
        // reported the file fixed while half of it still skipped v2.6.9.
        let mut from = 0usize;
        let mut chains = 0usize;
        while let Some(i) = text[from..].find(MARKER) {
            let start = from + i;
            let at = start + MARKER.len();
            // Only a real chain link, not any prose that happens to read
            // "... on **v2.5.0 was ...". Checked on the text BEFORE the marker
            // so the two spellings share one parse.
            if !LEAD_INS.iter().any(|lead| text[..start].ends_with(lead)) {
                from = at;
                continue;
            }
            let Some(found) = parse_version_prefix(&text[at..]) else {
                // Not a version after all -- prose, not a chain link.
                from = at;
                continue;
            };
            // Only the FIRST link of each chain is a claim about ordering. The
            // rest are history and are allowed to abbreviate.
            if found == *previous {
                checked += 1;
            } else if chains == 0 || found > *previous {
                // A first link naming something OTHER than the previous release
                // is the defect. `found > previous` catches a second chain in
                // the same file whose head is also too new.
                wrong.push(format!(
                    "  {doc} -- a chain's first link names v{found}, but \
                     v{previous} is the release before v{current}"
                ));
            }
            chains += 1;
            from = at + found.len();
            // One verdict per chain: skip to the next lead rather than walking
            // the whole tail, which is prior history and not this test's subject.
            if let Some(next) = text[from..].find("Current release") {
                from += next;
            } else {
                break;
            }
        }
        // A document with no chain is legitimate -- several anchors state a
        // version and nothing else. The corpus-level `checked > 0` below is
        // what keeps this from going inert.
        docs_with_chains += usize::from(chains > 0);
    }

    assert!(
        docs_with_chains > 0 && checked > 0,
        "{docs_with_chains} anchored document(s) carry a `{MARKER}` chain and \
         {checked} named v{previous} as the release before v{current}. Fail-closed: \
         a corpus where nothing matches would otherwise pass while checking nothing."
    );
    assert!(
        wrong.is_empty(),
        "{} release-line chain(s) SKIP a release (workspace = {version}, previous = \
         {previous}):\n{}\n\n\
         A version bump that rewrites the token and leaves the summary produces \
         exactly this: the new release wears the old one's description, and the \
         old one vanishes from the lineage. Insert `Built on **v{previous} \
         \"<codename>\"** -- <its summary>.` ahead of the existing chain.",
        wrong.len(),
        wrong.join("\n")
    );
}

/// A codename quoted NEAR the current version, past a date or a dash, must
/// still be the CHANGELOG's codename.
///
/// [`skip_to_codename`] trims only spaces and asterisks, which is deliberate --
/// it lets `**v2.3.9** (2026-08-20)` take the legitimate "states a version
/// only" path. `docs/STATUS.md` then grew a codename on the far side of exactly
/// that punctuation: `v2.6.10** (2026-08-31) — **"Abeyance"**`, v2.6.9's
/// codename under v2.6.10's number, with every existing check passing. The
/// document moved out from under the scan rather than the scan being wrong, so
/// this is a second check rather than a loosening of the first.
#[test]
fn a_codename_near_the_current_version_is_the_changelog_codename() {
    // Wide enough for ` (2026-09-01) — **"` and no wider. A larger window would
    // reach the NEXT release's codename in a chain and report a false failure.
    const WINDOW: usize = 40;

    let version = workspace_package_field("version");
    let current = version_core(&version).to_owned();
    let expected = codename_of(&changelog_header(&version));
    let token = format!("v{current}");

    let mut wrong: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for doc in chain_docs() {
        let text = read(doc);
        for (idx, _) in text.match_indices(&token) {
            // Require a token boundary, so `v2.6.1` does not match inside
            // `v2.6.10` -- the same trap `the_version_scan_requires_a_token_boundary`
            // pins for the reverse scan.
            let after = &text[idx + token.len()..];
            if after.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
                continue;
            }
            if text[..idx]
                .chars()
                .next_back()
                .is_some_and(|p| p.is_alphanumeric() || p == '_')
            {
                continue;
            }
            let win: String = after.chars().take(WINDOW).collect();
            let Some(open) = win.find('"') else { continue };
            // The gap between a version and its codename is punctuation and a
            // date -- ` "`, or `** (2026-09-01) -- **"`. It never contains a
            // LETTER. `README.md`'s version badge does: the URL continues
            // `-blue.svg" alt="Version"`, so the nearest quote is an attribute
            // delimiter and the codename it yields is ` alt=`. Caught by this
            // check's own first run, before it shipped -- and the narrower rule
            // tried first, rejecting `=<>/`, did NOT catch it, because the
            // offending gap holds none of them.
            if win[..open].contains(|c: char| c.is_ascii_alphabetic()) {
                continue;
            }
            let rest = &win[open + 1..];
            let Some(close) = rest.find('"') else {
                continue;
            };
            let name = &rest[..close];
            if name.is_empty() {
                continue;
            }
            checked += 1;
            if name != expected {
                wrong.push(format!(
                    "  {doc} -- v{current} is followed by \"{name}\", CHANGELOG says \"{expected}\""
                ));
            }
        }
    }

    assert!(
        checked > 0,
        "no document quotes a codename within {WINDOW} characters of v{current}. \
         Fail-closed: teach this check the new shape rather than leaving it \
         asserting nothing."
    );
    assert!(
        wrong.is_empty(),
        "{} site(s) name v{current} beside the wrong codename:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}
