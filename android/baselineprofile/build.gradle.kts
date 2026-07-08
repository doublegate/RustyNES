// v1.8.8 "Atlas" (Workstream J) — the Baseline / Startup Profile + Macrobenchmark
// module.
//
// This is a `com.android.test` module (it has no shipped code; it instruments the
// :app under test). It does TWO jobs:
//   1. BaselineProfileGenerator — exercises the cold-start -> first-frame journey
//      under `BaselineProfileRule.collect`, and AGP compiles the captured rules into
//      :app's assets/dexopt/baseline.prof (+ the startup-prof.txt class ordering).
//   2. StartupBenchmark — measures cold-start (TimeToInitialDisplay) and scroll jank
//      (FrameTimingMetric) so a regression is catchable (the release vitals gate).
//
// Both REQUIRE a connected device or a booted AVD to RUN. The Gradle-Managed Virtual
// Device below CAN spin one up headlessly in CI when the runner has nested
// virtualization / KVM (a managed AOSP AVD + KVM is the supported headless path) — but
// this repo's host CI does NOT run generation by default; it is a maintainer / device
// step. The maintainer generates the profile with, from android/:
//   ./gradlew :app:generateReleaseBaselineProfile
// (uses the managed `pixel6Api34` AVD below; -Pandroid.testInstrumentationRunnerArguments
//  or `useConnectedDevices` can switch to a plugged-in phone). This module is built
// (compiled) on every Gradle run so it stays one command away; only the *run* needs a
// device/AVD. The generated baseline-prof.txt is committed under :app so a normal release
// build ships the profile without a device in the loop.

plugins {
    id("com.android.test")
    // AGP 9 supplies Kotlin itself (built-in Kotlin) — no standalone kotlin.android.
    id("androidx.baselineprofile")
}

android {
    namespace = "com.doublegate.rustynes.baselineprofile"
    compileSdk = 37

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    defaultConfig {
        minSdk = 28 // Macrobenchmark floor (needs ART profile + dumpsys timing).
        targetSdk = 36
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        // v2.0.1 (ADR 0025): :app grew a `distribution` flavor dimension (foss/play).
        // This `com.android.test` module targets :app but declares no flavors of its
        // own, so AGP variant-matching needs to know which app flavor to profile —
        // pin it to `foss` (the default, isDefault = true) so `generateBaselineProfile`
        // (and Gradle sync) resolve unambiguously. The generated profile is
        // flavor-independent (launch/scroll classes are identical across flavors), so
        // profiling the foss variant is correct for both.
        missingDimensionStrategy("distribution", "foss")
    }

    // The app being profiled / benchmarked.
    targetProjectPath = ":app"

    // A Gradle-Managed Virtual Device so `generateBaselineProfile` can spin up an
    // emulator headlessly on a runner with KVM (an AOSP image — no Play Services,
    // fastest to boot, and exactly what the profile generation needs). This repo's host
    // CI doesn't invoke generation by default (it's a maintainer/device step), but this
    // managed device is the path that WOULD run it headlessly in a KVM-capable CI.
    // `useConnectedDevices = false` (in the baselineProfile block) routes generation
    // through this AVD; flip it true to use a plugged-in device instead.
    @Suppress("UnstableApiUsage")
    testOptions {
        managedDevices {
            allDevices {
                create<com.android.build.api.dsl.ManagedVirtualDevice>("pixel6Api34") {
                    device = "Pixel 6"
                    apiLevel = 34
                    systemImageSource = "aosp"
                }
            }
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
    }
}

// Generate against the `release` build of :app (the variant the user installs) and
// run on the managed AVD, not a connected device, so CI can do it headlessly.
baselineProfile {
    managedDevices += "pixel6Api34"
    useConnectedDevices = false
}

dependencies {
    implementation("androidx.test.ext:junit:1.2.1")
    implementation("androidx.test.espresso:espresso-core:3.6.1")
    implementation("androidx.test.uiautomator:uiautomator:2.3.0")
    // 1.5.0-alpha06 to match the AGP-9.2.1-compatible baselineprofile plugin (root
    // build.gradle.kts) — the 1.4.1 stable line's module guard rejects an AGP-9 app.
    implementation("androidx.benchmark:benchmark-macro-junit4:1.5.0-alpha06")
}
