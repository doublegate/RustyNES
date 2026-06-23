# RustyNES — Platform & Account Setup Runbook

> **Monetization model (decided).** The **ad-supported** path is primary: set up **both**
> **RevenueCat** (§5) and **AppLovin MAX** (§6), plus the Google Play one-time product (§4). The
> paid unlock is a one-time **"Full Version / Remove Ads" ($3.99)** keyed to the RevenueCat
> `premium` entitlement. This deliberately overrides the ad-free default in
> `to-dos/plans/v1.8.0-android-plan.md`; see `docs/rustynes-integration.md`.

A step-by-step checklist **for you to follow** to stand up every account and dashboard
needed to ship RustyNES as a freemium app (AppLovin MAX ads + a RevenueCat "remove ads"
purchase) on **Google Play (Android first)** and the **Apple App Store (iOS ~1 week later)**.

This covers the parts an engineer/Claude Code can't do for you because they need your
identity, payment details, and credentials. The code/build side lives in the skeleton zip,
the implementation brief, and `CLAUDE.md`.

---

## 0. At a glance

| Account | Cost | Verification | Key gotcha |
|---|---|---|---|
| Apple Developer Program | **$99 / year** | Org: D-U-N-S + verification call | Org enrollment can take days–weeks |
| Google Play Console | **$25 one-time** | ID; Org: D-U-N-S | **Personal accounts** must run a 12-tester / 14-day closed test before production |
| RevenueCat | **Free** to $2,500 MTR, then ~1% | — | "MTR" is gross revenue *before* store commission |
| AppLovin MAX | **Free** SDK | Payout/tax info to get paid | Each mediated network needs its own account |
| AdMob / Meta / Unity / Pangle | **Free** | per network | Needed to fill the MAX waterfall |

**Store commissions:** Apple 30% (15% under the Small Business Program / <$1M / year, and
15% on subscriptions). Google 15% on the first $1M/year, 30% above; 15% on subscriptions.
**Enroll in each store's Small Business Program** to get the 15% rate.

---

## 1. Decisions to make before you start

- [ ] **Account type — Apple:** Individual (your legal name is the seller) vs Organization
      (a legal entity name is the seller; requires a free **D-U-N-S number**, a public
      business website, and an Apple verification phone call).
- [ ] **Account type — Google:** Personal vs Organization. **This one has teeth:**
      - *Personal* accounts created after Nov 13, 2023 must run a **closed test with ≥12
        testers opted-in for 14 continuous days** before you can request production access.
      - *Organization* accounts are **exempt** from that testing gate but need a free
        **D-U-N-S number** and business verification.
      - **Recommendation:** if you have or will form an LLC, register **Organization** on
        Google to skip the 12-tester gate and ship faster. If staying personal, plan the
        14-day closed test into the timeline (it overlaps with QA you'd do anyway).
        *Full trade-off in §1a below.*
- [ ] **Paid model:** one-time non-consumable "Full Version / Remove Ads" (recommended for
      an emulator) vs auto-renewable subscription. Determines the product type you create.
- [ ] **Bundle / package identifier:** pick one and use it **identically** everywhere
      (Apple, Google, RevenueCat, AppLovin). Suggested: `app.rustynes` (matches the
      skeleton's `applicationId` / namespace).
- [ ] **App name & store presence:** final name, icon, screenshots, privacy policy URL,
      support email/URL. A **privacy policy URL is required** (and the AppLovin consent
      flow needs it too).

---

## 1a. Decision aid — Individual / Personal vs. Organization (LLC)

This single choice drives the seller name on both stores, whether Google's 12-tester gate
applies, and your liability/tax posture. You do **not** have to match across stores (Apple
Individual + Google Personal is fine), but matching gives consistent seller branding.

| Factor | Individual / Personal | Organization (e.g. LLC) |
|---|---|---|
| Seller name shown on store | Your personal legal name | Company legal entity name |
| Setup speed | Fast — no D-U-N-S, no business docs | Slower — free **D-U-N-S** + verification (Apple does a phone call) |
| Google 12-tester / 14-day gate | **Applies** (accounts after Nov 13 2023) | **Exempt** |
| Liability | Personal; no separation of assets | LLC separates personal from business (consult a pro) |
| Cost beyond store fees | None | LLC formation + state annual fees + maybe registered agent |
| Best for | Quick validation, hobby/solo launch | Ongoing product, multiple apps, taking payments, possible team |

**Choose Individual / Personal if:** you want to ship fast with minimal overhead, you're
validating the idea, and you're comfortable with your legal name as the public seller.
Just budget the Google **12-tester / 14-day** closed test (it overlaps with the QA testing
you'd run anyway, so it's rarely net-new work — only net-new *calendar time*).

**Choose Organization (LLC) if:** RustyNES is a serious, ongoing product you intend to
monetize and possibly expand, you want a company name as the seller, and you want to
separate business finances/liability. The main concrete launch payoff is **skipping
Google's 12-tester gate**; the main cost is the slower D-U-N-S/verification path (start it
on day one — see §2).

**Recommendation:** for a real, revenue-generating launch you plan to maintain, the
Organization route is usually worth it — chiefly for the Google testing-gate exemption, the
professional seller name, and the liability separation. If you mainly want to get a build in
front of users quickly, Individual/Personal is perfectly fine; just start the 14-day Google
closed test early. The slow steps either way are the **D-U-N-S number** and **LLC formation**,
so if there's any chance you'll go the org route, begin those now — they're easy to abandon
and hard to rush.

> Not legal or tax advice. LLC formation, liability, and tax treatment vary by state and by
> your situation — confirm specifics with an attorney or accountant before deciding.

---

## 2. Recommended sequence (critical path)

Long poles first — start the slow verifications on day one.

1. **Request a D-U-N-S number** (free) if going Organization on either store —
   https://developer.apple.com/support/D-U-N-S/ — it can take days.
2. **Start Apple Developer Program enrollment** (org verification is the slowest step).
3. **Register Google Play Console**; if personal, **begin recruiting/onboarding the 12
   testers immediately** and start the 14-day clock as soon as you have an installable build.
4. **Create RevenueCat + AppLovin accounts** (instant) and the network accounts.
5. Configure products, entitlements, ad units, and credentials (Sections 5–7).
6. Build → internal/closed testing → store review → production. Ship Android, then iOS.

---

## 3. Apple Developer Program + App Store Connect

**Enroll** — https://developer.apple.com/programs/enroll/ ($99/yr)
- [ ] Apple Account with **two-factor authentication** on; use your **legal name** (aliases
      delay approval).
- [ ] Choose Individual or Organization. Organization additionally needs a **D-U-N-S
      number** tied to the legal entity, legal binding authority, and a public website;
      Apple verifies by phone.
- [ ] Pay the $99 and accept the license agreement.

**App Store Connect** — https://appstoreconnect.apple.com
- [ ] **Agreements, Tax, and Banking** → sign the *Paid Apps* agreement and add **bank +
      tax** details. IAP cannot be sold until this is complete.
- [ ] Enroll in the **App Store Small Business Program** (15% rate).
- [ ] Register the **Bundle ID** (Certificates, Identifiers & Profiles) = `app.rustynes`.
- [ ] Create the **app record** (name, primary language, bundle ID).
- [ ] Create the **in-app purchase**: a **Non-Consumable** named e.g. `remove_ads`
      (or an auto-renewable subscription). Set price, localized name/description.
- [ ] Create credentials RevenueCat needs (Section 5): an **In-App Purchase Key** (.p8,
      under Users and Access → Integrations) and/or the **app-specific shared secret**.
- [ ] Info.plist items for the build: `NSUserTrackingUsageDescription`,
      `GADApplicationIdentifier` (AdMob app id), SKAdNetwork IDs, and an app
      `PrivacyInfo.xcprivacy` (see the implementation brief §6). Complete **App Privacy**
      (nutrition labels) in App Store Connect.

---

## 4. Google Play Console

**Register** — https://play.google.com/console/signup ($25 one-time)
- [ ] Google account with **2-Step Verification**.
- [ ] Choose **Personal** or **Organization** (see §1). Organization needs a **D-U-N-S
      number** + business documents.
- [ ] Complete **identity verification** (government ID; org: business docs).
- [ ] Set up the **Payments profile** (merchant) with **bank + tax** info — required for IAP.
- [ ] Enroll in Play's reduced **15%** service-fee tier (automatic for new accounts under $1M).

**App setup**
- [ ] Create the app; package name = `app.rustynes` (must match everywhere).
- [ ] Create the **in-app product**: a **Managed product** `remove_ads` (non-consumable),
      or a Subscription. Activate it.
- [ ] **Data safety** form, **content rating** (IARC) — rate Teen/12+, **not** child-directed
      (AppLovin forbids child-directed apps; see brief §6).
- [ ] Declare the **AD_ID** permission usage.
- [ ] Create a **service account** for RevenueCat (Section 5): in Google Cloud, create a
      service account + JSON key; in Play Console → Users & permissions (or API access),
      grant it access to view financial data and manage orders/subscriptions.
- [ ] If **Personal** account: set up the **Closed testing** track and run it with **≥12
      testers for 14 continuous days**, then apply for **production access**.
- [ ] **License testing** (so testers aren't charged for the Full Version): Play Console →
      **Settings → License testing** (account level) → add the **same** tester Gmail
      addresses. Track membership alone is *not* enough — without this they pay real money.
      Full detail and the no-purchase alternative are in **§5a**.

---

## 5. RevenueCat (entitlement layer)

**Sign up** — https://app.revenuecat.com (free up to $2,500 MTR, ~1% above)
- [ ] Create a **Project** for RustyNES.
- [ ] Add the **App Store app**: bundle id `app.rustynes` + the App Store Connect
      shared secret / In-App Purchase Key from §3.
- [ ] Add the **Play Store app**: package `app.rustynes` + upload the **service account
      JSON** from §4.
- [ ] Create **Products** mapped to the App Store product id and the Play product id
      (`remove_ads`).
- [ ] Create an **Entitlement** with identifier **`premium`** (this exact string is what the
      code checks) and attach the products to it.
- [ ] Create an **Offering** (mark it *current*) containing a **Package** with the product(s)
      — the shells purchase `offerings.current.availablePackages.first`.
- [ ] Copy the **public SDK API keys**: the Apple key (`appl_…`) and the Google key
      (`goog_…`) for the build (Section 8).
- [ ] *Optional:* configure a **Paywall** (RevenueCatUI) to skip building purchase UI.

---

## 5a. Unlock the paid version for your closed-test cohort (15 testers / 14 days)

Google requires a personal account to run a closed test with **≥12 testers opted-in for 14
continuous days** before production; you're running **15**, comfortably over the minimum.
Those testers need the **unlocked / Full Version** without paying. Because premium is a
single RevenueCat entitlement (`premium`) the app reads, every method below unlocks the whole
app through the *same* code path — there is no separate "tester build."

**Method 1 — RevenueCat promotional grant (recommended: no purchase, revocable).**
- In **RevenueCat → the tester's customer profile → Entitlements card → Grant**, pick
  `premium` and a duration — **1 month** to cover the test window, or **Lifetime**. Granted
  entitlements show as `rc_promo`, unlock everything immediately, and never charge the user.
- Bulk it with the REST API (Secret key):
  `POST https://api.revenuecat.com/v1/subscribers/{app_user_id}/entitlements/premium/promotional`
  with body `{"duration":"lifetime"}` — loop your 15 ids.
- Revoke anytime from the "…" menu on the granted entitlement, or let the duration lapse.
- **App User ID note:** to grant to a *specific* tester you need their RevenueCat App User ID.
  Either have them launch the app once (they appear under an anonymous id you can grant), or
  call `Purchases.logIn(<stable id, e.g. their email>)` in the tester build so you can grant
  ahead of time and keep the unlock stable across reinstalls.

**Method 2 — Google Play license testers (free *test purchases*; exercises the real buy flow).**
- Play Console → **Settings → License testing** → add the 15 tester Gmail accounts (your own
  publishing account is always a license tester).
- A tester taps **Buy** on the Full Version and completes it with a **test payment method** —
  the dialog shows a "test purchase" notice and **no money is charged** (no tax computed). It
  flows through Google Play Billing → RevenueCat → `entitlements["premium"].isActive`, so it
  validates the exact path real buyers use.
- They must be on this list **in addition** to the closed-test tester list — track membership
  alone does not make in-app purchases free.

**Method 3 — Google Play promo codes (optional).** Play Console can issue **promo codes** for
the one-time `remove_ads` product (limited quantity per quarter); hand one to a tester to
redeem the Full Version free. Heavier to manage than Method 1 for a small cohort.

**Not for the closed track — the debug override.** The shells include an internal
`TESTER_UNLOCK` debug override (`Billing.kt` / `Billing.swift`) that forces premium without a
purchase, but it is compiled-inert in release builds — and the closed-test track *is* a
release build. It only helps your own local QA, never the 15 testers. Use Method 1 or 2 for
them. (Codebase detail: brief §9a.)

**iOS (ships ~1 week later).** Method 1 (RevenueCat grant) works unchanged; for the real buy
flow use **App Store sandbox testers** (App Store Connect → Users and Access → Sandbox)
and/or TestFlight. The grant path and the `premium` entitlement are identical across
platforms.

---

## 6. AppLovin MAX (ad mediation)

**Sign up** — https://dashboard.applovin.com
- [ ] Add your app **twice** — one entry for Android (`app.rustynes`) and one for iOS
      (`app.rustynes`). If not yet live, add it manually by package/bundle id.
- [ ] Get the **SDK Key** (Account → General → Keys) — used in the init call.
- [ ] Create **Ad Units** per platform: an **Interstitial** and a **Rewarded** (both
      required — the Rewarded unit powers the free-tier "+11 min per ad" play-time grant).
      Copy each ad-unit id for the build.
- [ ] Enter your **payout + tax** info to receive ad revenue (note payout minimums / NET terms).
- [ ] MAX → Mediation → **Networks**: enable the recommended networks (Section 7).
- [ ] **Consent flow (EEA/UK/Switzerland):** enable MAX's **Terms & Privacy Policy flow**
      (Google UMP) and publish a GDPR message in AdMob. A Google-certified CMP integrated with
      the IAB TCF is **required to serve personalized ads** in the EEA/UK (since 16 Jan 2024)
      and Switzerland (since 31 Jul 2024); the flow must complete **before** the SDK
      initializes. Add every network you enabled to the consent message. (Brief §6g / §7.)

---

## 7. Mediation network accounts (fill the MAX waterfall)

For each network: create the account, create/add the app, link it into MAX (enter the
network's app id/keys in the MAX dashboard), and have the build include that network's
**adapter** (Gradle/SPM — see brief §7). iOS: add each network's **SKAdNetwork IDs**.

- [ ] **Google AdMob** — https://admob.google.com — create account + app; copy the **AdMob
      App ID** into the Android manifest and iOS `GADApplicationIdentifier`. Enables Google
      bidding (usually top demand). Also lets you publish the GDPR/UMP consent message.
- [ ] **Meta Audience Network** — https://www.facebook.com/audiencenetwork — create app, link to MAX.
- [ ] **Unity Ads** — https://dashboard.unity.com — create project, link to MAX.
- [ ] **Pangle (ByteDance/TikTok)** — https://www.pangleglobal.com — create app, link to MAX.

Expand later with Liftoff Monetize (Vungle), Mintegral, InMobi, or ironSource as you tune.

---

## 8. Credentials handoff (what to give the build, and where it goes)

Hand these to whoever wires the app (or drop into config). The skeleton already reads them.

| Value | Source | Android (`gradle.properties`) | iOS (Info.plist / xcconfig) |
|---|---|---|---|
| AppLovin SDK key | AppLovin → Keys | `applovinSdkKey` | `APPLOVIN_SDK_KEY` |
| RevenueCat public key | RevenueCat (per platform) | `revenueCatGoogleKey` (`goog_…`) | `REVENUECAT_API_KEY` (`appl_…`) |
| MAX interstitial ad-unit id | AppLovin → Ad Units | `maxInterstitialAdUnitId` | `MAX_INTERSTITIAL_AD_UNIT_ID` |
| Rewarded ad-unit id (required) | AppLovin → Ad Units | `maxRewardedAdUnitId` | `MAX_REWARDED_AD_UNIT_ID` |
| AdMob App ID | AdMob | `AndroidManifest` `com.google.android.gms.ads.APPLICATION_ID` | `GADApplicationIdentifier` |

Keep real keys out of source control (inject via `gradle.properties`/CI and an `.xcconfig`).

---

## 9. Pre-submission compliance checklist

(Full detail in the implementation brief §6–§7.)

**Android**
- [ ] Native `.so` is **16 KB page-size aligned** (NDK r28+, AGP 8.5.1+) — Play blocks
      non-compliant bundles.
- [ ] Play Billing **v8+** (RevenueCat bundles a compliant version).
- [ ] AD_ID permission declared; Data safety form complete; content rating set (Teen+).
- [ ] **Closed-test cohort can reach the Full Version** without paying — RevenueCat
      promotional grant and/or **License testing** Gmail list configured (§5a).
- [ ] **Certified CMP + IAB TCF** consent (Google UMP via MAX's flow) integrated and GDPR
      message published — required for personalized ads in the EEA/UK/CH; consent completes
      **before** SDK init.

**iOS**
- [ ] `NSUserTrackingUsageDescription` present; ATT requested **before** MAX init.
- [ ] **State in App Store Connect review notes that you use ATT** (omitting → rejection).
- [ ] SKAdNetwork IDs added (AppLovin Info.plist generator) + app `PrivacyInfo.xcprivacy`
      + App Privacy nutrition labels.
- [ ] **Third-party SDK privacy manifests + signatures present** — ship current
      AppLovin/adapter/RevenueCat versions; review runs binary checks (missing/unsigned → reject).
- [ ] EU users: TCF consent flow shown in addition to ATT.

**Both**
- [ ] Emulator only — **no bundled copyrighted ROMs**; provide a file-import path for
      user-owned ROMs/homebrew. Verify the current Apple Guideline 4.7 / Google emulator
      policy text at submission.

---

## 10. Realistic timeline

- **D-U-N-S** (if org): up to ~a few days to a couple of weeks.
- **Apple org verification:** days to weeks (individual is usually faster).
- **Google identity verification:** hours to ~2 business days.
- **Google personal-account closed test:** **14 days minimum** + a review (~up to 7 days)
  for production access — start this early or use an org account to skip it.
- **App review:** Apple typically a day or a few; Google often hours to a couple of days.

Plan **2–4 weeks** of account/verification overhead before either store can go live —
mostly parallelizable with development. Begin the slow verifications (§2) on day one.

---

## 11. Links

- Apple enroll: https://developer.apple.com/programs/enroll/
- Apple D-U-N-S: https://developer.apple.com/support/D-U-N-S/
- App Store Connect: https://appstoreconnect.apple.com
- Apple Small Business Program: https://developer.apple.com/app-store/small-business-program/
- Google Play signup: https://play.google.com/console/signup
- Google Play 12-tester rule: https://support.google.com/googleplay/android-developer/answer/14151465
- RevenueCat: https://app.revenuecat.com — docs: https://www.revenuecat.com/docs
- RevenueCat connect App Store: https://www.revenuecat.com/docs/getting-started/installation/app-store-connect
- RevenueCat connect Play: https://www.revenuecat.com/docs/getting-started/installation/google-play-store
- AppLovin dashboard: https://dashboard.applovin.com
- AdMob: https://admob.google.com
- Meta Audience Network: https://www.facebook.com/audiencenetwork
- Unity dashboard: https://dashboard.unity.com
- Pangle: https://www.pangleglobal.com

*Verify fees, commission rates, and policy text at the time you enroll — they move.*
