//! monetization.rs — Cross-platform entitlement & ad-pacing policy for RustyNES.
//!
//! # Monetization model (PRIMARY: ad-supported freemium)
//! The chosen model is an **ad-supported freemium** built on **RevenueCat** (entitlement /
//! billing) + **AppLovin MAX** (ad mediation). This is a deliberate maintainer override of the
//! ad-free default sketched in `to-dos/plans/v1.8.0-android-plan.md`: instead of a pure demo
//! timer, the free tier shows **interstitials** at natural breaks and offers **rewarded ads**
//! that extend play time, and the paid tier is a one-time **"Full Version / Remove Ads"**
//! purchase (**$3.99**) keyed to the RevenueCat `premium` entitlement. Every method on the
//! types below is in scope. See `docs/rustynes-integration.md` for how this maps onto the real
//! RustyNES repo (the Compose + wgpu-`SurfaceView` hybrid app, the `rustynes-mobile` bridge).
//!
//! # Purpose
//! This module is the single source of truth for the monetization behavior that MUST
//! stay identical between the Android and iOS builds:
//!
//!   1. **Entitlement state** — is the current user a paying ("premium") customer?
//!   2. **Ad pacing** — given that they are *not* premium, is *now* an acceptable
//!      moment to show an interstitial ad? (Paced by a launch grace + a minimum interval;
//!      there is no per-session interstitial count cap.)
//!   3. **Free-tier play-time gate** — a free user gets a base play budget per game
//!      session (8 min), extendable +11 min by each completed rewarded ad, capped at 2
//!      grants/session (→ 30 min max). Premium removes the gate entirely.
//!
//! The platform shells (Kotlin / Swift) own the *plumbing*: they talk to **RevenueCat** for the
//! entitlement and to **AppLovin MAX** for the actual ad load/show, then feed facts in
//! (`set_premium`, `notify_interstitial_shown`) and ask questions (`should_show_interstitial`,
//! `feature_enabled`). They own no policy. Because both shells call the *same* Rust object
//! through generated UniFFI bindings, the cadence rules and the paid-feature set cannot drift
//! between platforms — the cross-platform-share rationale behind the planned `rustynes-mobile`
//! bridge.
//!
//! # Why time is injected
//! The host passes a monotonic millisecond timestamp (`now_ms`) into every
//! time-dependent call rather than letting this module read a clock. That keeps the
//! pacing logic pure and deterministic, so the unit tests below fully exercise it
//! without mocking a system clock — the same discipline used for the emulator core.
//!
//! On the host side, "monotonic milliseconds" means:
//!   * Android: `android.os.SystemClock.elapsedRealtime()`  (Long → ULong)
//!   * iOS:     `DispatchTime.now().uptimeNanoseconds / 1_000_000`  (UInt64)
//!
//! # Usage (host pseudocode)
//! ```text
//! let policy = AdPolicy::new(AdConfig::default(), now_ms);  // once, at launch
//! policy.set_premium(rc_entitlement_active);               // from RevenueCat
//! if policy.should_show_interstitial(now_ms) {             // at a natural break
//!     // host loads + shows a MAX interstitial, then on "hidden":
//!     policy.notify_interstitial_shown(now_ms);
//! }
//!
//! // free-tier play-time gate, per game session:
//! policy.start_play();                                     // when a game begins
//! // ...once per second of unpaused emulation:
//! policy.add_active_time(1000);
//! if !policy.is_play_allowed() {                           // budget exhausted
//!     // pause; offer "Watch ad for +11 min" only if policy.can_offer_rewarded(),
//!     // else offer only "Buy Full Version". On the rewarded reward callback:
//!     policy.grant_rewarded_time();                        // +11 min (capped at 2)
//! }
//! ```
//!
//! All public items here are exported across the FFI boundary by `lib.rs`'s
//! `uniffi::setup_scaffolding!()`, so they appear in both the Kotlin and the Swift
//! bindings with the names documented inline below.
//!
//! # Determinism boundary (RustyNES-specific, load-bearing)
//! RustyNES guarantees bit-identical output for a given (ROM, input, seed) — the contract
//! that makes save-state round-trips, rollback netplay, TAS replay, and RetroAchievements
//! correct. NONE of this module's state (premium flag, play budget, ad cadence, the
//! host-injected `now_ms`) may ever flow into `rustynes-core::Bus` or the scheduler.
//! Monetization is strictly a frontend/host concern that reads emulator wall-time and
//! pauses the emulation thread; it must not influence emulated state. Keep it that way.

use std::sync::Mutex;

/// Tunable pacing parameters, exposed to the host so the values can be sourced from
/// a remote config / experiment without rebuilding the Rust core.
///
/// The first two fields pace *interstitials*; the last three define the free-tier
/// *play-time budget* and the rewarded "+11 min per ad" extension (see the play-time
/// methods on [`AdPolicy`]).
///
/// Generated binding names (Kotlin `data class` / Swift `struct`): `minIntervalMs`,
/// `launchGraceMs`, `basePlayMs`, `rewardPlayMs`, `maxRewardGrantsPerSession`,
/// `firstSessionPlayMs`, `suppressFirstSession`, `offlineGraceMs` — `ULong`/`UInt64` for the
/// `*_ms` fields, `UInt`/`UInt32` for the grant cap, `Boolean`/`Bool` for the suppress flag.
#[derive(Debug, Clone, uniffi::Record)]
pub struct AdConfig {
    /// Minimum elapsed time between two interstitials, in milliseconds.
    pub min_interval_ms: u64,
    /// Quiet period immediately after launch during which no interstitial is shown.
    pub launch_grace_ms: u64,
    /// Free-tier base play budget granted at the start of each game session, in ms.
    pub base_play_ms: u64,
    /// Play time granted per *completed* rewarded ad, in ms.
    pub reward_play_ms: u64,
    /// Maximum number of rewarded "+time" grants allowed per game session. Once this
    /// many grants have been given, the rewarded offer is withdrawn and only the
    /// Full Version prompt remains.
    pub max_reward_grants_per_session: u32,
    /// First-session play budget, in ms — applied by [`AdPolicy::start_play`] when the
    /// session index is 1, so a brand-new user gets a generous (or ungated) first game
    /// before the timer bites. Set very large for an effectively ungated first session.
    pub first_session_play_ms: u64,
    /// When `true`, no interstitial is shown during session #1 (protect the first
    /// impression). Paired with the session index fed via [`AdPolicy::begin_session`].
    pub suppress_first_session: bool,
    /// One-time, per-game-session "offline grace" budget, in ms — granted by
    /// [`AdPolicy::grant_offline_grace`] when a free user hits the wall but no rewarded ad
    /// can load (offline / no fill), so the session degrades gracefully instead of
    /// dead-ending. `0` disables the grace.
    pub offline_grace_ms: u64,
}

impl Default for AdConfig {
    /// Conservative defaults tuned for an emulator: long, focused play sessions where
    /// an interruption is more jarring than in a casual game. 4-minute spacing with a
    /// 30-second launch grace keeps ads from ever bracketing app startup.
    ///
    /// Free-tier defaults: an **8-minute** base budget (regular sessions) with a generous
    /// **30-minute** first session, **+11 minutes per completed rewarded ad, capped at 2
    /// grants** per session — so a fully ad-engaged free user reaches at most 8 + (2 × 11) =
    /// 30 minutes of play in a regular game session with only two ad interactions.
    fn default() -> Self {
        Self {
            min_interval_ms: 240_000,         // 4 minutes
            launch_grace_ms: 30_000,          // 30 seconds
            base_play_ms: 480_000,            // 8 minutes (regular free session)
            reward_play_ms: 660_000,          // 11 minutes per rewarded ad
            max_reward_grants_per_session: 2, // → +22 min max (2 × 11) → 30 min total
            first_session_play_ms: 1_800_000, // 30 minutes — generous first game
            suppress_first_session: true,     // no interstitials in session #1
            offline_grace_ms: 120_000,        // a one-time +2 min when offline at run-out
        }
    }
}

/// Provide `AdConfig::default()` to the foreign side as a free function, because
/// UniFFI Records do not carry methods across the FFI. Hosts that want the tuned
/// defaults call this instead of hand-constructing the struct.
///
/// Generated binding names: `defaultAdConfig()` (Kotlin) / `defaultAdConfig()` (Swift).
#[uniffi::export]
pub fn default_ad_config() -> AdConfig {
    AdConfig::default()
}

/// The set of features that are gated behind the **Full Version** entitlement. Centralizing
/// the list here is what guarantees Android and iOS gate the identical set.
///
/// **Two groups (maintainer decision 2026-06-23 — "expand the premium set"):**
///
/// *Persistence* (the original three locks the free demo disables):
///   * `SaveStates`       → the F1/F4 save/load slots + the thumbnail Save-States manager.
///   * `SaveOnExitResume` → write-an-`auto`-state on background + auto-resume on relaunch.
///   * `BatterySaves`     → persisting on-cart battery-backed SRAM (and FDS RAM) to disk.
///
/// *Power features* (newly premium — this **overrides** the earlier doc stance that
/// fast-forward was free; the free tier keeps full accuracy, video, audio, input, pause,
/// and in-session rewind, but these power tools now require the unlock):
///   * `FastForward`      → the fast-forward / turbo speed toggle.
///   * `Shaders`          → the NTSC / CRT / scanline / Bisqwit shader stack (free = plain).
///   * `Cheats`           → Game Genie + raw-RAM cheat entry.
///
/// The free tier is also time-gated per game session (see the play-time methods on
/// [`AdPolicy`]); purchasing the Full Version removes the timer and lifts all six locks.
/// RetroAchievements is deferred from the Android MVP, so its hardcore-mode save/rewind
/// disabling is a later-increment concern.
///
/// Generated binding names:
///   * Kotlin: `enum class PremiumFeature { SAVE_STATES, SAVE_ON_EXIT_RESUME, BATTERY_SAVES,
///     FAST_FORWARD, SHADERS, CHEATS }`
///   * Swift:  `enum PremiumFeature { case saveStates; …; case fastForward; case shaders; case cheats }`
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PremiumFeature {
    /// Save / load emulator save-states (F1/F4 + the Save-States manager). Free: disabled.
    SaveStates,
    /// Save-on-background (`onPause` writes an `auto` state) + auto-resume on relaunch.
    /// Free: disabled (a ROM never auto-resumes; no `auto` state is written).
    SaveOnExitResume,
    /// Persisting on-cart battery-backed SRAM (and FDS RAM) to disk so progress survives a
    /// close. Free: never written to disk (in-session battery RAM still works).
    BatterySaves,
    /// Fast-forward / turbo speed. Free: disabled (normal-speed play only).
    FastForward,
    /// The NTSC / CRT / scanline / Bisqwit shader stack. Free: plain (unfiltered) output.
    Shaders,
    /// Game Genie + raw-RAM cheat entry. Free: disabled.
    Cheats,
}

/// Interior, mutable state guarded by a `Mutex` so the host may call from any thread
/// (RevenueCat callbacks, ad callbacks, and the UI thread can all touch it).
struct State {
    /// Whether the premium entitlement is currently active.
    is_premium: bool,
    /// Monotonic timestamp captured at construction; anchors the launch grace window.
    launched_at_ms: u64,
    /// Monotonic timestamp of the last interstitial that was actually shown.
    last_shown_ms: Option<u64>,
    /// Total free-tier play budget granted for the current game session, in ms
    /// (base + any rewarded extensions). Reset by `start_play`.
    budget_ms: u64,
    /// Active (unpaused) play time consumed in the current game session, in ms.
    consumed_ms: u64,
    /// Number of rewarded "+time" grants already given in the current game session;
    /// compared against `AdConfig::max_reward_grants_per_session` to enforce the cap.
    reward_grants_this_session: u32,
    /// The app-session index (1 on the very first launch, incremented by the host each
    /// app session via `begin_session`). Drives first-session interstitial suppression
    /// and the generous first-session play budget.
    session_index: u32,
    /// Whether this game session's one-time offline-grace continuation has been spent.
    offline_grace_used: bool,
}

/// The policy object the host constructs once and holds for the app's lifetime.
///
/// UniFFI represents this as a reference-counted handle:
///   * Kotlin: `class AdPolicy(config: AdConfig, nowMs: ULong) : Disposable`
///   * Swift:  `class AdPolicy { init(config: AdConfig, nowMs: UInt64) }`
///
/// Construct with [`AdPolicy::new`].
#[derive(uniffi::Object)]
pub struct AdPolicy {
    cfg: AdConfig,
    state: Mutex<State>,
}

#[uniffi::export]
impl AdPolicy {
    /// Build a policy. `now_ms` is the host's current monotonic clock reading and
    /// becomes the anchor for the launch-grace window.
    ///
    /// Generated binding names: `AdPolicy(config, nowMs)` (Kotlin) /
    /// `AdPolicy(config:nowMs:)` (Swift).
    #[uniffi::constructor]
    pub fn new(config: AdConfig, now_ms: u64) -> Self {
        Self {
            cfg: config,
            state: Mutex::new(State {
                is_premium: false, // assume free until billing confirms otherwise
                launched_at_ms: now_ms,
                last_shown_ms: None,
                // Play budget is armed by `start_play` when a game actually starts; a
                // freshly constructed policy has no active game session yet.
                budget_ms: 0,
                consumed_ms: 0,
                reward_grants_this_session: 0,
                // Default to session 1 until the host calls `begin_session` with the
                // persisted count; conservative (treats an un-counted launch as the first).
                session_index: 1,
                offline_grace_used: false,
            }),
        }
    }

    /// Record the latest premium-entitlement state, as reported by RevenueCat
    /// (`CustomerInfo.entitlements["premium"].isActive`). Setting `true` makes every
    /// ad gate below return `false`, so ads stop immediately and without an app
    /// restart — the upgrade feels instantaneous to the user.
    ///
    /// Generated binding names: `setPremium(premium)` (both languages).
    pub fn set_premium(&self, premium: bool) {
        self.state.lock().unwrap().is_premium = premium;
    }

    /// Current paid-tier status. The UI uses this to hide ad containers up front and
    /// to reflect entitlement in menus.
    ///
    /// Generated binding names: `isPremium()` (both languages).
    pub fn is_premium(&self) -> bool {
        self.state.lock().unwrap().is_premium
    }

    /// Record the app-session index at launch (the host persists a counter in
    /// SharedPreferences / UserDefaults: 1 on first ever launch, +1 each app session) and
    /// re-anchor the launch-grace window to `now_ms`. Drives first-session interstitial
    /// suppression (`suppress_first_session`) and the generous first-session play budget
    /// (`first_session_play_ms`, applied by the next [`Self::start_play`]).
    ///
    /// Generated binding names: `beginSession(sessionIndex, nowMs)` (both languages).
    pub fn begin_session(&self, session_index: u32, now_ms: u64) {
        let mut s = self.state.lock().unwrap();
        s.session_index = session_index;
        s.launched_at_ms = now_ms;
    }

    /// The core decision: should the host present an interstitial *right now*?
    ///
    /// Returns `false` if any of the following hold:
    ///   * the user is premium (paid users never see ads),
    ///   * we are still inside the post-launch grace window, or
    ///   * not enough time has elapsed since the previous interstitial.
    ///
    /// The host should call this only at *natural* break points (ROM loaded, returned
    /// to the menu, save-state taken) — never mid-frame.
    ///
    /// Generated binding names: `shouldShowInterstitial(nowMs)` (both languages).
    pub fn should_show_interstitial(&self, now_ms: u64) -> bool {
        let s = self.state.lock().unwrap();

        if s.is_premium {
            return false; // paid → never
        }
        if self.cfg.suppress_first_session && s.session_index <= 1 {
            return false; // protect the first impression — no interstitials in session #1
        }
        if now_ms.saturating_sub(s.launched_at_ms) < self.cfg.launch_grace_ms {
            return false; // too soon after launch
        }
        match s.last_shown_ms {
            None => true, // first eligible break since launch
            Some(last) => now_ms.saturating_sub(last) >= self.cfg.min_interval_ms,
        }
    }

    /// Arm the cooldown. The host calls this immediately after an interstitial is
    /// *actually displayed* (e.g. AppLovin's `didDisplay` / `onAdDisplayed`), not when
    /// it merely decides to load one. Keeping this separate from
    /// [`Self::should_show_interstitial`] means a failed ad load does not consume the
    /// interval, so the next break point can retry.
    ///
    /// Generated binding names: `notifyInterstitialShown(nowMs)` (both languages).
    pub fn notify_interstitial_shown(&self, now_ms: u64) {
        self.state.lock().unwrap().last_shown_ms = Some(now_ms);
    }

    /// Whether a given premium feature is currently unlocked. This is the single
    /// authority both shells consult before enabling save-states, battery saves, etc.
    ///
    /// Generated binding names: `featureEnabled(feature)` (both languages).
    pub fn feature_enabled(&self, feature: PremiumFeature) -> bool {
        // The free tier keeps the full, accurate emulator; only conveniences and
        // persistence are paywalled. Every gated feature follows the entitlement.
        match feature {
            PremiumFeature::SaveStates
            | PremiumFeature::SaveOnExitResume
            | PremiumFeature::BatterySaves
            | PremiumFeature::FastForward
            | PremiumFeature::Shaders
            | PremiumFeature::Cheats => self.state.lock().unwrap().is_premium,
        }
    }

    // ---- Free-tier play-time budget + rewarded extension --------------------------
    //
    // These five calls implement the free-tier time gate: a base budget per game
    // session, extended +reward_play_ms by each completed rewarded ad, capped at
    // max_reward_grants_per_session grants. Premium bypasses the gate entirely. As with
    // the interstitial pacing, the host injects elapsed time (`add_active_time`) rather
    // than the core reading a clock, so the logic stays pure and is unit-tested below.

    /// Arm the play budget for a new game session: set it to the allotment (the generous
    /// `first_session_play_ms` during session #1, else `base_play_ms`) and reset the
    /// consumed-time counter, the per-session rewarded-grant counter, and the one-time
    /// offline-grace flag. The host calls this when a game (ROM) actually begins playing.
    ///
    /// Generated binding names: `startPlay()` (both languages).
    pub fn start_play(&self) {
        let mut s = self.state.lock().unwrap();
        s.budget_ms = if s.session_index <= 1 {
            self.cfg.first_session_play_ms
        } else {
            self.cfg.base_play_ms
        };
        s.consumed_ms = 0;
        s.reward_grants_this_session = 0; // the cap resets each game session
        s.offline_grace_used = false; // one offline grace per game session
    }

    /// Report active (unpaused) play time elapsed, in ms — typically once per second of
    /// running emulation. The host already pauses emulation for ads, the run-out prompt,
    /// and app-backgrounding, so it simply stops calling this while paused; the core
    /// never reads a clock and so stays deterministic and pause-agnostic.
    ///
    /// Generated binding names: `addActiveTime(deltaMs)` (both languages).
    pub fn add_active_time(&self, delta_ms: u64) {
        let mut s = self.state.lock().unwrap();
        s.consumed_ms = s.consumed_ms.saturating_add(delta_ms);
    }

    /// Whether a rewarded "+time" offer should be presented right now: true only for a
    /// free user who is still under the per-session grant cap. Once this returns false,
    /// the host should show *only* the Full Version prompt at the run-out.
    ///
    /// Generated binding names: `canOfferRewarded()` (both languages).
    pub fn can_offer_rewarded(&self) -> bool {
        let s = self.state.lock().unwrap();
        !s.is_premium && s.reward_grants_this_session < self.cfg.max_reward_grants_per_session
    }

    /// How many rewarded extensions remain in this game session. Useful for UI such as
    /// "3 ad-extensions left". Returns **0 for premium users** (they have no rewarded offer),
    /// so the value can be shown directly without misreporting "11 left" to a paid user; the
    /// offer itself is still gated on [`Self::can_offer_rewarded`].
    ///
    /// Generated binding names: `rewardGrantsRemaining()` (both languages).
    pub fn reward_grants_remaining(&self) -> u32 {
        let s = self.state.lock().unwrap();
        if s.is_premium {
            return 0; // premium has no rewarded offer; never report grants "remaining"
        }
        self.cfg
            .max_reward_grants_per_session
            .saturating_sub(s.reward_grants_this_session)
    }

    /// Grant one rewarded extension (`reward_play_ms`) to the current session, enforcing
    /// the per-session cap. Call this **only** from the ad network's *reward* callback
    /// (AppLovin `OnUserRewarded` / `didRewardUser`) — never on ad load, show, or
    /// dismiss — so the grant maps exactly to an ad the user watched for the required
    /// duration. Returns `true` if the grant was applied, or `false` if the cap had
    /// already been reached (in which case it is a no-op).
    ///
    /// Generated binding names: `grantRewardedTime()` (both languages).
    pub fn grant_rewarded_time(&self) -> bool {
        let mut s = self.state.lock().unwrap();
        if s.reward_grants_this_session >= self.cfg.max_reward_grants_per_session {
            return false; // cap reached — no more free extensions this session
        }
        s.budget_ms = s.budget_ms.saturating_add(self.cfg.reward_play_ms);
        s.reward_grants_this_session += 1;
        true
    }

    /// Whether the user may keep playing right now. Premium is always allowed; a free
    /// user is allowed while consumed time is below the granted budget.
    ///
    /// Generated binding names: `isPlayAllowed()` (both languages).
    pub fn is_play_allowed(&self) -> bool {
        let s = self.state.lock().unwrap();
        s.is_premium || s.consumed_ms < s.budget_ms
    }

    /// Remaining free-tier play time, in ms. Returns `None` for premium users to signal
    /// "unlimited". Drive the on-screen countdown from this value.
    ///
    /// Generated binding names: `playTimeRemainingMs(): ULong?` (Kotlin) /
    /// `playTimeRemainingMs() -> UInt64?` (Swift).
    pub fn play_time_remaining_ms(&self) -> Option<u64> {
        let s = self.state.lock().unwrap();
        if s.is_premium {
            None
        } else {
            Some(s.budget_ms.saturating_sub(s.consumed_ms))
        }
    }

    // ---- Offline grace ------------------------------------------------------------
    //
    // When a free user hits the wall but no rewarded ad can load (offline / no fill), a
    // one-time `offline_grace_ms` continuation keeps the session from dead-ending. It is
    // capped at once per game session (reset by `start_play`) so it can't be farmed.

    /// Whether a one-time offline-grace continuation is available right now: a free user
    /// who hasn't used it this session, with a non-zero `offline_grace_ms` configured.
    /// The host calls this at the run-out when a rewarded ad failed to load.
    ///
    /// Generated binding names: `canGrantOfflineGrace()` (both languages).
    pub fn can_grant_offline_grace(&self) -> bool {
        let s = self.state.lock().unwrap();
        !s.is_premium && !s.offline_grace_used && self.cfg.offline_grace_ms > 0
    }

    /// Grant the one-time offline-grace continuation (`offline_grace_ms`) for this game
    /// session. Returns `true` if applied, `false` if premium, already used, or disabled.
    ///
    /// Generated binding names: `grantOfflineGrace()` (both languages).
    pub fn grant_offline_grace(&self) -> bool {
        let mut s = self.state.lock().unwrap();
        if s.is_premium || s.offline_grace_used || self.cfg.offline_grace_ms == 0 {
            return false;
        }
        s.budget_ms = s.budget_ms.saturating_add(self.cfg.offline_grace_ms);
        s.offline_grace_used = true;
        true
    }

    // ---- In-progress persistence --------------------------------------------------
    //
    // The core is in-memory: it forgets the budget/consumed/grant counters when the
    // process dies, so a free user could kill-and-relaunch the same ROM for a fresh
    // budget. The host can close that hole by persisting `export_progress()` (e.g. in
    // SharedPreferences / UserDefaults, keyed by ROM) and `restore_progress()` before the
    // next `start_play()` of the same ROM. Skip it to accept restart-to-reset as intended.

    /// Snapshot the current game session's play-gate state for host persistence.
    ///
    /// Generated binding names: `exportProgress()` (both languages).
    pub fn export_progress(&self) -> PlayProgress {
        let s = self.state.lock().unwrap();
        PlayProgress {
            budget_ms: s.budget_ms,
            consumed_ms: s.consumed_ms,
            reward_grants_this_session: s.reward_grants_this_session,
            offline_grace_used: s.offline_grace_used,
        }
    }

    /// Restore a previously-[`Self::export_progress`]'d snapshot (e.g. after a relaunch),
    /// so the timer/cap survive a kill mid-run. Call instead of (or right after)
    /// `start_play` when resuming the same ROM. Ignored for premium (no gate to restore).
    ///
    /// Generated binding names: `restoreProgress(progress)` (both languages).
    pub fn restore_progress(&self, progress: PlayProgress) {
        let mut s = self.state.lock().unwrap();
        if s.is_premium {
            return;
        }
        s.budget_ms = progress.budget_ms;
        s.consumed_ms = progress.consumed_ms;
        s.reward_grants_this_session = progress.reward_grants_this_session;
        s.offline_grace_used = progress.offline_grace_used;
    }
}

/// A serializable snapshot of a game session's free-tier play-gate state, for the host to
/// persist across a process kill (closing the restart-to-reset hole). UniFFI marshals it
/// as a plain record; the host stores the four fields however it likes.
///
/// Generated binding names:
///   * Kotlin: `data class PlayProgress(budgetMs: ULong, consumedMs: ULong,
///     rewardGrantsThisSession: UInt, offlineGraceUsed: Boolean)`
///   * Swift:  `struct PlayProgress { var budgetMs: UInt64; var consumedMs: UInt64;
///     var rewardGrantsThisSession: UInt32; var offlineGraceUsed: Bool }`
#[derive(Debug, Clone, Copy, uniffi::Record)]
pub struct PlayProgress {
    /// Total granted budget for the session, in ms (base/first-session + rewarded + grace).
    pub budget_ms: u64,
    /// Active play time consumed so far this session, in ms.
    pub consumed_ms: u64,
    /// Rewarded "+time" grants already given this session (counts against the cap).
    pub reward_grants_this_session: u32,
    /// Whether this session's one-time offline grace has been spent.
    pub offline_grace_used: bool,
}

/// Clamp every [`AdConfig`] field to a sane range so a bad remote-config push (e.g. a
/// 0-ms base budget, or an absurd grant cap) can't brick the gate. The host fetches remote
/// values, overlays them on [`default_ad_config`], then passes the result through this
/// before constructing the [`AdPolicy`]. Pure + total; ranges mirror the addendum §9 table.
///
/// Generated binding names: `clampAdConfig(cfg)` (both languages).
#[uniffi::export]
pub fn clamp_ad_config(cfg: AdConfig) -> AdConfig {
    AdConfig {
        min_interval_ms: cfg.min_interval_ms.clamp(60_000, 1_800_000), // 1 min .. 30 min
        launch_grace_ms: cfg.launch_grace_ms.min(600_000),             // .. 10 min
        base_play_ms: cfg.base_play_ms.clamp(60_000, 3_600_000),       // 1 min .. 60 min
        reward_play_ms: cfg.reward_play_ms.clamp(30_000, 1_200_000),   // 30 s .. 20 min
        max_reward_grants_per_session: cfg.max_reward_grants_per_session.min(50),
        // 1 min .. 4 h: floored at 60_000 so a remote `0` can't brick the first session
        // (set a large value, not 0, for an effectively ungated first game).
        first_session_play_ms: cfg.first_session_play_ms.clamp(60_000, 14_400_000),
        suppress_first_session: cfg.suppress_first_session,
        offline_grace_ms: cfg.offline_grace_ms.min(600_000), // .. 10 min; 0 disables
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny fixed config makes the timing assertions easy to read. The play-time
    /// values here are deliberately small (8s base, 2s reward, cap 3) so the tests run
    /// instantly; the *production* constants (8 min / 11 min / 2) are pinned separately
    /// in `default_config_encodes_the_30_minute_contract`.
    fn cfg() -> AdConfig {
        AdConfig {
            min_interval_ms: 1_000,
            launch_grace_ms: 100,
            base_play_ms: 8_000,
            reward_play_ms: 2_000,
            max_reward_grants_per_session: 3,
            // first-session budget == base, and first-session suppression OFF, so the
            // existing pacing/budget tests below exercise the grace/interval/budget logic
            // in isolation. The first-session behaviour has its own dedicated tests.
            first_session_play_ms: 8_000,
            suppress_first_session: false,
            offline_grace_ms: 1_000,
        }
    }

    #[test]
    fn premium_users_never_see_ads() {
        let p = AdPolicy::new(cfg(), 0);
        p.set_premium(true);
        // Well past the grace window and the interval — still no ad, because premium.
        assert!(!p.should_show_interstitial(10_000));
        assert!(p.is_premium());
    }

    #[test]
    fn launch_grace_suppresses_early_ads() {
        let p = AdPolicy::new(cfg(), 0);
        assert!(!p.should_show_interstitial(50)); // inside the 100ms grace
        assert!(p.should_show_interstitial(150)); // just past it
    }

    #[test]
    fn interval_is_enforced_between_shows() {
        let p = AdPolicy::new(cfg(), 0);
        assert!(p.should_show_interstitial(150)); // first eligible break
        p.notify_interstitial_shown(150);
        assert!(!p.should_show_interstitial(800)); // 650ms later: too soon
        assert!(p.should_show_interstitial(1_150)); // 1000ms later: allowed
    }

    #[test]
    fn upgrading_mid_session_stops_ads_immediately() {
        let p = AdPolicy::new(cfg(), 0);
        assert!(p.should_show_interstitial(150));
        p.set_premium(true); // user buys "remove ads"
        assert!(!p.should_show_interstitial(150));
    }

    #[test]
    fn features_track_entitlement() {
        let p = AdPolicy::new(cfg(), 0);
        let all = [
            PremiumFeature::SaveStates,
            PremiumFeature::SaveOnExitResume,
            PremiumFeature::BatterySaves,
            PremiumFeature::FastForward,
            PremiumFeature::Shaders,
            PremiumFeature::Cheats,
        ];
        for f in all {
            assert!(!p.feature_enabled(f), "free tier must gate {f:?}");
        }
        p.set_premium(true);
        for f in all {
            assert!(p.feature_enabled(f), "premium must unlock {f:?}");
        }
    }

    // ---- Free-tier play-time budget + rewarded cap --------------------------------

    #[test]
    fn free_tier_starts_with_the_base_budget() {
        let p = AdPolicy::new(cfg(), 0);
        p.start_play();
        assert_eq!(p.play_time_remaining_ms(), Some(8_000));
        assert!(p.is_play_allowed());

        // Consume the whole base budget → no time left, play disallowed.
        p.add_active_time(8_000);
        assert_eq!(p.play_time_remaining_ms(), Some(0));
        assert!(!p.is_play_allowed());
    }

    #[test]
    fn paused_time_does_not_consume_budget() {
        let p = AdPolicy::new(cfg(), 0);
        p.start_play();
        p.add_active_time(3_000); // 3s of active play
        // Simulate a long pause (an ad, the run-out prompt, backgrounding): no calls.
        assert_eq!(p.play_time_remaining_ms(), Some(5_000));
        assert!(p.is_play_allowed());
    }

    #[test]
    fn a_rewarded_grant_extends_play() {
        let p = AdPolicy::new(cfg(), 0);
        p.start_play();
        p.add_active_time(8_000); // exhaust the base budget
        assert!(!p.is_play_allowed());

        assert!(p.grant_rewarded_time()); // +2s
        assert_eq!(p.play_time_remaining_ms(), Some(2_000));
        assert!(p.is_play_allowed());
    }

    #[test]
    fn rewarded_grants_are_capped_per_session() {
        let p = AdPolicy::new(cfg(), 0); // cap = 3 in the test config
        p.start_play();

        assert!(p.can_offer_rewarded());
        assert_eq!(p.reward_grants_remaining(), 3);

        assert!(p.grant_rewarded_time()); // 1
        assert!(p.grant_rewarded_time()); // 2
        assert!(p.grant_rewarded_time()); // 3 — cap reached
        assert_eq!(p.reward_grants_remaining(), 0);
        assert!(!p.can_offer_rewarded()); // offer withdrawn at the cap

        // The 4th grant is refused and is a no-op (budget unchanged beyond the 3 grants).
        assert!(!p.grant_rewarded_time());
        assert_eq!(p.play_time_remaining_ms(), Some(8_000 + 3 * 2_000));
    }

    #[test]
    fn start_play_resets_budget_and_cap() {
        let p = AdPolicy::new(cfg(), 0);
        p.start_play();
        p.grant_rewarded_time();
        p.grant_rewarded_time();
        p.add_active_time(5_000);

        // A new game session wipes consumed time, restores the base budget, and re-arms
        // all rewarded grants.
        p.start_play();
        assert_eq!(p.play_time_remaining_ms(), Some(8_000));
        assert_eq!(p.reward_grants_remaining(), 3);
        assert!(p.can_offer_rewarded());
    }

    #[test]
    fn premium_play_is_unlimited_and_offer_free() {
        let p = AdPolicy::new(cfg(), 0);
        p.set_premium(true);
        p.start_play();
        p.add_active_time(1_000_000); // play for ages
        assert!(p.is_play_allowed());
        assert_eq!(p.play_time_remaining_ms(), None); // None == unlimited
        assert!(!p.can_offer_rewarded()); // premium never needs the rewarded offer
        assert_eq!(p.reward_grants_remaining(), 0); // premium reports 0, not the cap
    }

    #[test]
    fn default_config_encodes_the_30_minute_contract() {
        // Pin the production constants: 8-min base, +11-min grants, 2-grant cap, which
        // is exactly 8 + 2*11 = 30 minutes of maximum free play per game session (only
        // two ad interactions instead of eleven).
        let c = default_ad_config();
        assert_eq!(c.base_play_ms, 480_000);
        assert_eq!(c.reward_play_ms, 660_000);
        assert_eq!(c.max_reward_grants_per_session, 2);
        let max_free_ms =
            c.base_play_ms + c.max_reward_grants_per_session as u64 * c.reward_play_ms;
        assert_eq!(max_free_ms, 1_800_000); // 30 minutes
    }

    #[test]
    fn granted_entitlement_fully_unlocks_app() {
        // A RevenueCat *promotional grant* and a Google Play *license-tester* sandbox
        // purchase both surface as entitlements["premium"].isActive == true, which the
        // shells forward via set_premium(true). This pins the contract a closed-test
        // cohort relies on: the single boolean unlocks every gate, with no tester-only
        // code path. (See runbook §5a / brief §9 for how testers are granted access.)
        let p = AdPolicy::new(cfg(), 0);
        p.start_play();
        p.set_premium(true); // as if RevenueCat reported a granted / sandbox entitlement

        // Ads off, all paid features on.
        assert!(!p.should_show_interstitial(10_000));
        assert!(p.feature_enabled(PremiumFeature::SaveStates));
        assert!(p.feature_enabled(PremiumFeature::SaveOnExitResume));
        assert!(p.feature_enabled(PremiumFeature::BatterySaves));

        // Timer removed: play never blocks and the run-out offer never appears.
        p.add_active_time(60 * 60 * 1000); // an hour of continuous play
        assert!(p.is_play_allowed());
        assert_eq!(p.play_time_remaining_ms(), None);
        assert!(!p.can_offer_rewarded());

        // Revoking the grant (e.g. after the 14-day test window) re-locks immediately.
        p.set_premium(false);
        assert!(!p.is_play_allowed()); // consumed time already exceeds the free budget
        assert!(!p.feature_enabled(PremiumFeature::SaveStates));
    }

    /// A config where the first session is generous (20s budget) + interstitials are
    /// suppressed, but later sessions fall back to the 8s base + normal pacing.
    fn first_session_cfg() -> AdConfig {
        AdConfig {
            first_session_play_ms: 20_000,
            suppress_first_session: true,
            ..cfg()
        }
    }

    #[test]
    fn first_session_is_generous_and_ad_free() {
        let p = AdPolicy::new(first_session_cfg(), 0);
        // Session 1 (the default): generous budget, no interstitials even past grace/interval.
        p.start_play();
        assert_eq!(p.play_time_remaining_ms(), Some(20_000));
        assert!(!p.should_show_interstitial(10_000));

        // Session 2: back to the base budget and normal interstitial pacing.
        p.begin_session(2, 0);
        p.start_play();
        assert_eq!(p.play_time_remaining_ms(), Some(8_000));
        assert!(p.should_show_interstitial(10_000));
    }

    #[test]
    fn offline_grace_is_one_time_per_session() {
        let p = AdPolicy::new(cfg(), 0); // offline_grace_ms = 1_000
        p.start_play();
        p.add_active_time(8_000); // exhaust the base budget
        assert!(!p.is_play_allowed());

        assert!(p.can_grant_offline_grace());
        assert!(p.grant_offline_grace()); // +1s
        assert_eq!(p.play_time_remaining_ms(), Some(1_000));
        assert!(p.is_play_allowed());

        // Only once per session.
        assert!(!p.can_grant_offline_grace());
        assert!(!p.grant_offline_grace());

        // A new game session re-arms it.
        p.start_play();
        assert!(p.can_grant_offline_grace());

        // Premium never needs it.
        p.set_premium(true);
        assert!(!p.can_grant_offline_grace());
        assert!(!p.grant_offline_grace());
    }

    #[test]
    fn progress_round_trips_across_a_relaunch() {
        let p = AdPolicy::new(cfg(), 0);
        p.start_play();
        p.add_active_time(3_000);
        assert!(p.grant_rewarded_time()); // budget 8_000 -> 10_000, 1 grant used
        let snap = p.export_progress();
        assert_eq!(snap.consumed_ms, 3_000);
        assert_eq!(snap.budget_ms, 10_000);
        assert_eq!(snap.reward_grants_this_session, 1);

        // Simulate a kill + relaunch: a fresh policy, restore instead of a fresh budget.
        let p2 = AdPolicy::new(cfg(), 0);
        p2.restore_progress(snap);
        assert_eq!(p2.play_time_remaining_ms(), Some(7_000)); // 10_000 - 3_000
        assert_eq!(p2.reward_grants_remaining(), 2); // cap 3, 1 used
    }

    #[test]
    fn clamp_ad_config_bounds_remote_values() {
        // A hostile/bad remote push: a 0-ms base budget would brick the gate; absurd
        // interval + cap. Clamp pulls everything back into the safe ranges.
        let bad = AdConfig {
            min_interval_ms: 10,                  // -> 60_000 floor
            base_play_ms: 0,                      // -> 60_000 floor (never a 0-ms gate)
            reward_play_ms: 1,                    // -> 30_000 floor
            max_reward_grants_per_session: 9_999, // -> 50 cap
            first_session_play_ms: 0,             // -> 60_000 floor (a 0 would brick session #1)
            ..default_ad_config()
        };
        let c = clamp_ad_config(bad);
        assert_eq!(c.min_interval_ms, 60_000);
        assert_eq!(c.base_play_ms, 60_000);
        assert_eq!(c.reward_play_ms, 30_000);
        assert_eq!(c.max_reward_grants_per_session, 50);
        assert_eq!(c.first_session_play_ms, 60_000);
        // A sane config is returned unchanged.
        assert_eq!(
            clamp_ad_config(default_ad_config()).base_play_ms,
            default_ad_config().base_play_ms
        );
    }
}
