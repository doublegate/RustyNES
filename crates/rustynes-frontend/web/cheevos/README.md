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

`index.html` does not load this module by default (it ships only when the
maintainer completes the build + proxy). To enable it, build the Rust frontend
with the `browser-cheevos` feature and add, before the trunk-built script:

```html
<script type="module" src="./cheevos/ra_glue.js"></script>
```

then point the glue's `RA_PROXY_BASE` at your deployed auth proxy
(`scripts/cheevos/auth-proxy.example.toml`).
