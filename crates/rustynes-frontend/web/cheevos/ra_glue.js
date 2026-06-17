// ra_glue.js — casual-only browser RetroAchievements host surface (ADR 0015).
//
// v1.5.0 "Lens" Workstream G. This hand-written ES module is the bridge between
// the Rust frontend (`wasm_cheevos.rs`, behind the off-by-default
// `browser-cheevos` feature) and the Emscripten-built rcheevos module
// (`rcheevos.wasm` + `rcheevos.js`, produced by
// `scripts/cheevos/build_rcheevos_wasm.sh`).
//
// It is intentionally MINIMAL and HONEST about the two structural constraints
// from ADR 0015:
//
//   1. CASUAL-ONLY IS STRUCTURAL. There is NO method here to enable hardcore,
//      and the Emscripten module deliberately does not export
//      rc_client_set_hardcore_enabled. Hardcore unlocks cannot be trusted in a
//      browser (DevTools can patch the wasm + memory), so casual is the only
//      honest mode — matching the loud in-UI caveat the Rust side renders.
//
//   2. THE RA USER-AGENT IS BROWSER-FORBIDDEN. RA identifies/allowlists clients
//      by their HTTP `User-Agent` (`RustyNES/<ver> rcheevos/<ver>` natively), but
//      browsers forbid scripts from setting `User-Agent`. So EVERY rcheevos
//      server call is routed through a maintainer-deployed AUTH PROXY that adds
//      the identifying header server-side. The proxy contract is documented in
//      `docs/cheevos-browser.md` + `scripts/cheevos/auth-proxy.example.toml`.
//      Point `RA_PROXY_BASE` at your deployed proxy. Until it is set, server
//      calls fail closed (status -1) and the Rust UI shows the experimental /
//      not-configured caveat — nothing silently "works".
//
// This file is a SCAFFOLD: the rc_client trampoline plumbing (read-memory,
// server-call, event-handler, the ccall/cwrap typed-pointer marshalling) is the
// maintainer's remaining wiring once the proxy is live and a real RA account is
// available to verify against (no headless path — ADR 0015). The shape below is
// the contract the Rust `#[wasm_bindgen(module = "/cheevos/ra_glue.js")]`
// imports bind to.

// The base URL of the auth proxy that injects the RA `User-Agent` server-side.
// Empty string = not configured; all server calls fail closed. Set this to your
// deployed proxy origin, e.g. "https://ra-proxy.example.org".
export const RA_PROXY_BASE = "";

let _module = null; // the instantiated Emscripten Module (rcheevos.wasm)
let _client = 0; // the rc_client_t pointer (0 = none)

// Instantiate the Emscripten rcheevos module. Resolves once the wasm is ready.
async function loadModule() {
  if (_module) return _module;
  const { default: createRcheevosModule } = await import("./rcheevos.js");
  _module = await createRcheevosModule();
  return _module;
}

// --- The host surface the Rust bridge imports ------------------------------
// Each is async/无-throw at the boundary: a failure resolves to a sentinel the
// Rust side treats as "RA unavailable", never an uncaught JS exception into wasm.

// Initialize the casual-only client. Returns true on success. Hardcore is NEVER
// enabled here (the toggle is unexported); rcheevos stays in its casual path for
// the proxy/identity flow.
export async function ra_init() {
  try {
    await loadModule();
    // NOTE (maintainer): create the rc_client with read-memory + server-call
    // trampolines (addFunction) here, install the event handler, and call
    // rc_client_set_unofficial_enabled(client, 0). Do NOT call
    // rc_client_set_hardcore_enabled — it is unexported and casual is forced.
    return _client !== 0 || RA_PROXY_BASE !== "";
  } catch (_e) {
    return false;
  }
}

// True only when the auth proxy is configured (a precondition for any RA login
// or unlock). The Rust UI uses this to render the "not configured" caveat.
export function ra_proxy_configured() {
  return RA_PROXY_BASE !== "";
}

// The rcheevos version string from the module (for the UI / identity line).
export async function ra_rcheevos_version() {
  try {
    const m = await loadModule();
    return m.ccall("rc_version_string", "string", [], []);
  } catch (_e) {
    return "";
  }
}

// Begin a casual login via the auth proxy. `username`/`password` are POSTed to
// the proxy, which forwards to RA with the identifying User-Agent and returns
// the login token. Resolves to the token string, or "" on failure.
// (Scaffold: the maintainer wires the proxy fetch + rc_client login completion.)
export async function ra_begin_login(_username, _password) {
  if (!ra_proxy_configured()) return "";
  // NOTE (maintainer): POST {username,password} to `${RA_PROXY_BASE}/login`,
  // feed the response into rc_client_begin_login_with_token, await completion.
  return "";
}

// Drive one frame of achievement logic. `readByte(addr)` is a JS callback the
// Rust side supplies (reads a NES CPU-bus byte). Returns a JSON string of any
// events raised this frame (empty array when none / unavailable).
// (Scaffold: marshals through rc_client_do_frame once the client is created.)
export function ra_do_frame(_readByte) {
  return "[]";
}

// Tear down the client + module (page unload / ROM close).
export function ra_shutdown() {
  if (_module && _client) {
    try {
      _module.ccall("rc_client_destroy", null, ["number"], [_client]);
    } catch (_e) {
      /* ignore */
    }
  }
  _client = 0;
}
