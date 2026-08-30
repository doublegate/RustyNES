# Contribution checklist — MiSTer FPGA

Every line traces to
`ref-docs/2026-08-23-mister-core-contribution-requirements.md`, which quotes the
MiSTer-devel wiki fetched 2026-08-23. **Nothing here is from memory.**

The whole list is checked at **v2.7.0**, which is the submission. Individual
items are marked with the release that settled them -- **(now)** for ones true
before this programme started, **(v2.6.6)** for the layout items the chassis
release landed -- so the remaining unchecked boxes are the real work rather than
a list nobody has looked at yet.

## Repository layout

Settled at **v2.6.6**, except the one item that needs a board.

- [x] `sys/` present and **verbatim** from `Template_MiSTer` **(v2.6.6)** —
      **57 files, 0 content differences** at `3ea1134c`, verified by SHA-256
      against a fresh clone; licence re-tallied over the 40 HDL files among them:
      0 GPL-2.0-only
- [x] `rtl/` present **(now)**
- [x] `releases/` present **(now)**
- [x] `RustyNES.qpf` **(v2.6.6)**
- [x] `RustyNES.qsf` **(v2.6.6)** — deliberately thin; the device and all 109
      pin assignments come from `sys/sys.tcl`
- [ ] `.srf` — **deliberately absent, decided rather than skipped.** The
      warnings that cannot be fixed at source live in Quartus's own megafunction
      library and in `sys/`, neither of which this core may edit. They are
      suppressed with `MESSAGE_DISABLE` assignments in the `.qsf`, each with the
      reason written beside it, which is legible in a diff where an `.srf`
      entry is not. Revisit if a reviewer asks for the conventional file.
- [x] `RustyNES.sdc` **(v2.6.6)** — short by construction: one clock domain,
      because `nes_top` divides the master clock with enables rather than
      deriving clocks
- [x] `rtl/emu.sv` implementing the **`emu`** module **(v2.6.6)**
- [x] `files.qip` **(v2.6.6)**
- [x] `clean.bat` **(v2.6.6)**
- [x] `.gitignore` **(now)**
- [x] `rtl/pll.qip` **(v2.6.6)** — not in the wiki's list, and **required**:
      `sys/pll_q17.qip` names that literal path, and its absence produced 42
      warnings

## Release artifact

- [ ] `releases/RustyNES_YYYYMMDD.rbf`, named exactly to the convention
- [ ] Unique **Home folder** chosen (non-arcade requirement)
- [x] **No MRA files** **(now)** — arcade-only, and including them would be wrong

## Licence

- [x] GPL-3.0-or-later **(now)** — and forced upward rather than chosen:
      `hps_io.sv` is GPL-3.0-or-later and not optional. v2.4.3 audit, 57 files,
      zero GPL-2.0-only

## Quality bar

- [ ] Core is accurate enough to demonstrate **preservation value**
- [ ] AccuracyCoin result stated as a **floor**, entry-for-entry, including
      `Skipped`/`NotRun`
- [ ] Runs on **real hardware**, both boards, one `.rbf`
- [ ] **AI-generated-code bar:** readability, plus *"evidence of quality and
      accuracy testing"* — the co-simulation record is that evidence, and the
      submission should link it explicitly rather than assume a reviewer finds it

## Provenance

- [ ] `docs/provenance.md` states the firewall and that no NES core was ever opened
- [ ] CI provenance job green — no black-boxed core in the tree
- [ ] Every RTL file carries its SPDX header

## Submission

- [ ] Email `newcores@misterfpga.org` with the repository link
- [ ] Await review (the page says days)
- [ ] **Decide deliberately** on the MiSTer-devel invitation and repository
      transfer — acceptance moves the repo, it is one-way, and this project owns it
- [ ] Add to the Cores list with the Home folder

## If declined as a duplicate

Not a failure path — a planned one. See
`ref-docs/2026-08-23-alternative-fpga-targets.md`.

- [ ] Retro Remake / SuperStation One — already the hardware target
- [ ] openFPGA / Analogue Pocket — demonstrated MiSTer-core porting path;
      `nes_top.sv` stays platform-agnostic precisely so this stays cheap
- [ ] The co-simulation evidence is publishable on its own terms regardless
