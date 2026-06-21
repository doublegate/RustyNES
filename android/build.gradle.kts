// Root build script — plugin versions only; modules apply them.
//
// v1.8.8 "Atlas" (Workstream A): bumped to AGP 9.2.1 for official compileSdk 36
// (Android 16) support — AGP 8.x maxes out at API 35 (it would only build 36 via
// the `android.suppressUnsupportedCompileSdk` escape hatch). AGP 9.2 requires
// Gradle 9.4.1 (see gradle-wrapper.properties) + JDK 17. AGP 9 ships *built-in
// Kotlin* (it bundles the Kotlin Gradle plugin), so the standalone
// `org.jetbrains.kotlin.android` plugin is dropped here and in :app — AGP applies
// Kotlin itself. The Compose compiler plugin is still a separate, explicitly
// versioned plugin (kept below, bumped to a Kotlin matching AGP 9.2's bundled KGP).
plugins {
    id("com.android.application") version "9.2.1" apply false
    // v1.8.8 "Atlas" (Workstream J): the Macrobenchmark `:baselineprofile` module is
    // a `com.android.test` module — declare that plugin id here so it resolves for
    // the new module (it shares AGP's 9.2.1 version coordinate).
    id("com.android.test") version "9.2.1" apply false
    // The Compose compiler plugin must match AGP 9.2's built-in Kotlin (KGP 2.3.10).
    id("org.jetbrains.kotlin.plugin.compose") version "2.3.10" apply false
    // v1.8.8 "Atlas" (Workstream J): the Baseline Profile Gradle plugin. The plan
    // named 1.4.1, but that stable line predates AGP 9 and its module-type guard
    // rejects an AGP-9.2.1 `com.android.application` module ("not a supported android
    // module"); the 1.5.0-alpha line is the first to widen the supported-AGP window to
    // 9.x (ART-metric repackage handling + the bumped maxAgpVersion). So the build pins
    // the 1.5.0-alpha06 benchmark/baseline-profile line to match AGP 9.2.1 (see the
    // matching benchmark-macro-junit4 in :baselineprofile). It is applied on BOTH :app
    // (consume + bundle the generated profile) and :baselineprofile (generate it).
    id("androidx.baselineprofile") version "1.5.0-alpha06" apply false
}
