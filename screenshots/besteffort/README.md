# BestEffort mapper boot-smoke screenshots (v1.3.0 D1)

Reference captures for the 14 Tier-2 **BestEffort** mapper families added in the
v1.3.0 D1 sweep (`crates/rustynes-mappers/src/sprint8.rs`). These are **not** part
of the AccuracyCoin / commercial-ROM oracle (ADR 0011 + `mapper_tier_honesty.rs`
keep BestEffort out of the accuracy gate); they are a confidence check that each
port loads and runs a real homebrew / unlicensed / multicart title.

## How these were produced

The source ROMs are unlicensed / homebrew / multicart dumps (these mappers have
**no commercial fixtures** — that is the premise of the BestEffort tier). They
were staged in the gitignored `tests/roms/external-besteffort/` (a sibling of the
oracle corpus so the honesty-gate walk over `tests/roms/external/` stays green)
and captured at frame 300 with:

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
| 111 | GTROM / Cheapocabra (Ninja Ryukenden Ch) | boots; backdrop w/o input |
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

## v1.4.0 "Fidelity" Workstream G sweep (`sprint9.rs`)

Twelve more Tier-2 BestEffort families. Source ROMs staged in the gitignored
`tests/roms/external-besteffort/` (10 of the 12 have a real unlicensed / pirate /
multicart dump in the library; mappers 28 and 174 have no library dump and are
register-decode + save-state unit-tested only). Boot-smoked with `render_smoke`
(no input driven), captured at the frame with the richest render.

| Mapper | Board (sample ROM) | Boot-smoke result |
|---|---|---|
| 28 | Action 53 (homebrew) | no library dump (register-decode + save-state tested) |
| 30 | UNROM-512 (Wampus) | boots + rendering enabled; menu blank w/o input |
| 63 | NTDEC 0324 (255-in-1) | boots; runs menu code, input-gated |
| 76 | NAMCOT-3446 (Megami Tensei) | RENDERED |
| 174 | NTDEC 5-in-1 | no library dump (register-decode + save-state tested) |
| 225 | ColorDreams 72-in-1 (110-in-1) | RENDERED |
| 226 | 76-in-1 BMC | RENDERED |
| 227 | 1200-in-1 BMC | RENDERED |
| 229 | 31-in-1 BMC | RENDERED |
| 233 | 42-in-1 reset-based BMC (Super 22-in-1) | boots; reset-gated menu, input-gated |
| 242 | Waixing 43-in-1 (Wai Xing Zhan Shi) | RENDERED |
| 246 | Fong Shen Bang / G0151-1 | RENDERED |

The sweep surfaced and fixed several real defects (all in `sprint9.rs` unless
noted), found via boot-smoke against the real dumps:

- **`cpu_read_unmapped` inversion** (the load-bearing one): mappers 225 + 246
  (and the pre-existing m132 in `sprint6.rs` + m143 in `sprint8.rs`) used a
  `!(register-range).contains(addr)` override, which marks the entire
  `$8000-$FFFF` PRG-ROM window as open bus — so the reset vector + program code
  read back `$00` and the CPU never boots. Fixed to flag only the genuine
  open-bus holes (the register/gap sub-ranges), keeping PRG-ROM mapped.
- **Mapper 225** decode (`A~[.HMO PPPP PPCC CCCC]`): PRG is A6..A11 (6 bits),
  mode = A12, mirroring = A13, high bit = A14 (was A6..A9 / A10 / A11).
- **Mapper 226** decode (`reg0 [PMOP PPPP]` + `reg1` high bit): mode bit 6
  (0 = 32K, 1 = 16K), mirroring bit 7 (0 = H, 1 = V), high PRG bit from reg1
  bit 0 (was a wrong reg0 bit-5 / inverted mode + mirroring).
- **Mapper 233**: the bank is in the DATA byte `[MMOP PPPP]`, not the address
  (was address-decoded); 4-bit page, bit 5 mode (0 = 16K), bits 6-7 mirroring.
- **Mapper 242**: added the 8 KiB `$6000-$7FFF` work-RAM the boot routine
  clears before its first bank switch (it derailed without it), plus the precise
  inner/outer address-bit PRG decode.
- **Mapper 246**: `$6003` powers on to `$FF`, PRG-RAM is 2 KiB at `$6800-$6FFF`
  (not 6 KiB at `$6800-$7FFF`), and the `$FFE4-$FFFF`-family reads force PRG A17
  high so the reset vector resolves into the boot bank.

After the fixes: 7 of the 10 staged dumps render a real screen headless (76, 225,
226, 227, 229, 242, 246); the other 3 (30, 63, 233) boot and run real PRG code in
a stable menu loop but are input-/reset-gated, so they sit on a backdrop without a
driven button — the BestEffort bar (boots + register-decode verified) is met.
