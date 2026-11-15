package com.doublegate.rustynes

/**
 * Where the repository-root `LICENSE` (the GPL-3.0 text) and `NOTICE` (the
 * third-party attributions, including the MIT and ISC notices of the code the
 * core derives from) sit inside the APK's assets.
 *
 * They are not checked in under `src/main/assets/`: `app/build.gradle.kts`
 * copies them from the repository root into a generated asset directory for
 * every variant (`copy<Variant>LicenseAssets`, wired with
 * `addGeneratedSourceDirectory`), so the app always ships exactly the files at
 * the root and no second copy can drift. The GPL requires that a recipient of
 * the binary gets its text, and the MIT and ISC licenses require their notices
 * in all copies; the desktop archives already ship both files (release.yml).
 * The About dialog shows them ([AboutDialog]), and `LicenseAssetsTest` checks
 * the merged assets hold both, byte-identical to the root.
 */
object LicenseAssets {
    /** The asset subdirectory the Gradle copy task writes. */
    const val DIR = "licenses"

    /** The GNU General Public License, version 3. */
    const val LICENSE = "$DIR/LICENSE"

    /** The project's NOTICE: copyright, provenance and third-party notices. */
    const val NOTICE = "$DIR/NOTICE"
}
