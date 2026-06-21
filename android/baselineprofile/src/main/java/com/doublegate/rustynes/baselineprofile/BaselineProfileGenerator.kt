package com.doublegate.rustynes.baselineprofile

import androidx.benchmark.macro.junit4.BaselineProfileRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * v1.8.8 "Atlas" (Workstream J): generates RustyNES's Baseline + Startup Profile.
 *
 * `BaselineProfileRule.collect` launches the release app repeatedly and records the
 * classes/methods ART touches on the critical path; AGP compiles them into
 * `:app/assets/dexopt/baseline.prof`. `includeInStartupProfile = true` additionally
 * emits the Startup Profile that reorders the launch classes into the first
 * `classes.dex` (the AGP 8.3+ DEX-layout win). Together these AOT-compile the cold
 * start + first-frame path, the documented ~20% (up to ~40% low/mid-tier) cold-start
 * win and fewer P95 frozen frames.
 *
 * RUN (on a device or the managed `pixel6Api34` AVD; cannot run headless on host CI):
 *   ./gradlew :app:generateReleaseBaselineProfile
 * Commit the produced `baseline-prof.txt` under `:app/src/.../generated/baselineProfiles/`
 * so a normal release build ships the profile without a device in the loop.
 */
@RunWith(AndroidJUnit4::class)
class BaselineProfileGenerator {

    @get:Rule
    val rule = BaselineProfileRule()

    @Test
    fun generate() = rule.collect(
        packageName = "com.doublegate.rustynes",
        // Feed the Startup Profile too (DEX-layout reordering on top of the AOT hint).
        includeInStartupProfile = true,
    ) {
        // The critical journey: cold launch -> the Compose shell's first frame
        // (the system splash dismisses once `contentReady` flips). We deliberately
        // stop at the idle/library screen — past it lies the async JNI ROM load + the
        // emu/render threads, which are native (.so) and not ART-profileable, and which
        // need a real ROM (no bundled commercial content). Profiling the launch +
        // Compose shell + UniFFI marshalling layer is exactly the DEX-layer win
        // Baseline Profiles target.
        pressHome()
        startActivityAndWait()
        // Let the first composition + library grid settle so its classes are captured.
        device.waitForIdle()
    }
}
