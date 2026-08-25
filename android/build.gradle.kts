// Root build script — plugin versions only; modules apply them.
//
// v1.8.8 "Atlas" (Workstream A): moved to AGP 9.x for official compileSdk 36
// (Android 16) support — AGP 8.x maxes out at API 35 (it would only build 36 via
// the `android.suppressUnsupportedCompileSdk` escape hatch). AGP 9.2 requires
// Gradle 9.4.1 OR LATER + JDK 17; the wrapper was moved to 9.7.1 in the v2.6.3
// dependency refresh, which is a floor being cleared rather than a pin changing.
//
// AGP 9 ships *built-in Kotlin* (it bundles the Kotlin Gradle plugin), so the
// standalone `org.jetbrains.kotlin.android` plugin is dropped here and in :app —
// AGP applies Kotlin itself.
//
// THERE IS THEREFORE NO `kotlin-gradle-plugin` VERSION IN THIS BUILD TO SET.
// AGP owns it. Asked directly during the v2.6.3 refresh, and worth recording
// because the obvious next move — re-adding `org.jetbrains.kotlin.android` with
// an explicit version — would reintroduce exactly the standalone plugin AGP 9
// replaces, and the two applying Kotlin at once is the failure this drop
// avoided. To move Kotlin, move AGP.
//
// A related measurement, so it is not re-derived: AGP 9.2.1 and 9.3.2 declare
// the SAME `kotlin-gradle-plugin` coordinate (2.2.10) in their published POMs.
//
// THAT COORDINATE IS AGP'S OWN RUNTIME DEPENDENCY, not the Kotlin the build
// compiles user code with. The distinction matters because reading it the other
// way leads straight to a real-sounding conclusion — "the Compose compiler
// plugin must equal the Kotlin compiler version, so 2.3.x against 2.2.10 will
// break the build" — which is correct about Kotlin and wrong about what that
// POM entry means. It was raised in review on exactly those grounds.
//
// Settled by building rather than by argument: with AGP 9.3.2 and the Compose
// compiler plugin at 2.3.21, `bundleFossRelease` + `bundlePlayRelease` report
// BUILD SUCCESSFUL in 17m20s, Compose compilation and R8 included. The
// pre-existing pairing of AGP 9.2.1 with a 2.3.x plugin had been building for
// releases before that.
//
// The Compose compiler plugin below is the version this build does control.
plugins {
    id("com.android.application") version "9.3.2" apply false
    // v1.8.8 "Atlas" (Workstream J): the Macrobenchmark `:baselineprofile` module is
    // a `com.android.test` module — declare that plugin id here so it resolves for
    // the new module (it shares AGP's version coordinate).
    id("com.android.test") version "9.3.2" apply false
    // The Compose compiler plugin tracks the Kotlin line AGP builds against, and
    // it is the only Kotlin version coordinate this build sets (see the header).
    // Moved 2.3.10 -> 2.3.21 within the same line at the v2.6.3 refresh.
    id("org.jetbrains.kotlin.plugin.compose") version "2.3.21" apply false
    // v1.8.8 "Atlas" (Workstream J): the Baseline Profile Gradle plugin. The plan
    // named 1.4.1, but that stable line predates AGP 9 and its module-type guard
    // rejects an AGP-9.x `com.android.application` module ("not a supported android
    // module"); the 1.5.0 line is the first to widen the supported-AGP window to
    // 9.x (ART-metric repackage handling + the bumped maxAgpVersion). The pin has
    // moved alpha06 -> rc01 within that same line, which is the same window with
    // fewer unknowns rather than a new dependency decision (see the matching
    // benchmark-macro-junit4 in :baselineprofile). It is applied on BOTH :app
    // (consume + bundle the generated profile) and :baselineprofile (generate it).
    id("androidx.baselineprofile") version "1.5.0-rc01" apply false
}
