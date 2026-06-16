# 15. Browser RetroAchievements — casual-only, deferred build track

Date: 2026-06-16

## Status

Accepted (v1.3.0 Workstream I) — **documented carryover**: the design + honesty
constraints are settled here; the build track + live verification are a maintainer
manual item (see Consequences).

## Context

v1.3.0 "Bedrock" scoped browser RetroAchievements (Workstream I) as a
**casual-mode-only** feature. Native RA already ships (opt-in, behind the
`retroachievements` feature, via the vendored **rcheevos** C library linked through
`cc` in `rustynes-cheevos`). Bringing it to the WebAssembly build hits three
structural blockers, all confirmed during v1.3.0:

1. **rcheevos is C.** `rustynes-cheevos` is `#![cfg(not(target_arch = "wasm32"))]`
   — on wasm the crate body is empty and `build.rs` early-returns without invoking
   `cc` (there is no C toolchain in the `wasm32-unknown-unknown` build). Linking
   rcheevos into the browser build needs a **second build track**: either an
   **Emscripten** (`emcc`) compile of rcheevos to wasm wired in via a separate
   artifact, or a **pure-Rust reimplementation** of the rcheevos runtime. Neither
   is present (the `emcc` toolchain is not installed in the build environment).
2. **The RA `User-Agent` is browser-forbidden.** RA identifies/allowlists clients
   by their HTTP `User-Agent` (`RustyNES/<ver> rcheevos/<ver>` natively), but
   browsers forbid scripts from setting `User-Agent`. The browser auth/identity
   path must adapt (a server-side proxy, or accept casual-only unauthenticated
   identification) — coordinated separately with the RA team.
3. **Hardcore integrity collapses in a browser.** DevTools can patch the running
   wasm + memory, so hardcore unlocks cannot be trusted. **Casual-only is the only
   honest mode** in the browser — with a loud in-UI caveat that hardcore is
   unavailable there.

## Decision

- **Ship v1.3.0 with native RA unchanged** and browser RA as a **documented
  carryover**, not a half-built feature. No dead/no-op `browser-cheevos` feature
  flag is added (it would imply functionality that does not exist); the planned
  flag + casual-only gating + UI-caveat design is recorded here instead.
- **Planned design (for whoever takes the build track):** a wasm-only
  `browser-cheevos` feature that selects an Emscripten-built rcheevos artifact (or
  a pure-Rust runtime), drives the existing `RaClient` host surface, **forces
  casual mode** (hardcore disabled, not merely off-by-default), routes auth through
  a proxy that supplies the identifying `User-Agent`, and shows a persistent
  "RetroAchievements: casual-only in the browser — hardcore is native-only" banner
  in the RA UI. Gated so the native build is byte-identical.
- **Honesty + determinism:** RA is frontend-side and observational; it never enters
  the FB/audio determinism oracle. The carryover changes nothing about the shipped
  native or wasm builds for v1.3.0.

## Consequences

- **Maintainer manual carryover** (mirrors the v1.2.0 F1 on-device-touch / F3
  live-netplay-matrix carryovers the maintainer accepted): the rcheevos→wasm build
  track + a **live-browser verification with an RA account** (no headless path)
  remain to be done by the maintainer when the Emscripten/pure-Rust path is set up.
  This is tracked, not built.
- Native RetroAchievements (achievements / leaderboards / rich presence / hardcore,
  opt-in) is **unaffected** and fully supported.
- The v1.4.0 plan's deferred backlog and `docs/compatibility.md` carry the
  cross-reference; the RA-API-drift maintenance cost is an ongoing flag, not code.
