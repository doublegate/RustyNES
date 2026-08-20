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
            // `starts_with` rather than `==`: a table header may carry a trailing
            // inline comment, and an exact match would silently skip the table --
            // which fails OPEN, since `in_table` would stay false and the panic at
            // the bottom would blame a missing key. (Review on #427.)
            in_table = t.starts_with("[workspace.package]");
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

/// Read a `MAJOR.MINOR.PATCH` starting at `s[0]`, stopping at the first
/// character that cannot be part of one.
///
/// Returns `None` rather than a partial parse, so a marker followed by something
/// that is not a version fails the audit instead of silently comparing garbage.
fn parse_version_prefix(s: &str) -> Option<String> {
    let end = s
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(s.len());
    let v = &s[..end];
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
                &text[at..(at + 24).min(text.len())]
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
            // A codename, when present, follows as ` "Name"` -- but the space
            // count is a prose detail, not a contract. Matching exactly one space
            // let a document write two and silently bypass the check while some
            // OTHER anchor kept `checked > 0`, which is the same fail-open shape
            // as the unterminated quote below. (Review on #427.)
            let tail = text[at + found.len()..].trim_start_matches(' ');
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
                    &rest[..rest.len().min(40)]
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
            .map(|l| format!("  {}", &l[..l.len().min(90)]))
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
