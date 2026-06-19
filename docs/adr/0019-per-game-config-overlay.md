# 19. Per-game `<rom>.json` config overlay (frontend-only, off-by-default)

Date: 2026-06-19

## Status

Accepted (v1.7.0 "Forge", Workstream H4).

## Context

RustyNES already carries two per-game correction layers, both frontend-only and
keyed on the header-excluded ROM CRC32:

- the **vendored game-DB** (`crates/rustynes-frontend/src/game_db.rs`, v1.1.0/
  v1.2.0) — region / mapper / submapper / mirroring corrections, with an iNES
  header rewrite (`apply_header_overrides`) at the load chokepoint plus a
  post-construction `set_mirroring_override`, and
- a **user overlay** (`game_db_user.txt`, edited via Tools → ROM Database) that
  overrides the vendored base by CRC.

Mesen2's "per-game config" goes further: a single ROM can carry its own
settings file. Workstream H4 wants the architectural keystone for that — a
per-game settings overlay — plus two concrete consumers (a Vs. System DIP-switch
editor and a lag-frame counter). The hard requirement is the project's
determinism firewall: the deterministic core (`rustynes-core` + the chip crates)
and the test harness MUST NOT consult any per-game file, AccuracyCoin must hold
100% (139/139), and with no file present the behaviour must be byte-identical to
today.

The question this ADR settles: where does a per-game `<rom>.json` live, how does
it compose with the existing game-DB, and how is the firewall preserved.

## Decision

Add a **frontend-only** `per_game` module owning a `PerGameConfig` schema and a
two-source resolver, layered *on top of* the v1.2.0 game-DB (it reuses
`GameDbEntry`, `rom_crc32`, and `apply_header_overrides` — it does not rebuild
them):

- **Schema.** `PerGameConfig` is `#[serde(default)]` everywhere: an `overrides`
  block (region / mapper / submapper / mirroring, expressed as JSON, mapped to a
  `GameDbEntry`), a Vs. `dip_switches: Option<u8>`, reserved
  `video`/`audio`/`input` free-form blocks (round-tripped for forward-compat,
  not yet consumed), and free-form `notes`. Every field is `Option` / defaulted,
  so a missing or partial file deserializes cleanly and an empty `{}` is inert.
- **Two sources, config-dir wins.** On load (after the CRC is known) the frontend
  resolves a `<rom>.json` from (1) a config-dir overlay
  (`<data-dir>/per-game/<CRC8>.json`) and (2) a sibling `<rom-stem>.json` next to
  the ROM. The config-dir overlay takes precedence — the same precedence as the
  v1.2.0 game-DB user overlay over the vendored base (user edits win over what
  ships beside the ROM). The editor only ever writes the config-dir overlay,
  never a sibling ROM file.
- **Same application paths.** The `overrides` flow through the existing
  `apply_header_overrides` (iNES header rewrite, pre-construction) so they stack
  on the game-DB corrections and the CRC key stays stable; mirroring and the Vs.
  DIP apply post-construction via the existing `Nes::set_mirroring_override` /
  `Nes::set_vs_dip` setters. No new core entry points.
- **Firewall.** The core + test harness build the `Nes` directly and never call
  the resolver. With no file (or an inert file) the load path applies nothing, so
  the shipped / native / `no_std` / wasm builds stay byte-identical and
  AccuracyCoin holds 100%. The persisted state-mutating surface (the resolved
  mirroring + DIP) lives in the save-state exactly like the game-DB mirroring, so
  netplay rollback stays consistent: both peers resolve the overlay from the
  **shared** ROM CRC (the same file, or none), the identical contract the game-DB
  already documents ("same CRC ⇒ same correction, so netplay peers agree").
- **Consumers.** The DIP-switch editor is folded into the existing ROM-Database
  panel (shown for Vs. carts; persists into the config-dir overlay). The
  lag-frame counter is a status-bar readout sampled from the core's output-only
  `was_input_polled_this_frame()` `debug-hooks` telemetry — a pure observation,
  gated behind a new off-by-default `[ui] show_lag_frames` toggle.

## Consequences

- The keystone is in place: future per-game settings add a field to
  `PerGameConfig` (consumed where a setter exists), no schema migration churn.
- One new workspace dependency (`serde_json`) for the JSON overlay.
- The reserved `video`/`audio`/`input` blocks are round-tripped but not yet
  consumed; wiring them is a follow-up that does not touch this firewall decision.
- On wasm there is no filesystem overlay, so the DIP editor applies edits live
  only (no persistence) — consistent with the rest of the wasm save surface.
- A live edit of the DIP/mirroring mid-netplay would desync, exactly as it would
  for the existing game-DB mirroring editor; this is unchanged behaviour, not a
  new firewall break (the determinism guarantee is at load-time resolution from
  the shared CRC).
