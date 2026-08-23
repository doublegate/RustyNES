# Contribution checklist — MiSTer FPGA

Every line traces to
`ref-docs/2026-08-23-mister-core-contribution-requirements.md`, which quotes the
MiSTer-devel wiki fetched 2026-08-23. **Nothing here is from memory.**

Checked at **v2.7.0**, not before. Items marked **(now)** are already settled.

## Repository layout

- [ ] `sys/` present and **verbatim** from `Template_MiSTer` — never modified
- [x] `rtl/` present **(now)**
- [x] `releases/` present **(now)**
- [ ] `.qpf` at the root for the core (one exists for `kitchen_sink` only)
- [ ] `.qsf`
- [ ] `.srf`
- [ ] `.sdc` — timing constraints
- [ ] Top-level `.sv` implementing the **`emu`** module
- [ ] `files.qip`
- [ ] `clean.bat`
- [x] `.gitignore` **(now)**

## Release artifact

- [ ] `releases/RustyNES_YYYYMMDD.rbf`, named exactly to the convention
- [ ] Unique **Home folder** chosen (non-arcade requirement)
- [ ] **No MRA files** — arcade-only, and including them would be wrong

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
