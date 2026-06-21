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
    // The Compose compiler plugin must match AGP 9.2's built-in Kotlin (KGP 2.3.10).
    id("org.jetbrains.kotlin.plugin.compose") version "2.3.10" apply false
}
