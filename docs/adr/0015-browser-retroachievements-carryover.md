# 15. Browser RetroAchievements — casual-only, deferred build track

Date: 2026-06-16

## Status

Accepted (v1.3.0 Workstream I). **Partially implemented in v1.5.0 "Lens"
Workstream G** — the buildable parts are now done (build track proven, casual-only
gating made structural, auth-proxy contract + stub, loud in-UI caveat); the live
hosting + RA-account verification remain a maintainer-manual item (no headless
path). See the v1.5.0 update at the end of Consequences.

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

## v1.5.0 "Lens" Workstream G update — what is now implemented

The buildable parts of the planned design were landed behind the default-OFF,
wasm-only `browser-cheevos` feature. Everything is additive + off-by-default, so
native RA, the default native build, and both default wasm builds are unchanged
(AccuracyCoin held 100% / 139-139; the native-RA + wasm clippy gates stay green).

**Done (buildable, in-tree):**

1. **The Emscripten rcheevos→wasm build track is proven.** `emcc` 6.0.0 compiles
   the SAME vendored rcheevos sources + defines the native `build.rs` uses (26
   translation units) to a `wasm32-unknown-emscripten` static archive, then links
   a loadable side module (`rcheevos.wasm` + `rcheevos.js`). Driver:
   `scripts/cheevos/build_rcheevos_wasm.sh` (the `.wasm`/`.js` outputs are
   gitignored build artifacts). It is a **separate artifact, not linked into the
   Rust `.wasm`**: trunk builds `wasm32-unknown-unknown`, whose ABI/libc/linking
   model is incompatible with an emscripten `.a`. The Rust side reaches it through
   the `web/cheevos/ra_glue.js` host surface, bound by
   `crates/rustynes-frontend/src/wasm_cheevos.rs`'s `#[wasm_bindgen(module = ...)]`.
2. **Casual-only is now STRUCTURAL at three independent layers**, any one of which
   alone keeps hardcore impossible: (a) the Emscripten module never exports
   `rc_client_set_hardcore_enabled`; (b) `ra_glue.js` exposes no hardcore method;
   (c) `BrowserRaSession` has no hardcore field/API and its `hardcore_blocks()` is
   `const false`. The auth-proxy stub also refuses to forward a hardcore award.
3. **The auth-proxy contract is documented + has a deployable stub.** RA's
   `User-Agent` is browser-forbidden, so every server call routes through a proxy
   that injects `RustyNES/<ver> rcheevos/<ver>` server-side. Contract:
   `scripts/cheevos/auth-proxy.example.toml`; reference stub (stdlib-only):
   `scripts/cheevos/auth_proxy_stub.py`; full spec: `docs/cheevos-browser.md`.
4. **A loud, persistent in-UI caveat** renders on wasm: a top-anchored banner that
   always says casual-only + experimental (and, when the proxy is unset, that
   login + unlocks are unavailable). Nothing silently pretends to work.

**Still maintainer-manual (no headless path):**

- Deploy the auth proxy (a host + TLS + a hardened CORS origin) and coordinate the
  exact `User-Agent` + casual-only intent with the RA team.
- Finish the `ra_glue.js` rc_client trampoline marshalling (read-memory /
  server-call / event-handler via `addFunction`) — scaffolded, with the contract
  shape in place — then point `RA_PROXY_BASE` at the deployed proxy.
- **Live-browser verification with a real RA account** (open the page, log in via
  the proxy, confirm a casual unlock). This has no headless path and is the
  acceptance gate for flipping this ADR to fully Implemented.
