# Browser RetroAchievements — casual-only side module (ADR 0015)

This directory holds the **casual-mode-only** browser RetroAchievements (RA)
build track for the wasm frontend. It is the v1.5.0 "Lens" Workstream G follow-up
to the ADR 0015 carryover.

**Status:** the build track is buildable and proven; live-browser + RA-account
verification (no headless path) and the auth-proxy deployment remain
maintainer-manual steps (see below and `../../../docs/cheevos-browser.md`).

## Why a separate Emscripten module (not linked into the Rust `.wasm`)

`rustynes-cheevos` links the vendored rcheevos **C** library via `cc` for the
native build. The browser frontend is compiled by trunk for
`wasm32-unknown-unknown`; Emscripten emits `wasm32-unknown-emscripten` objects
with a different ABI + libc + linking model, so an emscripten `.a` **cannot** be
`cc`-linked into a `wasm32-unknown-unknown` cdylib. The honest architecture
(anticipated by ADR 0015) is a **second build track**: rcheevos is compiled to
its own Emscripten module, loaded as JS glue alongside the Rust `.wasm`, and the
Rust side talks to it through the `wasm_cheevos.rs` `#[wasm_bindgen]` bridge.

## Files

| File | Committed? | What |
|---|---|---|
| `build_rcheevos_wasm.sh` (in `scripts/cheevos/`) | yes | Builds the module with `emcc` (same defines/sources as the native `build.rs`). |
| `ra_glue.js` | yes | Hand-written loader + the casual-only host surface the Rust bridge imports. |
| `rcheevos.js` | no (gitignored) | Emscripten-generated loader for `rcheevos.wasm`. Rebuilt by the script. |
| `rcheevos.wasm` | no (gitignored) | The compiled rcheevos module. Rebuilt by the script. |

## Build

```bash
export PATH=/usr/lib/emscripten:$PATH   # emcc is not on PATH by default here
emcc --version                          # confirm Emscripten is reachable
./scripts/cheevos/build_rcheevos_wasm.sh
```

This produces `rcheevos.wasm` + `rcheevos.js` in this directory. They are
gitignored build artifacts — rebuild on demand, never commit them.

## Casual-only is structural, not a toggle

Hardcore unlocks cannot be trusted in a browser: DevTools can patch the running
wasm + its memory. So the casual restriction is enforced at three layers, each
of which alone keeps hardcore impossible:

1. **The Emscripten module never exports `rc_client_set_hardcore_enabled`** — the
   only rcheevos entry point that turns hardcore on is not reachable from JS.
2. **`ra_glue.js` exposes no hardcore method** and never calls the toggle.
3. **The Rust `BrowserRaSession` (`wasm_cheevos.rs`) has no hardcore field or
   API** and the per-frame call path drives rcheevos in its default-casual state
   only.

rcheevos defaults to hardcore *on*; the glue's `ra_init` MUST leave hardcore
unset (the toggle is unexported, so it stays at the casual default for the
unauthenticated/proxy identity path the browser uses). See
`../../../docs/cheevos-browser.md` for the full contract.

## Wiring it into the page (maintainer step)

Measured end to end in v3.4.1 (headless Chromium against a copy of the Pages
layout: `ra_rcheevos_version()` returned `12.5`, every request a 200). Until
then this section asked for a `<script>` tag that is not needed and omitted the
two steps that are, and the browser path could not have loaded in any build.

1. **Build the module** with `scripts/cheevos/build_rcheevos_wasm.sh` (above).
   It is emitted as an ES module (`-sEXPORT_ES6=1`, since v3.4.1): the glue
   loads it with `import()`, and the UMD build it used to be had no default
   export to import.
2. **Build the frontend with the feature on the command line:**
   `trunk build --release --features browser-cheevos`. The glue needs no
   `<script>` tag: `wasm_cheevos.rs` binds it with
   `#[wasm_bindgen(module = "/web/cheevos/ra_glue.js")]`, so trunk emits it
   as a snippet (`snippets/<crate>-<hash>/web/cheevos/ra_glue.js`). (`index.html`
   carries no `data-cargo-features`; until v3.4.1 an empty one there made
   trunk ignore `--features`.)
3. **Place the module beside the page:** copy `rcheevos.js` and `rcheevos.wasm`
   into `dist/cheevos/`. The glue resolves them against the page, not against
   its own snippet path (where nothing copies them): `cheevos/` beside the page
   by default, or the directory a `<meta name="rustynes-rcheevos-base"
   content="...">` names.
4. **Point the glue at the proxy:** `<meta name="rustynes-ra-proxy-base"
   content="https://<host>/ra">` in the page (or `globalThis.RUSTYNES_RA_PROXY_BASE`,
   or `HOSTED_RA_PROXY_BASE` in `ra_glue.js`); the hosted service is
   `services/hosted/`. The glue reads these when `ra_init()` starts a session,
   not when the module is imported (v3.5.0, `T-WEB-RA-PROXY-LAZY`), so a page
   may set the base at any point before RetroAchievements starts; a live
   session keeps the base it started with until `ra_shutdown()`.

The public Pages demo does not ship this yet (maintainer, 2026-10-18): it waits
for RetroAchievements to receive the integration request.
