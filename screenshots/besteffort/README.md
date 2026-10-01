# BestEffort mapper boot-smoke screenshots (v1.3.0 D1)

> **v2.9.8: this directory is empty, and that is correct.** Every family that
> had a capture here has since been promoted to Core or Curated (the v2.1.0
> "Fathom" sweep, then GTROM in v2.9.6), and `coverage.py categorize` moves a
> directory to `external/` with its tier. It was not re-run after those
> promotions until v2.9.8, which also fixed its `tier.rs` parser (it read only
> the first arm per tier, so v2.9.6's second Curated arm went unclassified).
> None of today's 31 BestEffort families has a committed capture. The matrix
> below is the v1.3.0 record and names directories now under `external/`.

Reference captures for the 14 Tier-2 **BestEffort** mapper families added in the
v1.3.0 D1 sweep (`crates/rustynes-mappers/src/sprint8.rs`). These are **not** part
of the AccuracyCoin / commercial-ROM oracle (ADR 0011 + `mapper_tier_honesty.rs`
keep BestEffort out of the accuracy gate); they are a confidence check that each
port loads and runs a real homebrew / unlicensed / multicart title.

## How these were produced

The source ROMs are unlicensed / homebrew / multicart dumps (these mappers have
**no commercial fixtures** — that is the premise of the BestEffort tier). They
are staged in the gitignored `tests/roms/external/` alongside every other dump
(the coverage harness discovers a single `external/` ROM tree; the honesty gate
stays green because it keys on each mapper's *tier* in `tier.rs`, not the
directory). Only the **screenshots** are tier-split — BestEffort captures land
here in `screenshots/besteffort/` (Core/Curated go to `screenshots/external/`)
via `coverage.py categorize`. The original boot-smoke captures used frame 300:

```text
cargo run -p rustynes-test-harness --features commercial-roms --bin render_smoke \
    -- "<rom>" 300 screenshots/besteffort/mapper-NNN-NAME.png
```

**The ROMs themselves are never committed.** `render_smoke` drives **no input**,
so input-driven titles (press-Start menus) legitimately sit on a backdrop /
attract screen — a backdrop-only frame here means "boots without crashing", not
"broken".

## Boot-smoke matrix

| Mapper | Board (sample ROM) | Frame-300 result |
|---|---|---|
| 58 | multicart (116-in-1) | RENDERED |
| 60 | reset 4-in-1 multicart | RENDERED |
| 94 | UN1ROM (Senjou no Ookami) | boots; blank w/o input — flagged for input-driven re-test |
| 101 | Jaleco JF-10 (Urusei Yatsura) | RENDERED |
| 107 | Magic Dragon (Unl) | boots; backdrop w/o input |
| 111 | *Ninja Ryukenden* (Ch), which is not GTROM | **not supported since v2.9.6.** The image carries CHR-ROM: it is the MMC1 variant the GTROM page lists under "Variations", which ran as GTROM and jammed after 26 CPU cycles. The committed PNGs show that jam. GTROM itself is Curated and pinned by a CC0 fixture (`roster_boards.rs`) |
| 143 | Sachen TCA01 (Dancing Blocks) | boots; blank w/o input |
| 177 | Hengedianzi (American Man) | boots; backdrop w/o input |
| 218 | Magic Floor (homebrew) | boots (D1 PRG-size bug fixed; minimal-puzzle backdrop) |
| 231 | 20-in-1 multicart | RENDERED |
| 234 | Maxi 15 (AVE) | RENDERED |
| 29 | Sealie CUFROM | no ROM in library (homebrew, absent) |
| 31 | INL / 2A03 Puritans | no ROM in library (homebrew, absent) |
| 179 | Hengedianzi (variant) | no ROM in library (absent) |

The boot-smoke surfaced one real D1 defect — mapper 218 rejected its own 16 KiB
PRG (it required a 32 KiB multiple); fixed to accept + mirror 16 KiB NROM-128-style
(`sprint8.rs`, regression test `m218_accepts_16k_prg_and_mirrors_it`).
Input-driven re-capture of the backdrop-only boards is future work; they are
register-decode + save-state unit-tested and boot without panicking, which is the
BestEffort bar.
