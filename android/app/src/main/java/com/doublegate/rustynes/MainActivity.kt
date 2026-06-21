package com.doublegate.rustynes

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.OpenableColumns
import android.view.KeyEvent
import android.view.MotionEvent
import java.security.MessageDigest
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.OutlinedButton
import androidx.appcompat.app.AppCompatActivity
import androidx.appcompat.app.AppCompatDelegate
import androidx.core.os.LocaleListCompat
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.material3.adaptive.currentWindowAdaptiveInfo
import androidx.window.core.layout.WindowSizeClass
import androidx.compose.material3.Button
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.foundation.focusGroup
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.focus.FocusDirection
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.FilterQuality
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.boundsInWindow
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.isActive
import kotlinx.coroutines.withContext
import uniffi.rustynes_mobile.NesController
import uniffi.rustynes_mobile.RaLoginStatus
import uniffi.rustynes_mobile.RaToast

/**
 * RustyNES Android — first-boot Compose shell (v1.8.0 "Android", beta.1).
 *
 * This shell drives the byte-identical core entirely through the UniFFI-generated
 * [NesController] control surface: it loads a ROM from the Storage Access
 * Framework picker, runs the emulation on a background coroutine, blits each RGBA
 * frame to a [Bitmap], and routes on-screen + hardware-gamepad input into the
 * single late-latched controller mask. The wgpu/shader render path and the AAudio
 * sink land in beta.2/beta.3 via the `rustynes-android` JNI seam; nothing here
 * touches the determinism contract — input converges on one mask per port,
 * exactly as the desktop and wasm hosts do.
 */
class MainActivity : AppCompatActivity() {

    /** Holds the live controller so hardware key events (dispatched to the
     *  Activity, not Compose) can reach the same instance the UI drives. */
    private val emulator = EmulatorHandle()

    /** Hardware game-controller manager (v1.8.7): device->port assignment, hot-plug,
     *  per-pad remapping, analog/HAT decode, and turbo/autofire. Created in onCreate
     *  (needs the application Context) and registered/unregistered in onResume/onPause. */
    private lateinit var gamepad: GamepadManager

    /** Freemium entitlement (Workstream M); created in onCreate. */
    private lateinit var license: LicenseManager

    /** Play Games Services v2 (Workstreams D+E): sign-in + achievements + leaderboards.
     *  Created in onCreate; all calls no-op behind the default-off PGS_ENABLED flag.
     *  DISTINCT from RetroAchievements (rustynes-ra, v1.8.6). */
    lateinit var playGames: PlayGamesManager
        private set

    /** Play Games cloud-save Snapshots (Workstream D); rides on [playGames] sign-in. */
    lateinit var cloudSave: CloudSaveManager
        private set

    /** Play Integrity anti-tamper client over Billing (Workstream L); no-op behind the
     *  default-off PLAY_INTEGRITY_ENABLED flag + a real cloud project number. */
    private lateinit var integrity: IntegrityManager

    /** In-app update (flexible) + in-app review (Workstream L); no-op on sideload. */
    private lateinit var playUpdates: PlayUpdatesManager

    /** v1.8.8 "Atlas" (Workstream L): the in-app FLEXIBLE-update result launcher,
     *  registered in onCreate (must happen before the Activity is STARTED). */
    private lateinit var updateLauncher: androidx.activity.result.ActivityResultLauncher<androidx.activity.result.IntentSenderRequest>

    /** Set true once a flexible update finished downloading — the shell shows a
     *  "Restart to install" prompt (Compose state so it recomposes). */
    val updateReadyState = androidx.compose.runtime.mutableStateOf(false)

    /** Thermal-throttle listener (perf/battery); cancels fast-forward when hot. */
    private var thermalListener: android.os.PowerManager.OnThermalStatusChangedListener? = null

    /** v1.8.8 "Atlas" (Workstream K): held true to keep the system splash on screen
     *  until the Compose shell has produced its first frame (set from the shell). */
    @Volatile
    private var contentReady: Boolean = false

    /** v1.8.8 "Atlas" (Workstream H): whether we are currently in Picture-in-Picture.
     *  Compose reads it to hide the controls/HUD in the small PiP window; the loop
     *  keeps running so gameplay continues. A Compose [mutableStateOf] so a change
     *  recomposes the shell. */
    val inPipState = androidx.compose.runtime.mutableStateOf(false)

    /** v1.8.8 "Atlas" (Workstream H): the pending deep-link action from a Quick
     *  Settings tile / app shortcut / widget launch (resume / open / library), read
     *  by the Compose shell which then clears it. Seeded from the launch intent and
     *  refreshed on each onNewIntent (singleTop re-launch). */
    val deepLinkState = androidx.compose.runtime.mutableStateOf<String?>(null)

    /** v1.8.8 "Atlas" (Workstream H): the on-screen bounds of the gameplay image, set
     *  by the shell via [setGameplayBounds], used as the PiP `sourceRectHint` so the
     *  enter-PiP animation crops from the picture (not the whole window). */
    private var gameplayBounds: android.graphics.Rect? = null

    /** True while a ROM is loaded + running — gates auto-enter-PiP on leave-hint. */
    @Volatile
    var romRunningForPip: Boolean = false

    override fun onCreate(savedInstanceState: Bundle?) {
        // Install the Android-12+ system splash BEFORE super.onCreate(); keep it up
        // until the first Compose frame is ready (the bridge/ROM-DB load is brief).
        val splash = installSplashScreen()
        splash.setKeepOnScreenCondition { !contentReady }
        super.onCreate(savedInstanceState)
        // v1.8.8 "Atlas" (Workstream B): apply the persisted in-app UI language before
        // the first composition. AppCompat itself persists the last
        // setApplicationLocales() choice and restores it on relaunch, but we keep our
        // SharedPreferences as the single source of truth and re-assert it here so a
        // backup/restore or a prefs edit stays authoritative. System (empty tag) clears
        // any override and follows the device / per-app system language.
        applyPersistedLocale()
        license = LicenseManager(applicationContext)
        // v1.8.8 "Atlas" (Workstream J): the Play Billing `startConnection()` is
        // DEFERRED off the cold-start path to the first foreground (onResume) — it
        // does network/IPC and is not needed to draw the first frame (BillingClient is
        // designed to init lazily). The local entitlement cache is read synchronously
        // in the LicenseManager ctor, so the demo gate is already correct before connect.
        gamepad = GamepadManager(applicationContext, emulator)
        registerThermalBackoff()
        // v1.8.8 "Atlas" (Workstreams D+E+L): Play services managers. All are cheap
        // no-op shells when their gates (PGS_ENABLED / PLAY_INTEGRITY_ENABLED /
        // PLAY_BUILD) are off — the default build constructs them but they do nothing.
        playGames = PlayGamesManager(applicationContext)
        cloudSave = CloudSaveManager(applicationContext, playGames)
        integrity = IntegrityManager(applicationContext)
        playUpdates = PlayUpdatesManager(applicationContext)
        // PGS v2 auto-sign-in (no-op when off). Initialize early; sign-in is silent.
        playGames.initialize()
        // The in-app FLEXIBLE-update launcher must be registered before STARTED.
        updateLauncher = registerForActivityResult(
            androidx.activity.result.contract.ActivityResultContracts.StartIntentSenderForResult(),
        ) { /* a cancelled/failed flexible update is non-fatal; the user can retry */ }
        playUpdates.onUpdateDownloaded = { updateReadyState.value = true }
        // Opt-in crash reporter (Workstream L): off by default; installs the handler
        // only when the user has opted in (read synchronously from prefs).
        CrashReporter.install(applicationContext, AppSettings(this).crashReportsEnabled)
        // v1.8.8 "Atlas" (Workstream H): a launch from a tile / shortcut / widget
        // carries the deep-link action; seed it for the shell to consume.
        deepLinkState.value = intent?.getStringExtra(DeepLink.EXTRA_ACTION)
        enableEdgeToEdge()
        hideSystemBars()
        setContent {
            val settings = remember { AppSettings(this@MainActivity) }
            // Load any persisted per-pad remap tables + the autofire toggle once.
            LaunchedEffect(Unit) { gamepad.loadRemaps(settings) }
            val dark = when (settings.themeMode) {
                ThemeMode.System -> isSystemInDarkTheme()
                ThemeMode.Light -> false
                ThemeMode.Dark -> true
            }
            // v1.8.8 "Atlas" (Workstream B): a real Material 3 theme with Material You
            // dynamic color. On Android 12+ (API 31), when the user leaves "Material You"
            // on (default), the chrome is themed from the wallpaper-derived palette
            // (dynamicLight/DarkColorScheme); otherwise — older device, or toggled off —
            // the RustyNES brand scheme is used. Either way the background is forced
            // black so the letterboxed NES picture sits on black (the gameplay area is
            // NEVER tinted), and only the chrome (bars, menus, controls, Settings) picks
            // up the dynamic/brand color.
            val colorScheme = rememberColorScheme(dark, settings.dynamicColor, settings.accessibilityTheme)
            MaterialTheme(colorScheme = colorScheme.copy(background = Color.Black)) {
                Surface(
                    modifier = Modifier.fillMaxSize(),
                    color = MaterialTheme.colorScheme.background,
                ) {
                    EmulatorScreen(emulator, gamepad, license, settings)
                }
            }
            // The first composition has been laid out — let the splash dismiss.
            LaunchedEffect(Unit) { contentReady = true }
        }
    }

    /** Guards the one-time deferred Billing connect (v1.8.8 WS J cold-start deferral). */
    private var billingConnected = false

    override fun onResume() {
        super.onResume()
        // v1.8.8 "Atlas" (Workstream J): connect to Play Billing on the FIRST foreground
        // (kept off onCreate / the cold-start path). Subsequent resumes just re-verify.
        if (BuildConfig.PLAY_BUILD && ::license.isInitialized) {
            if (!billingConnected) {
                license.connect()
                billingConnected = true
            }
            // Re-verify entitlement against Play on each foreground (a purchase made
            // elsewhere, a refund, or a restore reflects here).
            license.refreshEntitlement()
        }
        // Start listening for controller hot-plug + enumerate connected pads.
        if (::gamepad.isInitialized) gamepad.register()
        // v1.8.8 "Atlas" (Workstream L): Play-services foreground work, all off the
        // cold-start path (first/each resume). Each no-ops on sideload / behind its flag.
        if (::playUpdates.isInitialized && !updateChecked) {
            // Flexible in-app update check (no-op on a non-Play install).
            playUpdates.checkForFlexibleUpdate(updateLauncher)
            updateChecked = true
        }
        if (::playUpdates.isInitialized) playUpdates.resumeStalledUpdate()
        // Warm the Play Integrity Standard token provider (no-op without the flag + a
        // real cloud project number). Defense-in-depth over Billing; never blocks.
        if (::integrity.isInitialized) integrity.prepareToken()
        // Confirm PGS sign-in state (PGS v2 auto-signs-in; refresh the flag silently).
        // The PGS v2 client factories need an Activity — bind this one (held weakly).
        if (::playGames.isInitialized) {
            playGames.attachActivity(this)
            playGames.ensureSignedIn()
        }
    }

    /** Guards the one-time in-app update check (Workstream L). */
    private var updateChecked = false

    override fun onPause() {
        super.onPause()
        // Stop listening for controller hot-plug while backgrounded.
        if (::gamepad.isInitialized) gamepad.unregister()
        onPauseSaveState()
    }

    // Cancel fast-forward when the device starts thermally throttling — the NES
    // itself is light, but uncapped fast-forward can heat a phone; emulation
    // speed (and thus determinism) is unaffected, only the host pacing.
    private fun registerThermalBackoff() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return
        val pm = getSystemService(POWER_SERVICE) as android.os.PowerManager
        val l = android.os.PowerManager.OnThermalStatusChangedListener { status ->
            if (status >= android.os.PowerManager.THERMAL_STATUS_SEVERE) emulator.turbo = false
        }
        pm.addThermalStatusListener(l)
        thermalListener = l
    }

    override fun onDestroy() {
        super.onDestroy()
        thermalListener?.let {
            (getSystemService(POWER_SERVICE) as android.os.PowerManager).removeThermalStatusListener(it)
        }
        // v1.8.8 "Atlas" (Workstream L): detach the in-app-update install listener.
        if (::playUpdates.isInitialized) playUpdates.release()
        // v1.8.8 "Atlas" (Workstreams D+E): clear the weakly-held Activity so PGS can't
        // touch a destroyed Activity.
        if (::playGames.isInitialized) playGames.attachActivity(null)
    }

    /** v1.8.8 "Atlas" (Workstream L): finish a downloaded flexible update (restarts the
     *  app). Called from the shell's "Restart to install" prompt. */
    fun completeFlexibleUpdate() {
        if (::playUpdates.isInitialized) playUpdates.completeFlexibleUpdate()
    }

    /** v1.8.8 "Atlas" (Workstream L): request the in-app review flow after a satisfying
     *  session (the API enforces its own quota; no CTA). No-op on sideload. */
    fun requestInAppReview() {
        if (::playUpdates.isInitialized) playUpdates.maybeRequestReview(this)
    }

    // v1.8.8 "Atlas" (Workstream H): a singleTop re-launch from a tile / shortcut /
    // widget delivers its deep-link here (the existing Activity instance is reused).
    // Update the backing intent + publish the action for the shell to consume.
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        intent.getStringExtra(DeepLink.EXTRA_ACTION)?.let { deepLinkState.value = it }
    }

    // v1.8.8 "Atlas" (Workstream H): record the on-screen gameplay-image bounds so
    // enter-PiP can use them as the sourceRectHint (the PiP shrink animates from the
    // picture, not the whole window). Called by the shell on layout.
    fun setGameplayBounds(rect: android.graphics.Rect) {
        gameplayBounds = rect
    }

    /**
     * v1.8.8 "Atlas" (Workstream H): enter Picture-in-Picture with the NES 8:7
     * display aspect + the gameplay sourceRectHint, so the emulator keeps running in
     * a floating window when the user leaves the app. PiP is API 26+ (== minSdk).
     */
    fun enterPip() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        if (!packageManager.hasSystemFeature(
                android.content.pm.PackageManager.FEATURE_PICTURE_IN_PICTURE,
            )
        ) {
            return
        }
        val params = android.app.PictureInPictureParams.Builder()
            // NES picture is 8:7 PAR-corrected; PiP clamps extreme ratios but 8:7 is fine.
            .setAspectRatio(android.util.Rational(8, 7))
            .also { b -> gameplayBounds?.let { b.setSourceRectHint(it) } }
            .build()
        runCatching { enterPictureInPictureMode(params) }
    }

    // Auto-enter PiP when the user navigates Home (or to recents) while a ROM is
    // running — the gameplay continues in the floating window instead of pausing.
    override fun onUserLeaveHint() {
        super.onUserLeaveHint()
        if (romRunningForPip && !inPipState.value && !emulator.paused) {
            enterPip()
        }
    }

    // PiP enter/exit: publish the mode so the shell hides the controls + HUD in the
    // small window (and restores them on exit). The emulation loop is untouched — it
    // keeps producing frames, so gameplay continues in PiP.
    override fun onPictureInPictureModeChanged(
        isInPictureInPictureMode: Boolean,
        newConfig: android.content.res.Configuration,
    ) {
        super.onPictureInPictureModeChanged(isInPictureInPictureMode, newConfig)
        inPipState.value = isInPictureInPictureMode
    }

    // v1.8.8 "Atlas" (Workstream B): apply the persisted in-app UI language. An empty
    // tag (LanguageMode.System) clears any per-app override (follows the system /
    // per-app system language); a non-empty tag forces that language. This delegates
    // to LocaleManager on API 33+ and to AppCompat's own override on API 24..32.
    private fun applyPersistedLocale() {
        val tag = AppSettings(this).language.tag
        val locales = if (tag.isEmpty()) {
            LocaleListCompat.getEmptyLocaleList()
        } else {
            LocaleListCompat.forLanguageTags(tag)
        }
        AppCompatDelegate.setApplicationLocales(locales)
    }

    // Re-assert immersive mode whenever the window regains focus: the system
    // shows the bars on focus loss (dialogs, the SAF picker, fold/unfold), so
    // without this they'd stay visible and overlay the on-screen controls.
    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) hideSystemBars()
    }

    // Hide the status + navigation bars (and the large-screen taskbar) in sticky
    // immersive mode: they stay hidden during play and only reappear transiently
    // on an edge swipe, then auto-hide — so they never sit on top of the gameplay
    // buttons. Applied identically on the cover and inner displays.
    private fun hideSystemBars() {
        WindowCompat.setDecorFitsSystemWindows(window, false)
        WindowInsetsControllerCompat(window, window.decorView).apply {
            hide(WindowInsetsCompat.Type.systemBars())
            systemBarsBehavior =
                WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        }
    }

    // Save-on-background: write the `auto` save-state for the current ROM so the
    // next open of it resumes where the player left off. The bridge's internal
    // lock serialises this against the emulation thread, so the snapshot is
    // consistent; it is a quick in-memory encode, fine to do on the main thread.
    private fun onPauseSaveState() {
        val ctrl = emulator.controller
        val sha = emulator.romSha
        // RetroAchievements progress sidecar (v1.8.6) is persisted unconditionally —
        // it is unlock progress, not a save-state, so the freemium gate below does
        // not apply. A no-op when no RA session / game is loaded (empty blob).
        if (ctrl != null && sha != null) {
            runCatching {
                val blob = ctrl.raSerializeProgress()
                if (blob.isNotEmpty()) RaProgressStore.save(this, sha, blob)
            }
        }
        // Save-on-background is a paid feature in the Play build; sideload builds
        // (PLAY_BUILD=false) always persist. The demo never persists state.
        if (BuildConfig.PLAY_BUILD && (!::license.isInitialized || !license.isUnlocked)) return
        if (ctrl != null && sha != null) {
            runCatching { SaveStateStore.save(this, sha, SaveStateStore.AUTO_SLOT, ctrl.saveState()) }
            // v1.8.8 "Atlas" (Workstream D): mirror the auto-resume slot to the cloud as
            // its own Snapshot (one independently-updatable unit) so the next device
            // resumes where this one left off. No-op unless cloud saves are active
            // (PGS_ENABLED + signed-in + the toggle); local saves stay authoritative.
            val cfg = AppSettings(this)
            if (::cloudSave.isInitialized && cloudSave.isActive(cfg)) {
                cloudSave.pushSlot(sha, SaveStateStore.AUTO_SLOT, cfg)
            }
        }
    }

    // Hardware gamepad / keyboard: Android dispatches KeyEvents to the Activity.
    // Try the assigned-gamepad path first (device->port + per-pad remap); if the key
    // wasn't from an assigned pad, fall back to the fixed keyboard profile (P1).
    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean =
        gamepad.onKey(event) || emulator.onKeyboard(keyCode, true) || super.onKeyDown(keyCode, event)

    override fun onKeyUp(keyCode: Int, event: KeyEvent): Boolean =
        gamepad.onKey(event) || emulator.onKeyboard(keyCode, false) || super.onKeyUp(keyCode, event)

    // Joystick motion: analog sticks, L/R triggers, and the d-pad-as-HAT axis arrive
    // here (NOT as KeyEvents). Without this override they were silently dropped, so
    // many pads' d-pads did nothing. Routed to the manager which decodes them per port.
    override fun onGenericMotionEvent(event: MotionEvent): Boolean =
        gamepad.onMotion(event) || super.onGenericMotionEvent(event)
}

// v1.8.8 "Atlas" (Workstream K): the RustyNES Material 3 brand color schemes. A
// deep indigo/violet primary (the launcher background family) with an amber/red
// secondary nods to the NES palette. v1.8.8 Workstream B folds in dynamic color
// (wallpaper-derived) on API 31+ via rememberColorScheme(), falling back to these.
private val BrandPrimary = Color(0xFF7C6FF0)
private val BrandSecondary = Color(0xFFFFB74D)
private val BrandTertiary = Color(0xFFE57373)

/** Light Material 3 scheme seeded with the RustyNES brand colors. */
private fun rustyNesLightColors() = lightColorScheme(
    primary = BrandPrimary,
    secondary = BrandSecondary,
    tertiary = BrandTertiary,
)

/** Dark Material 3 scheme seeded with the RustyNES brand colors. */
private fun rustyNesDarkColors() = darkColorScheme(
    primary = BrandPrimary,
    secondary = BrandSecondary,
    tertiary = BrandTertiary,
)

/**
 * v1.8.8 "Atlas" (Workstream B): resolve the active Material 3 [ColorScheme].
 *
 * Precedence (v1.8.8 WS I): an [accessibility] override (high-contrast / colorblind)
 * wins over everything — it is a stronger user need than the wallpaper palette. Else,
 * when [dynamicColor] is requested AND the device is Android 12+ (API 31), the scheme
 * is derived from the system wallpaper (Material You); otherwise the RustyNES brand
 * scheme is used. Light vs. dark is driven by [dark] (which already folds in the
 * user's Light/Dark/System theme choice + `isSystemInDarkTheme()`). The caller forces
 * the `background` to black so the gameplay letterbox is never tinted.
 */
@Composable
private fun rememberColorScheme(
    dark: Boolean,
    dynamicColor: Boolean,
    accessibility: AccessibilityTheme,
): ColorScheme {
    val context = androidx.compose.ui.platform.LocalContext.current
    // Accessibility theme overrides dynamic color + the brand scheme entirely.
    accessibilityColorScheme(accessibility, dark)?.let { return it }
    return when {
        dynamicColor && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S ->
            if (dark) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
        dark -> rustyNesDarkColors()
        else -> rustyNesLightColors()
    }
}

/**
 * Thin holder around the optional [NesController] plus the hardware-key mapping.
 * Compose owns the per-frame [ImageBitmap]; this owns the controller lifetime so
 * the Activity's key handlers and the Compose UI share one instance.
 */
class EmulatorHandle {
    var controller: NesController? = null

    /** Lowercase hex SHA-256 of the loaded ROM — the save-state directory key. */
    var romSha: String? = null

    /** Raw bytes of the loaded ROM — kept so RetroAchievements can (re-)identify
     *  the game via `raLoadGame` if login completes after the ROM was opened. */
    var romBytes: ByteArray? = null

    /** Emulation paused (the loop idles, no frames advance). Read by the loop. */
    @Volatile
    var paused: Boolean = false

    /** Fast-forward: drop the frame-pace delay + audio so the core runs ahead. */
    @Volatile
    var turbo: Boolean = false

    /** Mute the audio sink (the core still produces samples; they're discarded). */
    @Volatile
    var muted: Boolean = false

    // Each NES port's mask is the OR of two sources that must not clobber each
    // other: P1 also gets the on-screen virtual controller (multi-touch), and every
    // port gets its assigned hardware gamepad (via GamepadManager). applyPort()
    // combines them into that port's single late-latched bridge mask.
    @Volatile
    private var touchMask: Int = 0

    /** Per-port hardware-gamepad masks (port 0..3), set by [GamepadManager]. Index 0
     *  is OR'd with [touchMask] for P1. */
    private val gamepadMasks = IntArray(4)

    /** Hardware-keyboard mask — a default profile that always feeds P1 (so a USB/BT
     *  keyboard works even with no game pad assigned to port 0). */
    @Volatile
    private var keyboardMask: Int = 0

    /** Set the on-screen virtual-controller mask (the full set of pressed buttons). */
    fun setTouchMask(mask: Int) {
        // Touch + key updates can race (different threads); synchronize the
        // read-modify-combine so neither source clobbers the other's mask.
        synchronized(this) {
            touchMask = mask
            applyPort(0)
        }
    }

    /** Set a port's hardware-gamepad mask (the full pressed set), from the manager. */
    fun setGamepadMask(port: Int, mask: Int) {
        if (port !in 0..3) return
        synchronized(this) {
            gamepadMasks[port] = mask
            applyPort(port)
        }
    }

    /**
     * Hardware-keyboard fallback: a fixed default profile feeding P1, used when a
     * KeyEvent wasn't consumed by an assigned gamepad. Returns true if the key was a
     * mapped NES button.
     */
    fun onKeyboard(keyCode: Int, pressed: Boolean): Boolean {
        val bit = keyboardKeyToBit(keyCode) ?: return false
        synchronized(this) {
            keyboardMask = if (pressed) keyboardMask or bit else keyboardMask and bit.inv()
            applyPort(0)
        }
        return true
    }

    private fun applyPort(port: Int) {
        val mask = if (port == 0) {
            touchMask or gamepadMasks[0] or keyboardMask
        } else {
            gamepadMasks[port]
        }
        controller?.setButtons(port.toUInt(), mask.toUByte())
    }

    /** Re-push every port's mask — used after a fresh ROM load so the new controller
     *  immediately reflects any held inputs (and so port 0 isn't left stale). */
    fun reapplyAllPorts() {
        synchronized(this) { for (p in 0..3) applyPort(p) }
    }

    /** The current combined P1 mask (touch | gamepad[0] | keyboard) — the
     *  `local_mask` netplay feeds to `npAdvanceFrame` (where the bridge owns the
     *  latch, so `setButtons` is not the input path). Synchronized against the
     *  touch/key updaters. */
    fun p1Mask(): Int = synchronized(this) { touchMask or gamepadMasks[0] or keyboardMask }

    private fun keyboardKeyToBit(keyCode: Int): Int? = when (keyCode) {
        KeyEvent.KEYCODE_ENTER -> NesBit.START
        KeyEvent.KEYCODE_SPACE -> NesBit.SELECT
        KeyEvent.KEYCODE_Z -> NesBit.A
        KeyEvent.KEYCODE_X -> NesBit.B
        KeyEvent.KEYCODE_DPAD_UP -> NesBit.UP
        KeyEvent.KEYCODE_DPAD_DOWN -> NesBit.DOWN
        KeyEvent.KEYCODE_DPAD_LEFT -> NesBit.LEFT
        KeyEvent.KEYCODE_DPAD_RIGHT -> NesBit.RIGHT
        else -> null
    }
}

/** NES controller button bits — matches `rustynes_core::Buttons`. */
object NesBit {
    const val A = 0x01
    const val B = 0x02
    const val SELECT = 0x04
    const val START = 0x08
    const val UP = 0x10
    const val DOWN = 0x20
    const val LEFT = 0x40
    const val RIGHT = 0x80
}

/**
 * Load a ROM into [emulator] from raw bytes: build the controller, key it by
 * SHA-256, auto-resume the on-background save-state for that ROM if present, and
 * — when the bytes came from a SAF [uri] — take a persistable read grant + record
 * it in the recent-ROMs list so it survives reboot. Returns a status line. May
 * throw if the bytes are not a valid ROM (callers wrap in `runCatching`).
 */
private fun loadRom(
    context: Context,
    emulator: EmulatorHandle,
    bytes: ByteArray,
    uri: Uri?,
    name: String?,
    unlocked: Boolean,
    settings: AppSettings,
): String {
    val ctrl = NesController(bytes, 48_000u)
    val sha = sha256Hex(bytes)
    emulator.controller = ctrl
    emulator.romSha = sha
    emulator.romBytes = bytes
    // Apply this game's remembered video filter (per-game DB), if any.
    GameConfig.filter(context, sha)?.let { f ->
        settings.filter = VideoFilter.entries.getOrElse(f) { VideoFilter.None }
    }
    // Auto-resume the on-background save-state is a paid feature; the demo always
    // cold-boots the ROM.
    if (unlocked) {
        SaveStateStore.load(context, sha, SaveStateStore.AUTO_SLOT)?.let { blob ->
            runCatching { ctrl.loadState(blob) }
        }
    }
    if (uri != null) {
        runCatching {
            context.contentResolver.takePersistableUriPermission(
                uri,
                Intent.FLAG_GRANT_READ_URI_PERMISSION,
            )
        }
        RomLibrary.remember(context, uri.toString(), name ?: uri.lastPathSegment ?: "ROM")
    }
    // v1.8.8 "Atlas" (Workstream C): record / refresh this game in the box-art
    // library, keyed by the real ROM SHA-256. The mapper/region come cheaply from
    // the just-built controller's RomInfo; lastPlayed is stamped now. The user-owned
    // fields (favorite / box art / folder) are preserved by GameLibrary.upsert.
    val display = name ?: uri?.lastPathSegment ?: "ROM"
    val info = runCatching { ctrl.info() }.getOrNull()
    GameLibrary.upsert(
        context,
        GameEntry(
            sha = sha,
            name = display,
            uri = uri?.toString() ?: "",
            mapper = info?.mapperId?.toInt() ?: -1,
            region = info?.region?.name ?: "",
            lastPlayed = System.currentTimeMillis(),
        ),
    )
    return "Running" + (name?.let { " · $it" } ?: "")
}

/** Resolve a SAF document's human-readable display name for the recents list. */
private fun displayName(context: Context, uri: Uri): String {
    context.contentResolver.query(
        uri,
        arrayOf(OpenableColumns.DISPLAY_NAME),
        null,
        null,
        null,
    )?.use { c ->
        if (c.moveToFirst()) {
            val i = c.getColumnIndex(OpenableColumns.DISPLAY_NAME)
            if (i >= 0) return c.getString(i)
        }
    }
    return uri.lastPathSegment ?: "ROM"
}

private const val NES_WIDTH = 256
private const val NES_HEIGHT = 240

// NTSC frame period in nanoseconds (the wall-clock pacing floor for ROMs that
// emit little/no audio; sound-producing ROMs are paced by the blocking audio
// write). PAL/Dendy refinement is a later increment.
private const val FRAME_NANOS = 16_639_267L

/**
 * Low-latency mono audio sink fed by the core's [NesController.drainAudio].
 *
 * The core emits deterministic `f32` samples at the host rate; this is a pure
 * sink (the determinism contract is timing-only and lives outside the emitted
 * stream). A blocking `AudioTrack` write paces the emulation loop to real time
 * whenever sound is present — the audio clock, not the wall clock, drives frame
 * cadence — while a small buffer absorbs scheduling jitter.
 */
private class AudioPlayer(sampleRate: Int) {
    private val track: AudioTrack
    init {
        val minBuf = AudioTrack.getMinBufferSize(
            sampleRate,
            AudioFormat.CHANNEL_OUT_MONO,
            AudioFormat.ENCODING_PCM_FLOAT,
        )
        // ~4 NES frames of headroom (>= the platform minimum) absorbs jitter
        // without adding noticeable latency.
        val bufBytes = maxOf(minBuf, sampleRate * 4 / 60 * 4)
        track = AudioTrack.Builder()
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_GAME)
                    .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                    .build(),
            )
            .setAudioFormat(
                AudioFormat.Builder()
                    .setEncoding(AudioFormat.ENCODING_PCM_FLOAT)
                    .setSampleRate(sampleRate)
                    .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                    .build(),
            )
            .setBufferSizeInBytes(bufBytes)
            .setTransferMode(AudioTrack.MODE_STREAM)
            .setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY)
            .build()
        track.play()
    }

    /**
     * Write one frame of samples; blocks while the ring is full (paces the loop).
     *
     * UniFFI maps the core's `Vec<f32>` to a boxed `List<Float>`, so this copies
     * into a primitive `FloatArray` for the `AudioTrack` write. The per-frame
     * boxing is a known cost; a later increment moves the audio pull into the
     * `rustynes-android` JNI hot path (a primitive `float[]`/native ring) per the
     * v1.8.0 plan's Workstream C.
     */
    fun write(samples: List<Float>) {
        if (samples.isNotEmpty()) {
            val buf = samples.toFloatArray()
            track.write(buf, 0, buf.size, AudioTrack.WRITE_BLOCKING)
        }
    }

    /**
     * v1.8.4 hot path: write raw little-endian `f32` bytes straight to the
     * `PCM_FLOAT` track. The bytes come from `NesController.drainAudioBytes()` as a
     * single `ByteArray` (no per-sample `Float` boxing); `AudioTrack` reads them as
     * native-order PCM floats (Android is little-endian), so there's no float copy
     * either. Blocks while the ring is full (paces the loop), exactly like [write].
     */
    fun writeBytes(bytes: ByteArray) {
        if (bytes.isNotEmpty()) {
            // The samples are little-endian f32 (from `to_le_bytes`); set the buffer
            // order explicitly so the PCM_FLOAT track reads them correctly regardless
            // of the JVM's default (BIG_ENDIAN) — Android is LE, so this is identity.
            val bb = java.nio.ByteBuffer.wrap(bytes).order(java.nio.ByteOrder.LITTLE_ENDIAN)
            track.write(bb, bytes.size, AudioTrack.WRITE_BLOCKING)
        }
    }

    fun release() {
        runCatching { track.pause(); track.flush(); track.stop() }
        track.release()
    }
}

@Composable
private fun EmulatorScreen(
    emulator: EmulatorHandle,
    gamepad: GamepadManager,
    license: LicenseManager,
    settings: AppSettings,
) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val activity = context as? Activity
    // v1.8.8 "Atlas" (Workstream F/H): the typed host for PiP + deep-link + capture.
    val host = context as? MainActivity
    // Freemium is active only in the Play build; sideload/dev builds are unlimited.
    val unlocked = !BuildConfig.PLAY_BUILD || license.isUnlocked
    var frame by remember { mutableStateOf<ImageBitmap?>(null) }
    // v1.8.8 "Atlas" (Workstream H): true while we are in the PiP window — drives the
    // controls/HUD hide so only the gameplay picture shows in the floating window.
    val inPip = host?.inPipState?.value ?: false
    // v1.8.8 "Atlas" (Workstream F): the latest gameplay-frame bitmap (a reference to
    // the loop's reused buffer + the HD-pack buffer), captured for screenshot/clip.
    // Plain holder (not Compose state) read on a capture tap; the loop writes it.
    val capture = remember { CaptureState() }
    var recording by remember { mutableStateOf(false) }
    // Whether a ROM is currently loaded — drives the Open/Close toggle button and
    // gates the gameplay view vs. the idle (Open + recents) screen.
    var romLoaded by remember { mutableStateOf(false) }
    // HD-pack (v1.8.5): `hdActive` switches the UI to the Bitmap path (the GPU
    // SurfaceView is fixed 256x240; HD output is upscaled), `hd` holds its bitmap.
    var hdActive by remember { mutableStateOf(false) }
    val hd = remember { HdRender() }
    // Lua scripting (v1.8.6): whether a script is loaded + its rolling log output.
    var scriptLoaded by remember { mutableStateOf(false) }
    var scriptLog by remember { mutableStateOf("") }
    // RetroAchievements (v1.8.6): coarse login status text, the signed-in user (or
    // null), and the live HUD toast queue drained from the bridge each frame.
    var raStatus by remember { mutableStateOf("Logged out") }
    var raUserName by remember { mutableStateOf<String?>(null) }
    var raToasts by remember { mutableStateOf<List<RaToast>>(emptyList()) }
    // Set after a game was opened so RA can (re-)identify it once login completes;
    // cleared once raLoadGame has been issued for the current login + ROM.
    var raGameLoaded by remember { mutableStateOf(false) }
    // Netplay (v1.8.6, LAN/direct-IP): the panel sheet visibility, the latest status
    // snapshot (polled in the loop at the RA cadence), and the host's bound IP:port
    // to share once listening. `npActive` is mirrored each poll so the loop's
    // controls-gate (turbo/etc.) and the overlay react without re-locking the bridge.
    var showNetplay by remember { mutableStateOf(false) }
    var npStatus by remember { mutableStateOf<uniffi.rustynes_mobile.NpStatus?>(null) }
    var npHostInfo by remember { mutableStateOf<String?>(null) }
    // Online (room-code) netplay (v1.8.7): the host's 6-char code to share, set once
    // np_host_room returns (null otherwise).
    var npRoomCode by remember { mutableStateOf<String?>(null) }
    var npActive by remember { mutableStateOf(false) }
    // Tracks the previous login status to detect the LOGGED_OUT -> LOGGED_IN edge.
    var raWasLoggedIn by remember { mutableStateOf(false) }
    // Off-main-thread scope for the one-shot SAF loads (HD-pack parse, config I/O).
    val scope = rememberCoroutineScope()
    var status by remember { mutableStateOf(context.getString(R.string.status_open_rom)) }
    var recents by remember { mutableStateOf(RomLibrary.recents(context)) }
    // v1.8.8 "Atlas" (Workstream C): the box-art library state. `libraryVersion` is
    // bumped on every mutation to recompute the filtered/sorted view + folder list.
    var libraryVersion by remember { mutableStateOf(0) }
    var libFolder by remember { mutableStateOf<String?>(null) }
    var libFavoritesOnly by remember { mutableStateOf(false) }
    var libQuery by remember { mutableStateOf("") }
    var librarySort by remember { mutableStateOf(LibrarySort.RECENT) }
    val libraryEntries = remember(libraryVersion, libFolder, libFavoritesOnly, libQuery, librarySort) {
        GameLibrary.view(context, libFolder, libFavoritesOnly, libQuery, librarySort)
    }
    val libraryFolders = remember(libraryVersion) { GameLibrary.folders(context) }
    // The game a long-press/context action currently targets (set box art / move).
    var boxArtTarget by remember { mutableStateOf<GameEntry?>(null) }
    var folderTarget by remember { mutableStateOf<GameEntry?>(null) }
    var boxArtPreview by remember { mutableStateOf<BoxArtPreview?>(null) }
    // Folder batch-import progress (null = idle): (done, total).
    var importProgress by remember { mutableStateOf<Pair<Int, Int>?>(null) }
    // Demo session clock: seconds remaining this launch (full unlock = no limit).
    var demoSecondsLeft by remember { mutableStateOf(DEMO_SESSION_SECONDS) }
    var demoExpired by remember { mutableStateOf(false) }
    // Settings are created at the theme root and passed in (v1.8.3).
    // Drive the audio-mute flag from the persisted setting.
    LaunchedEffect(settings.muted) { emulator.muted = settings.muted }
    var showSettings by remember { mutableStateOf(false) }
    var showStates by remember { mutableStateOf(false) }
    // v1.8.8 "Atlas" (Workstream D): a surfaced cloud-save conflict awaiting the user's
    // keep-local / keep-cloud choice (null = none). Set by a Snapshots open/push that
    // diverged; cleared once resolved or dismissed.
    var cloudConflict by remember { mutableStateOf<SaveConflict?>(null) }
    var showAbout by remember { mutableStateOf(false) }
    var showControllers by remember { mutableStateOf(false) }
    // First-run onboarding shows until the user ticks "Do not show again".
    var showOnboarding by remember { mutableStateOf(!settings.onboardingSuppressed) }
    var screenSize by remember { mutableStateOf(androidx.compose.ui.unit.IntSize.Zero) }

    // Controller-aware UI (v1.8.7, #41). True while >= 1 hardware pad is assigned;
    // collected from the manager's StateFlow (seeded by its launch enumeration, so a
    // pad connected at start-up is covered, and disconnect flips it back declaratively).
    val controllerActive by gamepad.hardwareControllerActive.collectAsStateWithLifecycle()
    // When a pad is active AND the user left auto-hide on, the on-screen pad collapses
    // and the game view takes the freed space. Reverts automatically on unplug.
    val controlsHidden = controllerActive && settings.autoHideControllerOnPad
    // The control bar / menu surface, shared by the touch "MENU pill" and the
    // pad's Guide button / Start+Select chord. `menuOpen` mirrors into the manager so
    // it knows to route the pad to menu navigation (and consume those inputs).
    var controlsVisible by remember { mutableStateOf(false) }
    val focusManager = LocalFocusManager.current
    // First-item focus anchor so opening via pad lands focus on the menu (so the
    // d-pad immediately moves between entries, A activates, B dismisses).
    val menuFocusRequester = remember { FocusRequester() }
    // Set when the menu is opened by the pad (Guide/chord) so a LaunchedEffect grabs
    // focus once it's composed; cleared on close. Touch-opened menus don't grab focus.
    var menuWantsFocus by remember { mutableStateOf(false) }

    // Keep the manager's menuOpen flag in lockstep with the control bar's visibility,
    // so pad input is routed to navigation exactly while the menu is on screen.
    LaunchedEffect(controlsVisible) { gamepad.menuOpen = controlsVisible }

    // Wire the pad's system-menu + navigation callbacks. They fire on the input
    // dispatch thread; marshal to the UI thread before touching Compose state/focus.
    DisposableEffect(gamepad) {
        gamepad.onSystemMenu = {
            activity?.runOnUiThread {
                controlsVisible = true
                menuWantsFocus = true
            }
        }
        gamepad.onMenuNav = { dir ->
            activity?.runOnUiThread {
                focusManager.moveFocus(
                    when (dir) {
                        MenuDir.UP -> FocusDirection.Up
                        MenuDir.DOWN -> FocusDirection.Down
                        MenuDir.LEFT -> FocusDirection.Left
                        MenuDir.RIGHT -> FocusDirection.Right
                    },
                )
            }
        }
        // A activates the focused entry by dispatching a synthetic Enter key (the
        // Material Button's default key-activation), so no per-button plumbing is
        // needed. The Activity is the focus owner's window for KeyEvent dispatch.
        gamepad.onMenuSelect = {
            activity?.runOnUiThread {
                val v = activity.window?.decorView?.findFocus()
                if (v != null) {
                    val now = android.os.SystemClock.uptimeMillis()
                    v.dispatchKeyEvent(KeyEvent(now, now, KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_DPAD_CENTER, 0))
                    v.dispatchKeyEvent(KeyEvent(now, now, KeyEvent.ACTION_UP, KeyEvent.KEYCODE_DPAD_CENTER, 0))
                }
            }
        }
        gamepad.onMenuDismiss = {
            activity?.runOnUiThread {
                menuWantsFocus = false
                controlsVisible = false
                focusManager.clearFocus()
            }
        }
        onDispose {
            gamepad.onSystemMenu = null
            gamepad.onMenuNav = null
            gamepad.onMenuSelect = null
            gamepad.onMenuDismiss = null
            gamepad.menuOpen = false
        }
    }

    // When the menu is opened by the pad, grab focus onto its first entry once the bar
    // is composed (a frame after controlsVisible flips), so the d-pad drives it.
    LaunchedEffect(controlsVisible, menuWantsFocus) {
        if (controlsVisible && menuWantsFocus) {
            // Yield a frame so the control-bar Row (and its FocusRequester) exist.
            delay(50)
            runCatching { menuFocusRequester.requestFocus() }
            menuWantsFocus = false
        }
    }

    // Casting (item 1): track external presentation displays and present the
    // gameplay there while the phone keeps the controller.
    val castManager = remember { CastManager(context) }
    DisposableEffect(Unit) {
        castManager.register()
        onDispose { castManager.unregister() }
    }
    // Experimental Chromecast (CAF) spectator mirror (v1.8.7, #38) — PREPPED but
    // gated behind the default-off BuildConfig.CHROMECAST_ENABLED flag. When off,
    // the sender never touches CastContext, no Cast button is shown, and the
    // sendFrame call below compiles out. The Presentation-API cast above is the
    // primary low-latency path and is untouched.
    val chromecast = remember { ChromecastSender(context) }
    if (BuildConfig.CHROMECAST_ENABLED) {
        DisposableEffect(Unit) {
            chromecast.register()
            onDispose { chromecast.unregister() }
        }
    }
    // v1.8.8 "Atlas" (Workstream A): drive layout off Window Size Classes — the
    // single adaptive layout driver across phone / foldable cover+inner / tablet /
    // free-form / desktop / ChromeOS / TV windows. Replaces the ad-hoc
    // screenWidthDp threshold. `compact` width (< 600 dp) is the phone / folded
    // cover screen; `medium` (>= 600 dp) and `expanded` (>= 840 dp) are tablets,
    // the Z Fold inner display, and resizable desktop windows.
    val windowSizeClass = currentWindowAdaptiveInfo().windowSizeClass
    val isMediumWidth = windowSizeClass.isWidthAtLeastBreakpoint(WindowSizeClass.WIDTH_DP_MEDIUM_LOWER_BOUND)
    val isExpandedWidth = windowSizeClass.isWidthAtLeastBreakpoint(WindowSizeClass.WIDTH_DP_EXPANDED_LOWER_BOUND)

    // Current screen mode (item 5): each remembers its own controller size/opacity.
    // Cast wins; otherwise a compact-width window is the folded cover screen and a
    // medium/expanded window (unfolded inner display, tablet, desktop) is "Inner".
    val screenMode = when {
        castManager.casting -> ScreenMode.Cast
        !isMediumWidth -> ScreenMode.Cover
        else -> ScreenMode.Inner
    }

    // Native wgpu SurfaceView renderer (v1.8.4, Workstream B). Opt-in + API 33+;
    // read ONCE at launch so it never flips mid-session. When off (default), the
    // proven Compose Bitmap path is used unchanged. The emulation loop still packs
    // the Bitmap (for casting / the idle thumbnail); the SurfaceView just draws the
    // raw frame on the GPU.
    val gpuSurface = remember {
        if (settings.useGpuRenderer &&
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            NativeRenderer.ensureLoaded()
        ) {
            NesSurfaceView(context)
        } else {
            null
        }
    }
    // Drive the GPU renderer's filter + its tuning params from the settings; the
    // VideoFilter ordinals line up with the native filter codes. Re-applies live
    // whenever the selected filter OR any of its sliders change. No-op without it.
    LaunchedEffect(
        gpuSurface,
        settings.filter,
        settings.scanlineIntensity,
        settings.scanlineRows,
        settings.apertureMask,
        settings.ntscSaturation,
        settings.ntscSharpness,
        settings.ntscTint,
        settings.ntscPhase,
    ) {
        gpuSurface?.setFilter(settings.filter.ordinal, settings.filterParams(settings.filter))
    }

    // Per-game DB (v1.8.5): remember the chosen filter for the loaded ROM (by SHA),
    // so it reopens with that look. Fires on filter change; a no-op without a ROM.
    LaunchedEffect(settings.filter) {
        emulator.romSha?.takeIf { it.isNotEmpty() }?.let { sha ->
            // The JSON read+write is small but still disk I/O — keep it off the main
            // thread (this effect runs on the main dispatcher).
            withContext(Dispatchers.IO) {
                GameConfig.setFilter(context, sha, settings.filter.ordinal)
            }
        }
    }

    // SAF document picker — no broad storage permission required. The picked
    // bytes are handed straight to the core (no path), a persistable read grant
    // is taken, and the ROM is recorded in the recents list (Workstream E).
    val picker = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri != null) {
            runCatching {
                val name = displayName(context, uri)
                val bytes = (context.contentResolver.openInputStream(uri)
                    ?: throw java.io.IOException("can't open ROM stream")).use { it.readBytes() }
                status = loadRom(context, emulator, bytes, uri, name, unlocked, settings)
                recents = RomLibrary.recents(context)
            }.onFailure { status = "Failed to load ROM: ${it.message}" }
        }
    }

    // SAF picker for a custom .pal palette (a 192-byte RGB table; extra colours,
    // e.g. a 512-colour Mesen palette, are ignored). Applied live to the running
    // core via the bridge; presentation-only, so determinism is untouched.
    val palettePicker = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri != null) {
            runCatching {
                val bytes = (context.contentResolver.openInputStream(uri)
                    ?: throw java.io.IOException("can't open palette stream")).use { it.readBytes() }
                emulator.controller?.loadPalette(bytes)
                status = "Palette loaded: ${displayName(context, uri)}"
            }.onFailure { status = "Failed to load palette: ${it.message}" }
        }
    }

    // SAF picker to play a .rnm TAS movie (reads the bytes, seeks to its start).
    val moviePicker = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri != null) {
            runCatching {
                val bytes = (context.contentResolver.openInputStream(uri)
                    ?: throw java.io.IOException("can't open movie stream")).use { it.readBytes() }
                emulator.controller?.moviePlay(bytes)
                status = "Playing movie: ${displayName(context, uri)}"
            }.onFailure { status = "Failed to play movie: ${it.message}" }
        }
    }

    // SAF document creator to save the just-recorded .rnm movie (finalizes + writes).
    val movieSaver = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.CreateDocument("application/octet-stream"),
    ) { uri ->
        if (uri != null) {
            runCatching {
                val bytes = emulator.controller?.movieStopRecording() ?: ByteArray(0)
                context.contentResolver.openOutputStream(uri)!!.use { it.write(bytes) }
                status = "Saved movie (${bytes.size} bytes)"
            }.onFailure { status = "Failed to save movie: ${it.message}" }
        }
    }

    // SAF picker for an HD-pack .zip (Mesen-style hires.txt + PNG tiles). Loads it,
    // sizes the upscaled output bitmap, and switches the UI to the Bitmap path.
    val hdpackPicker = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri != null) {
            // Read + parse + decode the pack off the main thread (it can be large);
            // the bitmap/state updates land back on the main dispatcher.
            scope.launch {
                runCatching {
                    val dims = withContext(Dispatchers.IO) {
                        val bytes = (context.contentResolver.openInputStream(uri)
                            ?: throw java.io.IOException("can't open HD-pack stream")).use { it.readBytes() }
                        emulator.controller?.loadHdpackFromZipBytes(bytes)
                        emulator.controller?.hdpackDimensions() ?: listOf(0u, 0u)
                    }
                    val w = dims[0].toInt()
                    val h = dims[1].toInt()
                    if (w > 0 && h > 0) {
                        hd.w = w
                        hd.h = h
                        hd.bitmap = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
                        hd.pixels = IntArray(w * h)
                        hdActive = true
                        status = "HD-pack loaded (${w}x$h)"
                    } else {
                        status = "HD-pack: no usable tiles"
                    }
                }.onFailure { status = "Failed to load HD-pack: ${it.message}" }
            }
        }
    }

    // SAF picker for a Lua script (.lua) — loaded into the sandboxed engine; its
    // on_frame callback then runs each frame and its print/log output is shown.
    val scriptPicker = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri != null) {
            scope.launch {
                runCatching {
                    val src = withContext(Dispatchers.IO) {
                        (context.contentResolver.openInputStream(uri)
                            ?: throw java.io.IOException("can't open script stream")).use { it.readBytes() }
                            .decodeToString()
                    }
                    withContext(Dispatchers.IO) { emulator.controller?.loadScript(src) }
                    scriptLoaded = true
                    scriptLog = ""
                    status = "Script loaded: ${displayName(context, uri)}"
                }.onFailure { status = "Failed to load script: ${it.message}" }
            }
        }
    }

    // v1.8.8 "Atlas" (Workstream C): SAF image picker to set a game's box art. The
    // picked content URI is persisted (a read grant taken) and stored on the entry.
    val boxArtPicker = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.OpenDocument(),
    ) { uri ->
        val target = boxArtTarget
        if (uri != null && target != null) {
            runCatching {
                context.contentResolver.takePersistableUriPermission(
                    uri,
                    Intent.FLAG_GRANT_READ_URI_PERMISSION,
                )
            }
            GameLibrary.setBoxArt(context, target.sha, uri.toString())
            libraryVersion++
        }
        boxArtTarget = null
        boxArtPreview = null
    }

    // v1.8.8 "Atlas" (Workstream C): batch folder import. ACTION_OPEN_DOCUMENT_TREE
    // grants a persistable read over a whole directory; we enumerate it for ROM files
    // (+ sibling box-art images) off the main thread and register each in the library.
    val treeImporter = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.OpenDocumentTree(),
    ) { treeUri ->
        if (treeUri != null) {
            runCatching {
                context.contentResolver.takePersistableUriPermission(
                    treeUri,
                    Intent.FLAG_GRANT_READ_URI_PERMISSION,
                )
            }
            scope.launch {
                importProgress = 0 to 0
                val added = withContext(Dispatchers.IO) {
                    LibraryImport.importTree(context, treeUri) { done, total ->
                        importProgress = done to total
                    }
                }
                importProgress = null
                libraryVersion++
                status = context.getString(R.string.library_imported_count, added)
            }
        }
    }

    // v1.8.8 "Atlas" (Workstream C): auto-match box art from the libretro library for
    // a game (user-triggered). Shows a preview before applying; on no match offers the
    // manual SAF picker. Network fetch + decode run off the main thread.
    fun findBoxArt(entry: GameEntry) {
        boxArtTarget = entry
        boxArtPreview = BoxArtPreview.Searching(entry.name)
        scope.launch {
            val file = withContext(Dispatchers.IO) {
                // Prefer the user's ScreenScraper / TheGamesDB credentials if set;
                // fall back to the no-account libretro-thumbnails corpus.
                ScraperSources.fetchBoxArt(context, settings, entry.sha, entry.name)
            }
            boxArtPreview = if (file != null) {
                BoxArtPreview.Found(android.net.Uri.fromFile(file).toString(), entry.name)
            } else {
                BoxArtPreview.NotFound(entry.name)
            }
        }
    }

    // Identify the loaded ROM to RetroAchievements: compute its SHA-256, read any
    // saved progress sidecar, and call raLoadGame off-thread. A no-op unless RA is
    // enabled, a ROM is loaded, and the session is logged in. Marks raGameLoaded so
    // a login that completes after the ROM was opened can re-issue this once.
    fun raIdentifyGame() {
        val ctrl = emulator.controller ?: return
        val bytes = emulator.romBytes ?: return
        if (!settings.raEnabled) return
        scope.launch {
            runCatching {
                withContext(Dispatchers.IO) {
                    if (!ctrl.raIsEnabled() || ctrl.raLoginStatus() != RaLoginStatus.LOGGED_IN) {
                        return@withContext
                    }
                    val sha = MessageDigest.getInstance("SHA-256").digest(bytes)
                    val shaHex = sha.joinToString("") { "%02x".format(it) }
                    val sidecar = RaProgressStore.load(context, shaHex)
                    ctrl.raLoadGame(bytes, sha, sidecar)
                }
                raGameLoaded = true
            }.onFailure { status = "RA load failed: ${it.message}" }
        }
    }

    // Persist the current RA progress sidecar (if any) for the loaded ROM.
    fun raSaveProgress() {
        val ctrl = emulator.controller ?: return
        val sha = emulator.romSha ?: return
        if (!settings.raEnabled) return
        runCatching {
            val blob = ctrl.raSerializeProgress()
            if (blob.isNotEmpty()) RaProgressStore.save(context, sha, blob)
        }
    }

    // Netplay (v1.8.6): begin hosting. The socket bind is network I/O, so run it off
    // the main thread; on success surface the OS-bound port + this device's LAN IP as
    // "IP:port" for the joiner to dial. A ROM must be loaded (the ROM hash gates the
    // handshake) — the bridge errors otherwise.
    fun netplayHost(localPort: UShort, numPlayers: UByte) {
        val ctrl = emulator.controller
        if (ctrl == null) {
            status = "Open a ROM first, then host"
            return
        }
        scope.launch {
            runCatching {
                val bound = withContext(Dispatchers.IO) { ctrl.npHost(localPort, numPlayers) }
                val ip = localWifiIpv4(context) ?: "<this device's IP>"
                npHostInfo = "$ip:$bound"
                status = "Hosting on $ip:$bound — waiting for a player"
                host?.playGames?.unlock(PgsIds.ACH_FIRST_NETPLAY)
            }.onFailure { status = "Host failed: ${it.message}" }
        }
    }

    // Netplay (v1.8.6): join a host at "ip:port" (parse + bind + connect = network
    // I/O, so off the main thread). A ROM must be loaded so the ROM-hash check passes.
    fun netplayJoin(address: String) {
        val ctrl = emulator.controller
        if (ctrl == null) {
            status = "Open the same ROM first, then join"
            return
        }
        npHostInfo = null
        scope.launch {
            runCatching {
                withContext(Dispatchers.IO) { ctrl.npJoin(address) }
                status = "Joining $address…"
                host?.playGames?.unlock(PgsIds.ACH_FIRST_NETPLAY)
            }.onFailure { status = "Join failed: ${it.message}" }
        }
    }

    // Netplay (v1.8.7): host an online (room-code) session. Registers with the
    // signaling relay + STUN/NAT traversal (all network I/O, so off the main thread);
    // on success the bridge returns a 6-char room code to share. A ROM must be loaded
    // (the ROM hash gates the handshake). The NpNetConfig endpoints come from Settings
    // (defaulting to the placeholder relay), so an unconfigured relay fails here fast.
    fun netplayHostRoom() {
        val ctrl = emulator.controller
        if (ctrl == null) {
            status = "Open a ROM first, then host"
            return
        }
        npHostInfo = null
        npRoomCode = null
        scope.launch {
            runCatching {
                val cfg = netplayConfig(settings)
                val code = withContext(Dispatchers.IO) { ctrl.npHostRoom(2u, cfg) }
                npRoomCode = code
                status = "Hosting online — room code $code"
                host?.playGames?.unlock(PgsIds.ACH_FIRST_NETPLAY)
            }.onFailure { status = "Host failed: ${it.message}" }
        }
    }

    // Netplay (v1.8.7): join an online session by its 6-char room code. NAT traversal
    // = network I/O, so off the main thread. A ROM must be loaded so the ROM-hash check
    // passes; the code is persisted so the Join-online field prefills it next time.
    fun netplayJoinRoom(code: String) {
        val ctrl = emulator.controller
        if (ctrl == null) {
            status = "Open the same ROM first, then join"
            return
        }
        npHostInfo = null
        npRoomCode = null
        scope.launch {
            runCatching {
                val cfg = netplayConfig(settings)
                withContext(Dispatchers.IO) { ctrl.npJoinRoom(code, cfg) }
                status = "Joining room $code…"
                host?.playGames?.unlock(PgsIds.ACH_FIRST_NETPLAY)
            }.onFailure { status = "Join failed: ${it.message}" }
        }
    }

    // Share the room code via the system ACTION_SEND chooser (v1.8.7).
    fun shareRoomCode(code: String) {
        runCatching {
            val send = android.content.Intent(android.content.Intent.ACTION_SEND).apply {
                type = "text/plain"
                putExtra(
                    android.content.Intent.EXTRA_TEXT,
                    "Join my RustyNES game — room code: $code",
                )
            }
            context.startActivity(
                android.content.Intent.createChooser(send, "Share room code"),
            )
        }.onFailure { status = "Couldn't share: ${it.message}" }
    }

    // Netplay (v1.8.6): tear the session down and return to single-player.
    fun netplayLeave() {
        scope.launch {
            withContext(Dispatchers.IO) { runCatching { emulator.controller?.npLeave() } }
            npHostInfo = null
            npRoomCode = null
            npStatus = null
            npActive = false
            status = "Left netplay"
        }
    }

    // Open a recent ROM via its persistable content URI.
    fun openRecent(rom: RecentRom) {
        runCatching {
            val uri = Uri.parse(rom.uri)
            val bytes = (context.contentResolver.openInputStream(uri)
                ?: throw java.io.IOException("can't open recent ROM stream")).use { it.readBytes() }
            status = loadRom(context, emulator, bytes, uri, rom.name, unlocked, settings)
            recents = RomLibrary.recents(context)
            libraryVersion++
        }.onFailure { status = "Can't open ${rom.name}: ${it.message}" }
    }

    // v1.8.8 "Atlas" (Workstream C): open a library game by its entry. Loads from the
    // persistable SAF URI; an entry with no URI (a migrated recent that was never
    // re-opened, or a debug autoload) can't be loaded that way and reports as much.
    fun playGame(entry: GameEntry) {
        if (entry.uri.isEmpty()) {
            status = "No file for ${entry.name} — open it once to link it"
            return
        }
        runCatching {
            val uri = Uri.parse(entry.uri)
            val bytes = (context.contentResolver.openInputStream(uri)
                ?: throw java.io.IOException("can't open ROM stream")).use { it.readBytes() }
            status = loadRom(context, emulator, bytes, uri, entry.name, unlocked, settings)
            recents = RomLibrary.recents(context)
            libraryVersion++
        }.onFailure { status = "Can't open ${entry.name}: ${it.message}" }
    }

    // v1.8.8 "Atlas" (Workstream F): capture a screenshot of the current gameplay
    // frame to Pictures/RustyNES (MediaStore) and offer a share. The save (PNG
    // encode + ContentResolver I/O) runs off the main thread; a brief status/toast
    // reports the result. Gameplay-only — the captured bitmap carries no UI chrome.
    fun takeScreenshot() {
        if (!Capture.supported) {
            status = context.getString(R.string.capture_unsupported)
            return
        }
        val src = capture.latestFrame
        if (src == null) {
            status = context.getString(R.string.capture_screenshot_failed)
            return
        }
        // Snapshot the frame on the main thread (the loop reuses it in place), then
        // encode + save off-thread.
        val snapshot = runCatching { src.copy(Bitmap.Config.ARGB_8888, false) }.getOrNull()
        if (snapshot == null) {
            status = context.getString(R.string.capture_screenshot_failed)
            return
        }
        scope.launch {
            val uri = withContext(Dispatchers.IO) { Capture.saveScreenshot(context, snapshot) }
            runCatching { snapshot.recycle() }
            if (uri != null) {
                status = context.getString(R.string.capture_screenshot_saved)
                android.widget.Toast.makeText(
                    context,
                    context.getString(R.string.capture_screenshot_saved),
                    android.widget.Toast.LENGTH_SHORT,
                ).show()
                Capture.share(context, uri, image = true)
            } else {
                status = context.getString(R.string.capture_screenshot_failed)
            }
        }
    }

    // v1.8.8 "Atlas" (Workstream F): toggle gameplay-clip recording. Start arms a
    // rolling ring buffer (the loop offers frames to it); Stop drains it, encodes the
    // MP4 off-thread to Movies/RustyNES, and offers a share. Video-only for now (the
    // audio mux is a documented TODO in Capture.kt).
    fun toggleRecording() {
        if (!Capture.supported) {
            status = context.getString(R.string.capture_unsupported)
            return
        }
        val existing = capture.clip
        if (existing == null) {
            // Start: size the ring from the active picture (HD-pack vs base NES).
            val w = if (hdActive && hd.w > 0) hd.w else NES_WIDTH
            val h = if (hdActive && hd.h > 0) hd.h else NES_HEIGHT
            capture.clip = Capture.ClipBuffer(w, h)
            recording = true
            status = context.getString(R.string.capture_recording)
        } else {
            // Stop: detach the ring + encode it.
            capture.clip = null
            recording = false
            val w = existing.width
            val h = existing.height
            scope.launch {
                val frames = existing.drain()
                val uri = withContext(Dispatchers.IO) { Capture.encodeClip(context, frames, w, h) }
                if (uri != null) {
                    status = context.getString(R.string.capture_clip_saved)
                    android.widget.Toast.makeText(
                        context,
                        context.getString(R.string.capture_clip_saved),
                        android.widget.Toast.LENGTH_SHORT,
                    ).show()
                    Capture.share(context, uri, image = false)
                } else {
                    status = context.getString(R.string.capture_clip_failed)
                }
            }
        }
    }

    // v1.8.8 "Atlas" (Workstream H): keep the host's PiP gate in sync with whether a
    // ROM is actually running, so onUserLeaveHint only auto-enters PiP during play.
    LaunchedEffect(romLoaded) {
        host?.romRunningForPip = romLoaded
        // v1.8.8 "Atlas" (Workstream E): "first ROM loaded" PGS achievement. No-op
        // behind the default-off PGS_ENABLED flag / when not signed in; unlock is
        // idempotent server-side. DISTINCT from RetroAchievements (per-game) above.
        if (romLoaded) host?.playGames?.unlock(PgsIds.ACH_FIRST_ROM)
        // v1.8.8 "Atlas" (Workstream D): on first open of a ROM, pull any cloud copy of
        // its auto save-slot down so a save made on another device resumes here. No-op
        // unless cloud saves are active (flag + signed-in + toggle); local saves stay
        // authoritative otherwise. Runs off the main thread (Snapshot I/O is blocking).
        val sha = emulator.romSha
        if (romLoaded && sha != null && host?.cloudSave?.isActive(settings) == true) {
            // The Snapshots Tasks dispatch their own callbacks; just kick it off. The
            // pull writes into the local `.rns` (so a subsequent open auto-resumes it).
            host.cloudSave.pullSlot(
                sha,
                SaveStateStore.AUTO_SLOT,
                settings,
                onConflict = { c -> cloudConflict = c },
                onDone = { if (it) host.playGames.unlock(PgsIds.ACH_FIRST_CLOUD_SYNC) },
            )
        }
    }

    // v1.8.8 "Atlas" (Workstream H): consume a deep-link action from a Quick Settings
    // tile / app shortcut / widget launch. "resume" opens the last-played library
    // game; "open" raises the SAF picker; "library" shows the idle/library screen.
    // The action is cleared once handled so a recomposition/config-change doesn't
    // re-fire it.
    LaunchedEffect(host?.deepLinkState?.value) {
        val action = host?.deepLinkState?.value ?: return@LaunchedEffect
        when (action) {
            DeepLink.ACTION_RESUME -> DeepLink.lastPlayed(context)?.let { playGame(it) }
            DeepLink.ACTION_OPEN -> picker.launch(arrayOf("*/*"))
            DeepLink.ACTION_LIBRARY -> { /* idle screen already shows the library */ }
        }
        host.deepLinkState.value = null
    }

    // Demo countdown: tick once per second while a ROM is running, unpaused, and
    // not yet unlocked; on expiry, pause the emulator and raise the unlock sheet.
    // Purchasing (unlocked -> true) cancels the limit immediately.
    LaunchedEffect(unlocked) {
        if (unlocked) {
            demoExpired = false
            return@LaunchedEffect
        }
        while (true) {
            kotlinx.coroutines.delay(1000)
            if (emulator.controller != null && !emulator.paused && !demoExpired) {
                demoSecondsLeft -= 1
                if (demoSecondsLeft <= 0) {
                    demoExpired = true
                    emulator.paused = true
                }
            }
        }
    }

    // RetroAchievements auto-login (v1.8.6): on first composition, if RA is enabled
    // and a token was saved from a prior password login, init the session and
    // token-login silently (fire-and-forget; status/toasts are polled in the loop).
    LaunchedEffect(Unit) {
        if (settings.raEnabled && settings.raToken.isNotEmpty() && settings.raUsername.isNotEmpty()) {
            withContext(Dispatchers.IO) {
                emulator.controller // touch (no-op); RA session is controller-scoped
                runCatching {
                    // The session is created lazily on the first ra_* call; init it
                    // and token-login — but only when a controller already exists
                    // (the RA session is controller-scoped). If no ROM is open yet,
                    // there is no controller, so login happens when one is loaded.
                    val ctrl = emulator.controller
                    if (ctrl != null) {
                        ctrl.raInit(settings.raHardcore)
                        ctrl.raLoginToken(settings.raUsername, settings.raToken)
                    }
                }
            }
        }
    }

    // (Re-)identify the loaded game to RA whenever the ROM changes (keyed by SHA).
    // A no-op until logged in; the login-edge handler in the loop re-issues it then.
    LaunchedEffect(emulator.romSha) {
        raGameLoaded = false
        raIdentifyGame()
    }

    // Debug-only convenience: auto-load a ROM pushed to the app's external files
    // dir (`/sdcard/Android/data/<pkg>/files/autoload.nes`) so the render path
    // can be verified on-device without driving the SAF picker by hand. Never
    // shipped (BuildConfig.DEBUG-gated); release boots straight to the picker.
    LaunchedEffect(Unit) {
        if (BuildConfig.DEBUG && emulator.controller == null) {
            val auto = java.io.File(context.getExternalFilesDir(null), "autoload.nes")
            if (auto.exists()) {
                runCatching {
                    status = loadRom(context, emulator, auto.readBytes(), null, "autoload", unlocked, settings)
                }.onFailure { status = "Autoload failed: ${it.message}" }
            }
        }
    }

    // v1.8.8 "Atlas" (Workstream K): predictive back. Under targetSdk 36 on Android
    // 16, Activity.onBackPressed is no longer called, so every dismissable surface is
    // closed through OnBackPressedDispatcher via Compose BackHandler. The Compose
    // Dialog / ModalBottomSheet surfaces (Settings/States/About/Controllers/Netplay/
    // Onboarding) wire their own back-to-dismiss internally; these handlers cover the
    // custom (non-dialog) overlays the in-app menu opens. Ordering: Compose dispatches
    // to the most-recently-composed enabled handler first, so the menu bar closes
    // before any "exit app" fallthrough. We intentionally do NOT consume back when
    // nothing is open, so the system performs the default (predictive) exit.
    BackHandler(enabled = controlsVisible) {
        controlsVisible = false
        menuWantsFocus = false
        focusManager.clearFocus()
    }

    // v1.8.8 "Atlas" (Workstream A): expanded-width adaptive two-pane. On a tablet,
    // the Z Fold inner display, or a resized desktop/ChromeOS window (width >= 840 dp)
    // a persistent library rail is shown beside the player so the extra horizontal
    // space is used — tap a recent ROM in the rail to load it without opening a menu.
    // Compact/medium widths keep the single-pane phone layout. This is a solid first
    // version of the list-detail pane; TODO(v1.8.8 WS C): grow the rail into the full
    // box-art library grid (favorites / folders / search) via NavigableListDetailPaneScaffold.
    // v1.8.8 "Atlas" (Workstream C): the shared box-art library content, used as the
    // expanded-width list pane (below) and the compact idle screen (when no ROM is
    // loaded). The same handlers drive both so a phone and a tablet behave identically.
    val libraryContent: @Composable (Modifier) -> Unit = { mod ->
        LibraryScreen(
            entries = libraryEntries,
            folders = libraryFolders,
            selectedFolder = libFolder,
            favoritesOnly = libFavoritesOnly,
            query = libQuery,
            sort = librarySort,
            onOpen = { picker.launch(arrayOf("*/*")) },
            onImportFolder = { treeImporter.launch(null) },
            onSelectFolder = { folder, favs -> libFolder = folder; libFavoritesOnly = favs },
            onQueryChange = { libQuery = it },
            onSortChange = { librarySort = it },
            onPlay = { playGame(it) },
            onToggleFavorite = {
                GameLibrary.setFavorite(context, it.sha, !it.favorite)
                libraryVersion++
            },
            onSetBoxArt = { findBoxArt(it) },
            onMoveToFolder = { folderTarget = it },
            onRemove = {
                GameLibrary.remove(context, it.sha)
                libraryVersion++
            },
            modifier = mod,
        )
    }

    Row(modifier = Modifier.fillMaxSize()) {
        if (isExpandedWidth) {
            libraryContent(
                Modifier
                    .fillMaxHeight()
                    .width(340.dp)
                    .windowInsetsPadding(WindowInsets.safeDrawing),
            )
        }
    Column(
        modifier = Modifier
            .fillMaxHeight()
            .weight(1f),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        // The NES image takes the remaining vertical space and letterboxes
        // (ContentScale.Fit) — driving height from width would overflow on a
        // wide/foldable display and push the controls off-screen.
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .weight(1f)
                .background(Color.Black)
                // v1.8.8 "Atlas" (Workstream H): publish the gameplay-image bounds in
                // window coordinates so enter-PiP can crop the shrink animation from
                // the picture (sourceRectHint), not the whole window.
                .onGloballyPositioned { coords ->
                    val b = coords.boundsInWindow()
                    host?.setGameplayBounds(
                        android.graphics.Rect(
                            b.left.toInt(), b.top.toInt(), b.right.toInt(), b.bottom.toInt(),
                        ),
                    )
                },
            contentAlignment = Alignment.Center,
        ) {
            val current = frame
            // GPU SurfaceView render path (opt-in, v1.8.4). Mounted continuously and
            // draws each submitted frame on the GPU (black until the first frame).
            // GPU path is bypassed while an HD-pack is active (its output is upscaled,
            // but the GPU texture is fixed 256x240 — HD goes through the Bitmap path).
            // During netplay the frame is presented via the Bitmap path (the loop reads
            // the non-advancing index framebuffer into ARGB), so the GPU SurfaceView is
            // bypassed too — same as the HD-pack path.
            if (gpuSurface != null && !hdActive && !npActive && romLoaded) {
                androidx.compose.ui.viewinterop.AndroidView(
                    factory = { gpuSurface },
                    modifier = Modifier.fillMaxSize(),
                )
            }
            if ((gpuSurface == null || hdActive || npActive) && current != null) {
                // Crisp 8:7 PAR on the Bitmap path: the source bitmap is 256x240
                // (1:1), so constrain the Image to the NES 8:7 display aspect and
                // centre it — height-bounded + letterboxed on a wide (unfolded) inner
                // screen, not stretched. The GPU SurfaceView path already applies this
                // PAR letterbox internally (gfx.rs), so it is NOT wrapped here (no
                // double-apply). HD-pack frames carry their own aspect (the upscaled
                // bitmap), so they keep fillMaxSize + ContentScale.Fit instead.
                val nesAspect = if (hdActive) {
                    Modifier.fillMaxSize()
                } else {
                    Modifier
                        .aspectRatio(8f / 7f, matchHeightConstraintsFirst = true)
                        .align(Alignment.Center)
                }
                Image(
                    bitmap = current,
                    contentDescription = stringResource(R.string.cd_nes_screen),
                    modifier = nesAspect
                        .onSizeChanged { screenSize = it }
                        .then(
                            if (videoFiltersSupported && settings.filter != VideoFilter.None && screenSize.width > 0) {
                                Modifier.graphicsLayer {
                                    renderEffect = buildRenderEffect(
                                        settings.filter,
                                        screenSize.width.toFloat(),
                                        screenSize.height.toFloat(),
                                    )
                                }
                            } else {
                                Modifier
                            },
                        ),
                    contentScale = ContentScale.Fit,
                    // Nearest-neighbour: preserve the crisp pixel grid.
                    filterQuality = FilterQuality.None,
                )
            }
            // RetroAchievements toast HUD (v1.8.6) — unlock + login/server messages.
            // Text-only cards (no badge images); error toasts tint red. They clear
            // when the next poll returns an empty queue.
            if (raToasts.isNotEmpty()) {
                Column(
                    modifier = Modifier
                        .align(Alignment.TopEnd)
                        .padding(8.dp),
                    horizontalAlignment = Alignment.End,
                    verticalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    raToasts.forEach { toast ->
                        Column(
                            modifier = Modifier
                                .background(if (toast.isError) Color(0xD0701010) else Color(0xC0102030))
                                .padding(horizontal = 8.dp, vertical = 6.dp),
                        ) {
                            Text(
                                toast.title,
                                color = if (toast.isError) Color(0xFFFFCDD2) else Color.White,
                                fontSize = 12.sp,
                                fontWeight = androidx.compose.ui.text.font.FontWeight.Bold,
                            )
                            if (toast.detail.isNotEmpty()) {
                                Text(toast.detail, color = Color(0xFFCFD8DC), fontSize = 10.sp)
                            }
                        }
                    }
                }
            }
            // Netplay status overlay (v1.8.6) — a compact "connecting / synced f=… "
            // line while a LAN session is active. Reuses the RA/Lua overlay pattern.
            npStatus?.takeIf { npActive && it.phase != uniffi.rustynes_mobile.NpPhase.IDLE }?.let { s ->
                val line = when (s.phase) {
                    uniffi.rustynes_mobile.NpPhase.NEGOTIATING ->
                        "Netplay: ${s.detail.ifEmpty { "connecting" }}…"
                    uniffi.rustynes_mobile.NpPhase.CONNECTING -> "Netplay: connecting…"
                    uniffi.rustynes_mobile.NpPhase.IN_GAME ->
                        "Netplay: synced f=${s.currentFrame}" +
                            (s.pingMs?.let { " ping=${it}ms" } ?: "") +
                            (if (s.stalled) " (stall)" else "")
                    uniffi.rustynes_mobile.NpPhase.ERROR ->
                        "Netplay: ${s.message.ifEmpty { "disconnected" }}"
                    uniffi.rustynes_mobile.NpPhase.IDLE -> ""
                }
                Text(
                    line,
                    color = if (s.phase == uniffi.rustynes_mobile.NpPhase.ERROR || s.desync) {
                        Color(0xFFEF9A9A)
                    } else {
                        Color(0xFF80D8FF)
                    },
                    fontSize = 11.sp,
                    modifier = Modifier
                        .align(Alignment.BottomStart)
                        .padding(8.dp)
                        .background(Color(0xC0000000))
                        .padding(horizontal = 6.dp, vertical = 4.dp),
                )
            }
            // Lua script log overlay (v1.8.6) — the script's print/log output.
            if (scriptLoaded && scriptLog.isNotEmpty()) {
                Text(
                    scriptLog,
                    color = Color(0xFF7CFC00),
                    fontSize = 10.sp,
                    modifier = Modifier
                        .align(Alignment.TopStart)
                        .padding(8.dp)
                        .background(Color(0xC0000000))
                        .padding(horizontal = 6.dp, vertical = 4.dp),
                )
            }
            if (current == null) {
                // Idle screen. On a compact / medium-width window (phone, folded cover
                // screen) show the full box-art library grid here (v1.8.8 WS C). On an
                // expanded-width window the library already lives in the persistent side
                // pane, so the player area just shows the status line. Inset-padded so
                // nothing tucks under the system bars (visible on the idle screen).
                if (isExpandedWidth) {
                    // TODO(i18n): `status` is a dynamic, frequently-reassigned string
                    // (load results, error messages) — a deferred i18n surface.
                    Text(
                        status,
                        color = Color.White,
                        modifier = Modifier.safeDrawingPadding().padding(16.dp),
                    )
                } else {
                    libraryContent(
                        Modifier
                            .fillMaxSize()
                            .safeDrawingPadding(),
                    )
                }
            }
            // Folder batch-import progress banner (v1.8.8 WS C).
            importProgress?.let { (done, total) ->
                ImportProgressBanner(
                    done,
                    total,
                    modifier = Modifier.align(Alignment.BottomCenter).padding(12.dp),
                )
            }
            // While casting, a small banner over the (still-mirrored) phone picture.
            if (castManager.casting) {
                Text(
                    stringResource(R.string.casting_to, castManager.displayName ?: "TV"),
                    color = Color(0xFF80D8FF),
                    modifier = Modifier.align(Alignment.TopCenter).padding(8.dp),
                )
            }
        }

        // Control bar: open / states / reset / pause / fast-forward / settings.
        // Hidden until the controller's "RustyNES" pill is first tapped (it
        // toggles thereafter). Horizontally scrollable so all controls reach on a
        // narrow cover screen.
        var paused by remember { mutableStateOf(false) }
        var turbo by remember { mutableStateOf(false) }
        // `controlsVisible` is hoisted to EmulatorScreen scope (v1.8.7, #41) so the
        // pad's Guide/chord can open the same menu the touch "MENU pill" toggles.
        // Close the current ROM: persist RA progress, unload, and return to the idle
        // screen (Open button + recent-ROMs list). Mirrors the desktop close_rom.
        fun closeRom() {
            raSaveProgress()
            val ctrl = emulator.controller
            if (settings.raEnabled) runCatching { ctrl?.raUnloadGame() }
            emulator.controller = null
            emulator.romSha = null
            emulator.romBytes = null
            raGameLoaded = false
            frame = null
            romLoaded = false
            paused = false
            emulator.paused = false
            // v1.8.8 "Atlas" (Workstream F): discard any in-progress clip + the last
            // captured frame so a closed ROM leaves nothing to capture/encode.
            capture.clip?.clear()
            capture.clip = null
            capture.latestFrame = null
            recording = false
            status = "No ROM loaded"
            // v1.8.8 "Atlas" (Workstream L): closing a ROM is a natural "finished a
            // satisfying session" moment — request the in-app review flow. The Play API
            // enforces its own quota (so this no-ops most of the time, with no CTA) and
            // no-ops entirely on a sideloaded / non-Play install.
            host?.requestInAppReview()
        }
        // v1.8.8 "Atlas" (Workstream H): in the small PiP window, show only the
        // gameplay picture — the control bar is hidden (and the on-screen pad below).
        if (controlsVisible && !inPip) {
            Row(
            // focusGroup so the d-pad can move between entries when the menu is
            // opened via the pad's Guide button / Start+Select chord (v1.8.7, #41).
            // The Buttons are focusable by default; the first carries a FocusRequester
            // that a LaunchedEffect requests so focus lands here on a pad-open.
            modifier = Modifier
                .fillMaxWidth()
                // v1.8.8 "Atlas" (Workstream K): edge-to-edge is enforced at SDK 35+,
                // so pad the control bar in from the system bars / display cutout — the
                // gameplay Box above stays full-bleed, but the controls never sit under
                // a transient status/nav bar, a cutout, or the large-screen taskbar.
                .windowInsetsPadding(WindowInsets.safeDrawing)
                .horizontalScroll(rememberScrollState())
                .focusGroup()
                .padding(horizontal = 8.dp, vertical = 4.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            // Open a ROM, or Close the running one (toggles on whether one is loaded).
            if (romLoaded) {
                Button(
                    onClick = { closeRom() },
                    modifier = Modifier.focusRequester(menuFocusRequester),
                ) { Text(stringResource(R.string.action_close)) }
            } else {
                Button(
                    onClick = { picker.launch(arrayOf("*/*")) },
                    modifier = Modifier.focusRequester(menuFocusRequester),
                ) { Text(stringResource(R.string.action_open)) }
            }
            // Save-states are a paid feature; the demo hides the manager.
            if (unlocked) {
                OutlinedButton(onClick = { showStates = true }) { Text(stringResource(R.string.action_states)) }
            }
            OutlinedButton(onClick = { emulator.controller?.reset() }) { Text(stringResource(R.string.action_reset)) }
            OutlinedButton(onClick = {
                paused = !paused
                emulator.paused = paused
            }) { Text(stringResource(if (paused) R.string.action_resume else R.string.action_pause)) }
            // Fast-forward — disabled during netplay (rollback owns pacing). The ">>"
            // glyph is a universal fast-forward symbol, not a translatable word.
            OutlinedButton(
                onClick = {
                    turbo = !turbo
                    emulator.turbo = turbo
                },
                enabled = !npActive,
            ) { Text(if (turbo) ">> On" else ">>") }
            // v1.8.8 "Atlas" (Workstream F): screenshot + gameplay-clip capture.
            // Both gated to a running ROM and to API 29+ (scoped MediaStore). The
            // screenshot grabs the current frame; Record toggles a rolling MP4 clip.
            if (romLoaded && Capture.supported) {
                OutlinedButton(onClick = { takeScreenshot() }) {
                    Text(stringResource(R.string.action_screenshot))
                }
                OutlinedButton(onClick = { toggleRecording() }) {
                    Text(
                        stringResource(
                            if (recording) R.string.action_stop_record else R.string.action_record,
                        ),
                    )
                }
            }
            // v1.8.8 "Atlas" (Workstream H): enter Picture-in-Picture — gameplay keeps
            // running in a floating window. API 26+; shown only with a ROM loaded.
            if (romLoaded && Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                OutlinedButton(onClick = {
                    controlsVisible = false
                    host?.enterPip()
                }) { Text(stringResource(R.string.action_pip)) }
            }
            OutlinedButton(onClick = { showSettings = true }) { Text(stringResource(R.string.action_settings)) }
            // Hardware controllers (v1.8.7): port assignment, remapping, autofire.
            OutlinedButton(onClick = { showControllers = true }) { Text(stringResource(R.string.action_controllers)) }
            // Direct-IP / LAN netplay (v1.8.6). Ungated (not a paid feature here).
            OutlinedButton(onClick = { showNetplay = true }) {
                Text(stringResource(if (npActive) R.string.action_netplay_active else R.string.action_netplay))
            }
            OutlinedButton(onClick = { showAbout = true }) { Text(stringResource(R.string.action_about)) }
            // Cast gameplay to a connected TV/external display (only when present).
            if (castManager.available) {
                OutlinedButton(onClick = { castManager.toggle() }) {
                    Text(stringResource(if (castManager.casting) R.string.action_stop_cast else R.string.action_cast_to_tv))
                }
            }
            // Experimental Chromecast (CAF) spectator mirror (v1.8.7, #38). Only
            // compiled in / shown when the default-off flag is set. The standard CAF
            // MediaRouteButton handles device discovery + the chooser/controller
            // dialog; once a session connects, the emulation loop streams ~20-30fps
            // frames to the custom Web Receiver. Label clarifies it's a spectator view.
            if (BuildConfig.CHROMECAST_ENABLED) {
                Text(
                    if (chromecast.isCasting) "Casting…" else "Cast to TV (spectator ~20-30fps):",
                    color = Color.Gray,
                )
                androidx.compose.ui.viewinterop.AndroidView(
                    factory = { ctx ->
                        androidx.mediarouter.app.MediaRouteButton(ctx).also { btn ->
                            com.google.android.gms.cast.framework.CastButtonFactory
                                .setUpMediaRouteButton(ctx.applicationContext, btn)
                        }
                    },
                )
            }
            // Demo: an always-visible unlock affordance + the session countdown.
            if (!unlocked) {
                val price = license.product
                    ?.oneTimePurchaseOfferDetails?.formattedPrice ?: "$2.99"
                Button(onClick = { activity?.let { license.purchase(it) } }) {
                    Text(stringResource(R.string.action_unlock, price))
                }
                val mins = demoSecondsLeft / 60
                val secs = demoSecondsLeft % 60
                Text(
                    stringResource(R.string.demo_remaining, mins, secs),
                    color = Color.Gray,
                )
            }
            // Debug-only (and only meaningful when the freemium is active, i.e. a
            // PLAY_BUILD debug build): simulate the Full Unlock without Play.
            if (BuildConfig.DEBUG && BuildConfig.PLAY_BUILD) {
                OutlinedButton(onClick = { license.debugForceUnlocked(!unlocked) }) {
                    Text(if (unlocked) "DBG:demo" else "DBG:unlock")
                }
            }
            }
        } // end control bar (toggled by the RustyNES pill)

        // The multi-touch virtual NES controller, sized to the NES-001 aspect
        // (123:53, the real NES-004 proportions). Its width is `controllerScale`
        // (0.25–1.1×) of the available width, centered, so it rescales for the
        // active display AND the user's preference; >1 overruns the screen edges
        // by design. Its hit regions remap in lockstep (art + regions both derive
        // from the measured size). The host Box reserves the LARGEST (110%)
        // controller height, so changing the size never reflows/shifts the
        // gameplay view above it (item 7).
        //
        // Controller-aware UI (v1.8.7, #41): when a hardware pad is connected (and
        // auto-hide is on) the whole reserved block collapses, so its height frees up
        // and the gameplay Box above (which has weight(1f)) grows to fill it — the
        // game maximizes, especially on the unfolded inner screen. The pad's
        // Guide/chord still reaches the menu, and the touch "MENU pill" path is
        // unaffected when the controller is shown. Disconnect restores it declaratively.
        if (!controlsHidden && !inPip) {
            BoxWithConstraints(
                // v1.8.8 "Atlas" (Workstream K): keep the on-screen pad's bottom row
                // clear of the gesture-nav bar / cutout under edge-to-edge. Only the
                // safeDrawing insets are consumed (left/right/bottom) so the pad sits
                // fully in the safe area; the gameplay Box above remains full-bleed.
                modifier = Modifier
                    .fillMaxWidth()
                    .windowInsetsPadding(WindowInsets.safeDrawing),
            ) {
                val mw = maxWidth // capture: not visible inside the inner BoxScope
                val reserved = mw * 1.1f * 53f / 123f
                Box(
                    modifier = Modifier.fillMaxWidth().height(reserved),
                    contentAlignment = Alignment.Center,
                ) {
                    VirtualController(
                        emulator,
                        settings.hapticLevel,
                        { controlsVisible = !controlsVisible },
                        Modifier
                            .width(mw * settings.controllerScale(screenMode))
                            .aspectRatio(123f / 53f)
                            .alpha(settings.controllerOpacity(screenMode)),
                    )
                }
            }
        }
    } // end emulator Column
    } // end adaptive Row (the expanded-width library rail + the player column)

    // Settings + save-state manager sheets (v1.8.3).
    if (showSettings) {
        SettingsSheet(
            settings,
            screenMode,
            onLoadPalette = { palettePicker.launch(arrayOf("*/*")) },
            onClearPalette = {
                emulator.controller?.clearPalette()
                status = "Palette reset to default"
            },
            onMovieRecord = {
                emulator.controller?.movieRecordFromPowerOn()
                status = "Recording movie from power-on"
            },
            onMoviePlay = { moviePicker.launch(arrayOf("*/*")) },
            onMovieSave = { movieSaver.launch("movie.rnm") },
            onMovieStop = {
                emulator.controller?.movieStop()
                status = "Movie stopped"
            },
            onLoadHdpack = { hdpackPicker.launch(arrayOf("*/*")) },
            onUnloadHdpack = {
                emulator.controller?.unloadHdpack()
                hdActive = false
                hd.bitmap = null
                status = "HD-pack unloaded"
            },
            onLoadScript = { scriptPicker.launch(arrayOf("*/*")) },
            onUnloadScript = {
                emulator.controller?.unloadScript()
                scriptLoaded = false
                scriptLog = ""
                status = "Script unloaded"
            },
            raStatus = raStatus,
            raUser = raUserName,
            onRaLogin = { user, pass ->
                // Off-thread, like the SAF loads: init the session at the current
                // hardcore setting and fire the async password login. Status/toasts
                // are polled in the emulation loop; on the LOGGED_IN edge the token
                // is persisted and the loaded game is identified.
                settings.raUsername = user
                scope.launch {
                    withContext(Dispatchers.IO) {
                        runCatching {
                            val ctrl = emulator.controller
                            if (ctrl != null) {
                                ctrl.raInit(settings.raHardcore)
                                ctrl.raLoginPassword(user, pass)
                                raStatus = "Logging in…"
                            } else {
                                raStatus = "Open a ROM first, then log in"
                            }
                        }
                    }
                }
            },
            onRaLogout = {
                scope.launch {
                    withContext(Dispatchers.IO) { runCatching { emulator.controller?.raLogout() } }
                    settings.raToken = ""
                    raUserName = null
                    raStatus = "Logged out"
                }
            },
            raEnabled = settings.raEnabled,
            onRaEnabledChange = { on ->
                settings.raEnabled = on
                if (on) {
                    scope.launch {
                        withContext(Dispatchers.IO) {
                            runCatching { emulator.controller?.raInit(settings.raHardcore) }
                        }
                    }
                }
            },
            raHardcore = settings.raHardcore,
            onRaHardcoreChange = { hc ->
                settings.raHardcore = hc
                scope.launch {
                    withContext(Dispatchers.IO) {
                        runCatching { emulator.controller?.raSetHardcore(hc) }
                    }
                }
            },
            // Play Games cloud saves (Workstreams D+E) — DISTINCT from RA above. Only
            // surfaced when the Play Games build is active (BuildConfig.PGS_ENABLED).
            pgsSignedIn = host?.playGames?.isSignedIn == true,
            cloudSavesEnabled = settings.cloudSavesEnabled,
            onCloudSavesChange = { settings.cloudSavesEnabled = it },
            onPgsSignIn = { host?.playGames?.ensureSignedIn() },
            // v1.8.8 "Atlas" (Workstream B): apply the picked UI language immediately.
            // setApplicationLocales recreates the Activity so the new locale's resources
            // load; AppCompat persists the choice (and our AppSettings mirror is the
            // source of truth, re-asserted in onCreate). System (empty tag) clears the
            // override and follows the per-app / system language.
            onLanguageChange = { lang ->
                val locales = if (lang.tag.isEmpty()) {
                    LocaleListCompat.getEmptyLocaleList()
                } else {
                    LocaleListCompat.forLanguageTags(lang.tag)
                }
                AppCompatDelegate.setApplicationLocales(locales)
            },
            onDismiss = { showSettings = false },
        )
    }
    if (showStates) {
        StatesSheet(
            context, emulator.romSha, emulator,
            onStatus = { status = it },
            onSlotSaved = { slot ->
                // v1.8.8 "Atlas" (Workstream E): "first save-state" PGS achievement
                // (idempotent; no-op behind the default-off gate). DISTINCT from RA.
                host?.playGames?.unlock(PgsIds.ACH_FIRST_SAVE_STATE)
                // v1.8.8 "Atlas" (Workstream D): push the just-saved slot to the cloud
                // as its own Snapshot (one independently-updatable unit). No-op unless
                // cloud saves are active; a divergent conflict surfaces the picker.
                val sha = emulator.romSha
                if (sha != null && host?.cloudSave?.isActive(settings) == true) {
                    host.cloudSave.pushSlot(
                        sha, slot, settings,
                        onConflict = { c -> cloudConflict = c },
                        onDone = { if (it) host.playGames.unlock(PgsIds.ACH_FIRST_CLOUD_SYNC) },
                    )
                }
            },
            onDismiss = { showStates = false },
        )
    }
    // v1.8.8 "Atlas" (Workstream L): a FLEXIBLE in-app update finished downloading —
    // offer a "Restart to install" prompt (the user chose when; flexible never forces).
    if (host?.updateReadyState?.value == true) {
        androidx.compose.material3.AlertDialog(
            onDismissRequest = { host.updateReadyState.value = false },
            title = { Text(stringResource(R.string.update_ready_title)) },
            text = { Text(stringResource(R.string.update_ready_body)) },
            confirmButton = {
                androidx.compose.material3.TextButton(onClick = {
                    host.updateReadyState.value = false
                    host.completeFlexibleUpdate()
                }) { Text(stringResource(R.string.update_ready_restart)) }
            },
            dismissButton = {
                androidx.compose.material3.TextButton(onClick = {
                    host.updateReadyState.value = false
                }) { Text(stringResource(R.string.about_close)) }
            },
        )
    }

    // v1.8.8 "Atlas" (Workstream D): the cloud-save conflict picker. Surfaced when a
    // Snapshot open/push diverged between this device and the cloud; the user picks
    // which copy to keep (last-write-wins is the auto path; this is the manual
    // fallback). A richer 3-way merge UI is a deliberate TODO.
    cloudConflict?.let { conflict ->
        androidx.compose.material3.AlertDialog(
            onDismissRequest = { cloudConflict = null },
            title = { Text(stringResource(R.string.cloud_conflict_title)) },
            text = { Text(stringResource(R.string.cloud_conflict_body)) },
            confirmButton = {
                androidx.compose.material3.TextButton(onClick = {
                    host?.cloudSave?.resolveConflict(conflict, keepLocal = true)
                    cloudConflict = null
                }) { Text(stringResource(R.string.cloud_conflict_keep_local)) }
            },
            dismissButton = {
                androidx.compose.material3.TextButton(onClick = {
                    host?.cloudSave?.resolveConflict(conflict, keepLocal = false)
                    cloudConflict = null
                }) { Text(stringResource(R.string.cloud_conflict_keep_cloud)) }
            },
        )
    }
    if (showAbout) {
        AboutDialog(onDismiss = { showAbout = false })
    }
    if (showControllers) {
        ControllersSheet(
            gamepad = gamepad,
            settings = settings,
            onDismiss = { showControllers = false },
        )
    }
    if (showNetplay) {
        NetplaySheet(
            status = npStatus,
            hostInfo = npHostInfo,
            roomCode = npRoomCode,
            lastJoinAddress = settings.lastJoinAddress,
            lastRoomCode = settings.lastRoomCode,
            // Online play needs a real (non-placeholder) signaling relay configured.
            onlineConfigured = settings.npSignalingUrl.trim().isNotEmpty() &&
                settings.npSignalingUrl.trim() != NetplayEndpoints.SIGNALING_URL,
            onHost = { port, players -> netplayHost(port, players) },
            onJoin = { addr -> netplayJoin(addr) },
            onHostRoom = { netplayHostRoom() },
            onJoinRoom = { code -> netplayJoinRoom(code) },
            onLeave = { netplayLeave() },
            onSaveJoinAddress = { settings.lastJoinAddress = it },
            onSaveRoomCode = { settings.lastRoomCode = it },
            onShareRoomCode = { shareRoomCode(it) },
            onDismiss = { showNetplay = false },
        )
    }
    if (showOnboarding) {
        OnboardingDialogs(
            onSuppress = { settings.onboardingSuppressed = true },
            onFinished = { showOnboarding = false },
        )
    }

    // v1.8.8 "Atlas" (Workstream C): move-to-folder dialog (pick/create a collection).
    folderTarget?.let { target ->
        MoveToFolderDialog(
            current = target.folder,
            folders = libraryFolders,
            onConfirm = { folder ->
                GameLibrary.setFolder(context, target.sha, folder)
                libraryVersion++
                folderTarget = null
            },
            onDismiss = { folderTarget = null },
        )
    }

    // v1.8.8 "Atlas" (Workstream C): box-art preview dialog (auto-match -> preview ->
    // apply, or fall back to the manual SAF image picker).
    boxArtPreview?.let { preview ->
        BoxArtPreviewDialog(
            state = preview,
            onApply = { uri ->
                boxArtTarget?.let { GameLibrary.setBoxArt(context, it.sha, uri); libraryVersion++ }
                boxArtPreview = null
                boxArtTarget = null
            },
            onPickManually = { boxArtPicker.launch(arrayOf("image/*")) },
            onDismiss = { boxArtPreview = null; boxArtTarget = null },
        )
    }

    // Demo-expired gate: a blocking sheet over everything with Unlock + Restore.
    if (!unlocked && demoExpired) {
        DemoExpiredOverlay(
            price = license.product?.oneTimePurchaseOfferDetails?.formattedPrice ?: "$2.99",
            onUnlock = { activity?.let { license.purchase(it) } },
            onRestore = { license.refreshEntitlement() },
        )
    }

    // Emulation loop: run frames + render audio on a background dispatcher, then
    // publish each frame to Compose. Pacing is audio-clocked when sound is present
    // (the blocking AudioTrack write paces the loop to real time) with a wall-clock
    // floor so silent ROMs still run at ~60 Hz.
    LaunchedEffect(Unit) {
        val reuse = Bitmap.createBitmap(NES_WIDTH, NES_HEIGHT, Bitmap.Config.ARGB_8888)
        val pixels = IntArray(NES_WIDTH * NES_HEIGHT)
        val audio = AudioPlayer(48_000)
        // RetroAchievements is polled at a low cadence (every ~15 frames) — toasts
        // and login status don't need per-frame granularity, and skipping the FFI
        // round-trips keeps the hot path clean when RA is off.
        var raFrame = 0
        // Netplay status is polled at the same low cadence as RA, on its own
        // always-advancing counter (the RA counter only ticks when RA is enabled).
        var npFrame = 0
        // v1.8.8 "Atlas" (Workstream E): accumulate frames run with fast-forward
        // (turbo) engaged and post the PGS incremental achievement in batches (every
        // ~30 turbo frames) to avoid a per-frame FFI/Task hit. No-op behind the
        // default-off PGS_ENABLED gate; the increment auto-unlocks at the Console step
        // target. DISTINCT from RetroAchievements.
        var turboFrames = 0
        try {
            while (isActive) {
                val ctrl = emulator.controller
                if (ctrl != null && !romLoaded) {
                    romLoaded = true
                    // A fresh controller just appeared: (re-)apply Four Score for the
                    // current pad count and re-push every port's held mask onto it.
                    gamepad.onControllerReady()
                    emulator.reapplyAllPorts()
                }
                if (ctrl == null || emulator.paused) {
                    delay(50)
                    continue
                }
                // Advance the turbo/autofire pulse once per emulated frame, then push
                // the per-port masks updated below. Cheap no-op when nothing is held
                // in turbo. Must run BEFORE the input is latched into the core.
                gamepad.onFrameTick()
                // Netplay (v1.8.6): while a session is active the loop drives the core
                // via `npAdvanceFrame` (rollback owns pacing), NOT `runFrame`. Force
                // speed to 100% — no turbo / fast-forward / frame-skip / rewind — and
                // present + drain audio only on a frame that actually advanced.
                val np = ctrl.npIsActive()
                val turbo = !np && emulator.turbo
                // v1.8.8 "Atlas" (Workstream E): batch the turbo-frames achievement.
                if (turbo) {
                    turboFrames++
                    if (turboFrames >= 30) {
                        host?.playGames?.increment(PgsIds.ACH_TURBO_100, turboFrames)
                        turboFrames = 0
                    }
                }
                val start = System.nanoTime()
                // Emulate, play this frame's audio, and pack the framebuffer all
                // off the main thread (the blocking audio write and the 61k-pixel
                // RGBA->ARGB pack must never run on the UI thread). Only the cheap
                // setPixels + asImageBitmap stay on the UI thread.
                var usedHd = false
                var logLines: List<String> = emptyList()
                // Whether this iteration produced a frame to present (always for
                // single-player; for netplay only when the tick advanced — a stalled /
                // connecting / error tick produces nothing and skips present + audio).
                var producedFrame = true
                // Capture the HD bitmap once per iteration so an Unload on the UI
                // thread can't null it mid-frame (the local keeps the object alive).
                val hdBmp = if (np) null else if (hdActive) hd.bitmap else null
                withContext(Dispatchers.Default) {
                    if (np) {
                        // Netplay tick: REPLACES runFrame. `npAdvanceFrame` advanced the
                        // same `Nes`, so read the framebuffer via the non-advancing
                        // index path (running runFrame again would desync rollback).
                        val tick = ctrl.npAdvanceFrame(emulator.p1Mask().toUByte())
                        if (!tick.producedFrame) {
                            producedFrame = false
                            // Still drain the audio ring so it doesn't back up while
                            // connecting / stalling (discarded — no present this tick).
                            ctrl.drainAudioBytes()
                            return@withContext
                        }
                        // Index framebuffer -> ARGB via the shared composite LUT (the
                        // GPU SurfaceView is bypassed during netplay — the picture goes
                        // through the Bitmap path, like the HD-pack path).
                        packIndexToArgb(ctrl.indexFramebufferBytes(), pixels)
                        val audioBytes = ctrl.drainAudioBytes()
                        if (!emulator.muted) audio.writeBytes(audioBytes)
                        if (scriptLoaded) logLines = ctrl.drainScriptLog()
                        return@withContext
                    }
                    val fb = ctrl.runFrame()
                    if (hdBmp != null) {
                        // HD-pack: composite the upscaled frame (Bitmap path only —
                        // the GPU SurfaceView is fixed at 256x240).
                        val comp = ctrl.compositeHdFrame()
                        if (comp.size == hd.w * hd.h * 4) {
                            packRgbaToArgb(comp, hd.pixels)
                            usedHd = true
                        } else {
                            packRgbaToArgb(fb, pixels)
                        }
                    } else {
                        // Bisqwit needs the palette-index frame + NTSC phase; submit it
                        // BEFORE the frame so the render thread pairs them (it consumes
                        // the frame first). Only fetched while that filter is active.
                        if (gpuSurface != null && settings.filter == VideoFilter.Bisqwit) {
                            gpuSurface.submitIndexFrame(ctrl.indexFramebufferBytes(), ctrl.ntscPhase().toInt())
                        }
                        // Hand the raw RGBA frame to the GPU SurfaceView (opt-in path);
                        // no-op when the GPU renderer is off.
                        gpuSurface?.submitFrame(fb)
                        packRgbaToArgb(fb, pixels)
                    }
                    // Hot path: drain audio as raw bytes (no per-sample Float boxing)
                    // and write straight to the PCM_FLOAT track.
                    val audioBytes = ctrl.drainAudioBytes()
                    // In fast-forward the audio is dropped (writing it would block
                    // the loop back to real time); otherwise play unless muted.
                    if (!turbo && !emulator.muted) audio.writeBytes(audioBytes)
                    // Lua: drain the script's print/log output (cheap when empty).
                    if (scriptLoaded) logLines = ctrl.drainScriptLog()
                }
                if (producedFrame) {
                    val presented: Bitmap
                    if (usedHd && hdBmp != null) {
                        hdBmp.setPixels(hd.pixels, 0, hd.w, 0, 0, hd.w, hd.h)
                        frame = hdBmp.asImageBitmap()
                        presented = hdBmp
                    } else {
                        reuse.setPixels(pixels, 0, NES_WIDTH, 0, 0, NES_WIDTH, NES_HEIGHT)
                        frame = reuse.asImageBitmap()
                        presented = reuse
                    }
                    // v1.8.8 "Atlas" (Workstream F): publish the gameplay frame for
                    // capture (a reference to the just-blitted buffer, no UI chrome),
                    // and feed the clip ring when recording (it copies on the throttle
                    // beat). Both are cheap; the encode happens off-loop on Stop.
                    capture.latestFrame = presented
                    capture.clip?.offer(presented)
                    // Append new script log lines (keep the last 8 for the overlay).
                    if (logLines.isNotEmpty()) {
                        scriptLog = (scriptLog.split("\n").filter { it.isNotEmpty() } + logLines)
                            .takeLast(8).joinToString("\n")
                    }
                    // Mirror the picture to the external display while casting (no-op
                    // otherwise). Same main-thread publish point as the Compose frame.
                    castManager.pushFrame(reuse)
                    // Experimental Chromecast (CAF) spectator mirror (v1.8.7, #38):
                    // stream the palette-index plane to the Web Receiver, internally
                    // throttled to ~20-30fps and 64 KB-capped. Compiled out entirely
                    // in default builds (flag off); a cheap no-op when no Cast session.
                    if (BuildConfig.CHROMECAST_ENABLED) {
                        chromecast.sendFrame(ctrl.indexFramebufferBytes())
                    }
                }
                // Netplay status poll (v1.8.6): refresh the panel/overlay snapshot at a
                // low cadence whether or not a frame was produced (so a stall / connect
                // shows live). Its counter always advances (unlike the RA one).
                if (np) {
                    if (!npActive) npActive = true
                    if (++npFrame % 15 == 0) npStatus = ctrl.npStatus()
                } else if (npActive) {
                    // A session that ended (Leave / error tear-down) clears the snapshot.
                    npStatus = null
                    npActive = false
                    npHostInfo = null
                    npRoomCode = null
                }
                // RetroAchievements: poll the toast queue + login status at a low
                // cadence. Cheap when RA is off (a single `raIsEnabled` bool check).
                if (settings.raEnabled && (++raFrame % 15) == 0) {
                    val rctrl = emulator.controller
                    if (rctrl != null && rctrl.raIsEnabled()) {
                        // Assign unconditionally: raPollToasts returns the live,
                        // TTL'd toast set (it does not drain), so reflecting it
                        // every poll both shows new toasts and clears them once
                        // they expire (an empty poll => the HUD goes away).
                        raToasts = rctrl.raPollToasts()
                        val st = rctrl.raLoginStatus()
                        val loggedIn = st == RaLoginStatus.LOGGED_IN
                        if (loggedIn && !raWasLoggedIn) {
                            // LOGGED_OUT -> LOGGED_IN edge: persist the token + user
                            // (never the password) for silent re-login, surface the
                            // user, and identify the loaded game if not yet done.
                            rctrl.raToken()?.let { settings.raToken = it }
                            raUserName = rctrl.raUser()?.displayName ?: settings.raUsername
                            raStatus = "Signed in"
                            if (!raGameLoaded) raIdentifyGame()
                        } else if (!loggedIn && raWasLoggedIn) {
                            raUserName = null
                            raStatus = "Logged out"
                        } else if (st == RaLoginStatus.ERROR) {
                            raStatus = "Login failed"
                        }
                        raWasLoggedIn = loggedIn
                    }
                }
                // Fast-forward skips the pacing delay so the core runs ahead.
                if (!turbo) {
                    val remainingMs = (FRAME_NANOS - (System.nanoTime() - start)) / 1_000_000
                    if (remainingMs > 0) delay(remainingMs)
                }
            }
        } finally {
            audio.release()
        }
    }

    DisposableEffect(Unit) {
        onDispose {
            // Persist RA progress before tearing down the controller (ROM unload).
            raSaveProgress()
            emulator.controller = null
        }
    }
}

/**
 * Render state for an active HD-pack (v1.8.5): the upscaled output bitmap + its
 * scratch ARGB buffer and dimensions. Plain holder (not Compose state) read by the
 * emulation loop; the `hdActive` flag that switches the UI to the Bitmap path is a
 * separate Compose state.
 */
private class HdRender {
    var bitmap: Bitmap? = null
    var pixels: IntArray = IntArray(0)
    var w = 0
    var h = 0
}

/**
 * v1.8.8 "Atlas" (Workstream F): capture state shared between the emulation loop and
 * the screenshot/clip controls. The loop publishes the latest gameplay-frame bitmap
 * each presented frame (the same buffer it blits to Compose — no UI chrome) and, when
 * a clip is recording, offers each frame to the ring buffer. Reads/writes are cheap
 * references; the encode (on Stop) happens off the loop.
 */
private class CaptureState {
    /** The latest gameplay-frame bitmap (the loop's reused 256x240 buffer, or the
     *  HD-pack upscaled buffer when active). A capture copies from it. */
    @Volatile
    var latestFrame: Bitmap? = null

    /** Live clip ring buffer while recording (null when not). */
    @Volatile
    var clip: Capture.ClipBuffer? = null
}

/** Convert the core's RGBA8 framebuffer into packed ARGB_8888 pixels. */
private fun packRgbaToArgb(rgba: ByteArray, out: IntArray) {
    var i = 0
    var p = 0
    while (p < out.size) {
        val r = rgba[i].toInt() and 0xFF
        val g = rgba[i + 1].toInt() and 0xFF
        val b = rgba[i + 2].toInt() and 0xFF
        out[p] = (0xFF shl 24) or (r shl 16) or (g shl 8) or b
        i += 4
        p += 1
    }
}


/** Blocking sheet shown when the free 10-minute demo session expires. */
@Composable
private fun DemoExpiredOverlay(price: String, onUnlock: () -> Unit, onRestore: () -> Unit) {
    Box(
        modifier = Modifier.fillMaxSize().background(Color(0xE6000000)),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            modifier = Modifier.padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text("Demo time's up", color = Color.White)
            Spacer(Modifier.height(8.dp))
            Text(
                "Unlock the full version to keep playing — save states, resume, " +
                    "and in-cart battery saves included.",
                color = Color.LightGray,
            )
            Spacer(Modifier.height(20.dp))
            Button(onClick = onUnlock) { Text("Unlock $price") }
            Spacer(Modifier.height(8.dp))
            androidx.compose.material3.TextButton(onClick = onRestore) {
                Text("Restore purchase")
            }
        }
    }
}

// The on-screen controls now live in `VirtualController.kt` — a single multi-touch
// Canvas (the old per-button `TouchOverlay`/`PadButton` registered one input at a
// time and was replaced in v1.8.2).
