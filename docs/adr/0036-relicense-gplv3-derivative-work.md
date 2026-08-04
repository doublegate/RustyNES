# 36. Relicense to GPL-3.0-or-later: RustyNES is a derivative work of GPL emulators

Date: 2026-08-04

## Status

Accepted. **Corrects and supersedes** the license and provenance position taken in
[ADR-adjacent] `docs/originality-and-provenance.md` and `NOTICE` as they stood after
v2.2.5 "Colophon" (which asserted MIT/Apache-2.0 licensing and "no GPL emulator source
incorporated"). Changes the project license from `MIT OR Apache-2.0` to
`GPL-3.0-or-later`.

## Context

RustyNES's chip, mapper, PPU sprite-evaluation, NTSC-filter, and tooling code
contains material that was ported, adapted, or closely modeled from GPL-licensed
emulators. This is documented by the project's own in-source comments as they stood
before v2.2.5 — e.g. "Faithful port of Mesen2's `ProcessSpriteEvaluation`
(`NesPpu.cpp:1015-1141`)", "Ported bit-for-bit from puNES `JV001.c`", "numeric tables
ported verbatim from Bisqwit's C", and roughly a dozen "Ported from Mesen2
`<file>.h`" mapper comments. The full file-by-file record is in
`docs/originality-and-provenance.md` Section 1.

v2.2.5 "Colophon" reworded those comments to describe the same code as "behavioral
oracle cross-checks," rewrote `NOTICE` to state "No GPL-licensed emulator source is
incorporated," and kept the permissive `MIT OR Apache-2.0` license. A NESdev
community review (Fiskbit and NESdev staff) identified that this was incorrect: the
code carries bugs, constants, variable names, code ordering, and file/function/line
references that go well beyond oracle use, and scrubbing the "port" comments obscured
the provenance rather than fixing it. The reviewer was right.

The derived-from upstreams and their licenses:

- **Mesen2 / MesenCE** — GPL-3.0-or-later (extensive: CPU unstable stores, PPU
  sprite-eval/OAM model, ~15 mapper boards, EEPROM models, Bisqwit NTSC filter, UNIF
  tables, debug-symbol importer, PGO harness).
- **puNES** — GPL-2.0-or-later (JV001 / mapper 147 bit-for-bit, FDS per-CRC drive
  table).
- **FCEUX** — GPL-2.0-or-later (UNIF handling, some mapper banking).
- **Nestopia UE** — GPL-2.0-or-later (FME-7 / 5B audio detail).

Every one of these grants "or (at your option) any later version," so the
GPL-2.0-or-later material is upgradable to v3 and the combination is legally
consistent as a single GPL-3.0-or-later work. GeraNES (GPL-3.0-**only**) was used as
an oracle only, with no code derived, so it does not further constrain the license.

Incorporating GPL code makes the whole combined work a derivative work that can only
be distributed under the GPL. The prior permissive dual-license was therefore not a
license the project was entitled to offer.

## Decision

1. **Relicense the project to `GPL-3.0-or-later`.** `LICENSE` becomes the GPLv3 text;
   `LICENSE-MIT` and `LICENSE-APACHE` are removed; the workspace and per-crate
   `license` fields become `GPL-3.0-or-later`; `deny.toml` allows it for the project's
   own crates.
2. **State the derivation honestly.** `docs/originality-and-provenance.md` is rewritten
   to lead with the derivation table and the derivative-work declaration; `NOTICE`
   attributes each GPL upstream and the code derived from it; the README license and
   provenance text are corrected. The false "no GPL code incorporated" / "not a port"
   claims are withdrawn.
3. **Mark the source, accurately.** Each derived source file carries an
   `SPDX-License-Identifier: GPL-3.0-or-later` header and a specific provenance note
   naming its upstream file/function (e.g. Mesen2 `NesPpu.cpp`, puNES `JV001.c`) and
   pointing to the §1 table. The old scattered, imprecise per-line "port of" comments
   are not restored verbatim — the SPDX + provenance headers plus the centralized
   audited table in `docs/originality-and-provenance.md` + `NOTICE` are their
   accurate, discoverable replacement.
4. **Keep the genuinely-original claims, correctly scoped.** The crate topology,
   determinism contract, CI accuracy-honesty gates, and measure-first performance
   record remain the project's own work — but they describe architecture *around*
   incorporated code and never justified a whole-project "not a port" claim.

The SPDX choice is `GPL-3.0-or-later` (not `-only`) because every derived-from
component is "or-later" and no incorporated component is v3-only.

## Consequences

- **Redistribution terms change.** Downstream users and packagers must comply with the
  GPL: source availability, copyleft on derivatives, and preservation of these notices.
  Distributors who relied on the permissive terms of prior tagged releases keep those
  terms *for those releases* (history is immutable), but everything from v2.2.9 onward
  is GPL-3.0-or-later.
- **Compatibility maintained.** The incorporated permissive components (emu2413/MIT,
  TriCNES/MIT, rcheevos/MIT, blip_buf/LGPL-2.1-or-later, fonts) are all GPL-compatible
  and keep their own notices; combining them under GPLv3 is permitted.
- **Store/distribution implications.** GPLv3 is compatible with F-Droid and direct
  distribution. Apple App Store distribution of GPLv3 software is contested (the App
  Store terms conflict with GPLv3 §6/§10 for some interpretations); any future iOS
  store listing must be evaluated against that, and F-Droid / GitHub-Releases / direct
  IPA distribution are the safe channels. This is noted for the (unversioned, free)
  mobile-listing step referenced in ADR 0035.
- **Ongoing audit.** If further GPL-derived code is found, it is added to the
  provenance table and `NOTICE`, not reworded away. The license does not change again
  for that; GPL-3.0-or-later already covers it.
- **Accuracy unaffected.** This is a licensing/documentation change with zero
  emulation-core behavior change: AccuracyCoin holds 141/141 and nestest is 0-diff by
  construction.
