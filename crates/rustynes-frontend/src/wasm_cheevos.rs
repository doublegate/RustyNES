//! Casual-only browser RetroAchievements session (v1.5.0 "Lens" Workstream G,
//! ADR 0015). wasm-only, behind the default-OFF `browser-cheevos` feature.
//!
//! # What this is
//!
//! The browser analog of the native `rustynes_ra` session — but
//! **structurally casual-only**. It owns no `RaClient` (the native rcheevos FFI
//! does not exist on `wasm32-unknown-unknown`); instead it talks to the
//! Emscripten-built rcheevos side module through the `ra_glue.js` host surface
//! (see `web/cheevos/`), bound below via `#[wasm_bindgen(module = ...)]`.
//!
//! # Casual-only is structural, not a toggle (the load-bearing invariant)
//!
//! Hardcore unlocks cannot be trusted in a browser — DevTools can patch the
//! running wasm + its memory. So, exactly as ADR 0015 requires, casual is the
//! ONLY mode and it is enforced structurally at three independent layers:
//!
//! 1. The Emscripten module never exports `rc_client_set_hardcore_enabled`
//!    (`scripts/cheevos/build_rcheevos_wasm.sh`).
//! 2. `ra_glue.js` exposes no hardcore method and never calls the toggle.
//! 3. **This type has no hardcore field and no API to enable it.** There is no
//!    `set_hardcore`, no `hardcore` flag, and the gating predicate
//!    [`BrowserRaSession::hardcore_blocks`] is `const false` — the browser RA
//!    path never blocks the "soft" affordances (save-state load / rewind /
//!    cheats), because there is no hardcore session to protect.
//!
//! # Auth proxy (the browser-forbidden `User-Agent`)
//!
//! RA identifies clients by their HTTP `User-Agent`, which browsers forbid
//! scripts from setting. Every rcheevos server call is therefore routed through
//! a maintainer-deployed auth proxy that injects the header server-side
//! (`scripts/cheevos/auth-proxy.example.toml` + `docs/cheevos-browser.md`).
//! Until the proxy is configured, [`BrowserRaSession::proxy_configured`] is
//! `false` and the UI shows the not-configured / experimental caveat — nothing
//! silently pretends to work.
//!
//! # Status
//!
//! The build track + this scaffold compile cleanly and are wired into the UI
//! caveat path. The live rc_client trampoline marshalling in `ra_glue.js`, the
//! deployed proxy, and a live-browser unlock against a real RA account (no
//! headless path) are the maintainer's remaining manual steps — see ADR 0015.

// `RetroAchievements` / `DevTools` / `RustyNES` read as CamelCase "code" to
// `doc_markdown`, but they're product names in prose. The single async fn here
// is ALWAYS driven by `wasm_bindgen_futures::spawn_local` (the browser is
// single-threaded), so `JsFuture`'s non-`Send`-ness is irrelevant — silence
// `future_not_send` rather than fake-`Send` the JS handles (the same idiom as
// `wasm_idb.rs`).
#![allow(clippy::doc_markdown, clippy::future_not_send)]

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

// The host surface implemented by `web/cheevos/ra_glue.js`. These imports
// resolve when the module is loaded in the page; the Rust side compiles against
// the signatures regardless of whether the glue is deployed yet. Note the
// COMPLETE ABSENCE of any hardcore-enabling import — casual-only is structural.
#[wasm_bindgen(module = "/web/cheevos/ra_glue.js")]
extern "C" {
    /// Initialize the casual-only client. Resolves to `true` on success.
    /// Hardcore is never enabled (the toggle is unexported). v1.7.0 H1: this
    /// now registers the read-memory / server-call / event-handler trampolines
    /// in the side module and installs the event handler.
    #[wasm_bindgen(catch, js_name = ra_init)]
    async fn ra_init() -> Result<JsValue, JsValue>;

    /// `true` only when the auth proxy is configured (precondition for login /
    /// unlocks). Drives the UI "not configured" caveat.
    #[wasm_bindgen(js_name = ra_proxy_configured)]
    fn ra_proxy_configured() -> bool;

    /// Begin a casual login through the auth proxy. Resolves to `true` if the
    /// request was issued (completion is observed on later `ra_do_frame` polls).
    #[wasm_bindgen(catch, js_name = ra_begin_login)]
    async fn ra_begin_login(username: &str, password: &str) -> Result<JsValue, JsValue>;

    /// Begin identifying + loading a game from its ROM bytes. Resolves to `true`
    /// if the request was issued.
    #[wasm_bindgen(catch, js_name = ra_load_game)]
    async fn ra_load_game(rom: &[u8]) -> Result<JsValue, JsValue>;

    /// Drive one frame of achievement logic. `read_byte` is a JS callback that
    /// reads one NES CPU-bus byte. Returns a JSON array of this frame's events.
    #[wasm_bindgen(js_name = ra_do_frame)]
    fn ra_do_frame(read_byte: &js_sys::Function) -> String;

    /// Tear down the client + module.
    #[wasm_bindgen(js_name = ra_shutdown)]
    fn ra_shutdown();
}

/// The casual-only browser RA session held by the `App` as
/// `Option<BrowserRaSession>` on wasm when `browser-cheevos` is on.
///
/// Deliberately tiny: it carries only the state the UI caveat + the (scaffolded)
/// per-frame drive need. There is **no hardcore state** — see the module docs.
#[derive(Default)]
pub struct BrowserRaSession {
    /// `true` once the side module initialized successfully.
    initialized: bool,
    /// Cached `ra_proxy_configured()` result (the auth-proxy precondition).
    proxy_configured: bool,
}

impl BrowserRaSession {
    /// Construct an un-initialized session. Side-effect-free w.r.t. the network
    /// and the side module; call [`Self::init`] to load the Emscripten module.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Load + initialize the side module. Idempotent. Records whether the auth
    /// proxy is configured (the precondition for any login / unlock).
    pub async fn init(&mut self) {
        if self.initialized {
            return;
        }
        // A failed init (module absent / not built) leaves us un-initialized;
        // the UI then shows the experimental / not-built caveat.
        if let Ok(v) = ra_init().await {
            self.initialized = v.as_bool().unwrap_or(false);
        }
        self.proxy_configured = ra_proxy_configured();
    }

    /// Whether the side module initialized successfully.
    #[must_use]
    pub const fn initialized(&self) -> bool {
        self.initialized
    }

    /// Whether the auth proxy is configured. When `false`, RA login + unlocks
    /// are unavailable and the UI must say so loudly.
    #[must_use]
    pub const fn proxy_configured(&self) -> bool {
        self.proxy_configured
    }

    /// The gating predicate, mirroring the native `rustynes_ra` API
    /// shape so call sites stay symmetric. In the browser this is ALWAYS
    /// `false`: casual-only means there is no hardcore session to protect, so
    /// the soft affordances (save-state load / rewind / cheats) are never
    /// blocked. This is a structural guarantee, not a runtime decision.
    #[must_use]
    pub const fn hardcore_blocks(&self) -> bool {
        false
    }

    /// Begin a casual login through the auth proxy (v1.7.0 H1). No-op + returns
    /// `false` when the proxy is not configured / the module is not initialized.
    /// Completion is observed by later [`Self::do_frame`] polls (the login
    /// request is carried by the server-call trampoline in `ra_glue.js`).
    pub async fn begin_login(&self, username: &str, password: &str) -> bool {
        if !self.initialized || !self.proxy_configured {
            return false;
        }
        ra_begin_login(username, password)
            .await
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    /// Begin identifying + loading a game from its ROM bytes (v1.7.0 H1).
    /// Returns `false` when the module is not initialized.
    pub async fn load_game(&self, rom: &[u8]) -> bool {
        if !self.initialized {
            return false;
        }
        ra_load_game(rom)
            .await
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    /// Drive one frame of achievement logic (v1.7.0 H1), reading NES CPU-bus
    /// bytes through `read`. Returns the titles of any achievement-unlock
    /// events raised this frame (for HUD toasts). No-op (empty) when the module
    /// is not initialized.
    ///
    /// The `read` closure is wrapped in a JS function for the side module's
    /// read-memory trampoline; it is only invoked synchronously inside the
    /// `ra_do_frame` call, so the borrow never escapes.
    pub fn do_frame(&self, read: &mut dyn FnMut(u16) -> u8) -> Vec<String> {
        if !self.initialized {
            return Vec::new();
        }
        // `Closure::new` requires a `'static` body, but our `read` borrows the
        // emulator and only needs to be valid for this synchronous call.
        // `ra_do_frame` invokes the JS callback ONLY synchronously while we are
        // inside it (the browser is single-threaded and the side module never
        // retains the function pointer past the call), then we drop the closure
        // before returning. So we erase the borrow's lifetime to `'static`,
        // matching the native bridge's `ReadGuard` idiom.
        //
        // SAFETY: the erased pointer is only dereferenced by `ra_do_frame`
        // synchronously below, and `closure` is dropped at the end of this
        // function, so the call can never outlive the real borrow.
        #[allow(unsafe_code)] // localized lifetime erasure; see SAFETY above
        let read_static: &mut (dyn FnMut(u16) -> u8 + 'static) =
            unsafe { core::mem::transmute(read) };
        let closure = Closure::<dyn FnMut(u32) -> u32>::new(move |addr: u32| {
            u32::from(read_static((addr & 0xffff) as u16))
        });
        let json = ra_do_frame(closure.as_ref().unchecked_ref());
        drop(closure);
        parse_unlock_titles(&json)
    }

    /// The loud, persistent in-UI caveat banner text. Always casual-only +
    /// experimental; additionally flags when the auth proxy is not configured.
    #[must_use]
    pub const fn caveat_banner(&self) -> &'static str {
        if self.proxy_configured {
            "RetroAchievements (browser): EXPERIMENTAL and casual-only. Hardcore \
             is unavailable in the browser (DevTools can patch the running wasm) \
             — use the native build for hardcore."
        } else {
            "RetroAchievements (browser): EXPERIMENTAL and casual-only. Hardcore \
             is native-only. The auth proxy is not configured, so login + \
             unlocks are unavailable here."
        }
    }
}

impl Drop for BrowserRaSession {
    fn drop(&mut self) {
        if self.initialized {
            ra_shutdown();
        }
    }
}

/// rc_client `RC_CLIENT_EVENT_ACHIEVEMENT_TRIGGERED`.
const RC_CLIENT_EVENT_ACHIEVEMENT_TRIGGERED: f64 = 1.0;

/// Parse the JSON event array `ra_do_frame` returns and extract the titles of
/// achievement-unlock events (the only ones the casual HUD toasts). Tolerant of
/// a malformed / empty payload (returns an empty list).
fn parse_unlock_titles(json: &str) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(parsed) = js_sys::JSON::parse(json) else {
        return out;
    };
    let Ok(arr) = parsed.dyn_into::<js_sys::Array>() else {
        return out;
    };
    for ev in arr.iter() {
        let ty = js_sys::Reflect::get(&ev, &JsValue::from_str("type"))
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        if (ty - RC_CLIENT_EVENT_ACHIEVEMENT_TRIGGERED).abs() < f64::EPSILON {
            let title = js_sys::Reflect::get(&ev, &JsValue::from_str("title"))
                .ok()
                .and_then(|v| v.as_string())
                .unwrap_or_default();
            out.push(title);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // These run on the host (cargo test compiles the lib for the host target),
    // so the `extern` JS imports are never called here — we only exercise the
    // pure-Rust invariants. The wasm-only `extern` block is `cfg`-compiled away
    // on non-wasm because the whole module is gated on `target_arch = "wasm32"`
    // in `lib.rs`; this test module therefore only compiles under wasm tooling.
    // It documents the structural casual-only guarantee.

    #[test]
    fn hardcore_never_blocks_in_browser() {
        let s = BrowserRaSession::new();
        assert!(
            !s.hardcore_blocks(),
            "browser RA is casual-only: it must never block soft affordances"
        );
    }

    #[test]
    fn caveat_is_always_loud() {
        let s = BrowserRaSession::new();
        let banner = s.caveat_banner();
        assert!(banner.contains("casual-only"));
        assert!(banner.to_lowercase().contains("experimental"));
    }

    #[test]
    fn not_configured_caveat_flags_login_unavailable() {
        // A fresh session has no proxy configured, so the banner must additionally
        // tell the user login + unlocks are unavailable (nothing silently pretends
        // to work — the load-bearing honesty invariant of ADR 0015).
        let s = BrowserRaSession::new();
        assert!(!s.proxy_configured());
        assert!(s.caveat_banner().to_lowercase().contains("not configured"));
    }

    #[test]
    fn parse_unlock_titles_extracts_only_triggered_events() {
        // Marshalling contract: `ra_do_frame` returns a JSON event array where each
        // event has a numeric `type` and a `title`. Only ACHIEVEMENT_TRIGGERED
        // (type == 1) events are toasted; other event types (progress, leaderboard,
        // etc.) must be ignored. This pins the wasm boundary payload shape without a
        // browser (the pure-Rust parser runs on the host).
        let json = r#"[
            {"type":1,"title":"First Blood"},
            {"type":2,"title":"Some Progress Indicator"},
            {"type":1,"title":"Speedrun"}
        ]"#;
        let titles = parse_unlock_titles(json);
        assert_eq!(
            titles,
            vec!["First Blood".to_string(), "Speedrun".to_string()]
        );
    }

    #[test]
    fn parse_unlock_titles_tolerates_malformed_payload() {
        // A malformed / empty payload must never panic across the wasm boundary — it
        // degrades to "no unlocks this frame".
        assert!(parse_unlock_titles("").is_empty());
        assert!(parse_unlock_titles("not json").is_empty());
        assert!(parse_unlock_titles("{}").is_empty());
        assert!(parse_unlock_titles("[]").is_empty());
    }
}
