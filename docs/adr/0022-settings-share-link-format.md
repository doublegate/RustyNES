# 22. `?settings=` share-link format and versioning

Date: 2026-06-19

## Status

Accepted (v1.7.0 "Forge", Workstream H6 — web/wasm parity).

## Context

The browser build has no on-disk config (`directories` is native-only), so a
user's display/audio tweaks live only in memory for the session. Workstream H6
adds a way to **share** a viewing setup as a single URL — a `?settings=…` query
parameter that another user (or the same user on another machine) can open to
reproduce the look. The questions this ADR settles: what subset of `Config`
goes in the link, the wire format, the size/abuse guard, and the
cross-version compatibility posture.

## Decision

### Curated subset, not the whole `Config`

The link carries a dedicated `ShareSettings` DTO (`wasm_share.rs`), **not** the
full `Config`. The full config holds machine-local state that has no business in
a shared URL — recent-ROM filesystem paths, a saved RetroAchievements login
token, HD-pack paths, keybindings — and would bloat the blob well past a sane
URL length. `ShareSettings` captures only the presentation/display fields that
are meaningful and safe to transplant: the NTSC/CRT filter + its knobs,
overscan crop, theme, 8:7 pixel-aspect, UI zoom, FPS readout, and master
volume. `from_config` / `apply_to` map it to and from the live `Config`; only
those curated fields are ever overwritten on apply.

### Wire format: TOML → URL-safe base64

The DTO serializes via **TOML** (already the on-disk config format + a workspace
dep, so the field shapes stay consistent with `Config`) and is then encoded with
**URL-safe base64** (RFC 4648 §5: `+`→`-`, `/`→`_`, `=` padding stripped) so the
blob is a single URL-clean token. The codec (`base64url_encode` /
`base64url_decode` in `wasm_io.rs`) builds on the existing `btoa`/`atob` base64
helpers, adding no dependency.

### Safety

`ShareSettings::decode` is hardened against a hostile URL: the raw value is
length-capped (8 KiB) before decoding so a pathological query can't force a
large `atob` allocation, and any base64/UTF-8/TOML parse failure yields `None`
— the app silently keeps its defaults rather than erroring.

### Versioning posture

The blob embeds a `v` byte (`SHARE_VERSION`, currently 1). Readers **tolerate
any version**: every `ShareSettings` field is `#[serde(default)]`, so a link
minted by an older or newer build still decodes — unknown keys are ignored,
absent keys take the live default. `v` is therefore informational +
future-proofing (a hook for a future hard migration), not a hard gate. This
mirrors the `Config` `#[serde(default)]` discipline used everywhere else.

### Plumbing

On wasm boot, `config_from_url_or_default` applies any valid `?settings=` over a
fresh default `Config`. The live config's shareable subset is republished into a
thread-local each produced frame, and the JS-callable `rustynes_share_link`
export mints the full URL for the "Copy share link" button (which writes it to
the clipboard).

## Consequences

- A compact, URL-safe, tamper-tolerant link reproduces a viewing setup across
  machines/browsers without leaking local paths or login tokens.
- Adding a shareable field is a two-line change (one field on `ShareSettings`,
  one line each in `from_config`/`apply_to`), and old links keep working.
- The subset is deliberately presentation-only; it never carries input
  bindings or anything that could change emulation behaviour, and the whole
  module is wasm-only. Native + the deterministic core are unaffected
  (AccuracyCoin 100%, 139/139).
