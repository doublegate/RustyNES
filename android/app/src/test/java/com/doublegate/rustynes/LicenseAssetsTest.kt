package com.doublegate.rustynes

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.util.Properties

/**
 * The APK carries the repository-root LICENSE and NOTICE (see [LicenseAssets]).
 *
 * `unitTests.isIncludeAndroidResources` (app/build.gradle.kts) makes AGP merge
 * this variant's assets before the tests run and name the merged directory in
 * `com/android/tools/test_config.properties` on the test classpath. Reading
 * that directory checks the whole path a release takes -- the copy task, its
 * registration as a generated asset source, and the asset merge -- rather than
 * only the copy task's own output.
 */
class LicenseAssetsTest {
    private fun mergedAssets(): File {
        val stream = javaClass.classLoader!!.getResourceAsStream("com/android/tools/test_config.properties")
            ?: error("test_config.properties is not on the test classpath; is unitTests.isIncludeAndroidResources on?")
        val props = Properties().apply { stream.use { load(it) } }
        val dir = props.getProperty("android_merged_assets")
            ?: error("test_config.properties names no android_merged_assets: $props")
        return File(dir).also { assertTrue("merged assets dir $it does not exist", it.isDirectory) }
    }

    /** A repository-root file, from the module directory Gradle runs tests in. */
    private fun rootFile(name: String): File =
        listOf(File("../../$name"), File("../$name"), File(name)).firstOrNull { it.isFile }
            ?: error("$name not found from ${File(".").absolutePath}")

    @Test
    fun the_apk_assets_carry_license_and_notice_unchanged() {
        val assets = mergedAssets()
        for ((asset, root) in listOf(LicenseAssets.LICENSE to "LICENSE", LicenseAssets.NOTICE to "NOTICE")) {
            val packaged = assets.resolve(asset)
            assertTrue("$asset is not in the merged assets ($assets)", packaged.isFile)
            val expected = rootFile(root).readBytes()
            assertTrue("$root is empty", expected.isNotEmpty())
            assertArrayEquals("$asset differs from the repository-root $root", expected, packaged.readBytes())
        }
    }

    /** The About dialog offers both texts (a source check: Compose UI does not
     *  run on the JVM here). */
    @Test
    fun the_about_dialog_opens_both() {
        val src = listOf(
            File("src/main/java/com/doublegate/rustynes/Dialogs.kt"),
            File("app/src/main/java/com/doublegate/rustynes/Dialogs.kt"),
        ).firstOrNull { it.isFile }?.readText() ?: error("Dialogs.kt not found")
        val about = src.substringAfter("fun AboutDialog(").substringBefore("\n}\n")
        assertTrue("AboutDialog opens LICENSE", about.contains("LicenseAssets.LICENSE"))
        assertTrue("AboutDialog opens NOTICE", about.contains("LicenseAssets.NOTICE"))
        assertTrue("the text is read from the APK's assets", src.contains("assets.open(asset)"))
    }
}
