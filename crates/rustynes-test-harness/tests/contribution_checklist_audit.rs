// SPDX-License-Identifier: GPL-3.0-or-later
//! Every box in the `MiSTer` contribution checklist must carry a verdict.
//!
//! # The defect this exists to prevent
//!
//! `to-dos/mister/contribution-checklist.md` is the list that decides whether
//! the core is ready to submit at v2.7.0. Before v2.6.14 it had 30 boxes, 16 of
//! them unticked, and **14 of those 16 said nothing at all about why**.
//!
//! An unticked box with no reason is indistinguishable from three different
//! things: work that is genuinely outstanding, work that is blocked on
//! something outside this repository, and work that was *already done* and
//! never ticked. The v2.6.14 audit found the third case five times — the
//! provenance CI job, the SPDX sweep, the firewall statement, the `AccuracyCoin`
//! vector and the preservation-value case were all true and all unticked — so
//! the list was reporting the project as further from submission than it was,
//! by a fifth of its own length.
//!
//! It also found a box that could **never** be ticked honestly: it asked that
//! `docs/provenance.md` state "that no NES core was ever opened", which is
//! precisely the self-certification that same document forbids. That is not a
//! missing tick; it is a wrong requirement, and only reading every item found
//! it.
//!
//! # Why a structural gate rather than a review pass
//!
//! This is v2.6.11's rule applied to a different document: **prose cannot be
//! audited, an ordering can.** "Someone should re-read the checklist" is not a
//! check, and the evidence is that nobody did for four releases. What *is*
//! checkable is a shape:
//!
//! * a **ticked** box names the release that settled it — `**(now)**` for
//!   things true before the programme, or `**(vX.Y.Z)**`;
//! * an **unticked** box carries a verdict — `**BLOCKED`, `**DEFERRED`,
//!   `**DECIDED` or `**CONTINGENT` — so the reason survives in the file rather
//!   than in whoever last looked.
//!
//! The gate deliberately does not judge whether a verdict is *correct*. It
//! cannot, and pretending otherwise would be the "gate that passes without
//! testing its subject" this project keeps finding. What it enforces is that a
//! claim was made and can be argued with.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root is two levels above this crate")
        .to_path_buf()
}

/// One checklist box: its state, and every line that belongs to it.
struct Item {
    ticked: bool,
    line: usize,
    body: String,
    /// Set when a top-level line ends the item, so later indented lines are not
    /// folded into a box they do not belong to.
    ///
    /// This was a `\u{0}` sentinel appended to `body`, which worked and read as
    /// a trick: the closing state was encoded in the data it was guarding, and
    /// every reader had to know that. An explicit field says the same thing
    /// without the decoding step, and keeps `body` free of bytes nobody put
    /// there.
    closed: bool,
}

/// Split the document into items, folding each item's continuation lines into
/// it.
///
/// The continuation rule is load-bearing and was found by measurement: the
/// verdict markers do **not** sit on the first line of an item. The release
/// artifact's blocker is nine lines below its `- [ ]`, because the item states
/// what *is* done before it states what is not. A checker reading only the
/// first line of each box reports fourteen violations that are not there.
///
/// # A malformed box must be an error, not a separator
///
/// `- [ ]missing-space` matches neither prefix, so the first version of this
/// parser fell through to the separator branch and **dropped the box silently**
/// — and 29 surviving items still cleared a `>= 20` floor, so the audit passed
/// while one box went unchecked. That is the failure this gate exists to
/// prevent, reproduced inside the gate itself. Any top-level line beginning
/// `- [` that is not one of the two exact forms is now rejected by name.
///
/// # Continuations must be indented, and that is a requirement not an accident
///
/// Any non-blank unindented line ends the current item, so Markdown's "lazy
/// continuation" — wrapping an item's prose to column zero — is not supported.
/// The checklist uses indented continuations throughout and must keep doing so.
///
/// The failure mode is safe rather than silent: an unindented continuation
/// carrying a verdict is not folded into its item, so the box reads as
/// verdict-less and the gate FAILS. It cannot cause a box to pass unchecked,
/// which is the direction that would matter.
fn parse(md: &str) -> Result<Vec<Item>, String> {
    let mut items: Vec<Item> = Vec::new();
    for (idx, raw) in md.lines().enumerate() {
        // Reject anything checkbox-SHAPED that is not one of the two exact
        // forms, and test it on the TRIMMED line. An indented `  - [ ]` slips
        // past a column-zero test and is then folded into the previous item as
        // continuation text -- the box vanishes, and with the item count
        // unchanged (one added indented, none removed) the floor below cannot
        // notice. Same hole as the missing-space case, one indent over.
        let trimmed = raw.trim_start();
        if trimmed.starts_with("- [") && !raw.starts_with("- [x] ") && !raw.starts_with("- [ ] ") {
            return Err(format!(
                "line {}: malformed checkbox -- expected exactly `- [x] ` or `- [ ] ` at \
                 column zero: {raw}",
                idx + 1
            ));
        }
        if let Some(rest) = raw.strip_prefix("- [x] ") {
            items.push(Item {
                ticked: true,
                line: idx + 1,
                body: rest.to_string(),
                closed: false,
            });
        } else if let Some(rest) = raw.strip_prefix("- [ ] ") {
            items.push(Item {
                ticked: false,
                line: idx + 1,
                body: rest.to_string(),
                closed: false,
            });
        } else if raw.starts_with("- ")
            || (!raw.is_empty() && !raw.starts_with(char::is_whitespace))
        {
            // A new top-level block ends the current item. A blank line does
            // not: an item may separate its paragraphs and still be one item.
            //
            // `char::is_whitespace` rather than `' '`: a TAB-indented
            // continuation would otherwise read as unindented and cut its item
            // short, which fails safe -- the verdict is not folded in, so the
            // box reports as verdict-less -- but fails for a reason that has
            // nothing to do with the checklist.
            if let Some(last) = items.last_mut() {
                last.closed = true;
            }
        } else if let Some(last) = items.last_mut()
            && !last.closed
        {
            last.body.push('\n');
            last.body.push_str(raw);
        }
    }
    Ok(items)
}

/// `**(now)**` or `**(v2.6.14)**`.
///
/// Exactly three all-digit dot-separated components, deliberately: `v2.7` and
/// `v2.7.0-rc1` are rejected. Neither can occur here — this workspace **cannot
/// carry a `SemVer` pre-release version** at all, because every intra-workspace
/// dependency is a caret requirement and a caret does not match a pre-release,
/// so `version = "2.7.0-rc.1"` fails to resolve before any test runs.
/// `release_anchor_audit` guards that separately. Loosening this to accept
/// forms the project cannot produce would trade a real check for a
/// hypothetical one.
fn has_release_tag(body: &str) -> bool {
    let mut rest = body;
    while let Some(at) = rest.find("**(") {
        let tail = &rest[at + 3..];
        if let Some(end) = tail.find(")**") {
            let tag = &tail[..end];
            if tag == "now" {
                return true;
            }
            let versionish = tag.starts_with('v')
                && tag[1..].split('.').count() == 3
                && tag[1..]
                    .split('.')
                    .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
            if versionish {
                return true;
            }
        }
        rest = &rest[at + 3..];
    }
    false
}

const VERDICTS: [&str; 4] = ["**BLOCKED", "**DEFERRED", "**DECIDED", "**CONTINGENT"];

/// A verdict must be the whole word, not a prefix of one.
///
/// A raw substring match accepts `**BLOCKEDNESS`, which is not a verdict and
/// would let a box pass on a typo. The marker must be followed by something
/// that cannot continue the word.
fn has_verdict(body: &str) -> bool {
    VERDICTS.iter().any(|v| {
        body.match_indices(v).any(|(at, _)| {
            body[at + v.len()..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric())
        })
    })
}

fn checklist() -> String {
    let path = repo_root().join("to-dos/mister/contribution-checklist.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} unreadable: {e}", path.display()))
}

#[test]
fn every_checklist_box_carries_a_verdict() {
    // Fail closed, and the floor is the CURRENT count rather than a round
    // number well below it. At `>= 20` a document that lost ten boxes still
    // passed, which is the same "a check that examines nothing reports a pass"
    // shape the floor is meant to prevent.
    const MIN_ITEMS: usize = 30;

    let md = checklist();
    let items = parse(&md).unwrap_or_else(|e| panic!("{e}"));
    assert!(
        items.len() >= MIN_ITEMS,
        "parsed {} checklist items, expected at least {MIN_ITEMS}; boxes have \
         been removed or the syntax has changed and this gate is no longer \
         reading its subject",
        items.len()
    );

    let mut bad: Vec<String> = Vec::new();
    for it in &items {
        let first = it.body.lines().next().unwrap_or_default();
        if it.ticked && !has_release_tag(&it.body) {
            bad.push(format!(
                "line {}: TICKED with no release tag -- add **(now)** or **(vX.Y.Z)**: {first}",
                it.line
            ));
        }
        if !it.ticked && !has_verdict(&it.body) {
            bad.push(format!(
                "line {}: UNTICKED with no verdict -- add one of {}: {first}",
                it.line,
                VERDICTS.join(", ")
            ));
        }
    }

    assert!(
        bad.is_empty(),
        "{} checklist box(es) carry no verdict:\n  {}\n\n\
         An unticked box with no reason cannot be told apart from work that is \
         outstanding, work that is blocked elsewhere, and work already done and \
         never ticked. The v2.6.14 audit found the third case five times.",
        bad.len(),
        bad.join("\n  ")
    );
}

#[test]
fn the_checklist_still_has_unticked_boxes_and_says_so() {
    // The counterpart to the gate above, and the reason it is a separate test:
    // a checklist that ticked everything would satisfy the verdict rule
    // vacuously. Submission is v2.7.0 and hardware is not attached, so a fully
    // ticked list would be a claim this project cannot support.
    let md = checklist();
    let items = parse(&md).unwrap_or_else(|e| panic!("{e}"));
    let unticked = items.iter().filter(|i| !i.ticked).count();
    assert!(
        unticked > 0,
        "every checklist box is ticked, which would mean the core is ready to \
         submit -- rung 6 needs hardware nobody here has, so this is a false \
         claim rather than a milestone"
    );
    assert!(
        md.contains("must be complete **by v2.7.0**"),
        "the checklist no longer states when it must be complete"
    );
}
