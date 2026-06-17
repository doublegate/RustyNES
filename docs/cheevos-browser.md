# Browser RetroAchievements (casual-only)

> v1.5.0 "Lens" Workstream G. Implements the buildable parts of the ADR 0015
> carryover. **EXPERIMENTAL, casual-only, off by default.** Hardcore is
> native-only. Native RetroAchievements (`docs` + `crates/rustynes-cheevos`) is
> unaffected.

This is the spec for running RetroAchievements (RA) in the WebAssembly frontend.
It is the browser analog of the native RA integration, but with three structural
constraints that the design works around honestly rather than papering over.

## Why the browser is different

| Constraint | Native | Browser | Resolution |
|---|---|---|---|
| rcheevos is C | linked via `cc` into the binary | cannot link a `wasm32-unknown-emscripten` `.a` into a `wasm32-unknown-unknown` cdylib | a separate Emscripten **side module** + a `#[wasm_bindgen]` bridge |
| RA identity is an HTTP `User-Agent` | set on the `ureq` agent | browsers FORBID scripts setting `User-Agent` | an **auth proxy** injects it server-side |
| Hardcore integrity | trustworthy | DevTools can patch the running wasm + memory | **casual-only, structurally** (no hardcore path at any layer) |

## Architecture

```text
  ┌─────────────────────────┐        ┌──────────────────────────┐
  │ Rust frontend (.wasm)    │        │ Emscripten rcheevos       │
  │ wasm32-unknown-unknown   │  JS    │ side module (.wasm + .js) │
  │ src/wasm_cheevos.rs      │◀──────▶│ web/cheevos/rcheevos.*     │
  │  BrowserRaSession        │ ra_glue│ (rc_client_* exports,      │
  │  (no hardcore API)       │  .js   │  hardcore toggle UNEXPORTED)│
  └─────────────┬───────────┘        └──────────────────────────┘
                │ fetch (RA server calls)
                ▼
       ┌─────────────────────┐  inject User-Agent   ┌────────────────────┐
       │ auth proxy           │ ───────────────────▶ │ retroachievements.org│
       │ (maintainer-hosted)  │  refuse hardcore     └────────────────────┘
       └─────────────────────┘
```

- **`scripts/cheevos/build_rcheevos_wasm.sh`** compiles rcheevos to the side
  module with `emcc` (same sources/defines as the native `build.rs`). Output is
  gitignored — rebuild on demand.
- **`crates/rustynes-frontend/web/cheevos/ra_glue.js`** (committed) is the loader
  plus the casual-only host surface the Rust bridge imports.
- **`crates/rustynes-frontend/src/wasm_cheevos.rs`** (behind the default-OFF,
  wasm-only `browser-cheevos` feature) owns `BrowserRaSession` and renders the
  caveat. It has **no hardcore field or API**.

## Casual-only is structural (the load-bearing invariant)

Hardcore unlocks cannot be trusted in a browser, so casual is the only honest
mode. It is enforced at **three independent layers** — each alone keeps hardcore
impossible:

1. The Emscripten module **never exports `rc_client_set_hardcore_enabled`** — the
   only entry point that turns hardcore on is unreachable from JS.
2. `ra_glue.js` exposes **no hardcore method** and never calls the toggle.
3. `BrowserRaSession` has **no hardcore state and no API** to enable it;
   `hardcore_blocks()` is `const false` (the browser RA path never blocks the
   soft affordances — there is no hardcore session to protect).

The auth proxy adds a fourth backstop: it refuses to forward a hardcore award
(`enforce_casual_only`).

## Auth proxy contract

RA identifies/allowlists clients by the HTTP `User-Agent`
`RustyNES/<crate-ver> rcheevos/<rcheevos-ver>` (the `RA_USER_AGENT` const in
`crates/rustynes-cheevos/src/http.rs`; a native regression test guards the
leading `RustyNES/` token). Browsers forbid setting it, so every server call from
the browser is routed through a small proxy that injects the header server-side.

- **Spec / config:** `scripts/cheevos/auth-proxy.example.toml`.
- **Reference stub (stdlib-only Python):** `scripts/cheevos/auth_proxy_stub.py` —
  injects the `User-Agent`, enforces CORS to the page origin, and refuses
  hardcore awards. Run it for local development:

  ```bash
  python3 scripts/cheevos/auth_proxy_stub.py --config scripts/cheevos/auth-proxy.example.toml
  ```

- **Endpoints** the glue uses (relative to the proxy origin):
  - `POST /login {username, password}` → `{token}` | `{error}` (the glue never
    sends a password anywhere else; only the returned token is persisted).
  - `POST /ra <raw rcheevos request>` → `<raw RA response>` (User-Agent injected).

Point `RA_PROXY_BASE` in `web/cheevos/ra_glue.js` at the deployed proxy origin.
Until it is set, `BrowserRaSession::proxy_configured()` is `false`, server calls
fail closed, and the UI shows the "not configured" caveat.

## Building + enabling

```bash
# 1. Build the rcheevos side module (emcc must be on PATH).
export PATH=/usr/lib/emscripten:$PATH
./scripts/cheevos/build_rcheevos_wasm.sh

# 2. Build the Rust frontend with the feature on (wasm-only, default OFF).
cargo build -p rustynes-frontend --target wasm32-unknown-unknown --features browser-cheevos
#   ... or via trunk, adding `--features browser-cheevos` to the rust link in
#   web/index.html (and a `<script type="module" src="./cheevos/ra_glue.js">`).
```

## Status + remaining maintainer-manual steps

**Done (in-tree, buildable):** the build track (proven with `emcc` 6.0.0), the
three-layer structural casual gating, the auth-proxy contract + stub, and the loud
in-UI caveat. Off by default, so the shipped native + wasm builds are byte-
identical and AccuracyCoin holds 100% (139/139).

**Maintainer-manual (no headless path — ADR 0015):**

1. Deploy the auth proxy (host + TLS + hardened CORS) and coordinate the exact
   `User-Agent` + casual-only intent with the RA team.
2. Finish the `ra_glue.js` rc_client trampoline marshalling (read-memory /
   server-call / event-handler via `addFunction`); the contract shape is in place.
3. **Live-browser verification with a real RA account** — log in via the proxy and
   confirm a casual unlock in a browser. This is the acceptance gate for flipping
   ADR 0015 to fully Implemented.
