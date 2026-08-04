# Provenance, Derivation, and License

This document is the honest record of where RustyNES's code comes from. It exists
because earlier versions of this file, of `NOTICE`, and of the in-source comments
got the provenance **wrong** — they described code that was ported from other
emulators as "oracle cross-checks" and licensed the whole project under a
permissive MIT/Apache license it was not entitled to use. A NESdev community
review (thanks to Fiskbit and the NESdev staff) was correct on the substance, and
this document, the relicense to GPLv3, and the attribution below are the
correction.

The short version:

- **RustyNES incorporates and is derived from code from GPL-licensed emulators**,
  principally **Mesen2** (GPL-3.0-or-later) and, for several mappers and the FDS
  drive model, **puNES** / **FCEUX** / **Nestopia** (GPL-2.0-or-later). This is not
  oracle use; it is derivation. The original source comments said so ("Faithful
  port of Mesen2's `ProcessSpriteEvaluation`", "Ported bit-for-bit from puNES
  `JV001.c`", etc.) before a v2.2.5 edit reworded them.
- **RustyNES is therefore a derivative work and is licensed
  [GPL-3.0-or-later](../LICENSE).** The earlier "MIT OR Apache-2.0" dual license
  and the "no GPL code is incorporated" claim were incorrect and are withdrawn.
- **Credit is given below and in `NOTICE`**, per subsystem, to the projects the
  code was derived from.
- Some parts of RustyNES *are* genuinely original — the crate topology, the
  determinism contract, the CI accuracy-honesty gates, the measure-first
  performance record. Those claims are kept, but they never justified calling the
  whole project "not a port," and they do not exempt the derived code from the GPL.

> **A note on AI assistance.** RustyNES is heavily AI-assisted software. That does
> not change any of the above: code an LLM emits by reproducing GPL source is still
> GPL-derived, and the human directing the tool is responsible for what lands in the
> tree. "Laundering others' code through an AI" — the reviewer's phrase — is exactly
> the failure mode this document exists to correct, not excuse.

Authoritative companions: [`NOTICE`](../NOTICE) (the legal attribution file),
[`docs/adr/0036-relicense-gplv3-derivative-work.md`](adr/0036-relicense-gplv3-derivative-work.md)
(the decision record for this relicense), `CHANGELOG.md`, and
`tests/roms/LICENSES.md` (test-ROM provenance).

---

## 1. What is derived from GPL-licensed emulators

The table below is the honest derivation record, rebuilt from the in-source
comments as they stood **before** the v2.2.5 rewording (recoverable from the git
history of that change) and cross-checked against the sources in `ref-proj/`. Each
row is code in RustyNES that was ported, adapted, or closely modeled from the named
GPL emulator — not merely behavior observed and reimplemented from documentation.
"Source license" is the license the upstream file carries; because every upstream
here is GPL-2.0-**or-later** or GPL-3.0-**or-later**, all of it is compatible with
distributing the combined work under GPL-3.0-or-later.

| RustyNES file | Derived from | Upstream source | Upstream license |
| --- | --- | --- | --- |
| `crates/rustynes-cpu/src/cpu.rs` | Mesen2 | `SyaSxaAxa` unstable-store opcodes, `Core/NES/NesCpu.h` | GPL-3.0-or-later |
| `crates/rustynes-ppu/src/ppu.rs` | Mesen2 | `ProcessSpriteEvaluation` (`NesPpu.cpp:1015-1141`), `ReadSpriteRam`, the OAM-data-bus / sprite-evaluation read paths | GPL-3.0-or-later |
| `crates/rustynes-ppu/src/palette_gen.rs` | Bisqwit; ares | Bisqwit NES palette method; ares `fc/ppu/color.cpp` integration | Bisqwit (see §6); ares BSD-2/Apache-2.0 |
| `crates/rustynes-apu/src/blip.rs` | blip_buf (Blargg) | band-limited synthesis (`blip_buf`) | LGPL-2.1-or-later |
| `crates/rustynes-apu/src/opll.rs` | emu2413 (upstream MIT; Mesen2 vendors it) | `emu2413.{h,cpp}` | MIT |
| `crates/rustynes-frontend/src/ntsc_bisqwit.rs` | Bisqwit; Mesen2 | Bisqwit `nes_ntsc`-style composite model as implemented by Mesen2's `BisqwitNtscFilter`; **numeric tables ported verbatim** | GPL-3.0-or-later (Mesen2) |
| `crates/rustynes-gfx-shaders/src/crt_stack.rs`, `src/lib.rs` | CRT-Royale, crt-guest-advanced, Sony Megatron | single-pass WGSL reimplementations of those shaders (see §6) | GPL-2.0-or-later / permissive |
| `crates/rustynes-mappers/src/m016_bandai_fcg.rs` | Mesen2 | `Eeprom24C01` / `Eeprom24C02`, `Core/NES/Mappers/Bandai/` | GPL-3.0-or-later |
| `crates/rustynes-mappers/src/m035_jy_asic.rs` | Mesen2 | `JyCompany` register decode | GPL-3.0-or-later |
| `crates/rustynes-mappers/src/m069_sunsoft_fme7.rs` | Mesen2 / Nestopia | Sunsoft 5B audio + FME-7 | GPL-3.0-or-later / GPL-2.0-or-later |
| `crates/rustynes-mappers/src/m176_bmc_fk23c.rs` | Mesen2 | `Waixing/Fk23C.h` | GPL-3.0-or-later |
| `crates/rustynes-mappers/src/m268_bmc_coolboy.rs` | Mesen2 / FCEUX | `Mmc3Variants/MMC3_Coolboy.h` banking | GPL-3.0-or-later / GPL-2.0-or-later |
| `crates/rustynes-mappers/src/m513_sachen_9602.rs` | Mesen2 | `Sachen/Sachen9602.h` | GPL-3.0-or-later |
| `crates/rustynes-mappers/src/mmc3_clones.rs` | Mesen2 | `Waixing/Mapper253.h`, `InvertPrgBits`, MMC3 variants | GPL-3.0-or-later |
| `crates/rustynes-mappers/src/multicart_discrete.rs` | Mesen2 | `Ntdec/Mapper221.h`, `Txc/Bmc11160.h` | GPL-3.0-or-later |
| `crates/rustynes-mappers/src/ntdec.rs` | Mesen2 | NTDEC boards | GPL-3.0-or-later |
| `crates/rustynes-mappers/src/sachen_discrete.rs` | Mesen2 | `Sachen/Sachen8259.h`, `Txc/TxcChip.h` | GPL-3.0-or-later |
| `crates/rustynes-mappers/src/kaiser.rs` | Mesen2 | Kaiser boards | GPL-3.0-or-later |
| `crates/rustynes-mappers/src/fds.rs` | puNES | `fds.c` per-CRC drive-timing table | GPL-2.0-or-later |
| `crates/rustynes-mappers/src/lib.rs` (mapper 147 / JV001, UNIF dispatch) | puNES; FCEUX | `JV001.c` / `mapper_147.c` (**ported bit-for-bit**); UNIF board handling | GPL-2.0-or-later |
| `crates/rustynes-mappers/src/unif.rs` | Mesen2; FCEUX | `UnifLoader.cpp` + `unif.cpp` board-name tables | GPL-3.0-or-later / GPL-2.0-or-later |
| `crates/rustynes-frontend/src/debugger/source_map.rs` | Mesen2 | `DbgImporter` / `NesDbgImporter` | GPL-3.0-or-later |
| `crates/rustynes-test-harness/src/bin/pgo_trainer.rs` | Mesen2 | `PGOHelper` corpus-sweep harness | GPL-3.0-or-later |

This list is maintained as the derivation is audited further; if additional
GPL-derived code is found, it is added here and in `NOTICE` rather than reworded
away. Beyond the files above, the reviewer specifically noted that bugs, constants,
variable names, and code ordering can carry provenance even without a comment —
where that is true of any code in this tree, it is GPL-derived and covered by the
GPL-3.0-or-later license of the whole.

---

## 2. License: GPL-3.0-or-later, because RustyNES is a derivative work

RustyNES is licensed **GPL-3.0-or-later** ([`LICENSE`](../LICENSE)). This is not a
preference; it is a requirement that follows from §1. Incorporating GPL-3.0
(Mesen2) and GPL-2.0-or-later (puNES/FCEUX/Nestopia, all granting "or any later
version") code makes the combined work a derivative that can only be distributed
under the GPL. GPL-3.0-or-later is the correct expression: the GPL-2.0-or-later
material upgrades to v3, and Mesen2/higan are GPL-3.0-or-later.

The earlier **MIT OR Apache-2.0** dual license was wrong for this codebase and is
withdrawn. The `LICENSE-MIT` and `LICENSE-APACHE` files are removed. Source
released under the old license in prior tagged releases remains under whatever
terms accompanied it at the time — that history cannot be retroactively changed —
but the current tree, and every release from v2.2.9 onward, is GPL-3.0-or-later.

Permissively-licensed components that RustyNES genuinely incorporates
(emu2413/MIT, TriCNES/MIT, rcheevos/MIT, blip_buf/LGPL-2.1-or-later, bundled
fonts) keep their own licenses; each is GPL-compatible and is attributed in
`NOTICE`. Combining them under the project's GPL-3.0-or-later umbrella is what
those licenses permit.

The `cargo-deny` license gate (`deny.toml`) allows `GPL-3.0-or-later` for the
project's own crates alongside the permissive licenses of the dependency graph.

---

## 3. The reference emulators still consulted as oracles

Separately from the derived code in §1, RustyNES also *does* use emulators as
behavioral oracles — running them to observe documented hardware behavior when a
test ROM is ambiguous, without deriving code. The distinction is real, but the
earlier documents abused it by filing genuine ports under this heading. The
honest position is: some use was oracle-only, and some was derivation (§1), and
this project previously mislabeled the second as the first.

| Reference emulator | License | Documented use |
| --- | --- | --- |
| Mesen2 / MesenCE | GPL-3.0-or-later | Derivation (§1) **and** oracle |
| puNES | GPL-2.0-or-later | Derivation (§1) **and** oracle |
| FCEUX | GPL-2.0-or-later | Derivation (§1) **and** oracle |
| Nestopia UE | GPL-2.0-or-later | Derivation (§1, FME-7/5B) **and** oracle |
| GeraNES | GPL-3.0-only | Oracle / cross-check only (no code derived) |
| higan | GPL-3.0-or-later | Scheduler-structure reference / oracle |
| ares | BSD-2-Clause / Apache-2.0 | Palette-integration reference (§1) / oracle |
| TriCNES | MIT | Incorporated (§5) **and** timing-calibration reference (§4) |

Because the license of the derived-from GPL code governs regardless of how any
one file was used, the whole project is GPL-3.0-or-later; the oracle/derivation
distinction affects attribution, not the license.

---

## 4. What is genuinely RustyNES's own

These claims are true and are kept — but they describe original *architecture and
method built around* the incorporated code, not a clean-room emulator. Owning the
derivation in §1 does not require pretending the surrounding system is not real
work; it requires not overstating it into a "not a port" claim, which is what the
earlier document did.

- **The crate topology and ownership model.** The strictly one-directional
  `rustynes-{cpu,ppu,apu,mappers,core}` graph, the Bus-owns-all-mutable-state
  design, and the narrow per-chip trait boundaries are RustyNES's own structure
  (`docs/architecture.md`).
- **The determinism contract and the `#![no_std]` core.** Same seed + ROM + input
  ⇒ bit-identical framebuffer and audio, enforced by the `thumbv7em-none-eabihf`
  no-default-features cross-compile in CI. This is a design discipline, not code
  taken from any emulator.
- **The one-clock, every-cycle-bus-access timebase (ADR 0029).** The single-cycle
  counter and split-around-access `start_cycle`/`end_cycle` PPU catch-up are
  RustyNES's implementation. It is conceptually similar to Mesen2's cycle-stepped
  approach (and, given §1, some of the surrounding NES code is Mesen2-derived), but
  the scheduler substrate itself is original design.
- **Machine-checked accuracy honesty (ADR 0011).** The Core/Curated/BestEffort
  mapper tiering, the `snapshot_schema_audit` field-vs-schema gate, and the
  build-fails-not-the-reader honesty posture are the project's own contribution.
- **Measure-first performance with published rejections.** `docs/performance.md`
  records optimizations that were measured and *rejected* with their numbers — an
  unusual discipline that is genuinely the project's own.
- **The 2-cycle-ALE octal-latch PPU model and its honest caveat (ADR 0030).** The
  physical octal-latch model was an independent modeling choice, but — as already
  disclosed in v2.2.6 and retained here — its *timing* was calibrated to TriCNES
  (MIT) rather than derived from an independent measurement, which is why RustyNES
  reproduced TriCNES's Rad Racer hybrid-address artifact. The v2.3.0 "Datum II"
  work reworks this to be documentation-derived. TriCNES is MIT-licensed, so this
  is an attribution/fidelity matter, not a GPL one.

---

## 5. Incorporated permissive components

Genuinely incorporated, each GPL-compatible and attributed in `NOTICE`:

| Component | License | Copyright | Where |
| --- | --- | --- | --- |
| emu2413 v1.5.9 | MIT | 2020 Mitsutaka Okazaki | `crates/rustynes-apu/src/opll.rs` (Rust port; VRC7 audio, ADR 0006) |
| TriCNES (commit 9199870) | MIT | 2025 Chris Siebert | `crates/rustynes-{ppu,cpu,core}` (ported models) + vendored golden oracle |
| rcheevos v12.3.0 | MIT | 2018 RetroAchievements.org | `crates/rustynes-cheevos/vendor/rcheevos/` (optional `retroachievements` feature) |
| blip_buf | LGPL-2.1-or-later | Shay Green (Blargg) | `crates/rustynes-apu/src/blip.rs` (band-limited synthesis; GPLv3-compatible) |
| Font Awesome Free / bundled fonts | their own licenses (OFL-1.1 etc.) | respective authors | `crates/rustynes-frontend/assets/fonts/` |

MIT, ISC, BSD, and LGPL-2.1-or-later are all compatible with GPL-3.0-or-later, so
incorporating them into the GPL project is permitted; their own notices are
preserved in `NOTICE`.

---

## 6. Video shaders and NTSC-decode filters

The CRT shader stack (`crates/rustynes-gfx-shaders/`) and the NTSC-decode filters
(`ntsc_bisqwit.rs`, `ntsc_lmp88959.rs`) reproduce the look of community shaders —
CRT-Royale (TroggleMonkey, GPL-2.0-or-later), crt-guest-advanced (guest.r), Sony
Megatron (MajorPainInTheCactus), Bisqwit's NES composite model, and EMMIR's
NTSC-CRT. These were reviewed at the source level and reimplemented as single
fullscreen WGSL passes on RustyNES's own uniform/pipeline conventions.

Two honest points here, corrected from the earlier document:

- The Bisqwit NTSC filter's **numeric tables were ported verbatim** (the original
  comment said so). That is derivation, listed in §1. The two-level composite
  *signal shape* is documented at the NESdev wiki, but the specific coefficient
  tables came from Bisqwit's C as carried by Mesen2, so the GPL applies.
- The CRT shaders are single-pass reimplementations rather than translations of the
  upstream multi-pass sources, and copyright does not protect a visual look. But
  since the whole project is now GPL-3.0-or-later anyway, and CRT-Royale is itself
  GPL-2.0-or-later, this is moot for licensing — they are credited as influences in
  `NOTICE` and the project's license covers them regardless.

All of these features are optional and default-off and do not affect the
deterministic emulation core or its AccuracyCoin results.

---

## 7. Test ROMs

Every ROM committed under `tests/roms/` is a public-domain work released for
validating NES emulators, catalogued per-author in `tests/roms/LICENSES.md`
(blargg's suites, kevtris/AccuracyCoin material, and others). **No commercial
Nintendo software is bundled**; users test commercial dumps they own from the
gitignored `tests/roms/external/`. The AccuracyCoin battery is MIT-licensed
(Chris Siebert / 100thCoin).

---

## 8. The correction, owned

For the record, because the reviewer was right that scrubbing the comments looked
like sweeping this under the rug:

- The in-source comments originally, and correctly, described this code as ports of
  Mesen2 / puNES / FCEUX (with file, function, and line-number references).
- v2.2.5 "Colophon" reworded those comments to call the same code "oracle
  cross-checks" and asserted "No GPL-licensed emulator source is incorporated."
  **That assertion was false**, and the rewording obscured the provenance rather
  than clarifying it.
- v2.2.9 corrects this the right way: it (a) relicenses the project to
  GPL-3.0-or-later, (b) states plainly that other emulators' code was incorporated
  during implementation, (c) credits every derived-from source here and in
  `NOTICE`, and (d) marks each derived source file with an accurate
  `SPDX-License-Identifier: GPL-3.0-or-later` header and a specific provenance note
  (naming its upstream file/function) that points back to the §1 table. The old
  scattered, imprecise per-line "port of" comments are not restored verbatim — the
  SPDX + provenance headers plus this audited table are their accurate,
  discoverable replacement.

Responsibility for what the AI tooling put into this codebase, and for the earlier
mislabeling, rests with the project. This document is the correction of record.
