package com.doublegate.rustynes

/**
 * v3.6.0 (`T-ANDROID-API37`): the `ACCESS_LOCAL_NETWORK` decision, as plain JVM
 * code so `LocalNetworkAccessTest` can pin it.
 *
 * Android 17 (API 37) blocks local-network traffic by default for an app that
 * targets API 37: outgoing and incoming TCP, and UDP unicast, multicast and
 * broadcast, to or from a local address, whichever API made the socket (a
 * native socket from the Rust netplay crate included). UDP fails with `EPERM`,
 * TCP times out. Access needs the runtime permission
 * `android.permission.ACCESS_LOCAL_NETWORK`, in the `NEARBY_DEVICES` group, so
 * a user who already granted that group is not asked again. An app targeting
 * API 36 or lower keeps an implicit grant on Android 17, and Google's guidance
 * is not to request the permission before targeting 37 (developer.android.com,
 * "Local network permission"). RustyNES targets 37 from v3.6.0.
 *
 * Which RustyNES traffic is local: **LAN / direct-IP netplay** (`npHost` /
 * `npJoin`), UDP between two devices on one network, both directions. It cannot
 * work without the grant, so it is asked for first and not started on a
 * refusal: the user is told why, instead of watching a join time out.
 *
 * Room-code netplay is not local. `rustynes-netplay`'s `NatConnect` learns and
 * exchanges each peer's STUN-reflexive PUBLIC address and punches to it, or
 * relays through TURN; it never offers a host (192.168.x.x) candidate. Two
 * players on one Wi-Fi therefore meet through their router's public address,
 * which is Internet traffic, so it is not asked for.
 *
 * Not local, not asked for: RetroAchievements, the box-art scrapers and the
 * hosted lobby (Internet hosts), the local DNS server (exempt on port 53), and
 * the Presentation-API external display (`Cast.kt`, no sockets). The dormant
 * Cast Application Framework sender (`ChromecastSender.kt`, `play` only,
 * `CHROMECAST_ENABLED = false`) would need re-checking before it is enabled;
 * Google points such senders at the system Output Switcher, which needs no grant.
 */
object LocalNetworkAccess {
    /** The permission's name (`Manifest.permission.ACCESS_LOCAL_NETWORK`, API 37). */
    const val PERMISSION = "android.permission.ACCESS_LOCAL_NETWORK"

    /** The first API level that enforces it (Android 17, `VERSION_CODES.CINNAMON_BUN`). */
    const val ENFORCED_FROM_SDK = 37

    /** What to do before a feature that may use the local network. */
    enum class Decision {
        /** Go ahead: not enforced here, or already granted. */
        PROCEED,

        /** Show the system prompt first and act on its answer ([afterPrompt]). */
        ASK,
    }

    /** What to do with the system prompt's answer. */
    enum class Outcome {
        /** Start LAN play. */
        PROCEED,

        /** Do not start it; say why and where the permission is. */
        REFUSED,
    }

    /**
     * Before LAN play.
     *
     * @param deviceSdk `Build.VERSION.SDK_INT`.
     * @param targetSdk `ApplicationInfo.targetSdkVersion`.
     * @param granted the permission is held (`checkSelfPermission`). Ignored
     *   where the permission is not enforced: on a device below API 37 it does
     *   not exist, and asking for an unknown permission is answered "denied".
     */
    fun decide(deviceSdk: Int, targetSdk: Int, granted: Boolean): Decision = when {
        deviceSdk < ENFORCED_FROM_SDK -> Decision.PROCEED
        targetSdk < ENFORCED_FROM_SDK -> Decision.PROCEED
        granted -> Decision.PROCEED
        else -> Decision.ASK
    }

    /** After the system prompt answered [granted]. */
    fun afterPrompt(granted: Boolean): Outcome = if (granted) Outcome.PROCEED else Outcome.REFUSED
}
