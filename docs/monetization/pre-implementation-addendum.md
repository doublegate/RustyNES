# RustyNES — Pre-Implementation Addendum: Optimizations, Enhancements & Ad Strategy

**Audience:** Claude Code (and the engineer/owner). Read **after** `implementation-brief.md`.
This is the "resolve before you build and ship" layer: an ad-placement strategy tuned to a
*vintage-console emulator*, ad **scoping** for this app category, performance items that are
specific to running ads alongside a cycle-accurate emulator, and product enhancements. Where
a claim is load-bearing it cites a source in §9.

> TL;DR for this app type: the retro-emulator audience is **ad-averse**. The free tier is
> **time-gated to 8 minutes** per game session (no save states, no battery saves); each
> **completed rewarded ad grants +11 minutes**, and a cheap one-time **Full Version / remove-ads**
> unlock removes the timer and unlocks saves. Lean on **rewarded** (opt-in) ads, keep
> interstitials sparing and never over the game, and suppress ads in the first session.
> Aggressive interstitials or subscriptions will tank reviews and retention for this category
> specifically.

---

## 1. Read the market before designing the ads

The flagship mobile emulators set user expectations, and that expectation is **no ads**:
- **RetroArch** ships free, open-source, and explicitly **ad-free**; its maintainers state
  they have never burdened users with in-app ads, monetization SDKs, or paywalled features.
- **Delta** (No. 1 on the iOS charts at launch) is **free with no ads and no paywalls**,
  funded by donations/Patreon.
- **Lemuroid** (Android, ~25 systems) is free and ad-free.
- When the iOS emulator category opened in 2024, several early entrants were publicly
  criticized as **"ad-riddled"** with third-party trackers — the opposite of a good look.
- The single-system emulators that *do* monetize converge on a **light-ads + ~$4–5 one-time
  "remove ads"** model (e.g. Mupen64's $4.99 remove-ads + cloud backups). Subscription-heavy
  emulators (e.g. $5.49/week) are widely mocked.

**Implication for RustyNES:** ads are *tolerated* but must be minimal and respectful. The
realistic, well-reviewed pattern is **rewarded-primary + sparing interstitials + a cheap
lifetime remove-ads**, with a genuinely good free tier so you compete with the ad-free
incumbents on quality, not just price. Treat the remove-ads IAP — not ad volume — as the
primary revenue lever.

---

## 2. Recommended ad strategy for an emulator

### 2a. Format mix (in priority order)
1. **Rewarded video — primary.** Opt-in, ~95%+ completion, highest eCPM ($15–30 tier-1),
   and the only format the community broadly accepts because it's a *value exchange*. Gate
   optional conveniences behind it (see 2c).
2. **Interstitial — sparing, transitions only.** Full-screen, decent eCPM ($12–25 video
   tier-1), but the format most likely to wreck retention if mistimed. Use rarely and only
   at true transitions (see 2b).
3. **Banner — avoid over the game; optional in menus only.** A banner over the emulated
   viewport obscures a 256×240 screen and reads as user-hostile. If used at all, restrict to
   the library/settings screens. Given the ad-averse audience, consider skipping banners.
4. **App-open — avoid.** High risk of interrupting a user resuming a game; easy to trip the
   stores' disruptive-ads rules. Not recommended for this app type.

### 2b. Where interstitials may fire (the only acceptable transition points)
Interstitials are appropriate only for apps with clear start/stop points; an emulator's
library↔game transitions qualify. Show **only**:
- On **exit from a game back to the library** (a natural stopping point), shown **before**
  the library renders, not after.
- Optionally when **navigating between major library sections**.

**Never** show an interstitial:
- on app launch or cold start;
- when **entering/starting** a game (don't make someone wait to play);
- mid-gameplay or mid-frame, ever;
- over a **resumed** game (app foreground);
- on app exit.

### 2c. Rewarded design — the free-tier engine (the format to invest in)

The free tier is **time-gated**: a free user gets a base **8-minute** play budget per game
session, **no save states**, and **no battery-backed (SRAM) saves**. When the budget runs
out, the emulator pauses and the user is offered two choices — **buy the Full Version**
(removes the timer and unlocks save states + battery saves) or **watch a rewarded ad for +2
minutes** of play.

**Primary rewarded mechanic — "earn more time":** every rewarded ad the user **completes**
(watched for the duration the ad network requires) grants **+11 minutes** of play time. Grant
the time **only** from the ad network's reward callback — AppLovin `OnUserRewarded` (Kotlin)
/ `didRewardUser` (Swift) — **never** on ad load, show, or dismiss, so the grant maps exactly
to a qualifying view. This is the ideal pattern for this audience: strictly opt-in, a clear
value exchange, and it makes the timer feel fair rather than punitive. It also makes rewarded
— not interstitial — the dominant free-tier ad surface, which is exactly where this category
tolerates ads.

Notes:
- The +11 min grant is **capped at 2 rewarded ads per game session** — a maximum of
  **+22 minutes**, so a fully ad-engaged free user reaches **30 minutes** total (8 base + 22).
  Each of the 11 is a paid rewarded impression. Once the cap is hit, **stop offering the
  rewarded option** and present **only** the Full Version prompt — this converts the most
  engaged free users toward the purchase. The cap resets at the start of each game session.
- This rewarded flow is **separate from the interstitial cadence** in §2d: it is
  user-initiated and **not** subject to the interstitial interval.
- Reuse the same rewarded mechanic for **optional conveniences** as a try-before-you-buy
  funnel — a session unlock of fast-forward/turbo, an extra save-state slot, a shader/CRT
  trial, cheat access — the permanent versions of which the Full Version IAP unlocks.
- Premium users never see the timer, the run-out prompt, or any offer.

### 2d. Cadence — recommended defaults (conservative for this audience)
Industry norms are 2–3 min intervals and 4–6 interstitials/session; for an *immersive,
ad-averse emulator* go gentler:
- **First session: zero interstitials.** A 60–90s first-ad delay improves D1 retention
  5–8% with negligible revenue loss; for this audience, suppress the whole first session.
- **Minimum interval: ≥ 4 minutes** between interstitials (emulator sessions are long).
- **No per-session interstitial count cap** — pacing is purely the interval + first-session
  suppression + the premium check (the cadence is deliberately gentle already).
- **Segment by session depth:** a first-week user sees fewer ads than a veteran.
- **Premium users: zero ads** (already enforced by the core).
- Keep all of this **remotely tunable** (see §5) so you can adjust without a new build.

### 2e. Codify cadence in the shared core (extend `AdPolicy`)
The existing `AdPolicy` already handles premium suppression, launch grace, and the interval.
Add session-awareness so the rules above live in one shared, tested place:

```rust
// Extends crates/rustynes-monetization/src/monetization.rs — add to AdConfig and AdPolicy.
// New AdConfig field:
//   suppress_first_session: bool   // true → no interstitials during session #1
//
// New State field:
//   session_index: u32             // 1 on first ever launch, incremented per session

#[uniffi::export]
impl AdPolicy {
    /// Host calls at the start of each app session, passing the persisted session count.
    pub fn begin_session(&self, session_index: u32, now_ms: u64) {
        let mut s = self.state.lock().unwrap();
        s.session_index = session_index;
        s.launched_at_ms = now_ms;
    }

    // Augment should_show_interstitial with one extra gate (before the interval check):
    //   if cfg.suppress_first_session && s.session_index <= 1 { return false; }
    //
    // There is NO per-session interstitial count cap — pacing is interval + first-session
    // suppression + the premium check only.
}
```
The host persists `session_index` (e.g. SharedPreferences / UserDefaults) and passes it to
`begin_session` at launch. Everything else stays host-agnostic and unit-testable.

### 2f. Free-tier play-time budget + rewarded extension (core)

The 8-minute timer and the "+11 minutes per ad" grant are monetization logic, so they live in
the shared core — one tested implementation for both platforms. **This is now implemented in
`crates/rustynes-monetization/src/monetization.rs`** (13 unit tests pass; Kotlin/Swift bindings regenerated). The
`AdConfig` and `AdPolicy` additions are:

```rust
// Further AdConfig fields:
//   base_play_ms: u64                  // free-tier base budget per game session (480_000 = 8 min)
//   reward_play_ms: u64                // play time per COMPLETED rewarded ad (120_000 = 2 min)
//   max_reward_grants_per_session: u32 // cap on rewarded extensions per game session (11)
//
// Additional State fields:
//   budget_ms: u64                     // total granted budget for the current game session
//   consumed_ms: u64                   // active (unpaused) play time consumed this session
//   reward_grants_this_session: u32    // how many +11-min grants already given this session

#[uniffi::export]
impl AdPolicy {
    /// Call when a game starts. Free users get base_play_ms and a fresh grant counter;
    /// premium is unlimited.
    pub fn start_play(&self) {
        let mut s = self.state.lock().unwrap();
        s.budget_ms = self.cfg.base_play_ms;
        s.consumed_ms = 0;
        s.reward_grants_this_session = 0; // reset the 2-grant cap each game session
    }

    /// Host reports ACTIVE play time elapsed (e.g. once per second of unpaused emulation).
    /// The host already pauses for ads/background, so it simply stops calling this while
    /// paused — the core stays pause-agnostic and deterministic (no clock read inside).
    pub fn add_active_time(&self, delta_ms: u64) {
        let mut s = self.state.lock().unwrap();
        s.consumed_ms = s.consumed_ms.saturating_add(delta_ms);
    }

    /// Whether a rewarded "+11 min" offer should be shown right now: free user, under the
    /// per-session cap. Once false, present ONLY the Full Version prompt.
    pub fn can_offer_rewarded(&self) -> bool {
        let s = self.state.lock().unwrap();
        !s.is_premium && s.reward_grants_this_session < self.cfg.max_reward_grants_per_session
    }

    /// Remaining rewarded extensions this session (for UI like "3 ad-extensions left").
    pub fn reward_grants_remaining(&self) -> u32 {
        let s = self.state.lock().unwrap();
        self.cfg.max_reward_grants_per_session.saturating_sub(s.reward_grants_this_session)
    }

    /// Grant +reward_play_ms, enforcing the per-session cap. Call ONLY from the rewarded
    /// reward callback (OnUserRewarded / didRewardUser) — never on load/show/dismiss.
    /// Returns true if the grant was applied, false if the cap was already reached.
    pub fn grant_rewarded_time(&self) -> bool {
        let mut s = self.state.lock().unwrap();
        if s.reward_grants_this_session >= self.cfg.max_reward_grants_per_session {
            return false; // cap reached — no more free extensions this session
        }
        s.budget_ms = s.budget_ms.saturating_add(self.cfg.reward_play_ms);
        s.reward_grants_this_session += 1;
        true
    }

    /// May the user keep playing right now? Premium is always allowed.
    pub fn is_play_allowed(&self) -> bool {
        let s = self.state.lock().unwrap();
        s.is_premium || s.consumed_ms < s.budget_ms
    }

    /// Remaining free-tier play time in ms; None = unlimited (premium). Drive the on-screen
    /// countdown from this.
    pub fn play_time_remaining_ms(&self) -> Option<u64> {
        let s = self.state.lock().unwrap();
        if s.is_premium { None } else { Some(s.budget_ms.saturating_sub(s.consumed_ms)) }
    }
}
```

With `max_reward_grants_per_session = 11` and `reward_play_ms = 120_000`, a fully ad-engaged
free user reaches **8 + (2 × 11) = 30 minutes** maximum per game session.

Host flow (both platforms, identical because the logic is in the core):
1. On game start → `start_play()`.
2. Each second of **unpaused** emulation → `add_active_time(1000)`; read
   `play_time_remaining_ms()` to update the countdown UI.
3. When `is_play_allowed()` returns false → **pause the emulator** and present the run-out
   prompt. Offer *Watch ad for +11 min* **only if `can_offer_rewarded()`** (i.e. under the
   2-grant cap); otherwise show **only** *Buy Full Version*. Use `reward_grants_remaining()`
   to label the option (e.g. "3 left").
4. On the rewarded **reward callback** → `grant_rewarded_time()` (it returns false and is a
   no-op once the cap is hit), then resume.
5. A completed purchase sets premium → `is_play_allowed()` is always true; the timer and
   prompt disappear immediately.

Unit tests to add alongside the existing ones: budget exhausts after `base_play_ms` of active
time; a reward grant adds exactly `reward_play_ms`; the **11th grant succeeds and the 12th
returns false / is a no-op**; `can_offer_rewarded()` flips to false at the cap; premium is
always allowed and `play_time_remaining_ms()` returns `None`; paused time (no `add_active_time`
calls) does not
consume budget.

---

## 3. Scoping ad *content* for an all-ages retro audience

NES-era games attract nostalgic adults **and** kids, even though the app is rated Teen and is
**not** child-directed (AppLovin forbids child-directed apps — see brief §6). Keep the ads
themselves age-appropriate:
- **AdMob / Google bidding:** call `setMaxAdContentRating()` (G / PG / T / MA) on the Mobile
  Ads SDK `RequestConfiguration` **before** init, or set it per-app in the AdMob UI. Ratings
  are cumulative (T allows G/PG/T, blocks MA). **Recommend PG or T.**
- **AppLovin network:** in MAX → Ad Review → Manage → Applications → **Ad Filtering**, enable
  the **Dating Ads** and **Mature Audiences (17+)** filters.
- **Other mediated networks (Meta, Unity, Pangle):** each enforces content rating in **its
  own dashboard** — set the equivalent cap there too; MAX-side filtering only covers AppLovin.
- Do **not** tag for child-directed treatment (that would forbid AppLovin and gut fill);
  the goal is an all-ages-safe ceiling, not COPPA child mode.

**Revenue expectations** (so the model is realistic, tier-1): interstitial video ~$12–25
eCPM, rewarded ~$15–30. With the conservative cadence above, ad ARPDAU will be modest — the
remove-ads IAP is the real lever.

---

## 4. Performance items to resolve before release (emulator-specific)

A cycle-accurate NES core runs a hard real-time loop (~1.79 MHz CPU model, 60 Hz frame
pacing, continuous APU sample generation). Ad SDKs are heavy, asynchronous, and network- and
GPU-bound. The two must be **isolated**.

- **Thread isolation (critical):** run emulation on a dedicated high-priority thread; do all
  ad SDK work (load/show, mediation, video decode) on the **main/UI thread**. Never call ad
  SDK methods from the emulation loop. The monetization FFI calls (`should_show_*`,
  `notify_*`) are cheap but should still be invoked at frame boundaries / transitions, not
  mid-frame.
- **Pause/resume semantics:** when an interstitial or rewarded ad displays, **pause the CPU,
  PPU, APU, and audio output**, release audio focus, and snapshot enough state to resume
  cleanly. Resume on ad dismissal. Verify a save-state is never lost to an ad or an
  app-background triggered by an ad click. Wire this to the same lifecycle path as
  phone-call/background interruptions.
- **Preload, but don't contend:** cache the next interstitial during idle/menu time so the
  show is instant (a spinner is worse than no ad). Do the network load on a background
  thread so it never steals cycles from the emulation thread. Reload immediately after each
  ad is dismissed.
- **Cold-start latency:** initializing MAX + several adapters at launch adds startup cost.
  Initialize the ad SDK **off the critical path** (after the first frame / library is
  interactive) but early enough to cache an ad before the first eligible transition. Measure
  time-to-interactive with and without ad init.
- **Binary size & memory:** MAX plus a handful of mediation adapters adds meaningful size and
  RAM (commonly several to ~15+ MB). A retro emulator is expected to be lightweight, so keep
  the adapter set lean (the recommended four — Google, Meta, Unity, Pangle — not every
  available network) and measure APK/IPA delta. Premium users should ideally **never
  initialize** the ad SDK at all.
- **Battery / thermal:** video ads and emulation both draw power; avoid back-to-back ads and
  honor the conservative caps to prevent thermal throttling that would also hurt emulation
  frame pacing.
- **Render-surface interaction:** the emulator presents on a GL/Vulkan (Android) / Metal
  (iOS) surface. Interstitials/rewarded are full-screen and present over it cleanly; if you
  ever add a menu banner, mind z-order and the `SurfaceView` vs `TextureView` choice on
  Android. Avoid ad-SDK GC churn on the render thread (jank). Confirm the **16 KB page-size
  alignment** for the Rust `.so` (brief §6a) — unrelated to ads but a hard release gate.
- **Consent flow timing:** show the UMP/ATT consent flow without blocking the user from
  reaching the library and starting a game; gather consent, then init/cache ads in the
  background.

---

## 5. Product enhancements to consider

- **Cheap one-time "Remove Ads / Full Version" (~$3.99)** as the *primary* monetization,
  matching the category norm. Likely converts better here than a subscription.
- **Remote-configurable cadence:** source `AdConfig` (interval, first-session suppression) from
  RevenueCat metadata or Firebase Remote Config so you can tune pacing post-launch without
  shipping a build.
- **RevenueCat Paywalls (RevenueCatUI):** dashboard-configured paywall instead of hand-built
  purchase UI; A/B testable.
- **House ads / cross-promo:** use AdMob house ads (free) to promote the Full Version unlock
  inside the rewarded/interstitial inventory you control.
- **Impression-level ad revenue (ILRD):** forward MAX per-impression revenue to analytics to
  compute true LTV and to A/B cadence variants against retention.
- **"Support the developer" framing:** given this community donates to ad-free apps, consider
  positioning the IAP partly as support, and keep the free tier genuinely good.
- **Compete on quality, not just price:** the ad-free incumbents win on features — save
  states, **CRT/shader filters**, controller support, achievements. A strong free tier is
  what makes light ads forgivable. Decide which of these are free vs. paywalled (the
  `PremiumFeature` enum) — see brief §10.

---

## 6. Decisions to resolve before coding

- [ ] Confirm **rewarded-primary + sparing-interstitial** mix (vs. interstitial-heavy).
- [ ] Confirm the **free-tier model**: 8-min base play budget, no save states, no
      battery-backed saves (decided — see §2c).
- [ ] Confirm the **rewarded "+11 min" grant** and the **2-grant per-session cap**
      (max +22 min → 30 min total) — decided; confirm the values.
- [ ] Confirm **base play minutes (8)** and **reward minutes (2)**, and whether they're
      remote-configurable at launch.
- [ ] Confirm **no banners** (or menu-only).
- [ ] Lock the **starting cadence** (see §7) and whether it's remote-configurable at launch.
- [ ] Set the **max ad content rating** target (PG vs T) across all networks.
- [ ] Confirm the **remove-ads / Full Version IAP** type (one-time non-consumable) and price (~$3.99).
- [ ] Finalize the **`PremiumFeature` set** — at minimum `SaveStates` and `BatterySaves`
      (unlimited play is implied by premium); optionally `FastForward`, shaders, cheats.
- [ ] Confirm whether premium users **skip ad-SDK initialization** entirely.

---

## 7. Recommended starting configuration (baseline)

A concrete, conservative baseline to implement first, then tune via remote config:

| Setting | Value |
|---|---|
| **Free-tier play budget (base)** | **8 min** per game session |
| **Play time per completed rewarded ad** | **+11 min** |
| **Rewarded extension cap** | **2 per session** (max +22 min; 30 min total) |
| **Free tier excludes** | Save states, battery-backed (SRAM) saves |
| First session | **No interstitials** (`suppress_first_session = true`) |
| Launch grace (sessions 2+) | 90 s |
| Min interval between interstitials | **240 s (4 min)** |
| Session cap | **None** (paced by interval + first-session suppression only) |
| Interstitial placement | On **game→library exit only** |
| Rewarded | Opt-in; **primary use = +11 min play time**; also gates fast-forward / extra save-state / shaders / cheats |
| Banners | **None** (or menu screens only) |
| App-open ads | **None** |
| Max ad content rating | **PG** (raise to T only if fill is too low) |
| Premium | **Unlimited play + save states + battery saves**; zero ads; skip ad-SDK init if feasible |
| Remove-ads / Full Version IAP | One-time non-consumable, ~$3.99 |

This maps onto the `AdConfig` defaults plus the §2e session fields. Keep the values in remote
config from day one so you can A/B without a release.

---

## 8. How this changes the existing skeleton

- ✅ **Done:** `crates/rustynes-monetization/src/monetization.rs` now implements §2f (`start_play`, `add_active_time`,
  `grant_rewarded_time` → bool with the 2-grant cap, `can_offer_rewarded`,
  `reward_grants_remaining`, `is_play_allowed`, `play_time_remaining_ms`, plus
  `base_play_ms` / `reward_play_ms` / `max_reward_grants_per_session` in `AdConfig`), the
  `PremiumFeature` enum gates `SaveStates` + `BatterySaves` + `FastForward`, all 13 unit tests
  pass, and the Kotlin/Swift bindings are regenerated (`core/generated/`).
- ⬜ **Remaining (shell wiring):** add a **`MaxRewardedAd` / `MARewardedAd`** gate in each
  shell, mirroring `AdGate`. On the reward callback (`OnUserRewarded` / `didRewardUser`), call
  **`grantRewardedTime()`** then resume the emulator.
- ⬜ **Remaining (UI):** a countdown driven by `playTimeRemainingMs()`, and a **run-out prompt**
  ("Buy Full Version" / "Watch ad for +11 min") shown when `isPlayAllowed()` is false, with the
  emulator paused behind it; offer the rewarded option only while `canOfferRewarded()` is true.
- ⬜ Drive `addActiveTime(...)` only from **unpaused** emulation (stop during ads, the run-out
  prompt, and app background) so paused time never burns the budget.
- ⬜ Optional: §2e session-cadence extension (`begin_session`, first-session suppression) is
  still a documented sketch, not yet in the core.
- ⬜ Add `setMaxAdContentRating` (Android, before MAX/AdMob init) and the AppLovin Ad Filtering
  + per-network content caps (§3).
- ⬜ Ensure the emulator's pause/resume path is invoked on ad display/dismiss (§4).

---

## 9. Live-tuning the policy via remote config

`AdConfig` is injected at construction, so every pacing and free-tier value can be sourced
from a remote config and changed **without an app update or rebuild**. This is what lets you
find the revenue/retention balance after launch instead of guessing it before.

Expose these fields (all already in `AdConfig`):

| Field | Default | Typical tuning range |
|---|---|---|
| `base_play_ms` | 480 000 (8 min) | 300 000–900 000 (5–15 min) |
| `reward_play_ms` | 120 000 (2 min) | 60 000–300 000 (1–5 min) |
| `max_reward_grants_per_session` | 11 | 0–20 (0 disables the rewarded extension) |
| `min_interval_ms` | 240 000 (4 min) | 120 000–360 000 |
| `launch_grace_ms` | 30 000 | 0–120 000 |

Pattern:
- Source values from **RevenueCat** offering metadata or **Firebase Remote Config**.
- Start from `default_ad_config()` as the safe baseline; overlay any fetched values; build
  `AdPolicy` with the merged config. **Never block startup on the fetch** — if it fails or
  the device is offline, run on the defaults (this also fixes the offline case in
  `recommendations.md`).
- **Clamp every remote value in the host** to a sane range before passing it in, so a bad
  remote push can't brick the gate (e.g. a 0-ms base budget).

Experiments: A/B the base budget (5 vs 8 min), reward minutes, and the cap via RevenueCat
Experiments or Remote Config conditions; read the outcome from the §10 funnel. Pair with
RevenueCat price/paywall experiments and MAX waterfall A/B once you have baseline data.

---

## 10. Analytics — the monetization funnel to instrument

RevenueCat shows purchases and MAX shows ad revenue, but the decisions you most need to tune
live *between* them: the timer → rewarded → purchase funnel. Log it yourself (Firebase /
Amplitude / PostHog). Emit these from the shells at the points the core surfaces:

| Event | Fire when | Useful properties |
|---|---|---|
| `session_start` | `start_play()` | `game_id`, `is_premium` |
| `play_runout` | `is_play_allowed()` → false | `session_seconds`, `grants_used` |
| `rewarded_offered` | run-out prompt shows the ad option | `grants_remaining` |
| `rewarded_completed` | reward callback → `grant_rewarded_time()` true | `grant_index` |
| `rewarded_failed` | rewarded load/show failed (no fill, offline) | `reason` |
| `reward_cap_reached` | `can_offer_rewarded()` → false at run-out | `session_seconds` |
| `paywall_shown` | Full Version prompt shown | `trigger` = runout \| cap \| menu |
| `purchase_completed` / `purchase_restored` | RevenueCat success | `product_id` |
| `interstitial_shown` | `notify_interstitial_shown()` | `session_index` |
| `ad_revenue` | MAX impression-level revenue (ILRD) callback | `revenue`, `network`, `format` |

Headline metrics to watch: share of sessions that hit run-out; rewarded **accept rate**
(offered → completed); **cap-hit rate**; **cap-hit → purchase** conversion (the core
monetization signal); ARPDAU; and rewarded fill/latency. Forward `ad_revenue` to RevenueCat
too, so LTV and the ad-vs-no-ad cohorts are computed in one place.

Privacy: any of this that counts as tracking must sit behind the same consent as ads (ATT on
iOS, TCF on EU Android — brief §6b/§6g), use non-PII identifiers, and be declared in the iOS
privacy manifest / Play Data safety form.

---

## 11. Sources

- Interstitial placement & frequency: https://support.google.com/admob/answer/6066980 ,
  https://support.google.com/admob/answer/6201350 , https://www.publift.com/blog/interstitial-ads-a-best-practice-guide-for-publishers ,
  https://adreact.com/blog/interstitial-ad-best-practices-mobile-games/ ,
  https://adapty.io/blog/mobile-interstitial-ads/
- Ad content rating controls: https://support.google.com/admob/answer/7562142 ,
  https://developers.google.com/admob/android/targeting ,
  https://support.applovin.com/en/max/faq/stop-unwanted-or-inappropriate-ads
- Emulator market/monetization context: https://techcrunch.com/2024/05/03/retro-game-emulator-delta-app-store-ios/ ,
  https://www.libretro.com/ ,
  https://www.gamesradar.com/platforms/iphone/after-weeks-of-ad-riddled-apps-and-bizarre-delistings-retroarch-is-finally-on-the-app-store-to-handle-all-your-retro-emulation-needs-for-free/ ,
  https://mobilesyrup.com/2024/05/05/fake-delta-app-android-best-emulators/

*Verify SDK APIs, eCPM ranges, and store policy text at implementation time — they move.*
