# `:baselineprofile` — Baseline / Startup Profile + Macrobenchmark (v1.8.8 "Atlas", Workstream J)

This `com.android.test` module generates RustyNES's **Baseline Profile** (an AOT-compile
hint for the cold-start -> first-frame path) and **Startup Profile** (DEX-layout class
reordering), and benchmarks startup + scroll jank. Both **require a device or a booted
AVD** — they cannot run on the headless host CI. The infra is wired so generation is a
single command for the maintainer.

## Generate the profile (maintainer, on a device / AVD)

From `android/`:

```bash
# Uses the managed `pixel6Api34` AOSP AVD (auto-created/booted), or a connected device
# if you flip useConnectedDevices=true in baselineprofile/build.gradle.kts.
./gradlew :app:generateReleaseBaselineProfile
```

AGP runs `BaselineProfileGenerator`, captures the rules, and writes them under
`app/src/<variant>/generated/baselineProfiles/`. **Commit that output** — a normal
`./gradlew :app:bundleRelease` then bundles it into `assets/dexopt/baseline.prof`
(installed at runtime by `androidx.profileinstaller`) with no device in the release loop.
`:app`'s `baselineProfile { automaticGenerationDuringBuild = false }` keeps ordinary
release builds from trying to spin up a device.

## Measure startup + jank (maintainer, on a device / AVD)

```bash
./gradlew :baselineprofile:connectedReleaseAndroidTest
```

`StartupBenchmark` reports `StartupTimingMetric` (timeToInitialDisplay) and
`FrameTimingMetric` (frameOverrunMs P99) for `CompilationMode.None()` vs
`Partial(BaselineProfileMode.Require)`. The release vitals targets: cold start well
under the Play 5 s bad-behaviour line, and frame-overrun **P99 <= 0 ms**.

## Compose stability metrics (host, no device)

```bash
./gradlew :app:assembleRelease -Pcompose.metrics
# reports under app/build/compose-reports/, metrics under app/build/compose-metrics/
```
