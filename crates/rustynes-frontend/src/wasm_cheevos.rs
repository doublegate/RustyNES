//! Casual-only browser RetroAchievements session (v1.5.0 "Lens" Workstream G,
//! ADR 0015). wasm-only, behind the default-OFF `browser-cheevos` feature.
//!
//! # What this is
//!
//! The browser analog of the native [`crate::ra_session`] module — but
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

use wasm_bindgen::prelude::*;

// The host surface implemented by `web/cheevos/ra_glue.js`. These imports
// resolve when the module is loaded in the page; the Rust side compiles against
// the signatures regardless of whether the glue is deployed yet. Note the
// COMPLETE ABSENCE of any hardcore-enabling import — casual-only is structural.
#[wasm_bindgen(module = "/web/cheevos/ra_glue.js")]
extern "C" {
    /// Initialize the casual-only client. Resolves to `true` on success.
    /// Hardcore is never enabled (the toggle is unexported).
    #[wasm_bindgen(catch, js_name = ra_init)]
    async fn ra_init() -> Result<JsValue, JsValue>;

    /// `true` only when the auth proxy is configured (precondition for login /
    /// unlocks). Drives the UI "not configured" caveat.
    #[wasm_bindgen(js_name = ra_proxy_configured)]
    fn ra_proxy_configured() -> bool;

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

    /// The gating predicate, mirroring the native [`crate::ra_session`] API
    /// shape so call sites stay symmetric. In the browser this is ALWAYS
    /// `false`: casual-only means there is no hardcore session to protect, so
    /// the soft affordances (save-state load / rewind / cheats) are never
    /// blocked. This is a structural guarantee, not a runtime decision.
    #[must_use]
    pub const fn hardcore_blocks(&self) -> bool {
        false
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
}
