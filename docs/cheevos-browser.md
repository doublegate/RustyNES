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
  plus the casual-only host surface the Rust bridge imports. As of v1.7.0 "Forge"
  H1 it implements the rc_client **wasm trampoline marshalling**: the
  read-memory / server-call / event-handler callbacks are registered with
  `addFunction`, `ra_init` creates the client + installs the event handler, the
  server-call trampoline marshals an `rc_api_request_t` → an auth-proxy `fetch` →
  an `rc_api_server_response_t` (so rcheevos sees a normal completion), and
  `ra_do_frame(readByte)` drives a frame and returns a JSON event array.
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
  hardcore awards. It is configured from a TOML file **or** (as of v2.1.10)
  purely from environment variables, so the same script drives both local dev and
  the container deploy. Run it for local development:

  ```bash
  # File-driven:
  python3 scripts/cheevos/auth_proxy_stub.py --config scripts/cheevos/auth-proxy.example.toml
  # Env-driven (no config file):
  RA_PROXY_BIND=127.0.0.1:8092 RA_ALLOWED_ORIGINS='http://127.0.0.1:8081' \
    RA_USER_AGENT='RustyNES/2.1.10 rcheevos/12.3.0' \
    python3 scripts/cheevos/auth_proxy_stub.py
  ```

- **Deployable stack (v2.1.10 "Web Parity"):** `deploy/` now ships the proxy as a
  first-class `ra-proxy` compose service (`deploy/Dockerfile.raproxy` runs the
  same stub, env-configured) behind the shared Caddy TLS reverse proxy, which
  exposes it at `https://<DOMAIN>/ra/*`. See `deploy/README.md` §"Browser
  RetroAchievements (auth proxy)". The proxy holds **no RA secret** — it injects
  only the non-secret identity header; the user's own login transits at request
  time. Configuration is env-only (`RA_USER_AGENT` / `RA_ALLOWED_ORIGINS` /
  `RA_UPSTREAM` / `RA_ENFORCE_CASUAL`), never a committed credential.

- **Request shape** the glue produces: the server-call trampoline forwards each
  rcheevos request **verbatim** — it takes the path + query of the rcheevos-built
  URL and re-targets it at the proxy origin (`${RA_PROXY_BASE}<path?query>`),
  POSTing the `post_data` body with the rcheevos `content_type`. The proxy's one
  job is to re-target that path at upstream RA and inject the identifying
  `User-Agent` header (the reference stub forwards `self.path` to `upstream`
  verbatim). rcheevos itself drives the login (`rc_client_begin_login_with_*`)
  through this same path; the glue never sends a password anywhere except as part
  of the rcheevos login request body, and only the returned token is persisted.

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
three-layer structural casual gating, the auth-proxy contract + stub, the loud
in-UI caveat, and — **as of v1.7.0 "Forge" H1** — the full `ra_glue.js` rc_client
**trampoline marshalling** (read-memory / server-call / event-handler via
`addFunction`, the request → proxy `fetch` → response bridge, client create +
event-handler install, and the `ra_do_frame` per-frame driver) plus the Rust
bridge methods (`begin_login` / `load_game` / `do_frame`) over it. The side-module
build script now also exports `set_event_handler` and the `getValue` / `setValue`
/ `HEAPU8` runtime methods the marshalling reads/writes the rcheevos structs with.
Off by default, so the shipped native + wasm builds are byte-identical and
AccuracyCoin holds 139/141 (the two newest upstream PPU tests are known gaps).

**v2.1.10 "Web Parity" update — the auth proxy is now a deployable service.**
The deploy stack (`deploy/`) gained a first-class `ra-proxy` compose service
(`deploy/Dockerfile.raproxy`) behind the shared Caddy TLS proxy at
`https://<DOMAIN>/ra/*`, env-configured (no committed config or secret). The
reference stub grew env-var configuration so one script serves both local dev and
the container. `docker compose up` now brings the proxy up alongside signaling +
STUN/TURN. What remains is genuinely un-CI-able: standing the stack on a real host
and a live RA login (below).

**Maintainer-manual (no headless path — ADR 0015) — the only remaining carryovers:**

1. **Stand the proxy up on a real host** — `docker compose up` in `deploy/` with
   `RA_USER_AGENT`/`RA_ALLOWED_ORIGINS` set (TLS via Caddy), and coordinate the
   exact `User-Agent` + casual-only intent with the RA team. Then set
   `RA_PROXY_BASE` in `web/cheevos/ra_glue.js` to `https://<DOMAIN>/ra`. The
   service + config are code-complete (`deploy/README.md` §"Browser
   RetroAchievements"); only running it on a live host + the RA-team coordination
   remain.
2. **Live-browser verification with a real RA account** — build with
   `--features browser-cheevos`, add the `<script type="module" src="./cheevos/ra_glue.js">`
   to `web/index.html`, build the side module, log in through the deployed proxy,
   and confirm a casual unlock in a real browser. CI cannot do this (it needs a
   human, a live RA login, and the deployed proxy). This is the acceptance gate
   for flipping ADR 0015 to fully Implemented; native RA is unaffected. The
   copy-paste runbook is `deploy/README.md` §"Browser RA live-verify checklist".
