# 43. v3.0.0 is the API major and a release-candidate core; hardware verification moves to v3.x

Date: 2026-09-29

## Status

Accepted. Decided by the maintainer on 2026-09-29, after v2.9.3 "Handset".
**Supersedes the Decision of [ADR 0041](0041-hardware-release-is-v3.0.0.md)**
insofar as it defines v3.0.0 as "the first hardware-verified FPGA core". ADR
0041 stays as written: its line structure (v2.7.x, v2.8.x and v2.9.x audit
lines) is history, and its MAJOR-trigger amendment to `VERSION-PLAN.md` ("a new
deliverable class") still stands for whichever release first ships a
hardware-verified core. [ADR 0042](0042-v3-removes-the-v2-7-5-deprecations-and-the-dead-nmi-edge-detector.md)
is unchanged and is now v3.0.0's main content.

## Context

ADR 0041 made v3.0.0 the release in which a `RustyNES_MiSTer` bitstream is
verified on a SuperStation One, with every bring-up strand (A-F) passing. On
2026-09-29 the maintainer moved the board session, the mobile device runs, and
the fixes each produces **after** v3.0.0. v2.9.3 shipped on that basis, and
ADR 0041's 2026-09-29 amendment recorded that the two could not both stand.

Three facts shaped the answer:

- **v3.0.0 is MAJOR anyway.** ADR 0042 schedules a real API break for it (19
  deprecated items removed, the NMI edge detector removed, `LockstepBus`
  renamed, `serialize_header` removed) and a `.rns` BUS-section format change.
  Under `VERSION-PLAN.md`'s first MAJOR trigger that alone requires the bump.
- **The bitstreams exist and are timing-clean,** but they are not
  hardware-verified. Seed 2 also turned out to be build-date-scoped: it closed
  on 260928 and missed on-die setup by 0.121 ns on 260929. So a v3.0.0 pair
  has to come from its own sweep.
- **Everything else on the path to v3.0.0 can be done on this machine.**
  Surveyed on 2026-09-29, about 230 open items across both repositories, most
  of them host-verifiable.

## Decision

1. **v3.0.0 is the API major plus a release-candidate core.** It carries ADR
   0042 in full and the BUS-section version bump. It ships both bitstreams, on-die
   and off-die, each swept at the release's own build date, and **labelled
   "release candidate, not hardware-verified"**.
2. **Hardware verification becomes a later v3.x release.** That covers the board
   session (Strands A-F), the mobile device run, and the fixes they produce.
   Whether that release is a MINOR (v3.1.0) or its own MAJOR is decided when
   it is planned. ADR 0041's "new deliverable class" trigger is available to it.
3. **The line from v2.9.4 to v3.0.0** follows
   [`to-dos/plans/v2.9.4-to-v3.0.0-line-plan.md`](../../to-dos/plans/v2.9.4-to-v3.0.0-line-plan.md):
   records and CI (v2.9.4), accuracy (v2.9.5), mappers (v2.9.6), targeted
   platform features (v2.9.7), performance and the SDRAM arbiter (v2.9.8), the
   release candidate (v2.9.9), then v3.0.0.
4. **External work before v3.0.0 is limited to the libretro `.info` upstream
   sync**, submitted at v3.0.0. App stores, F-Droid and the MiSTer-devel
   submission stay after the hardware work.

## Consequences

- **The "no hardware has run any bitstream" statements stay true through
  v3.0.0.** They are flipped in the hardware-verification release, not before.
  v3.0.0's release notes must say plainly that its bitstreams are unverified.
- **`to-dos/plans/v3.0.0-superstation-core-plan.md` is rewritten** for this
  scope. Its bring-up gate moves to the hardware-verification release's plan.
  `VERSION-PLAN.md`'s "Planned next" table carries the new rows.
- **`to-dos/mister/contribution-checklist.md`'s deadline moves** from v3.0.0
  to the hardware-verification release (v3.x), because the checklist is the
  submission's and the submission needs a board. The sentence and
  `contribution_checklist_audit.rs`, which pins it, change in the same commit.
- **Every release that rebuilds a bitstream sweeps seeds at its own build date,
  for both builds.** v3.0.0 does, and so does v2.9.9 for the release candidate.
- **Risk accepted:** a MAJOR release ships an FPGA artefact nobody has run on
  hardware. The label, the release notes and the unchanged hardware statements
  are what keep that honest.

## Amendment (2026-10-01, v2.9.8): the API break lands before v3.0.0

The maintainer moved ADR 0042's removals, and the other permanent breaks found
while v2.9.8 was being built, into v2.9.8 (see ADR 0042's amendment of the same
date). v3.0.0 therefore no longer carries the API break. Its other content, the
release-candidate core and both bitstreams labelled not hardware-verified, is
unchanged. The version number of the release that now carries the break is
decided at its cut.
