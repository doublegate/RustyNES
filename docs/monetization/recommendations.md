# RustyNES — Recommendations & Considerations (Mobile Monetization)

Considerations that go *beyond* the baseline design in the other docs: design holes in the
free-tier mechanic worth closing before launch, engineering-robustness items, and
store/product polish. The free-tier model itself (8-min budget, +2 min per rewarded ad,
11-grant/+22-min cap → 30 min max, no save states, no battery saves) is decided and
implemented in `crates/rustynes-monetization/src/monetization.rs`; this document is about the edges around it.

Two adjacent topics live in other docs to keep them next to the work they affect:
**compliance/consent** (the EEA/UK/CH certified-CMP requirement and iOS SDK privacy
manifests) is in `platform-setup-runbook.md` §6/§9 and `implementation-brief.md` §6d/§6g/§7;
**live-tuning + analytics** (remote-config fields and the monetization funnel) is in
`pre-implementation-addendum.md` §9/§10. Pointers in §5 below.

> TL;DR: the timer + rewarded loop is sound, but four edges will bite if left implicit — a
> free user can restart to dodge the timer, an **offline** free user dead-ends with no ad and
> no purchase path, "**no saves at all**" is a 1-star-review risk, and gating the **very first
> session** kills first-impression conversion. Close those, keep the ad SDK off the
> emulator's critical path, and put a real paywall at the run-out moment.

---

## 1. Free-tier mechanic — design holes to close

These are specific to the timer + rewarded model and are the ones most likely to surprise you
in the wild.

### 1a. Session-reset abuse + session semantics
The budget and grant counter reset on every `start_play()`, so a free user can simply kill
and relaunch a game to get a fresh 8 minutes without watching a single ad. What *saves* this
design today is the no-save-states rule — restarting loses their run — so decide it
consciously rather than by accident. Define crisply when `start_play()` fires: a **session =
continuous play of one ROM from load to unload**; pausing to the menu and resuming the same
ROM does **not** reset it; switching ROMs does. If you want to harden against restart-farming,
either persist the in-progress play state across relaunch (§1f) or add a soft **daily**
free-minutes budget on top of the per-session budget.

### 1b. The offline trap (high priority)
A free user offline — plane, subway, no signal — hits the 8-minute wall with **no way out**:
rewarded ads need the network to load, and the purchase flow needs it too. Don't let that
dead-end read like a bug:
- The grant only fires on the reward callback, so a failed/again-no-fill load means *nothing
  happens* — the run-out prompt must explicitly handle "no ad available right now."
- Consider granting a small **offline grace** continuation, or surfacing a clear message, so
  the session degrades gracefully instead of locking.
- Sourcing `AdConfig` from remote config with safe local defaults (addendum §9) means an
  offline launch still runs on `default_ad_config()` rather than stalling.

### 1c. The no-save cliff is a review risk
"8 minutes, then you lose everything, and you can never save" is a sharp edge for an RPG or a
long platformer — exactly the audience that leaves angry reviews about it. Consider giving the
free tier **one auto-resume slot** (restores the current game on relaunch — not a true
multi-slot save-state), while keeping real save-states (`PremiumFeature::SaveStates`) and
battery-backed cartridge persistence (`PremiumFeature::BatterySaves`) premium. It softens the
cliff without giving away the headline premium feature.

### 1d. Protect the first impression
You already suppress interstitials in the first session; extend that philosophy to the timer.
Gating a brand-new user at 8 minutes on their very first game — before they're hooked — hurts
conversion and retention. Give the first session a longer (or ungated) budget so value lands
before friction. This can be a remote-config condition (addendum §9), so you can tune the
first-run generosity without a release.

### 1e. Preload the rewarded ad before run-out
Kick off a rewarded ad load at roughly **60–90 s remaining** (read from
`play_time_remaining_ms()`) so that when the user taps "Watch ad for +2 min" it plays
instantly instead of showing a spinner at the worst possible moment. Gate the offer on
`can_offer_rewarded()` and label it with `reward_grants_remaining()`.

### 1f. Persisting in-progress play state (optional)
The core is in-memory: it forgets `consumed_ms` and `reward_grants_this_session` when the
process dies. If you want the timer/cap to survive a kill-and-relaunch mid-run (closing the
§1a hole), add a small serialize/restore hook to the core and have the host persist it
(SharedPreferences / UserDefaults), restoring it before the next `start_play()` of the same
ROM. Skip this if you accept restart-to-reset as intended behavior.

### 1g. The grant is client-trusted
A rooted/jailbroken user can spoof the reward callback that calls `grant_rewarded_time()`. At
a one-time ~$3.99 price point this is fine to accept. If it ever matters, AppLovin supports
**server-side reward verification** for rewarded ads.

---

## 2. Engineering robustness

- **Never block the emulator on the ad SDK.** If MAX init fails or stalls, the app must still
  play (degrade to no-ads). Defer ad-SDK init off the cold-start critical path; a premium user
  can skip ad-SDK init entirely.
- **Tick `add_active_time` coarsely** — once per second of unpaused emulation, not per frame —
  to avoid 60 Hz mutex contention on the high-priority emulation thread. A useful property of
  the current design: the budget is **tick-derived, not wall-clock-derived**, so it is immune
  to the `SystemClock.elapsedRealtime()` (counts sleep) vs `DispatchTime` uptime (does not)
  discrepancy. Stop ticking while paused (ads, run-out prompt, background) and paused time
  never burns budget.
- **Test-mode ads only during development.** Clicking your own *live* ads is the fastest way
  to get an AppLovin/AdMob account banned. Use MAX's Mediation Debugger test mode and the
  RevenueCat sandbox; never click production ads.
- **Cross-platform entitlement won't transfer for free.** A one-time IAP bought on Android
  does **not** make the user premium on iOS (or vice versa) unless you add account linking /
  login (RevenueCat with a shared app user id). For a solo launch, per-store purchases are
  normal — just decide it explicitly and make **Restore Purchases** prominent so a user who
  reinstalls or switches devices on the *same* store recovers their unlock. The same RevenueCat
  identity matters when granting testers the unlocked build by App User ID (runbook §5a).

---

## 3. Store & product polish

- **Put a real paywall at the run-out moment** — that's your highest-intent conversion point.
  RevenueCat Paywalls (brief §8b) render it natively; lead with the value: *unlimited play +
  save states + battery saves*. Tag the trigger (`runout` vs `cap` vs `menu`) for analytics
  (addendum §10).
- **Don't show a constant ticking countdown.** A hostile clock for the whole session hurts the
  feel and the reviews. Surface remaining time subtly, and make it prominent only in the last
  minute or at run-out.
- **Android binary size / ABIs.** Ship per-ABI splits via `cargo-ndk` (arm64-v8a primary;
  add armeabi-v7a / x86_64 only as needed) inside the App Bundle, and keep the mediation
  adapter set lean — the ad SDK + adapters add several MB and ship in the binary even for
  premium users (you can't strip them per-user; you *can* skip initializing them).
- **Emulator review hygiene** (the recurring reason these apps get pulled): ship the emulator
  only — **no bundled copyrighted ROMs** — provide a file-import path for user-owned
  ROMs/homebrew (SAF on Android, `UIDocumentPicker` on iOS), and keep any "where to get games"
  guidance pointed at legal homebrew/public-domain sources. Verify the current Apple Guideline
  4.7 and Google emulator-policy text at submission (brief §6f).

---

## 4. Priority ordering (if you only do some of this)

1. Offline trap (§1b) and the run-out "no ad available" path — these are correctness, not
   polish; without them an offline user looks broken.
2. First-session generosity (§1d) and the run-out paywall (§3) — these move conversion the
   most.
3. No-save auto-resume slot (§1c) — biggest lever on reviews/retention for the genre.
4. Session semantics + restart hardening (§1a/§1f) — decide now even if you defer the code.
5. Robustness items (§2) — cheap insurance; do them as you wire the shells.

---

## 5. Covered elsewhere (so this doc isn't duplicated)

- **Consent / privacy compliance (release blockers):** EEA/UK/Switzerland certified CMP +
  IAB TCF, and iOS third-party SDK privacy manifests + signatures — see
  `implementation-brief.md` §6d/§6g/§7 and `platform-setup-runbook.md` §6/§9. iOS ATT is in
  brief §6b.
- **Live-tuning via remote config** (the `AdConfig` field list, clamping, experiments) and
  the **analytics funnel** (events, headline metrics) — see
  `pre-implementation-addendum.md` §9 and §10.
- **The free-tier core model and host flow** (`start_play` … `grant_rewarded_time` … the
  11-grant cap) — see `pre-implementation-addendum.md` §2c/§2f and the FFI surface in
  `build-and-bindings.md`.

*Verify SDK APIs, store policy text, and consent/attribution requirements at implementation
time — they move.*
