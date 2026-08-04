package com.doublegate.rustynes

/**
 * Shared, flavor-neutral declarations for the `foss` / `play` façade split (v2.0.1,
 * ADR 0025).
 *
 * Everything here is **pure Kotlin / AOSP** — no `com.google.*`, no ads, no billing —
 * so it lives in `src/main` and is visible to BOTH the `foss` and `play` source sets.
 * The Google-Play-specific glue (Play Games, Play Integrity, Cast framework, in-app
 * update/review — all free, no monetization) lives in `src/play/`; the
 * byte-for-byte-API-compatible no-op stand-ins live in `src/foss/`.
 *
 * RustyNES is permanently open-source and income-free (ADR 0035): there is no Billing,
 * no ads, no freemium, and no paid unlock in any flavor.
 *
 * These declarations moved OUT of the proprietary glue files (which are now
 * `play`-only) precisely because `MainActivity` (a `src/main` file) references them —
 * had they stayed in `src/play/` the `foss` variant would not compile. Their values
 * are channel-independent, so a single definition serves both flavors.
 */

/**
 * Play Games Services achievement / leaderboard ids (v1.8.8 "Atlas", Workstream E).
 *
 * `MainActivity` references these ids when it fires an unlock/increment, so the object
 * must resolve in both flavors. In the `foss` build the `PlayGamesManager` façade is a
 * no-op, so an `unlock(PgsIds.ACH_…)` call is swallowed and the ids are inert — but
 * the *symbols* must still exist for `MainActivity` to compile. In the `play` build the
 * real `PlayGamesManager` posts them to the PGS backend.
 */
object PgsIds {
    /** Unlocked the first time any ROM boots. */
    const val ACH_FIRST_ROM: String = "achievement_first_rom_loaded"

    /** Unlocked the first time the user writes a save-state slot. */
    const val ACH_FIRST_SAVE_STATE: String = "achievement_first_save_state"

    /** Unlocked the first time a netplay session connects. */
    const val ACH_FIRST_NETPLAY: String = "achievement_first_netplay"

    /** Incremental: accumulated frames run with fast-forward (turbo) engaged. The
     *  Console-side step target is 100 (so this fires at ~100 turbo frames ≈ a few
     *  seconds of fast-forward). We post deltas via `PlayGamesManager.increment`. */
    const val ACH_TURBO_100: String = "achievement_turbo_100_frames"

    /** Unlocked the first time a cloud save syncs (Workstream D ties in here). */
    const val ACH_FIRST_CLOUD_SYNC: String = "achievement_first_cloud_sync"

    /** A single, minimal leaderboard: total play time in seconds. (Leaderboards are
     *  thin for an emulator; this one is enough to exercise the LeaderboardsClient and
     *  is harmless if the maintainer chooses not to publish it.) */
    const val LB_TOTAL_PLAY_SECONDS: String = "leaderboard_total_play_seconds"
}

/**
 * The (server-side, decrypted) integrity verdict outcome, as the app reasons about it
 * (v1.8.8 "Atlas", Workstream L).
 *
 * A pure enum with no Google dependency, so it is shared. On-device this is always
 * [UNKNOWN]: the `play` `IntegrityManager` cannot decrypt the token locally (that is
 * the maintainer's server endpoint), and the `foss` `IntegrityManager` façade never
 * requests a token at all. The app treats UNKNOWN as "no signal"; nothing is gated on
 * it (there is no entitlement to revoke — every feature is free).
 */
enum class IntegrityVerdict {
    /** Verdict not yet available (flag off, no cloud project, no server endpoint, or
     *  the FOSS build which never requests a token). */
    UNKNOWN,

    /** Server confirmed a genuine, Play-recognized, licensed binary. */
    GENUINE,

    /** Server flagged a tampered / unrecognized / unlicensed binary. */
    TAMPERED,
}
