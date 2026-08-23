# MiSTer FPGA core contribution — requirements, process, and what they mean for RustyNES

**Dated supplemental reference, 2026-08-23.** `ref-docs/` is immutable; corrections
land as a new dated file, never as an edit to this one.

**Primary source:** the MiSTer-devel wiki page
*Contributing a Core to MiSTer FPGA*
(<https://github.com/MiSTer-devel/Wiki_MiSTer/wiki/Contributing-a-Core-to-MiSTer-FPGA>),
fetched 2026-08-23. Every requirement below is quoted or paraphrased from it
rather than recalled, because the whole point of this file is that the RTL and the
release process are built against the real bar.

---

## 1. What the core must demonstrate

> The core must demonstrate **preservation value** through accurate implementation
> of the original system.

MiSTer discourages redundant cores. `NES_MiSTer` already exists, is GPL-3.0,
covers ~150 mappers and FDS, and scores **121/125 on AccuracyCoin** — where real
Famicom AV hardware also scores ~121/125. **There is no published accuracy
headroom**, and this must be understood as a fact about the submission rather
than a problem to solve. See §6.

## 2. The AI-generated-code bar, verbatim

> Fully AI generated code should meet a **minimum reasonable bar for readability**
> and include **some evidence of quality and accuracy testing**.

This is the single most important sentence on the page for this project, and it is
favourable. The co-simulation apparatus — a per-cycle bus comparison against a
141/141 AccuracyCoin emulator, with every gate demonstrated to fail by mutation
before it is trusted — **is** that evidence, in a form no incumbent core can
currently show. The contribution case is built on this, not on coverage.

## 3. Licensing

> Publish under compatible open-source licenses, such as **GPLv3 or MIT**.

`RustyNES_MiSTer` is **GPL-3.0-or-later**. Settled, and already forced upward
rather than chosen: the v2.4.3 `sys/` licence audit found 57 files, **zero
GPL-2.0-only**, and `hps_io.sv` is GPL-3.0-or-later and **not optional** — it is
how a core receives a ROM from the HPS and reaches the OSD. The combined bitstream
must therefore be GPL-3.0-or-later. No relicensing is needed or possible.

## 4. Repository layout

| Path | Contents |
|---|---|
| `sys/` | The MiSTer framework, **verbatim** |
| `rtl/` | Core implementation |
| `releases/` | Binary releases (and MRA files, arcade only) |

Plus, at the repository root: `.qpf`, `.qsf`, `.srf`, `.sdc`, the top-level `.sv`,
`files.qip`, `clean.bat`, `.gitignore`.

**`sys/` may not be modified.** Framework updates overwrite local changes, and all
cores are expected to carry it unchanged. `RustyNES_MiSTer/sys/` is currently an
empty placeholder (`.gitkeep` + `README.md`) — populating it verbatim from
`Template_MiSTer` is a rung-6 task, not an earlier one.

## 5. Release naming

- Non-arcade: `<core_name>_YYYYMMDD.rbf` → **`RustyNES_YYYYMMDD.rbf`**
- Arcade: `Arcade-<core_name>_YYYYMMDD.rbf` — **not applicable here**

**MRA files are arcade-only** and do not apply to a console core. Non-arcade cores
must instead specify a **unique Home folder** when added to the Cores list.

## 6. Submission process

1. Email **`newcores@misterfpga.org`** with a link to the GitHub repository.
2. Await review — the page says **within days**.
3. Accept the invitation to the **MiSTer-devel** organisation.
4. **Transfer the repository** to MiSTer-devel. *You remain the primary
   maintainer.*
5. Add the core to the Cores list, specifying the unique Home folder.

Step 4 is worth reading twice before submitting: acceptance means the repository
moves. That is a one-way action on a repo this project owns, and it should be a
deliberate decision at v2.7.0 rather than a reflex.

## 7. What this means for the v2.5.1 → v2.7.0 line

- The layout requirements are cheap and late — `sys/`, `files.qip`, `.sdc`,
  `clean.bat` are all rung-6 work, and none of them gate the PPU or APU rungs.
- The **licence question is already closed**, which removes what the prior plan
  ranked as risk 1.
- The **accuracy bar is where the effort goes**, and the evidence apparatus is
  what distinguishes this submission. That argues for gating hard on AccuracyCoin
  entry-for-entry parity (rung 5) even though a pass *count* would be easier to
  report.
- **The core may still be declined as a duplicate.** Retro Remake and openFPGA are
  planned alternative homes; see the companion file
  `2026-08-23-alternative-fpga-targets.md`.
