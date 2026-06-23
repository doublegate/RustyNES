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
//!      session (8 min), extendable +2 min by each completed rewarded ad, capped at 11
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
//!     // pause; offer "Watch ad for +2 min" only if policy.can_offer_rewarded(),
//!     // else offer only "Buy Full Version". On the rewarded reward callback:
//!     policy.grant_rewarded_time();                        // +2 min (capped at 11)
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
/// *play-time budget* and the rewarded "+2 min per ad" extension (see the play-time
/// methods on [`AdPolicy`]).
///
/// Generated binding names:
///   * Kotlin: `data class AdConfig(minIntervalMs: ULong, launchGraceMs: ULong,
///     basePlayMs: ULong, rewardPlayMs: ULong, maxRewardGrantsPerSession: UInt)`
///   * Swift:  `struct AdConfig { var minIntervalMs: UInt64; var launchGraceMs: UInt64;
///     var basePlayMs: UInt64; var rewardPlayMs: UInt64; var maxRewardGrantsPerSession: UInt32 }`
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
}

impl Default for AdConfig {
    /// Conservative defaults tuned for an emulator: long, focused play sessions where
    /// an interruption is more jarring than in a casual game. 4-minute spacing with a
    /// 30-second launch grace keeps ads from ever bracketing app startup.
    ///
    /// Free-tier defaults: an 8-minute base budget, +2 minutes per completed rewarded
    /// ad, capped at 11 grants per session — so a fully ad-engaged free user reaches at
    /// most 8 + (11 × 2) = 30 minutes of play in a single game session.
    fn default() -> Self {
        Self {
            min_interval_ms: 240_000,          // 4 minutes
            launch_grace_ms: 30_000,           // 30 seconds
            base_play_ms: 480_000,             // 8 minutes
            reward_play_ms: 120_000,           // 2 minutes
            max_reward_grants_per_session: 11, // → +22 min max → 30 min total
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

/// The set of features that are gated behind the **Full Unlock** entitlement. Centralizing
/// the list here is what guarantees Android and iOS gate the identical set.
///
/// These are exactly the **three persistence locks** the locked RustyNES v1.8.0 Android plan
/// disables in the free demo (everything else — full accuracy, video, audio, input, **pause,
/// fast-forward, rewind**, the whole ROM library — is identical in both tiers and stays free):
///   * `SaveStates`       → the F1/F4 save/load slots + the thumbnail Save-States manager.
///   * `SaveOnExitResume` → write-an-`auto`-state on background + auto-resume on relaunch.
///   * `BatterySaves`     → persisting on-cart battery-backed SRAM (and FDS RAM) to disk.
///
/// The free tier is also time-gated to an 8-minute demo session (see the play-time methods on
/// [`AdPolicy`]); purchasing Full Unlock removes the timer and lifts all three locks at once.
///
/// Note: in-session rewind (the 600-frame ring) and fast-forward are **free** — they are not
/// persistence and the plan keeps them in the demo. RetroAchievements is deferred from the
/// Android MVP, so its hardcore-mode save/rewind disabling is a later-increment concern.
///
/// Generated binding names:
///   * Kotlin: `enum class PremiumFeature { SAVE_STATES, SAVE_ON_EXIT_RESUME, BATTERY_SAVES }`
///   * Swift:  `enum PremiumFeature { case saveStates; case saveOnExitResume; case batterySaves }`
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PremiumFeature {
    /// Save / load emulator save-states (F1/F4 + the Save-States manager). Demo: disabled.
    SaveStates,
    /// Save-on-background (`onPause` writes an `auto` state) + auto-resume on relaunch.
    /// Demo: disabled (a ROM never auto-resumes; no `auto` state is written).
    SaveOnExitResume,
    /// Persisting on-cart battery-backed SRAM (and FDS RAM) to disk so progress survives a
    /// close. Demo: never written to disk (in-session battery RAM still works).
    BatterySaves,
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
            | PremiumFeature::BatterySaves => self.state.lock().unwrap().is_premium,
        }
    }

    // ---- Free-tier play-time budget + rewarded extension --------------------------
    //
    // These five calls implement the free-tier time gate: a base budget per game
    // session, extended +reward_play_ms by each completed rewarded ad, capped at
    // max_reward_grants_per_session grants. Premium bypasses the gate entirely. As with
    // the interstitial pacing, the host injects elapsed time (`add_active_time`) rather
    // than the core reading a clock, so the logic stays pure and is unit-tested below.

    /// Arm the play budget for a new game session: set it to the base allotment and
    /// reset both the consumed-time counter and the per-session rewarded-grant counter.
    /// The host calls this when a game (ROM) actually begins playing.
    ///
    /// Generated binding names: `startPlay()` (both languages).
    pub fn start_play(&self) {
        let mut s = self.state.lock().unwrap();
        s.budget_ms = self.cfg.base_play_ms;
        s.consumed_ms = 0;
        s.reward_grants_this_session = 0; // the cap resets each game session
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
    /// "3 ad-extensions left". Returns 0 for premium users (they never need them) only
    /// incidentally — gate the offer on [`Self::can_offer_rewarded`], not on this value.
    ///
    /// Generated binding names: `rewardGrantsRemaining()` (both languages).
    pub fn reward_grants_remaining(&self) -> u32 {
        let s = self.state.lock().unwrap();
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
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny fixed config makes the timing assertions easy to read. The play-time
    /// values here are deliberately small (8s base, 2s reward, cap 3) so the tests run
    /// instantly; the *production* constants (8 min / 2 min / 11) are pinned separately
    /// in `default_config_encodes_the_30_minute_contract`.
    fn cfg() -> AdConfig {
        AdConfig {
            min_interval_ms: 1_000,
            launch_grace_ms: 100,
            base_play_ms: 8_000,
            reward_play_ms: 2_000,
            max_reward_grants_per_session: 3,
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
        assert!(!p.feature_enabled(PremiumFeature::SaveStates));
        assert!(!p.feature_enabled(PremiumFeature::SaveOnExitResume));
        assert!(!p.feature_enabled(PremiumFeature::BatterySaves));
        p.set_premium(true);
        assert!(p.feature_enabled(PremiumFeature::SaveStates));
        assert!(p.feature_enabled(PremiumFeature::SaveOnExitResume));
        assert!(p.feature_enabled(PremiumFeature::BatterySaves));
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
    }

    #[test]
    fn default_config_encodes_the_30_minute_contract() {
        // Pin the production constants: 8-min base, +2-min grants, 11-grant cap, which
        // is exactly 8 + 11*2 = 30 minutes of maximum free play per game session.
        let c = default_ad_config();
        assert_eq!(c.base_play_ms, 480_000);
        assert_eq!(c.reward_play_ms, 120_000);
        assert_eq!(c.max_reward_grants_per_session, 11);
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
}
