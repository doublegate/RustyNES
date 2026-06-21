package com.doublegate.rustynes

import android.content.Context
import java.io.File
import java.io.PrintWriter
import java.io.StringWriter
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * Opt-in crash reporting (v1.8.8 "Atlas", Workstream L).
 *
 * Privacy-first: **off by default** (explicit consent via the Settings toggle), so the
 * app's "collects nothing by default" data-safety posture is preserved. When the user
 * opts in, an uncaught-exception handler writes the stack trace to a **local** file the
 * user can read / share — RustyNES uploads nothing on its own. This keeps post-launch
 * Android-vitals regressions diagnosable without a third-party reporter and without a
 * google-services.json (adding Firebase Crashlytics without the maintainer's config
 * file would break the build, so it is deliberately NOT pulled in here — it is the
 * documented maintainer option below).
 *
 * Maintainer option: to ship Firebase Crashlytics instead, add the
 * `com.google.gms.google-services` plugin + `firebase-crashlytics` dep + the
 * maintainer's `google-services.json`, then gate `setCrashlyticsCollectionEnabled` on
 * the same opt-in flag. That requires the Firebase project (maintainer ops) and is left
 * out so the default build keeps compiling.
 *
 * ## Android-vitals launch gate (the bad-behaviour thresholds, WS L)
 * The Play "bad behaviour" thresholds RustyNES must clear before / after promotion:
 *  - user-perceived **crash rate < 1.09%**
 *  - **ANR rate < 0.47%** (input not handled within 5 s — never block the UI thread on
 *    the native bridge)
 *  - **per-device crash/ANR < 8%** (one bad SoC family can trip a store warning even
 *    with a healthy global average)
 * Listing under Games, 60 fps emulation clears the games slow-session gate (a frame
 * within 50 ms / ~20 fps) easily. This opt-in reporter exists to diagnose any
 * regression against those numbers.
 */
object CrashReporter {
    private const val DIR = "crash-logs"
    private const val MAX_LOGS = 10

    /** Whether the opt-in handler has been installed (so we chain, not double-install). */
    @Volatile
    private var installed = false

    private fun dir(ctx: Context): File = File(ctx.filesDir, DIR).apply { mkdirs() }

    /**
     * Install the opt-in uncaught-exception handler. A no-op unless [enabled] is true
     * (the user's Settings opt-in). Chains to the previous default handler so the
     * normal crash dialog / process kill still happens after we record the trace.
     */
    fun install(ctx: Context, enabled: Boolean) {
        if (!enabled || installed) return
        installed = true
        val app = ctx.applicationContext
        val previous = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { thread, throwable ->
            runCatching { writeLog(app, thread, throwable) }
            previous?.uncaughtException(thread, throwable)
        }
    }

    private fun writeLog(ctx: Context, thread: Thread, throwable: Throwable) {
        val stamp = SimpleDateFormat("yyyyMMdd-HHmmss", Locale.US).format(Date())
        val sw = StringWriter()
        PrintWriter(sw).use { throwable.printStackTrace(it) }
        val body = buildString {
            append("RustyNES crash report\n")
            append("Time:    ").append(stamp).append('\n')
            append("Version: ").append(BuildConfig.VERSION_NAME)
                .append(" (").append(BuildConfig.VERSION_CODE).append(")\n")
            append("Device:  ").append(android.os.Build.MANUFACTURER).append(' ')
                .append(android.os.Build.MODEL).append('\n')
            append("Android: ").append(android.os.Build.VERSION.RELEASE)
                .append(" (API ").append(android.os.Build.VERSION.SDK_INT).append(")\n")
            append("Thread:  ").append(thread.name).append("\n\n")
            append(sw.toString())
        }
        File(dir(ctx), "crash-$stamp.txt").writeText(body)
        pruneOldLogs(ctx)
    }

    /** Keep only the most recent [MAX_LOGS] crash files. */
    private fun pruneOldLogs(ctx: Context) {
        val logs = dir(ctx).listFiles { f -> f.name.startsWith("crash-") }?.sortedByDescending { it.name }
            ?: return
        logs.drop(MAX_LOGS).forEach { it.delete() }
    }

    /** The saved crash logs, newest first (for the Settings "share crash logs" action). */
    fun logs(ctx: Context): List<File> =
        dir(ctx).listFiles { f -> f.name.startsWith("crash-") }
            ?.sortedByDescending { it.name } ?: emptyList()

    /** Delete all saved crash logs (the Settings "clear" action). */
    fun clear(ctx: Context) {
        dir(ctx).listFiles()?.forEach { it.delete() }
    }
}
