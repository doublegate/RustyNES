package com.doublegate.rustynes

import android.content.Context
import android.net.wifi.WifiManager
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.foundation.background
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.net.Inet4Address
import java.net.NetworkInterface
import uniffi.rustynes_mobile.NpNetConfig
import uniffi.rustynes_mobile.NpPhase
import uniffi.rustynes_mobile.NpStatus

/**
 * Direct-IP / LAN netplay UI (v1.8.6).
 *
 * The Rust bridge ([uniffi.rustynes_mobile.NesController]) owns the GGPO-style
 * rollback session; this is pure host/UI policy. There is NO STUN/TURN or internet
 * matchmaking — two devices on the same Wi-Fi, the host shares its `IP:port`, the
 * joiner dials it. While a session is active the emulation loop drives the core via
 * `npAdvanceFrame` instead of `runFrame` (rollback owns pacing), so turbo /
 * fast-forward / frame-skip / rewind are all gated off.
 */

/** Host vs. Join — the two ways to start a direct-IP session. */
private enum class NetplayRole(val label: String) { Host("Host"), Join("Join") }

/** LAN (direct-IP) vs. Online (room-code) — the two transports. */
private enum class NetplayMode(val label: String) { Lan("LAN"), Online("Online (room code)") }

/**
 * Default online-netplay endpoints (v1.8.7, Phase C). The Phase-B bridge ships NO
 * hardcoded defaults — Phase C owns them, and [AppSettings] lets the user override
 * each in the "Netplay (online)" Settings section.
 *
 * IMPORTANT (maintainer carryover): [SIGNALING_URL] is a CLEARLY-PLACEHOLDER value.
 * Online play does NOT work until the maintainer hosts the `deploy/` relay stack
 * (signaling server + optional coturn) and replaces this URL (in Settings or here).
 * Until then the UI shows a caveat and host/join will fail fast on the placeholder.
 */
object NetplayEndpoints {
    /** Placeholder signaling relay — REPLACE with the hosted `deploy/` stack URL.
     *  Pathless (`wss://<DOMAIN>`): the `deploy/Caddyfile` proxies the WebSocket at
     *  the site root, so no `/ws` path segment is appended. */
    const val SIGNALING_URL = "wss://relay.rustynes.example"
}

/**
 * Build the bridge's [NpNetConfig] from the user-overridable Settings endpoints.
 * Empty STUN → the bridge falls back to Google's public STUN servers. TURN is only
 * configured when the URL + both credentials are present (else punch-or-fail).
 */
fun netplayConfig(settings: AppSettings): NpNetConfig {
    val turnUrl = settings.npTurnUrl.trim().ifBlank { null }
    val turnUser = settings.npTurnUser.trim().ifBlank { null }
    val turnSecret = settings.npTurnSecret.ifBlank { null }
    return NpNetConfig(
        stunServers = emptyList(),
        turnUrl = turnUrl,
        turnUser = turnUser,
        turnSecret = turnSecret,
        signalingUrl = settings.npSignalingUrl.trim(),
    )
}

/**
 * The bottom-sheet netplay panel. Mirrors [SettingsSheet] / the other sheets: a
 * Host/Join segmented control, the role-specific controls, a live status row
 * driven by [status] (polled at the RA cadence in the emulation loop), and a Leave
 * button.
 *
 * @param status the latest [NpStatus] snapshot, or null when no session is active.
 * @param onHost called with (localPort, numPlayers) to begin hosting; localPort 0u
 *               lets the OS pick. Returns nothing — the bound port + LAN IP surface
 *               through [hostInfo].
 * @param hostInfo the "IP:port" to share once hosting started (null until then).
 * @param lastJoinAddress the persisted "ip:port" prefilled into the Join field.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun NetplaySheet(
    status: NpStatus?,
    hostInfo: String?,
    roomCode: String?,
    lastJoinAddress: String,
    lastRoomCode: String,
    onlineConfigured: Boolean,
    onHost: (UShort, UByte) -> Unit,
    onJoin: (String) -> Unit,
    onHostRoom: () -> Unit,
    onJoinRoom: (String) -> Unit,
    onLeave: () -> Unit,
    onSaveJoinAddress: (String) -> Unit,
    onSaveRoomCode: (String) -> Unit,
    onShareRoomCode: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    val active = status != null && status.phase != NpPhase.IDLE
    var mode by remember { mutableStateOf(NetplayMode.Lan) }
    var role by remember { mutableStateOf(NetplayRole.Host) }
    var portText by remember { mutableStateOf("") }
    var joinText by remember { mutableStateOf(lastJoinAddress) }
    var roomText by remember { mutableStateOf(lastRoomCode) }

    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 20.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text("Netplay")

            if (!active) {
                // LAN vs. Online (room-code) transport selector.
                Row(
                    modifier = Modifier.fillMaxWidth().selectableGroup(),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    NetplayMode.entries.forEach { m ->
                        FilterChip(
                            selected = mode == m,
                            onClick = { mode = m },
                            label = { Text(m.label) },
                        )
                    }
                }
            }

            when (mode) {
                NetplayMode.Lan -> Text(
                    "Two devices on the same Wi-Fi. The host shares its IP and port; " +
                        "the other player joins it. No internet matchmaking.",
                    fontSize = 12.sp,
                    color = Color.Gray,
                )
                NetplayMode.Online -> {
                    Text(
                        "Play over the internet: the host gets a 6-character room code " +
                            "to share; the other player enters it.",
                        fontSize = 12.sp,
                        color = Color.Gray,
                    )
                    if (!onlineConfigured) {
                        Text(
                            "Online play requires a relay server. None is configured yet, " +
                                "so only LAN works until one is set in Settings → " +
                                "Netplay (online).",
                            fontSize = 12.sp,
                            color = Color(0xFFFFB300),
                        )
                    }
                }
            }

            if (!active) {
                // Host / Join segmented control (shared by both transports).
                Row(
                    modifier = Modifier.fillMaxWidth().selectableGroup(),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    NetplayRole.entries.forEach { r ->
                        FilterChip(
                            selected = role == r,
                            onClick = { role = r },
                            label = { Text(r.label) },
                        )
                    }
                }

                when (mode) {
                    NetplayMode.Lan -> when (role) {
                        NetplayRole.Host -> {
                            Text("Players: 2", fontSize = 13.sp)
                            OutlinedTextField(
                                value = portText,
                                onValueChange = { portText = it.filter(Char::isDigit).take(5) },
                                label = { Text("Port (empty = auto)") },
                                singleLine = true,
                                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                                modifier = Modifier.fillMaxWidth(),
                            )
                            Button(
                                onClick = {
                                    val port = portText.toUShortOrNull() ?: 0u
                                    onHost(port, 2u)
                                },
                                modifier = Modifier.fillMaxWidth(),
                            ) { Text("Start hosting") }
                        }
                        NetplayRole.Join -> {
                            OutlinedTextField(
                                value = joinText,
                                onValueChange = { joinText = it },
                                label = { Text("Host address (ip:port)") },
                                placeholder = { Text("192.168.1.50:7000") },
                                singleLine = true,
                                modifier = Modifier.fillMaxWidth(),
                            )
                            Button(
                                onClick = {
                                    val addr = joinText.trim()
                                    onSaveJoinAddress(addr)
                                    onJoin(addr)
                                },
                                enabled = joinText.contains(':'),
                                modifier = Modifier.fillMaxWidth(),
                            ) { Text("Join") }
                        }
                    }
                    NetplayMode.Online -> when (role) {
                        NetplayRole.Host -> {
                            // Players is fixed at 2 for the room-code path.
                            Text("Players: 2", fontSize = 13.sp)
                            Button(
                                onClick = onHostRoom,
                                modifier = Modifier.fillMaxWidth(),
                            ) { Text("Host online") }
                        }
                        NetplayRole.Join -> {
                            OutlinedTextField(
                                value = roomText,
                                onValueChange = {
                                    // Auto-uppercase, alphanumeric only, max 6 chars.
                                    roomText = it.uppercase()
                                        .filter(Char::isLetterOrDigit)
                                        .take(6)
                                },
                                label = { Text("Room code") },
                                placeholder = { Text("ABC123") },
                                singleLine = true,
                                keyboardOptions = KeyboardOptions(
                                    capitalization = KeyboardCapitalization.Characters,
                                ),
                                modifier = Modifier.fillMaxWidth(),
                            )
                            Button(
                                onClick = {
                                    val code = roomText.trim()
                                    onSaveRoomCode(code)
                                    onJoinRoom(code)
                                },
                                enabled = roomText.length == 6,
                                modifier = Modifier.fillMaxWidth(),
                            ) { Text("Join online") }
                        }
                    }
                }
            }

            // The bound LAN address to share (host only, once listening).
            if (hostInfo != null) {
                Spacer(Modifier.height(4.dp))
                Text(
                    "On the same Wi-Fi, tell the other player to Join:",
                    fontSize = 12.sp,
                    color = Color.Gray,
                )
                Text(hostInfo, fontSize = 16.sp, color = Color(0xFF80D8FF))
            }

            // The room code to share (online host only, once registered).
            if (roomCode != null) {
                Spacer(Modifier.height(4.dp))
                Text("Room code", fontSize = 12.sp, color = Color.Gray)
                Text(
                    roomCode,
                    fontSize = 40.sp,
                    fontFamily = FontFamily.Monospace,
                    letterSpacing = 6.sp,
                    color = Color(0xFF80D8FF),
                )
                Text(
                    "Tell the other player to enter this code.",
                    fontSize = 12.sp,
                    color = Color.Gray,
                )
                OutlinedButton(
                    onClick = { onShareRoomCode(roomCode) },
                    modifier = Modifier.fillMaxWidth(),
                ) { Text("Share code") }
            }

            // Live status row.
            if (status != null && status.phase != NpPhase.IDLE) {
                Spacer(Modifier.height(4.dp))
                NetplayStatusRow(status)
                OutlinedButton(onClick = onLeave, modifier = Modifier.fillMaxWidth()) {
                    Text("Leave")
                }
            }
        }
    }
}

/** A one-line live status read-out for the panel (phase, role, ping, frames, desync). */
@Composable
private fun NetplayStatusRow(status: NpStatus) {
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            // A red dot on desync; otherwise green while in-game, amber while
            // negotiating/connecting.
            val dot = when {
                status.desync -> Color(0xFFE53935)
                status.phase == NpPhase.IN_GAME -> Color(0xFF66BB6A)
                status.phase == NpPhase.ERROR -> Color(0xFFE53935)
                else -> Color(0xFFFFB300)
            }
            Spacer(
                Modifier
                    .size(10.dp)
                    .clip(CircleShape)
                    .background(dot),
            )
            Spacer(Modifier.size(8.dp))
            val phaseLabel = when (status.phase) {
                NpPhase.IDLE -> "Idle"
                NpPhase.NEGOTIATING -> "Connecting (online)"
                NpPhase.CONNECTING -> "Connecting"
                NpPhase.IN_GAME -> "In game"
                NpPhase.ERROR -> "Error"
            }
            val roleLabel = if (status.isHost) "Host" else "Joiner"
            Text("$phaseLabel · $roleLabel", fontSize = 13.sp)
            // "via relay" badge — shown when the connection fell back to a TURN
            // relay (symmetric NAT). Now wired by #40 (NpStatus.relayed reflects the
            // relay-transport hand-off).
            if (status.relayed) {
                Spacer(Modifier.size(8.dp))
                Text(
                    "via relay",
                    fontSize = 11.sp,
                    color = Color(0xFFCE93D8),
                    modifier = Modifier
                        .clip(CircleShape)
                        .background(Color(0x33CE93D8))
                        .padding(horizontal = 6.dp, vertical = 2.dp),
                )
            }
        }
        // While negotiating (online room-code NAT traversal), show the sub-step.
        if (status.phase == NpPhase.NEGOTIATING && status.detail.isNotEmpty()) {
            Text("${status.detail}…", fontSize = 12.sp, color = Color.Gray)
        }
        val ping = status.pingMs
        if (ping != null) {
            Text("Ping: ${ping} ms", fontSize = 12.sp, color = Color.Gray)
        }
        if (status.phase == NpPhase.IN_GAME) {
            val confirmed = status.confirmedFrame?.toString() ?: "-"
            Text(
                "Frame ${status.currentFrame} · confirmed $confirmed" +
                    if (status.stalled) " · stalling" else "",
                fontSize = 12.sp,
                color = Color.Gray,
            )
        }
        if (status.phase == NpPhase.ERROR && status.message.isNotEmpty()) {
            Text(status.message, fontSize = 12.sp, color = Color(0xFFEF9A9A))
        }
    }
}

/**
 * This device's site-local IPv4 address on the active network interface, for the
 * "share this IP" hint. Prefers the Wi-Fi interface; falls back to the first
 * non-loopback site-local IPv4. Returns null if none is found.
 */
fun localWifiIpv4(context: Context): String? {
    // First try the Wi-Fi service's current connection (the common LAN case).
    runCatching {
        val wifi = context.applicationContext
            .getSystemService(Context.WIFI_SERVICE) as? WifiManager
        @Suppress("DEPRECATION")
        val ip = wifi?.connectionInfo?.ipAddress ?: 0
        if (ip != 0) {
            // WifiInfo.ipAddress is little-endian int.
            return "%d.%d.%d.%d".format(
                ip and 0xFF, (ip shr 8) and 0xFF, (ip shr 16) and 0xFF, (ip shr 24) and 0xFF,
            )
        }
    }
    // Fallback: scan interfaces for a site-local IPv4 (e.g. on Ethernet/USB tether).
    return runCatching {
        // `getNetworkInterfaces()` is a nullable platform type (it can return null
        // when no interfaces are enumerable); guard it so `.toList()` can't NPE.
        java.util.Collections.list(NetworkInterface.getNetworkInterfaces() ?: return null)
            .asSequence()
            .filter { it.isUp && !it.isLoopback }
            .flatMap { it.inetAddresses.toList().asSequence() }
            .filterIsInstance<Inet4Address>()
            .firstOrNull { it.isSiteLocalAddress }
            ?.hostAddress
    }.getOrNull()
}

/**
 * The 512-entry composite-2C02 NES palette LUT as packed ARGB_8888, indexed by the
 * core's palette-index value `(emphasis << 6) | colour` (0..=511). Byte-identical to
 * the core's `rustynes_ppu::palette::build_rgba_lut_from_base(NES_PALETTE)` — the
 * same `NES_PALETTE` table and the same 13/16 per-channel emphasis attenuation.
 *
 * It lets the netplay loop turn the core's non-advancing `indexFramebufferBytes()`
 * (which `npAdvanceFrame` already produced) into ARGB pixels for the Bitmap path,
 * WITHOUT calling `runFrame` again (which would advance the core and desync the
 * rollback session). Custom `.pal` palettes and the Vs. RGB PPUs are not reflected
 * during a netplay session — an accepted limitation of the LAN path.
 */
object NetplayPalette {
    // The 64-entry composite NES master palette (matches rustynes_ppu NES_PALETTE).
    private val BASE = intArrayOf(
        0x6A6D6A, 0x001380, 0x1E008A, 0x39007A, 0x550056, 0x5A0018, 0x4F1000, 0x3D1C00,
        0x253A00, 0x004E00, 0x004600, 0x004A18, 0x00405A, 0x000000, 0x000000, 0x000000,
        0xB9BCB9, 0x184BCF, 0x4B24E8, 0x7C12E0, 0xAB13B5, 0xB72164, 0xAB3718, 0x8B5A00,
        0x5C7A00, 0x209000, 0x008F00, 0x008C42, 0x007D8A, 0x000000, 0x000000, 0x000000,
        0xFFFFFF, 0x64A0FF, 0x8479FF, 0xAC68FF, 0xDA60FF, 0xE26BC5, 0xDC834C, 0xC39A18,
        0x9CB000, 0x60C000, 0x30C83C, 0x28C58C, 0x3CB7C9, 0x4C4C4C, 0x000000, 0x000000,
        0xFFFFFF, 0xC8DDFF, 0xD5CCFF, 0xE5C7FF, 0xF5C5FF, 0xFAC9E6, 0xF8D2BD, 0xEFDA99,
        0xE1E188, 0xC8E788, 0xB0EA9C, 0xA4EBBD, 0xAAE5E2, 0xB0B0B0, 0x000000, 0x000000,
    )

    /** The full 512-entry ARGB LUT, built once. */
    val ARGB: IntArray = IntArray(512).also { lut ->
        for (e in 0 until 8) {
            val emphRed = e and 1 != 0
            val emphGreen = e and 2 != 0
            val emphBlue = e and 4 != 0
            for (c in 0 until 64) {
                val rgb = BASE[c]
                var r = (rgb shr 16) and 0xFF
                var g = (rgb shr 8) and 0xFF
                var b = rgb and 0xFF
                // apply_emphasis: dim the non-emphasized channels by 13/16.
                if (emphRed) { g = (g * 13) shr 4; b = (b * 13) shr 4 }
                if (emphGreen) { r = (r * 13) shr 4; b = (b * 13) shr 4 }
                if (emphBlue) { r = (r * 13) shr 4; g = (g * 13) shr 4 }
                lut[(e shl 6) or c] = (0xFF shl 24) or (r shl 16) or (g shl 8) or b
            }
        }
    }
}

/**
 * Convert the core's little-endian `u16` palette-index framebuffer (2 bytes/pixel,
 * `(emphasis << 6) | colour`) into packed ARGB_8888 pixels via [NetplayPalette].
 * The masking to 0x1FF keeps a stray high bit from running off the 512-entry LUT.
 */
fun packIndexToArgb(idx: ByteArray, out: IntArray) {
    var i = 0
    var p = 0
    val n = out.size
    val lut = NetplayPalette.ARGB
    while (p < n && i + 1 < idx.size) {
        val lo = idx[i].toInt() and 0xFF
        val hi = idx[i + 1].toInt() and 0xFF
        out[p] = lut[((hi shl 8) or lo) and 0x1FF]
        i += 2
        p += 1
    }
}
