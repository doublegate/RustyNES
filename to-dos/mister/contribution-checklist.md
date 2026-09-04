# Contribution checklist — MiSTer FPGA

Every line traces to
`ref-docs/2026-08-23-mister-core-contribution-requirements.md`, which quotes the
MiSTer-devel wiki fetched 2026-08-23. **Nothing here is from memory.**

The whole list must be complete **by v2.7.0**, which is the submission; it is NOT complete now, and an item left unchecked below is carried deliberately rather than overlooked. Individual
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
- [x] `RustyNES.qsf` **(v2.6.6)** — deliberately thin: it carries no location
      assignment of its own. **Re-measured at v2.6.14 and the count on this line
      was incomplete.** `sys/sys.tcl` supplies 109 (including the 45 SDRAM
      pins, which is why the SDRAM work needed no pin file of its own), and it
      stops short of the I/O board -- a core must additionally pick one of two
      36-assignment variants, and this one sources `sys/sys_analog.tcl`, the
      standard board, rather than `sys_dual_sdram.tcl`. **145 in total**, from
      two scripts, neither of them this file.
- [ ] `.srf` — **DECIDED — deliberately absent, not skipped.** It stays
      unticked because the file genuinely is not there and the list must not
      claim otherwise; the item is settled because the decision was taken, not
      because the artifact exists. The
      warnings that cannot be fixed at source live in Quartus's own megafunction
      library and in `sys/`, neither of which this core may edit. Most are
      suppressed with `MESSAGE_DISABLE` assignments in the `.qsf`, each with the
      reason written beside it, which is legible in a diff where an `.srf`
      entry is not.
      **Three are NOT suppressed, and cannot be** (corrected v2.6.7 — this line
      previously said they all were). `MESSAGE_DISABLE` was measured to have no
      effect on `13050`/`13051`, and the PLL `RST` warning carries **no message
      ID at all**, so there is nothing for an assignment to name. A hand-written
      `.srf` is worse than nothing here: a malformed one made Quartus abort
      after the resource summary with no footer and **exit status 0**, which
      reads exactly like success. What holds instead is an attribution gate —
      `tb/quartus_clean.py` fails if any Warning or Error cites `rtl/` or `tb/`,
      and it passes over a non-zero scan.

      **REVISITED v2.6.15, before a reviewer asked, and it corrects one clause
      of the reason above.** Two upstream sources disagree: the contributing
      wiki lists `.srf` among the standard files *"required by the template"*,
      while the Template's own Readme calls it *"optional file to disable some
      warnings which are safe to disable"*. The template **does ship one** —
      `Template.srf`, 29 rules, with `Template_Q13.srf` beside it.

      Reading it refutes the sentence above about the PLL warning. That warning
      is recorded as carrying **"no message ID at all, so there is nothing for
      an assignment to name"**, which is true of `MESSAGE_DISABLE` and not of
      the mechanism `.srf` uses: `Template.srf` suppresses it with a rule keyed
      on ID `9999` and the literal text `RST`, plus four more matching the full
      *"RST port on the PLL is not properly connected"* sentence. There **is**
      something to name; it is simply not a `MESSAGE_DISABLE` ID.

      **Still not adopted, and now for a cost rather than an impossibility.** An
      `.srf` changes the compile's warning SET, which `tb/check_warnings.py`
      pins against `tb/quartus-warnings.txt`, so adopting one means a full
      Quartus run and a regenerated baseline — and the abort-with-exit-0 hazard
      above is a reason to do that deliberately rather than alongside other
      work. The instance paths need adapting too: `Template.srf`'s rules name
      `emu:emu|pll:pll|pll_0002:pll_inst`, and this core instantiates its PLL
      differently — the same naming difference that cost v2.6.6 −13.901 ns when
      `sys_top.sdc`'s clock-group glob matched nothing. Scheduled for the next
      release that rebuilds the bitstream, with the baseline regenerated in the
      same change.
- [x] `RustyNES.sdc` **(v2.6.6)** — short because the constraints are DERIVED,
      not because there is one clock. **The reason on this line expired and was
      corrected at v2.6.14**: it said "one clock domain, because `nes_top`
      divides the master clock with enables rather than deriving clocks", true
      of v2.6.3 and false since v2.6.13, which added `clk_sdram` at 4x and the
      phase-shifted `clk_sdram_ps`. The shipped timing report names all three
      (`emu|pll|...|general[0..2]...|divclk`). `derive_pll_clocks` picks them
      up, and no false path is cut between them **deliberately**:
      `sys/sys_top.sdc` puts every clock matching its core-PLL glob in ONE
      `-exclusive` group, and `-exclusive` cuts paths BETWEEN groups — so the
      framework domains are cut and `clk_sys` <-> `clk_sdram` stays analysed,
      which is what makes ADR 0039's safety argument falsifiable.
- [x] `rtl/emu.sv` implementing the **`emu`** module **(v2.6.6)**
- [x] `files.qip` **(v2.6.6)**
- [x] `clean.bat` **(v2.6.6)**
- [x] `.gitignore` **(now)**
- [x] `rtl/pll.qip` **(v2.6.6)** — not in the wiki's list, and **required**:
      `sys/pll_q17.qip` names that literal path, and its absence produced 42
      warnings

## Release artifact

- [x] The release artifact is published under MiSTer's naming **(v2.6.15)** — was **NOT**, and the old wording is preserved below. Previously: **NOT under MiSTer's naming
      convention**.

      The publishing half is done **(v2.6.7)** — committed to the sibling's
      `releases/` and attached to the GitHub release on both repos, produced by
      `scripts/release-rbf.sh`, which refuses a compile with errors or negative
      per-clock slack at any corner, and labelled in the release body as never
      having run on hardware.

      **BLOCKED — on a maintainer decision, and the cost is now measured rather
      than paraphrased (v2.6.14).** The previous wording said
      `Distribution_MiSTer` "selects the newest bitstream by the DATE in the
      filename", which is the wiki's paraphrase. The mechanism is in
      `Main_MiSTer/file_io.cpp`, and it is sharper than that:

      - `get_display_name()` searches the filename for the literal `"_20"`,
        requires at least six characters after it, and then does `*p = 0` —
        **truncating the display name at that point** — taking up to fifteen
        following characters as `datecode`. With no `"_20"`, `datecode` becomes
        `"------"` and the name is left whole.
      - `DirentComp()` compares the truncated names first and only falls through
        to `strcasecmp(de1.datecode, de2.datecode)` when they are equal.

      `RustyNES_MiSTer-v2.6.13.rbf` contains no `_20`. Three consequences
      follow, none cosmetic: every released version is a **separate core entry**
      rather than versions of one; the entry is named
      `RustyNES_MiSTer-v2.6.13` rather than `RustyNES`; and ordering between
      them is alphabetic on the version string, so **v2.6.9 sorts after
      v2.6.13**. Under `RustyNES_20260903.rbf` all builds group under one
      `RustyNES` entry ordered by datecode, which is what `rbf_hide_datecode`
      then hides.

      It does **not** affect the Home folder, which comes from `CONF_STR`'s
      first field and is already `RustyNES` (v2.6.7). And it has no effect until
      submission, because nothing harvests `releases/` until the core is
      accepted.

      **RESOLVED v2.6.15, and the decision was extended rather than reversed.**
      `releases/` now carries `RustyNES_YYYYMMDD.rbf`, which is the path
      `Distribution_MiSTer` reads out of the repository; the version-named copy
      is uploaded alongside it to the GitHub releases, which no upstream tool
      parses. Two names, because there are two audiences and only one of them is
      a parser.

      **And the cost was worse than this entry recorded.** It said the naming
      "has no effect until submission". It has no effect until submission and
      then it has a total one: `Distribution_MiSTer`'s builder strips a date by
      taking the stem's last nine characters, requires `_` plus exactly eight
      digits, and `continue`s — skips outright — any file that yields no date.
      A version-named bitstream is invisible to it, so an accepted core would
      appear in the Cores table and **ship nothing, with no error anywhere**.

      The datecode comes from the tag's commit rather than `date`, so rebuilding
      a tag reproduces its filename. `tb/check_rbf_name.py` carries both
      parsers' rules with nine mutations, including the name this repository
      shipped through v2.6.14, and CI runs it over every committed bitstream.
      v2.6.14's artifact was renamed rather than rebuilt — same bytes.
- [x] Unique **Home folder** chosen (non-arcade requirement) **(v2.6.7)** —
      **RESOLVED against `Main_MiSTer`'s own source**, which is the only place
      that actually states it. `user_io.cpp`'s `user_io_get_confstr(0)` returns
      the text up to the **first** semicolon and `user_io_read_core_name()`
      assigns that as the core name; the MkDocs *Core Paths* page then says the
      standard path is `/media/fat/games/<CORE>`, "where `<CORE>` is the
      internal core name". So `CONF_STR`'s opening `"RustyNES;;"` already gives
      a Home folder of `/media/fat/games/RustyNES`, and it is unique — the
      incumbent NES core's internal name is `NES`.

      The empty field between the two semicolons is simply the **next entry**
      being empty, not a directory field. That was the thing the earlier note
      could not establish: the MkDocs `developer/conf_str` page documents every
      entry type (`F`, `O`, `R`, `J`, `V`) and says nothing about the first
      line, and the `Main_MiSTer` wiki page for it does not render. Reading an
      existing console core's `CONF_STR` was unavailable under ADR 0037, so the
      answer came from the framework's parser instead — which is a better
      source than an example anyway.
- [x] **No MRA files** **(now)** — arcade-only, and including them would be wrong

## Licence

- [x] GPL-3.0-or-later **(now)** — and forced upward rather than chosen:
      `hps_io.sv` is GPL-3.0-or-later and not optional. v2.4.3 audit, 57 files,
      zero GPL-2.0-only

## Quality bar

- [x] Core is accurate enough to demonstrate **preservation value** **(v2.6.14)**
      — measured rather than asserted, and by two independent surfaces. The
      AccuracyCoin status vector is identical to the oracle's **entry for entry
      across all 146 entries**, with 146 of 146 executed on both sides and none
      `NotRun` (v2.6.5). Six commercial titles render **byte-identically over
      all 61,440 pixels** (v2.6.11), published as a montage whose build script
      refuses to publish a tile that differs from the oracle. blargg's 2005 APU
      battery is 11/11 and `instr_test-v5` 16/16.
- [x] AccuracyCoin result stated as a **floor**, entry-for-entry, including
      `Skipped`/`NotRun` **(v2.6.5)** — stated in `RustyNES_MiSTer/README.md`,
      which a reviewer reaches first: "the vector is now IDENTICAL entry for
      entry across all 146, with 146 of 146 executed on both sides and none
      `NotRun`". Stronger than a floor, and reported as a **vector** rather than
      a pass count precisely so a regression cannot hide behind an unchanged
      total.
- [ ] Runs on **real hardware**, both boards, one `.rbf`
      **BLOCKED — no board.** Neither a DE10-Nano nor a SuperStation One is
      attached to this machine, confirmed by checking the USB bus, serial
      devices, removable block devices and mounts rather than assumed. Nothing
      in this repository can close it. **Unblocks on hardware.**
- [ ] **AI-generated-code bar:** readability, plus *"evidence of quality and
      accuracy testing"* — the co-simulation record is that evidence, and the
      submission should link it explicitly rather than assume a reviewer finds it
      **BLOCKED — on the submission itself.** **The evidence is now also
      ARGUED rather than merely linkable (v2.6.15):**
      `RustyNES_MiSTer/docs/submission-case.md` is the document the email will
      point at — what is different about this core and how a reviewer verifies
      it in one line of a table they already maintain, the ladder as the answer
      to the AI-assistance question, the `StudioII_MiSTer` and `PCXT-EGA_MiSTer`
      precedents, and what is NOT claimed stated first rather than buried.
      Written before the submission deliberately, so its claims could be checked
      against the repository while there was still time to find one that does
      not hold. The evidence exists and is
      linkable today (`docs/rung1-6502.md` through `docs/rung7-mappers.md`, the
      142-gate suite, the mutation records). What is missing is the act of
      pointing a reviewer at it, which happens in the submission email.
      **Unblocks at v2.7.0.**

## Provenance

- [x] `docs/provenance.md` states the firewall **(v2.6.14)** — §"The firewall,
      extended to HDL (ADR 0037)" names `NES_MiSTer`, `fpganes` "and any other
      NES `rtl/`" as strict black boxes, and §"If you do derive" separates a
      measurement from a licence event: *"Matches NES_MiSTer's output on this
      test" is a measurement; "adapted from NES_MiSTer" is a licence event.*

      **The second half of this item is STRUCK, not satisfied.** It used to read
      "and that no NES core was ever opened", and that is a claim the same
      document forbids: §"Do not self-certify" says *never assert "no
      third-party code is incorporated" or "licence-clean" as a finished claim
      ... surface provenance status for human and expert review, and state
      uncertainty.* The box as written could only have been ticked by writing
      the one sentence the project's provenance rules exist to prevent. Found by
      the v2.6.14 audit; the firewall half is what was actually being asked for.
- [x] CI provenance job green — no black-boxed core in the tree **(v2.6.14)** —
      green on `main`, and the job is two checks rather than one: a `find` for a
      `NES_MiSTer` or `fpganes` directory (`ok: no black-boxed NES core
      directory in the tree`), and an SPDX sweep. It cannot stop someone reading
      a reference core — nothing in CI can — only stop the result of having read
      one landing unnoticed, and it says so in its own comment.
- [x] Every RTL file carries its SPDX header **(v2.6.14)** — `ok: 31 RTL files,
      all carrying the SPDX header` on `main`. **Fail-closed**: zero files
      examined is an error, not a pass, and the `find` parentheses are
      load-bearing for the same reason — without them `-print0` binds only to
      the last `-o` branch, so `.sv` files match and are never printed.

## Submission

Every item here is **BLOCKED — the submission IS v2.7.0**, by the programme's own
definition, and three of the four are somebody else's action rather than this
project's. They are listed so the sequence is visible, not because they are
outstanding work.

- [ ] Email `newcores@misterfpga.org` with the repository link
      **BLOCKED — v2.7.0.** Sending it before the quality bar closes is the
      whole thing the checklist exists to prevent.
- [ ] Await review (the page says days)
      **BLOCKED — not ours to do**, and it follows the email.
- [ ] **Decide deliberately** on the MiSTer-devel invitation and repository
      transfer — acceptance moves the repo, it is one-way, and this project owns it
      **BLOCKED — on being accepted**, and then it is a maintainer decision
      rather than a task. Named here so acceptance does not arrive as a
      surprise with a one-way consequence attached.
- [ ] Add to the Cores list with the Home folder
      **BLOCKED — on acceptance.** The Home folder itself is already settled:
      `CONF_STR`'s first field gives `/media/fat/games/RustyNES`, and it is
      unique — the incumbent core's internal name is `NES` (v2.6.7).

## If declined as a duplicate

Not a failure path — a planned one. See
`ref-docs/2026-08-23-alternative-fpga-targets.md`.

- [ ] Retro Remake / SuperStation One — already the hardware target
      **CONTINGENT — on being declined**, and cheap if it happens: the
      SuperStation One is already the second hardware target of rung 6, with
      128 MB of integrated SDRAM against the DE10-Nano's add-on.
- [ ] openFPGA / Analogue Pocket — demonstrated MiSTer-core porting path;
      `nes_top.sv` stays platform-agnostic precisely so this stays cheap
      **CONTINGENT — on being declined.** The design decision that keeps it
      cheap is already taken and holds today: `nes_top.sv` carries no MiSTer
      framework dependency, and `emu.sv` is the only file that does.
- [ ] The co-simulation evidence is publishable on its own terms regardless
      **DECIDED — this is a statement, not a task**, and it is unconditional:
      the ladder, its goldens and its mutation records stand whatever any
      distribution decides, which is why it is written down rather than left as
      a consolation.
