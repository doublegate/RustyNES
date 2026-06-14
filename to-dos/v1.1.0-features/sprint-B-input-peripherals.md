# v1.1.0 · Sprint B — Input, peripherals & QoL  → beta.1

Extension points: `crates/rustynes-core/src/input_device.rs` (the `InputDevice` enum,
currently `Zapper`, `Vaus`), `crates/rustynes-frontend/src/input.rs`, `config.rs`,
the mapper parse path in `crates/rustynes-mappers/src/lib.rs`, and the cheat panel.

## T-110-B1 — New controllers (Power Pad, Family BASIC keyboard)  (Power Pad core DONE)

- Add `InputDevice` variants implementing `write_strobe`/`read`/`peek`; opt-in per
  port so the default controller path stays byte-identical.
- **Refs:** `ref-proj/Mesen2/.../Input/PowerPad.h`, `FamilyBasicKeyboard.h`.
- **Done when:** selectable per port in settings; determinism preserved when unset.
- **Power Pad core ✅ DONE (2026-06-14):** `input_device::PowerPadState` +
  `InputDevice::PowerPad` — the 12-button dual-shift-register serial protocol
  (`NESdev`/Mesen bit layout: buttons LSb-first on `$4017` D3/D4, `$4016` strobe).
  `Nes::set_power_pad(port, buttons)` + `Bus::set_power_pad` attach/update; save-state
  tag 3 (`bus_snapshot.rs`); exported as `rustynes_core::PowerPadState`. Opt-in =
  byte-identical default path; `no_std` clean. Unit tests: no-button serial pattern,
  button→serial-position mapping (Mesen-matched), strobe-high reload, save-state
  round-trip, 12-bit mask. **Remaining:** frontend wiring — `ExpansionDevice::PowerPad`
  + a 12-key default mapping (mat is a P2-port device) + `InputState`/`FrameInputs`/
  `SharedInput` plumbing + the `latch` feed + a Settings selector. (The mat is fed
  digital buttons, not the mouse the Zapper/Vaus use, so it needs new input-mapping
  infra — hence the core/frontend split.)
- **Family BASIC keyboard — DEFERRED:** a 72-key row/column matrix on the Famicom
  expansion port ($4016 col-select/row-step + $4017 4-bit reads), much larger and
  hard to verify without the copyrighted Family BASIC software. Separate follow-up.

## T-110-B2 — Turbo / autofire  ✅ DONE (2026-06-14)

- Frame-counter gating in `input.rs` + a `[input.turbo]` config block (per-button
  period). **Done when:** configurable; off by default.
- **DONE:** `[input] turbo_a` / `turbo_b` / `turbo_period` config (off by default =
  empty mask = byte-identical input). The gate `emu::apply_turbo(buttons, frame, mask,
  period)` strobes the masked buttons on/off keyed on the **emulated frame number**
  (`Nes::frame()`, a new pure read-only accessor) — applied in `EmuCore::latch` (covers
  the native emu-thread + synchronous + wasm-winit paths, which latch per produced
  frame) and on the local input in both netplay produce paths before `add_local_input`.
  Because the gate runs where input meets the NES and the **gated bits are what get
  latched / recorded / sent**, it is deterministic and rollback / TAS / netplay-safe
  (the remote + replay use the stored bits verbatim; run-ahead speculates with the
  already-gated buttons; movie playback replays recorded gated bits). `SharedInput`
  carries the turbo mask/period across the winit→emu thread boundary. UI: Settings →
  Input "Turbo / autofire" (Turbo A / Turbo B checkboxes + speed slider). Unit tests:
  strobe-masks-only, period-widening, off-is-identity (+ period-0 clamp). Native + both
  wasm flavours clippy clean; no_std core unchanged; AccuracyCoin/oracle unaffected.
  **Limitation:** the lightweight `wasm-canvas` embed (`wasm.rs`, separate minimal
  path) sets buttons directly and does not apply turbo; the main winit/native paths do.

## T-110-B3 — Input-display overlay  ✅ DONE (2026-06-14)

- New egui panel polling `InputState` (button/dpad grid per player). Read-only.
- **Done when:** toggleable overlay; useful for TAS/streaming.
- **DONE:** new `crates/rustynes-frontend/src/debugger/input_display_panel.rs` —
  a floating tool panel that draws a stylized NES controller per active player
  (D-pad cross + Select/Start pills + B/A buttons) with each held button lit.
  Wired as `ToolPanel::InputDisplay` (Tools → "Input Display" menu + debugger
  toolbar "Input HUD" checkbox). The app pushes the held-button snapshot each
  frame via `DebuggerOverlay::set_input_display([Buttons;4], players)` (mirrors
  the `set_fps` pull pattern; players = 2, or 4 with Four Score). Reads the
  same winit-thread `InputState` the emulator is fed — frontend-only, no core /
  produce-path / determinism impact. Native + both wasm flavours clippy clean.
  Later: show turbo-strobe state once T-110-B2 lands; analog Zapper/Vaus
  visualisation.

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
