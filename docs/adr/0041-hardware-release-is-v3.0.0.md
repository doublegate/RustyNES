# 41. The first hardware-verified FPGA core ships as v3.0.0, and a new deliverable class is a MAJOR bump

Date: 2026-09-22

## Status

Accepted. Decided by the maintainer. Amends the MAJOR-bump rule in
`VERSION-PLAN.md` §"Versioning guidelines". Amends — does not supersede —
[ADR 0037](0037-mister-fpga-core-independent-hdl-implementation.md), whose
"v2.6-v2.9 programme" now ends at v3.0.0. Supersedes the version assignment (not
the content) of `to-dos/plans/v2.7.0-shakedown-plan.md` and
`to-dos/plans/v2.7.0-mister-core-plan.md`. Leaves ADR 0003 and ADR 0028 untouched.

## Context

From v2.6.21 onward the record defined **v2.7.0 "Shakedown"** as the session with
the board: the release in which a `RustyNES_MiSTer` bitstream first runs on a
SuperStation One, the four properties no co-simulation gate can reach are
measured, and the MiSTer-devel submission decision is made on evidence.

Two things changed before that session started.

1. **Four audit reports landed** in #544 (`docs/audits/`): core, frontend,
   libretro and RTL. They are AI-written, and a calibration pass over 26 of their
   claims found the core, libretro and frontend reports largely accurate at the
   cited lines, and the RTL report the least reliable (two defects refuted, a
   fabricated resource table). Acting on them properly means triage, a failing test
   per finding, a fix, and the full gate set. That is several releases of work, and
   the RTL half of it changes the bitstream.
2. **Taking a board to the bench before that work is done measures a design that
   is about to change.** Every RTL fix re-opens the seed sweep and the `.rbf`, and
   invalidates every hardware reading taken from the old one. The bring-up belongs
   after the last planned RTL change, not before it.

So the hardware milestone moves. The question this ADR settles is what number it
gets. `VERSION-PLAN.md` said to bump MAJOR **only** for an incompatible public-API
break or a save-state break that cannot migrate, which is exactly what v2.0.0
"Timebase" was (ADR 0028). The first hardware-verified FPGA core is neither. Under
the old rule it would be v2.10.0 or similar, and the version number would say
nothing about the one release in this project's history that changes what
RustyNES *is*: from an emulator with a simulated FPGA sibling to a shipped
hardware core.

## Decision

1. **The first hardware-verified FPGA core is v3.0.0.** Both repositories tag it
   together.
2. **The MAJOR-bump rule gains a second trigger: a new deliverable class.** A MAJOR
   bump marks either (a) an incompatible public-API or save-state break, as before,
   or (b) the first release of a new *class* of shipped artefact, such as a hardware
   core, verified to the standard that class requires. (b) is deliberately narrow:
   a new platform (Android, iOS, libretro) stays MINOR, because it is a new host
   for the same emulator. An FPGA core is not a host, it is a second
   implementation of the machine.
3. **The line to v3.0.0:**
   - **v2.7.x**: the core and frontend audits.
   - **v2.8.x**: the libretro and RTL audits, including the off-die SDRAM build.
   - **v2.9.x**: re-audit, optimisation, final seed sweeps, release-candidate
     `.rbf`, and the bring-up on the SuperStation One.
   - **v3.0.0**: the verified core. The on-die `.rbf` is the headline; the off-die
     (SDRAM) `.rbf` ships as a clearly labelled secondary.
4. **v3.0.0 may also carry a real API break**, if it is worth carrying. v2.7.5
   deprecates the dead surface the core audit located: the unused `Cpu::Bus`
   methods and the orphaned `ApuBus`. Whether v3.0.0 removes them is decided at
   v2.9.0 and recorded there. **No save-state break is planned.** The additive
   `internal_data_bus` snapshot section in v2.8.0 follows ADR 0028's rules and does
   not break cross-version loading within the v2 epoch.
5. **Nothing is renamed retroactively.** v2.6.21-v2.6.23 shipped with release bodies
   naming v2.7.0 as the board session; those bodies are history and stay as
   published. Forward-looking prose is updated; shipped records are not.

## Consequences

- `VERSION-PLAN.md`'s MAJOR rule cites this ADR, and its "Planned next" table
  carries v2.7.x, v2.8.x, v2.9.x and v3.0.0.
- "v2.8+" stops meaning "features after the hardware release". The deferred
  features (FDS, expansion audio, save states, cheats, PAL, the other mapper
  families, the DE10-Nano second board) are relabelled **post-v3.0.0**, so that
  each version number means one thing.
- The engine-lineage release notes `docs/release-notes/v2.7.0.md`, `v2.7.1.md` and
  `v2.8.0.md` keep their names. That folder is engine-lineage history throughout,
  disclaimed as such in its README, and its `v2.0.0`-`v2.6.0` files have always
  shared numbers with shipped RustyNES releases, whose own notes live in
  `.github/release-notes/`. Renaming only the three that the new line reaches
  would break that folder's convention rather than resolve a collision.
- The "no hardware has run any bitstream" anchors stay true for three more minor
  lines. They are flipped in the v3.0.0 PR and nowhere earlier.
- `to-dos/mister/contribution-checklist.md` changes its deadline from v2.7.0 to
  v3.0.0, together with `contribution_checklist_audit.rs`, which pins that
  sentence.
- The cost is time: the board is in hand and will not be used until v2.9.2. That
  was weighed and accepted. The alternative is measuring a bitstream the audit
  line is about to replace. The known risk it leaves open is that two public spec
  sources name the SuperStation One's FPGA as a Cyclone V SX 5CSXFC6D6F31, while
  this build targets the DE10-Nano's 5CSEBA6U23. Stock MiSTer cores are reported
  running on the SS1 unmodified, so the build is expected to load, but that is
  confirmed only at v2.9.2's Strand A, on the stock core, before ours is loaded.
