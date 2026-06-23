# RustyNES — Monetization Integration Notes (real-repo mapping)

How this monetization layer maps onto the **actual** RustyNES source (`README` + the workspace +
`to-dos/plans/v1.8.0-android-plan.md`). The architecture, crate, determinism, and toolchain facts
below come straight from that plan and apply regardless of monetization model; the **monetization
model itself is a deliberate maintainer choice** (see the banner).

> ## Monetization model: ad-supported freemium (PRIMARY — chosen)
> This project's **RevenueCat + AppLovin MAX** model is the **primary, chosen** path, a deliberate
> maintainer override of the ad-free default sketched in `v1.8.0-android-plan.md`:
> - **Free Google Play download** with a one-time non-consumable **"Full Version / Remove Ads"**
>   IAP at **$3.99**, keyed to the **RevenueCat `premium`** entitlement (RevenueCat wraps Play
>   Billing / StoreKit, so a single entitlement drives both platforms).
> - **Free tier = ad-supported**: **interstitials** at natural breaks (paced by a launch grace +
>   a 4-min interval — **no per-session count cap**) plus **rewarded ads** that extend an 8-minute
>   play session **+11 min each, capped at 2 grants → 30 min**. The three persistence features
>   (§4) stay paywalled.
> - Purchasing the Full Version removes ads + the timer and unlocks the persistence features.
>
> The full interstitial + rewarded + entitlement surface in `core/` is **in scope and primary**.
> The plan's ad-free $2.99 `full_unlock` direct-Billing variant is the road **not** taken; it is
> noted here only where it explains a plan reference.

---

## 0. Version & launch-timing reality

Public `main` is **v1.5.0 "Lens"** (v1.6.0 "Studio" in dev); the Android app is the planned
**v1.8.0** milestone and iOS **v1.9.0**. Per the maintainer's 2026-06-20 timing decision:
- **v1.8.0–v1.8.7 are GitHub-Releases / sideload-only and ship full-featured** (no demo / no ads).
- **v1.8.8 is the planned Google Play debut** — the polished build that flips the freemium layer
  on. So "v1.8.8" is a real, meaningful target, even though no v1.8.8 *tag* exists yet.
- The freemium layer (ads + the entitlement gate) is gated behind a **`PLAY_BUILD`** switch (a
  `play` product flavor / `BuildConfig` flag, default **false**): sideload/dev/GitHub → no ads,
  full-featured; the Play AAB → freemium. Wire the flavor in **v1.8.2** so every interim sideload
  release stays full-featured while the freemium code rides along dormant until v1.8.8.

The **15-tester / 14-day closed test** (runbook §5a) therefore applies to the **v1.8.8 Play debut**.

---

## 1. Crate naming & placement  *(fixed in this repo)*

RustyNES already ships a workspace crate named **`rustynes-core`** (Bus/scheduler/console/save
states), so this crate is named **`rustynes-monetization`** (lib `rustynes_monetization`), Swift
module `RustyNesMonetization`, Kotlin package `com.doublegate.rustynes.monetization.ffi`. The locked plan also factors two
new mobile host crates you should slot beside:
- **`rustynes-mobile`** — a platform-agnostic **UniFFI control surface** over the core (load ROM
  from a byte buffer, set the per-port `Buttons` mask, run-frame, borrow the framebuffer, save/load
  state, query status), reused by both Android and iOS.
- **`rustynes-android`** — thin hand-rolled `jni` 0.21 glue (hand the `ANativeWindow` to wgpu, the
  audio sink) that UniFFI can't express.

Placement: keep `rustynes-monetization` a **mobile-only** member, off the desktop/wasm/default build
(it pulls UniFFI). Aligned to the workspace toolchain — **edition 2024, Rust 1.96** — and passes the
four gates (`fmt`, `clippy -D warnings`, `doc`, `test`).

---

## 2. Where the monetization *policy* lives

With the ad-supported model chosen, the policy lives in **shared Rust** — the `AdPolicy` object in
this `rustynes-monetization` crate, beside `rustynes-mobile` — so Android and iOS run **one**
implementation of the ad cadence, the rewarded play-time gate, and the feature gates (the same
cross-platform-share rationale the plan uses for `rustynes-mobile`). The native shells own only the
*plumbing*:
- **RevenueCat** → the `premium` entitlement → fed into the core via `set_premium(isActive)`.
- **AppLovin MAX** → loads/shows interstitials + rewarded ads; on the *reward* callback the shell
  calls `grant_rewarded_time()`, and on display it calls `notify_interstitial_shown(now)`.
- The shell asks `should_show_interstitial(now)`, `is_play_allowed()`, `play_time_remaining_ms()`,
  `can_offer_rewarded()`, and `feature_enabled(...)` and renders accordingly.

The **single source of truth** holds: premium flows in only via `set_premium`, fed from RevenueCat
(plus the debug `TESTER_UNLOCK` override for on-device QA — the analogue of the plan's
`debug.force_unlocked`). The 13 unit tests pin the cross-platform contract.

(For contrast: the plan's ad-free variant would instead put a `BillingManager` + demo timer in
Kotlin/Swift. That's the road not taken; the shared-Rust policy here is the chosen approach.)

---

## 3. Architecture (LOCKED: hybrid — not winit/egui)

The ship target is **Hybrid**: keep the existing **wgpu** renderer drawing the NES image onto an
Android **`SurfaceView`** (`ANativeWindow` → wgpu), run the core + render/audio loop on a dedicated
native thread (the Android analogue of the desktop `emu-thread`, `Arc<Mutex<EmuCore>>`), and let
**Jetpack Compose** own all chrome — top bar, settings, SAF ROM picker, **the touch overlay**, the
save-state manager, **and the ad/timer HUD + the "Remove Ads — Full Version ($3.99)" / "Restore"
sheet** (or a RevenueCat paywall). iOS (v1.9.0) mirrors this with **SwiftUI + wgpu→Metal** over the
same bridge. AppLovin's interstitial/rewarded views render as native overlays above the surface.

This **corrects the earlier draft of this doc**, which recommended reusing `rustynes-frontend` via
winit/egui and avoiding Compose. Per the lock: winit+wgpu+egui on `android-activity` is only the
**beta.1 first-boot spike** (and an optional in-surface power-user/debugger overlay); the shippable
app uses **Compose**, and pure-Compose-without-wgpu was explicitly **rejected** (it would discard the
NTSC/CRT/Bisqwit shader stack). So the demo HUD and paywall are **native Compose/SwiftUI**, not egui;
there is no egui in the shipped mobile UI.

(Render note: the locked plan ships filters via an **AGSL `RuntimeShader`** post-process on the
Compose path for v1.8.0, with the full wgpu-on-`SurfaceView` WGSL stack as a documented follow-up.
Either way it's presentation-only and determinism-safe.)

---

## 4. Premium-feature mapping = the three persistence features

`PremiumFeature` gates exactly the three **persistence** features; the free (ad-supported) tier
keeps everything else:

| Variant | What it gates (locked on the free, ad-supported tier) |
|---|---|
| `SaveStates` | F1/F4 save/load slots + the thumbnail Save-States manager |
| `SaveOnExitResume` | `onPause` writes an `auto` state + auto-resume on relaunch |
| `BatterySaves` | persisting on-cart battery-backed SRAM (and FDS RAM) to disk |

**Free even on the ad tier (do NOT gate):** full emulation accuracy, video + shaders, audio, all
input, **pause, fast-forward, and in-session rewind** (the 600-frame ring is RAM, not persistence).
The free tier's play time is governed by the 8-min session + rewarded-ad extensions, not by gating
these. (An earlier draft briefly gated `Rewind`/`FastForward` — removed.)

Save format is unchanged from desktop: **`.rns`**, laid out as `<rom-sha256>/slot-N.rns`, now under
Android `Context.filesDir` (cross-device-portable, byte-identical).

RetroAchievements is **deferred** from the Android MVP, so its hardcore-mode disabling of
save-state/rewind/cheats is a later-increment concern, not a v1.8.0 interaction.

---

## 5. The determinism boundary (unchanged, reinforced)

Monetization state (the play-time gate, ad cadence, the premium flag, the persistence gates,
`now_ms`) must **never** enter `rustynes-core::Bus` or the scheduler — the bit-determinism contract
underpins save-states, the `.rns`/`.rnm` formats, rollback netplay, and TAS, and desktop⇄Android
cross-play depends on the core staying platform-independent. Concretely, per the plan: touch/gamepad
converge on the one `Buttons` mask → `SharedInput` → `EmuCore::latch` at the **same late-latch a
keypress uses** (no new determinism surface, exactly as the wasm touch overlay proved); run-ahead's
speculative frames are already rolled back and never reach save-states/TAS; new mobile
pacing/throttle knobs are **frontend-only**. The free and paid tiers are **byte-identical** for a
given input — the difference is ads + a session clock + the three persistence gates, all host-side.
Tick `add_active_time` from the frame loop only while unpaused; call `start_play()` at launch /
first ROM load.

---

## 6. Unlocking the app for the 15 closed-test testers

With RevenueCat primary, **all** of runbook §5a applies — use whichever fits:
- **RevenueCat promotional grant** (Method 1) — grant the `premium` entitlement to a tester from
  the dashboard or REST API; no purchase, revocable. Simplest way to hand the unlocked build to the
  cohort.
- **Google Play License testing** (Method 2) — free *test purchase* of the "Remove Ads / Full
  Version" product, exercising the real RevenueCat → entitlement path; add testers to the License
  testing list in addition to the closed-test track.
- **`TESTER_UNLOCK` debug override** / `PLAY_BUILD=false` — for local on-device QA (the analogue of
  the plan's `debug.force_unlocked`); inert in the release/closed-test build.

The closed-track upload is still what exercises the real billing flow (Billing can't transact on a
sideloaded build), and this is the **v1.8.8** Play-debut cohort.

---

## 7. Build & toolchain (from the v1.8.0 plan)

- **NDK r27 LTS**; verify **16 KB page alignment** (a Play requirement for Android 15+ regardless of
  NDK).
- Build the `.so` with **`cargo-ndk`** invoked from Gradle via **`cargo-ndk-android-gradle`** →
  **AAB** (not `cargo-apk`, which is APK-only / can't publish to Play). Targets `aarch64-linux-android`
  (ship) + `x86_64-linux-android` (emulator/CI).
- **`minSdk 26`** (AAudio floor), **`targetSdk 35`** (Play mandate). **`cpal 0.18`** has a native
  AAudio backend at API 26, so the audio ring works unchanged. JNI glue is **`jni` 0.21**.
- Billing: **RevenueCat** bundles the Play **Billing Library** (note v8 is mandatory by 2026-08-31,
  which current RevenueCat ships); RevenueCat handles acknowledge/restore. AppLovin MAX SDK 13+ for
  ad mediation; complete the **certified-CMP/UMP consent flow before MAX init** in EEA/UK/CH.
- Enroll in **Play App Signing**; keep a **guaranteed sideload / F-Droid / GitHub-Releases channel**
  (full-featured, no ads) so the project never depends solely on Play.

---

## 8. ROM policy & data safety (already store-compliant)

ROM import via the **Storage Access Framework** (`ACTION_OPEN_DOCUMENT`, optional
`ACTION_OPEN_DOCUMENT_TREE`) with **persistable URI grants**; bytes go to `Nes::from_rom` (never a
path). **No bundled commercial ROMs, no downloader** — which is exactly why emulators are allowed on
Play. Data-safety = "no data collected" for the MVP. Keep this model on iOS via `UIDocumentPicker`.

---

## 9. Deferred from the Android MVP

**Netplay, RetroAchievements, and Lua are deferred** to later point releases (UDP compiles but mobile
CGNAT needs the TURN story; rcheevos needs a Compose OAuth UI; mlua cross-compiles but is gated).
Don't design the monetization layer around them.

---

## 10. License

Dual **MIT OR Apache-2.0** — a commercial freemium Play build with the proprietary RevenueCat/
AppLovin SDKs is fine (no GPL conflict). The source is public, and per the launch timing the
**sideload / GitHub builds are full-featured with no ads (`PLAY_BUILD=false`)**; only the Play AAB
carries ads + the timer + the entitlement gate. That's consistent and good for goodwill — set
expectations rather than obscuring it.

---

## 11. RustyNES-side change checklist

- [x] **Monetization decided:** ad-supported (RevenueCat + AppLovin), one-time "Full Version /
      Remove Ads" at **$3.99** — overrides the plan's ad-free default.
- [ ] Stand up `rustynes-mobile` (UniFFI control surface) + `rustynes-android` (`jni` 0.21 glue) +
      the Gradle module; add `rustynes-monetization` as a mobile-only member beside them.
- [ ] Wire the shells: RevenueCat → `set_premium`; AppLovin MAX → `should_show_interstitial` /
      `notify_interstitial_shown` and the rewarded → `grant_rewarded_time` (§2).
- [ ] Gate `SaveStates` / `SaveOnExitResume` / `BatterySaves` / `FastForward` / `Shaders` / `Cheats` (§4); keep in-session rewind free (fast-forward/shaders/cheats are now premium, decided 2026-06-23).
- [ ] Wire the `PLAY_BUILD` flavor (v1.8.2); keep sideload builds full-featured (no ads); flip the
      freemium layer on at the **v1.8.8** Play debut.
- [ ] Create the one-time **"Remove Ads / Full Version" product ($3.99)** + the `premium`
      entitlement in RevenueCat + a license-tested account + a closed-track upload (§6).
- [ ] Keep all monetization out of `rustynes-core` and the determinism path (§5).
- [ ] Reconcile against `to-dos/plans/v1.8.0-android-plan.md` and `docs/STATUS.md` at merge time.

*Grounded in the public RustyNES README and `to-dos/plans/v1.8.0-android-plan.md` (architecture,
crate list, the locked monetization + timing decisions, toolchain). Verify exact symbols against the
source at merge time.*
