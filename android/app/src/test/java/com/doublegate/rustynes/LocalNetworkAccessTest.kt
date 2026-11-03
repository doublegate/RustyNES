package com.doublegate.rustynes

import com.doublegate.rustynes.LocalNetworkAccess.Decision
import com.doublegate.rustynes.LocalNetworkAccess.Outcome
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/**
 * v3.6.0 (`T-ANDROID-API37`): the `ACCESS_LOCAL_NETWORK` flow for LAN netplay,
 * on the JVM. The decision is [LocalNetworkAccess]; the Compose glue in
 * `MainActivity.kt` (`withLocalNetwork`) only feeds it `SDK_INT`, the target
 * and `checkSelfPermission`, and launches the prompt on [Decision.ASK].
 */
class LocalNetworkAccessTest {
    private val enforced = LocalNetworkAccess.ENFORCED_FROM_SDK

    /** Android 17 and later, targeting 37: ask unless already granted. */
    @Test
    fun enforced_asks_until_granted() {
        assertEquals(Decision.ASK, LocalNetworkAccess.decide(enforced, enforced, granted = false))
        assertEquals(Decision.PROCEED, LocalNetworkAccess.decide(enforced, enforced, granted = true))
        assertEquals(Decision.ASK, LocalNetworkAccess.decide(enforced + 1, enforced, granted = false))
    }

    /** Below Android 17 the permission does not exist; asking for it would be
     *  answered "denied", which would refuse LAN play on every older device. */
    @Test
    fun older_devices_never_ask() {
        for (sdk in 26 until enforced) {
            assertEquals("sdk $sdk", Decision.PROCEED, LocalNetworkAccess.decide(sdk, enforced, granted = false))
        }
    }

    /** An app targeting 36 or lower keeps the implicit grant on Android 17, and
     *  Google says not to request the permission before targeting 37. */
    @Test
    fun a_lower_target_never_asks() {
        for (target in 26 until enforced) {
            assertEquals(
                "target $target",
                Decision.PROCEED,
                LocalNetworkAccess.decide(enforced, target, granted = false),
            )
        }
    }

    /** The prompt's answer: a refusal does not start LAN play (it could only
     *  time out); a grant does. */
    @Test
    fun the_answer_decides_lan_play() {
        assertEquals(Outcome.PROCEED, LocalNetworkAccess.afterPrompt(granted = true))
        assertEquals(Outcome.REFUSED, LocalNetworkAccess.afterPrompt(granted = false))
    }

    @Test
    fun the_permission_name_is_the_platform_one() {
        assertEquals("android.permission.ACCESS_LOCAL_NETWORK", LocalNetworkAccess.PERMISSION)
    }

    /** The decision above only matters if the build targets 37 and the manifest
     *  declares the permission: a runtime request for an undeclared permission
     *  is denied without a prompt. Both are read from the sources, since the
     *  merged manifest is not on the unit-test classpath. */
    @Test
    fun the_build_targets_37_and_declares_the_permission() {
        val gradle = moduleFile("build.gradle.kts").readText()
        val target = Regex("""(?m)^\s*targetSdk\s*=\s*(\d+)""").find(gradle)?.groupValues?.get(1)?.toInt()
        assertEquals("targetSdk in app/build.gradle.kts", enforced, target)
        val compile = Regex("""(?m)^\s*compileSdk\s*=\s*(\d+)""").find(gradle)?.groupValues?.get(1)?.toInt()
        assertTrue("compileSdk $compile < targetSdk", compile != null && compile >= enforced)

        val manifest = moduleFile("src/main/AndroidManifest.xml").readText()
        assertTrue(
            "AndroidManifest.xml must declare ${LocalNetworkAccess.PERMISSION}",
            Regex("""<uses-permission\s+android:name="${Regex.escape(LocalNetworkAccess.PERMISSION)}"""")
                .containsMatchIn(manifest),
        )
    }

    /** The Compose glue cannot run on the JVM, so its wiring is checked in the
     *  source: LAN host and join go through `withLocalNetwork`, which asks
     *  [LocalNetworkAccess.decide] and launches the prompt on ASK. */
    @Test
    fun lan_host_and_join_go_through_the_gate() {
        val src = moduleFile("src/main/java/com/doublegate/rustynes/MainActivity.kt").readText().replace("\r\n", "\n")
        assertTrue(src.contains("onHost = { port, players -> withLocalNetwork { netplayHost(port, players) } }"))
        assertTrue(src.contains("onJoin = { addr -> withLocalNetwork { netplayJoin(addr) } }"))
        val gate = src.substringAfter("fun withLocalNetwork(action: () -> Unit) {").substringBefore("\n    }\n")
        assertTrue(gate, gate.contains("LocalNetworkAccess.decide(Build.VERSION.SDK_INT, context.applicationInfo.targetSdkVersion, granted)"))
        assertTrue(gate, gate.contains("localNetworkPrompt.launch(LocalNetworkAccess.PERMISSION)"))
        assertTrue(src.contains("LocalNetworkAccess.Outcome.PROCEED -> action?.invoke()"))
        // A refusal is shown in the sheet: `status` is drawn only on the idle screen.
        val refused = src.substringAfter("LocalNetworkAccess.Outcome.REFUSED -> {").substringBefore("}")
        assertTrue(refused, refused.contains("npNotice = status"))
        assertTrue(src.contains("notice = npNotice,"))
    }

    /** Gradle runs unit tests from the module directory (`android/app`); fall
     *  back to the repository layout for a runner started elsewhere. */
    private fun moduleFile(path: String): File =
        listOf(File(path), File("app", path), File("android/app", path)).firstOrNull { it.isFile }
            ?: error("$path not found from ${File(".").absolutePath}")
}
