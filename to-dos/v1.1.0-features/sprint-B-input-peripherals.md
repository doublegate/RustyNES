# v1.1.0 · Sprint B — Input, peripherals & QoL  → beta.1

Extension points: `crates/rustynes-core/src/input_device.rs` (the `InputDevice` enum,
currently `Zapper`, `Vaus`), `crates/rustynes-frontend/src/input.rs`, `config.rs`,
the mapper parse path in `crates/rustynes-mappers/src/lib.rs`, and the cheat panel.

## T-110-B1 — New controllers (Power Pad, Family BASIC keyboard)

- Add `InputDevice` variants implementing `write_strobe`/`read`/`peek`; opt-in per
  port so the default controller path stays byte-identical.
- **Refs:** `ref-proj/Mesen2/.../Input/PowerPad.h`, `FamilyBasicKeyboard.h`.
- **Done when:** selectable per port in settings; determinism preserved when unset.

## T-110-B2 — Turbo / autofire

- Frame-counter gating in `input.rs` + a `[input.turbo]` config block (per-button
  period). **Done when:** configurable; off by default.

## T-110-B3 — Input-display overlay

- New egui panel polling `InputState` (button/dpad grid per player). Read-only.
- **Done when:** toggleable overlay; useful for TAS/streaming.

## T-110-B4 — ROM / game database + per-game overrides + Game Genie code DB

- CRC/SHA-keyed data file applying mirroring/mapper/region/palette fixes at parse
  time (`rustynes-mappers` parse path), plus a Game Genie code-name lookup in the
  cheat panel.
- **Refs:** `ref-proj/Mesen2/.../GameDatabase.cpp`, `ref-proj/nestopia/.../NstImageDatabase.cpp`.
- **Done when:** known problem ROMs auto-corrected; overrides are data, not code;
  the oracle set stays byte-identical (overrides only apply to listed CRCs).

## Verification
- Determinism: default controller path byte-identical with new devices unset.
- AccuracyCoin/oracle unaffected; new-device unit tests; config round-trip tests.
