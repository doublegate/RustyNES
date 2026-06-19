// ra_glue.js — casual-only browser RetroAchievements host surface (ADR 0015).
//
// v1.5.0 "Lens" Workstream G scaffold; v1.7.0 "Forge" H1 wires the rc_client
// wasm trampoline marshalling. This hand-written ES module is the bridge
// between the Rust frontend (`wasm_cheevos.rs`, behind the off-by-default
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
//      calls fail closed (http_status_code -1) and the Rust UI shows the
//      experimental / not-configured caveat — nothing silently "works".
//
// MAINTAINER-MANUAL CARRYOVERS (no headless path — ADR 0015):
//   - Deploy the auth proxy (scripts/cheevos/auth_proxy_stub.py is a reference)
//     and point RA_PROXY_BASE at its origin.
//   - Verify a casual unlock end-to-end in a real browser with a real RA
//     account. CI cannot do this (it needs a human + a live RA login + the
//     deployed proxy). The trampoline marshalling below is exercised only by
//     that live verify.

// The base URL of the auth proxy that injects the RA `User-Agent` server-side.
// Empty string = not configured; all server calls fail closed. Set this to your
// deployed proxy origin, e.g. "https://ra-proxy.example.org".
export const RA_PROXY_BASE = "";

// rc_console id for the NES (RC_CONSOLE_NINTENDO). Mirrors the native bridge.
const RC_CONSOLE_NINTENDO = 7;

// rc_client event-type constants (rc_client.h) we surface to JS / Rust. We only
// translate the handful the casual browser HUD needs; everything else is
// reported as { type } so the Rust side can ignore it.
const RC_CLIENT_EVENT_ACHIEVEMENT_TRIGGERED = 1;
const RC_CLIENT_EVENT_GAME_COMPLETED = 15;
const RC_CLIENT_EVENT_SERVER_ERROR = 16;

let _module = null; // the instantiated Emscripten Module (rcheevos.wasm)
let _client = 0; // the rc_client_t pointer (0 = none)

// Session epoch, bumped on every shutdown (and thus on every ROM close / reset
// / page teardown). An async server call captures the epoch (and the client
// pointer) at issue time and re-checks them when the network resolves: if the
// session has since been torn down (or replaced), the rcheevos completion
// callback + its callbackData point into freed/reallocated wasm memory, so we
// MUST NOT invoke it (use-after-free / dangling-pointer guard — ADR 0015).
let _clientEpoch = 0;

// addFunction pointers (kept so we never re-register / can remove on shutdown).
let _readFnPtr = 0;
let _serverFnPtr = 0;
let _eventFnPtr = 0;

// The JS read-memory closure installed for the duration of one do_frame call
// (set by ra_do_frame, cleared after). Maps a NES CPU-bus address -> byte.
let _readByte = null;

// The event queue drained by ra_do_frame. The C event trampoline pushes owned
// JS objects here; ra_do_frame serializes + clears it each frame.
let _events = [];

// Instantiate the Emscripten rcheevos module. Resolves once the wasm is ready.
async function loadModule() {
  if (_module) return _module;
  const { default: createRcheevosModule } = await import("./rcheevos.js");
  _module = await createRcheevosModule();
  return _module;
}

// --- The rc_client C trampolines (wasm function-table entries) --------------

// read-memory: uint32_t (*)(uint32_t address, uint8_t* buffer, uint32_t
// num_bytes, rc_client_t* client). Reads `num_bytes` NES bus bytes through the
// installed JS closure into the wasm `buffer`; returns the count written.
function readMemoryTrampoline(address, buffer, numBytes, _client) {
  const m = _module;
  if (!m || !_readByte) return 0;
  let written = 0;
  for (let i = 0; i < numBytes; i++) {
    // rcheevos hands us the RA flat address; for the NES that is the CPU bus
    // address directly (the native bridge maps it via ra_addr_to_nes, but the
    // identity holds for $0000..=$FFFF — out-of-range reads return 0).
    const addr = (address + i) & 0xffff;
    const byte = _readByte(addr) & 0xff;
    m.HEAPU8[buffer + i] = byte;
    written++;
  }
  return written;
}

// server-call: void (*)(const rc_api_request_t* request,
//   rc_client_server_callback_t callback, void* callback_data,
//   rc_client_t* client).
//
// rc_api_request_t { const char* url; const char* post_data;
//   const char* content_type; rc_buffer_t buffer; } — the first three are
// pointers at offsets 0/4/8 (wasm32). We POST the body to the auth proxy
// (verbatim path + body), then build an rc_api_server_response_t
// { const char* body; size_t body_length; int http_status_code; } and invoke
// the rcheevos completion callback with it.
function serverCallTrampoline(request, callback, callbackData, _client) {
  const m = _module;
  if (!m) return;

  // Capture the live-session identity at issue time. If the session is torn
  // down or replaced before the async fetch resolves, `callback`/`callbackData`
  // become dangling wasm pointers — the completion below refuses to invoke
  // them when this no longer matches (use-after-free guard, ADR 0015).
  const issueEpoch = _clientEpoch;
  const issueClient = _client;

  // Read the request struct fields (pointers at 0/4/8).
  const urlPtr = m.getValue(request + 0, "i32");
  const postPtr = m.getValue(request + 4, "i32");
  const ctypePtr = m.getValue(request + 8, "i32");
  const url = urlPtr ? m.UTF8ToString(urlPtr) : "";
  const postData = postPtr ? m.UTF8ToString(postPtr) : "";
  const contentType = ctypePtr
    ? m.UTF8ToString(ctypePtr)
    : "application/x-www-form-urlencoded";

  // Build a completion that marshals the HTTP outcome into an
  // rc_api_server_response_t and invokes the rcheevos callback.
  const complete = (status, bodyText) => {
    // Session-validity guard: if the client was shut down / reset / replaced
    // since this call was issued, the rcheevos callback + callbackData point
    // into freed (or reallocated) wasm memory. Skip the callback entirely —
    // invoking it would be a use-after-free. We allocate nothing and free
    // nothing for callbackData (it is owned by the now-dead rcheevos session).
    if (
      _clientEpoch !== issueEpoch ||
      _client !== issueClient ||
      _client === 0 ||
      !_module
    ) {
      return;
    }
    let bodyPtr = 0;
    let bodyLen = 0;
    if (bodyText) {
      bodyLen = m.lengthBytesUTF8(bodyText);
      bodyPtr = m._malloc(bodyLen + 1);
      m.stringToUTF8(bodyText, bodyPtr, bodyLen + 1);
    }
    // rc_api_server_response_t { body (i32), body_length (i32 on wasm32),
    // http_status_code (i32) } => 12 bytes.
    const respPtr = m._malloc(12);
    m.setValue(respPtr + 0, bodyPtr, "i32");
    m.setValue(respPtr + 4, bodyLen, "i32");
    m.setValue(respPtr + 8, status, "i32");
    try {
      // Invoke the rcheevos server-completion callback (a wasm function ptr):
      // void(*)(const rc_api_server_response_t*, void* callback_data).
      getDynCall(m)(callback, respPtr, callbackData);
    } finally {
      m._free(respPtr);
      if (bodyPtr) m._free(bodyPtr);
    }
  };

  if (RA_PROXY_BASE === "") {
    // Not configured: fail closed (status -1), exactly as documented. The Rust
    // UI then shows the not-configured caveat — nothing silently "works".
    complete(-1, "");
    return;
  }

  // Forward verbatim to the proxy: it re-targets upstream RA and injects the
  // identifying User-Agent server-side. We forward only the path+query of the
  // rcheevos-built URL (the proxy owns the upstream origin).
  let path = url;
  try {
    const u = new URL(url);
    path = u.pathname + u.search;
  } catch (_e) {
    // Relative/odd URL — forward as-is.
  }
  const target = RA_PROXY_BASE.replace(/\/$/, "") + path;

  fetch(target, {
    method: postData ? "POST" : "GET",
    headers: { "Content-Type": contentType },
    body: postData || undefined,
  })
    .then(async (resp) => {
      const text = await resp.text();
      complete(resp.status, text);
    })
    .catch(() => {
      complete(-1, "");
    });
}

// Resolve a callable for an indirect wasm function pointer across Emscripten
// versions: prefer Module.dynCall, else the raw function table (wasmTable).
function getDynCall(m) {
  if (typeof m.dynCall === "function") {
    return (ptr, a, b) => m.dynCall("vii", ptr, [a, b]);
  }
  const table = m.wasmTable || (m.asm && m.asm.__indirect_function_table);
  return (ptr, a, b) => table.get(ptr)(a, b);
}

// event-handler: void (*)(const rc_client_event_t* event, rc_client_t* client).
// rc_client_event_t { uint32_t type; rc_client_achievement_t* achievement;
//   ... } — type at offset 0, achievement ptr at offset 4. We translate just
// the casual-HUD-relevant events into queued JS objects.
function eventTrampoline(eventPtr, _client) {
  const m = _module;
  if (!m || !eventPtr) return;
  const type = m.getValue(eventPtr + 0, "i32") >>> 0;
  switch (type) {
    case RC_CLIENT_EVENT_ACHIEVEMENT_TRIGGERED: {
      // rc_client_achievement_t: title (const char*) at offset 0.
      const achPtr = m.getValue(eventPtr + 4, "i32");
      let title = "";
      if (achPtr) {
        const titlePtr = m.getValue(achPtr + 0, "i32");
        title = titlePtr ? m.UTF8ToString(titlePtr) : "";
      }
      _events.push({ type, title });
      break;
    }
    case RC_CLIENT_EVENT_GAME_COMPLETED:
    case RC_CLIENT_EVENT_SERVER_ERROR:
      _events.push({ type });
      break;
    default:
      _events.push({ type });
      break;
  }
}

// --- The host surface the Rust bridge imports ------------------------------
// Each is async/non-throwing at the boundary: a failure resolves to a sentinel
// the Rust side treats as "RA unavailable", never an uncaught JS exception.

// Initialize the casual-only client. Returns true on success. Hardcore is NEVER
// enabled here (the toggle is unexported); rcheevos stays in its casual path for
// the proxy/identity flow.
export async function ra_init() {
  try {
    const m = await loadModule();
    if (_client !== 0) return true;
    // Register the three rc_client trampolines in the wasm function table. The
    // Emscripten signature string is <return><args...>:
    //   read-memory  "iiiii" — u32(u32 address, ptr buffer, u32 num_bytes, ptr client)
    //   server-call  "viiii" — void(ptr request, ptr callback, ptr data, ptr client)
    //   event-handler "vii"  — void(ptr event, ptr client)
    _readFnPtr = m.addFunction(readMemoryTrampoline, "iiiii");
    _serverFnPtr = m.addFunction(serverCallTrampoline, "viiii");
    _eventFnPtr = m.addFunction(eventTrampoline, "vii");

    _client = m.ccall(
      "rc_client_create",
      "number",
      ["number", "number"],
      [_readFnPtr, _serverFnPtr],
    );
    if (_client === 0) return false;
    m.ccall("rc_client_set_event_handler", null, ["number", "number"], [
      _client,
      _eventFnPtr,
    ]);
    // Unofficial achievements off by default (matches the native bridge). Do
    // NOT call rc_client_set_hardcore_enabled — it is unexported; casual is
    // forced structurally.
    m.ccall("rc_client_set_unofficial_enabled", null, ["number", "number"], [
      _client,
      0,
    ]);
    return true;
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

// Begin a casual login via the auth proxy. rcheevos issues the login request
// through the server-call trampoline (routed to the proxy), so we just kick off
// rc_client_begin_login_with_password and let the trampoline carry the network.
// Returns true if the request was issued (completion is observed on later
// do_frame polls via user-info), false if RA is not configured / not ready.
export async function ra_begin_login(username, password) {
  if (!ra_proxy_configured() || _client === 0 || !_module) return false;
  const m = _module;
  const uPtr = allocCString(m, username);
  const pPtr = allocCString(m, password);
  try {
    m.ccall(
      "rc_client_begin_login_with_password",
      "number",
      ["number", "number", "number", "number", "number"],
      // No completion callback (0) — login state is read back via user-info.
      [_client, uPtr, pPtr, 0, 0],
    );
    return true;
  } catch (_e) {
    return false;
  } finally {
    m._free(uPtr);
    m._free(pPtr);
  }
}

// Begin loading a game from raw ROM bytes (rcheevos hashes them to identify the
// game). Returns true if the request was issued. Completion is observed on
// later do_frame polls.
export async function ra_load_game(romBytes) {
  if (_client === 0 || !_module) return false;
  const m = _module;
  const len = romBytes.length;
  const dataPtr = m._malloc(len);
  m.HEAPU8.set(romBytes, dataPtr);
  try {
    m.ccall(
      "rc_client_begin_identify_and_load_game",
      "number",
      ["number", "number", "number", "number", "number", "number", "number"],
      [_client, RC_CONSOLE_NINTENDO, 0, dataPtr, len, 0, 0],
    );
    return true;
  } catch (_e) {
    return false;
  } finally {
    m._free(dataPtr);
  }
}

// Drive one frame of achievement logic. `readByte(addr)` is a JS callback the
// Rust side supplies (reads a NES CPU-bus byte). Returns a JSON string of any
// events raised this frame (empty array when none / unavailable).
export function ra_do_frame(readByte) {
  if (_client === 0 || !_module) return "[]";
  const m = _module;
  _readByte = readByte;
  _events = [];
  try {
    m.ccall("rc_client_do_frame", null, ["number"], [_client]);
  } catch (_e) {
    // swallow — an exception must not cross back into wasm.
  } finally {
    _readByte = null;
  }
  return JSON.stringify(_events);
}

// Tear down the client + module (page unload / ROM close).
export function ra_shutdown() {
  if (_module && _client) {
    try {
      _module.ccall("rc_client_destroy", null, ["number"], [_client]);
    } catch (_e) {
      /* ignore */
    }
    if (_module.removeFunction) {
      try {
        if (_readFnPtr) _module.removeFunction(_readFnPtr);
        if (_serverFnPtr) _module.removeFunction(_serverFnPtr);
        if (_eventFnPtr) _module.removeFunction(_eventFnPtr);
      } catch (_e) {
        /* ignore */
      }
    }
  }
  _client = 0;
  _readFnPtr = 0;
  _serverFnPtr = 0;
  _eventFnPtr = 0;
  _readByte = null;
  _events = [];
  // Invalidate any in-flight server call: its completion will see a bumped
  // epoch (and a cleared `_client`) and refuse to invoke the now-dangling
  // rcheevos callback.
  _clientEpoch++;
}

// Allocate a NUL-terminated C string in the module heap; caller frees it.
function allocCString(m, s) {
  const len = m.lengthBytesUTF8(s) + 1;
  const ptr = m._malloc(len);
  m.stringToUTF8(s, ptr, len);
  return ptr;
}
