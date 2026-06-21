package com.doublegate.rustynes.baselineprofile

import androidx.benchmark.macro.BaselineProfileMode
import androidx.benchmark.macro.CompilationMode
import androidx.benchmark.macro.FrameTimingMetric
import androidx.benchmark.macro.StartupMode
import androidx.benchmark.macro.StartupTimingMetric
import androidx.benchmark.macro.junit4.MacrobenchmarkRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * v1.8.8 "Atlas" (Workstream J): the startup + jank Macrobenchmark.
 *
 * Measures cold-start `StartupTimingMetric` (timeToInitialDisplay /
 * timeToFullDisplay) and `FrameTimingMetric` (frameDurationCpuMs / frameOverrunMs)
 * across compilation modes so the Baseline Profile's effect is quantified and a
 * regression is catchable:
 *   - [startupNoCompilation] : `CompilationMode.None()` — the cold floor.
 *   - [startupBaselineProfile]: `Partial(BaselineProfileMode.Require)` — what users
 *     get; the delta vs None() is the win. The release vitals gate is P99
 *     `frameOverrunMs <= 0` and cold start well under the Play 5 s line.
 *
 * RUN (device / AVD only — not headless host CI):
 *   ./gradlew :baselineprofile:connectedReleaseAndroidTest
 */
@RunWith(AndroidJUnit4::class)
class StartupBenchmark {

    @get:Rule
    val rule = MacrobenchmarkRule()

    @Test
    fun startupNoCompilation() = startup(CompilationMode.None())

    @Test
    fun startupBaselineProfile() =
        startup(CompilationMode.Partial(BaselineProfileMode.Require))

    private fun startup(mode: CompilationMode) = rule.measureRepeated(
        packageName = "com.doublegate.rustynes",
        metrics = listOf(StartupTimingMetric(), FrameTimingMetric()),
        iterations = 10,
        startupMode = StartupMode.COLD,
        compilationMode = mode,
    ) {
        pressHome()
        startActivityAndWait()
        // Settle the first frames so FrameTimingMetric captures the launch animation.
        device.waitForIdle()
    }
}
