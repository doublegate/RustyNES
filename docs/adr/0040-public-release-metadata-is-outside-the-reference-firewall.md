# 40. Public release metadata about a black-boxed core is outside the reference firewall; its source remains inside

Date: 2026-09-19

## Status

Accepted. Scopes — does not widen — the reference firewall stated in
`docs/ai-emulator-provenance-guardrails.md` and extended to HDL by
[ADR 0037](0037-mister-fpga-core-independent-hdl-implementation.md). Supersedes
no ADR. Decided by the maintainer after the question was escalated rather than
self-certified, per `RustyNES_MiSTer/docs/provenance.md` §"Do not self-certify".

## Context

`NES_MiSTer` is a strict black box under ADR 0037. The rule is stated in
`RustyNES_MiSTer/docs/provenance.md` and enumerates what it covers:

> **Not permitted:** opening, reading, quoting or transcribing its **source** —
> not the RTL, not its constants, not its module or signal names, not its
> comments. Not "for reference", not once.
>
> **Permitted:** instantiating a third-party core as an *opaque testbench
> module* and comparing its **outputs** against ours.

A question the rule does not settle in those words arose at v2.6.22. The
v2.7.0 "Shakedown" plan requires, before any submission decision, that the
incumbent be **re-measured on the same AccuracyCoin build** (Strand F, row F1 of
`RustyNES_MiSTer/docs/bringup-log.md`). The justification for re-measuring is
that the previously-recorded figure — 121/125 — predates a period in which the
incumbent changed. Stating that justification means referring to the fact that
it changed, and to when.

An automated reviewer (Antigravity, an unattended first-pass) raised this as a
**blocking provenance-firewall violation** on PR #20, on the reasoning that the
rule says *never opened, read, quoted or transcribed* and reading a commit log
is reading. The text it objected to was not new: it had been on `main` in
`RustyNES_MiSTer/README.md` since v2.6.21 and in
`to-dos/plans/v2.7.0-shakedown-plan.md`, and v2.6.22 propagated it to
`docs/submission-case.md`.

Two further observations bear on the decision.

**The firewall's purpose is not engaged by metadata.** The rule exists to
prevent reproducing copyrighted *expression* into this project's RTL — the
failure recorded in `docs/provenance-failure-postmortem.md`. A release date
carries no expression: no constant, no table, no identifier, no ordering,
nothing that could be transcribed into a `.sv` file. Nothing in this core's RTL
derives from any such observation.

**But the concern is not baseless either.** This project is proposing a
competing core to the same distribution. Watching the incumbent's repository is
a different activity from reading its source, and the appearance of doing so has
a cost even where the rule does not forbid it. A per-entry enumeration of *which
behaviours* the incumbent had changed — which the first draft carried — sits
closest to that line and adds nothing the argument needs.

A second, separate objection followed in a later review round: that the written
record of this disagreement ("A reviewer read that…", "declined as stated",
"Flagged for the maintainer") had been placed in `README.md` and
`docs/submission-case.md`, turning project documentation into a ledger for
review disputes.

## Decision

**1. Public release metadata is outside the firewall. Source remains inside.**

Permitted, about a black-boxed core: **when** it published changes, **that** it
published changes, and anything it advertises publicly about itself. These are
observations of a project's public activity, not of its source.

Not permitted, unchanged: its RTL, constants, tables, module or signal names,
comments, code ordering — in whole or in paraphrase, and whether reached through
files, a commit log, a diff, or any other route. **The route is irrelevant; the
artefact is what the rule names.** A commit *diff* is source. A commit
*subject line* is metadata.

**2. Enumerating what a black-boxed core changed is not permitted, even from
metadata.** Listing which behaviours or registers the incumbent's recent work
named is a description of its development, sits nearest the line, and is never
required: the argument only ever needs *that* it moved. Removed at v2.6.22 and
not to be reintroduced.

**3. A capability observation about a black-boxed core is dropped where it is
not load-bearing.** The clause "plus a netlist-accurate composite encoder this
core does not have" is removed. It describes what the incumbent *contains*
rather than when it changed, and the re-measurement justification does not need
it. Where the submission case must state a feature gap honestly — and it must —
it does so from our own capability list, which
`RustyNES_MiSTer/docs/submission-case.md` already carries as a table of what
**we** lack.

**4. The reasoning lives here, not in the documents.** `README.md` and
`docs/submission-case.md` carry the resulting text and a reference to this ADR.
They do not carry the argument, the review exchange, or a maintainer flag. This
is the project's existing convention — rationale belongs in a numbered ADR, and
`master-core/modules/40-docs-and-adrs.md` says so: *"Capture rationale and cost
in ADRs, not changelog-style history."*

## Consequences

**Strand F is unblocked and stays honest.** The bring-up log's F1 row can state
why the incumbent is being re-measured without the statement being a firewall
question every time it is read. The measurement itself is unchanged and still
gates the submission decision; nothing here licenses *predicting* the result.

**The line is now citable rather than re-derived.** The next reviewer to raise
it — and one will — gets an ADR number instead of a fresh argument. The previous
round cost a blocking finding, a decline, a partial edit, and an escalation.

**It scopes rather than widens, and that distinction is load-bearing.** ADR 0037
makes widening the firewall a maintainer-only act. This decision removes
material (the enumeration, the capability clause) and permits nothing that the
rule's own enumeration did not already leave out. Anything genuinely ambiguous
escalates to a new ADR before source is opened, exactly as ADR 0037 requires.

**A residual risk is accepted, in writing.** A strict reader may still regard
any attention to the incumbent's repository as improper for a party proposing a
competing core. That reading is understood and not adopted: the rule enumerates
source, the purpose is expression, and Strand F cannot be honest without
knowing whether its baseline figure is current. If MiSTer-devel expresses a
different view at submission, this ADR is superseded rather than argued.

**What this does not do.** It does not license reading a diff, a file, or a
patch; it does not license reproducing any identifier or constant; and it does
not license describing the incumbent's implementation from any source. The
escalation ladder in `AGENTS.md` — vendored documentation, the open Internet,
black-box comparison, and only then derived source under a fresh ADR — is
untouched.
