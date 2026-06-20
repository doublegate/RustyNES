package com.doublegate.rustynes

import android.content.Context
import android.hardware.input.InputManager
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import org.json.JSONObject

/**
 * Hardware game-controller support for RustyNES Android (v1.8.7, #37 + #32).
 *
 * Android ships no Jetpack controller library, so this owns the whole platform-API
 * surface: it enumerates [InputDevice]s, listens for hot-plug via
 * [InputManager.InputDeviceListener], assigns each pad a NES port (`0..3`,
 * first-seen order), keeps a per-descriptor remap table, and folds all of it into
 * the four per-port masks it drives through [EmulatorHandle.setGamepadMask].
 *
 * Two input forms are decoded (the d-pad-as-HAT bug this fixes):
 *  - **Buttons + d-pad keys** arrive as [KeyEvent]s (`KEYCODE_BUTTON_*`,
 *    `KEYCODE_DPAD_*`) → [onKey].
 *  - **Analog sticks, triggers, and the d-pad-as-HAT axis** arrive as
 *    [MotionEvent]s (`AXIS_HAT_X/Y`, `AXIS_X/Y`, `AXIS_Z/RZ`, trigger axes) →
 *    [onMotion]. Many pads (8BitDo, DualSense, OTG) report the d-pad ONLY as a HAT
 *    axis, so without this they did nothing.
 *
 * Turbo/autofire (#32): a button can be remapped to "Turbo A"/"Turbo B" (or a global
 * A/B autofire toggle); held, the manager pulses that bit at ~15 Hz, driven off the
 * emulation loop's [onFrameTick].
 *
 * Four Score auto-enables once `>= 3` pads are assigned (so ports 2/3 reach the core
 * through the multiplexer), and disables again at `<= 2`.
 *
 * Determinism is unaffected: every pad converges on the same single late-latched
 * per-port mask the touch overlay and netplay already drive.
 */
class GamepadManager(
    private val context: Context,
    private val emulator: EmulatorHandle,
) {
    /** Live per-port assignment: deviceId -> port (0..3). First-seen order. */
    private val deviceToPort = LinkedHashMap<Int, Int>()

    /** Per-port logical (non-turbo) button mask, before the turbo pulse is mixed in. */
    private val baseMask = IntArray(4)

    /** Per-port set of NES bits currently driven by a held turbo input (pulsed). */
    private val turboBits = IntArray(4)

    /** Per-descriptor remap tables, loaded from / saved to settings (JSON). */
    private val remaps = HashMap<String, ControllerProfile>()

    /** Cached descriptor per deviceId so a remove (device already gone) can still
     *  resolve its profile, and so capture flows know which device fired. */
    private val deviceDescriptors = HashMap<Int, String>()

    /** Global A/B autofire toggle (applies to whichever inputs map to plain A/B). */
    @Volatile
    var autofireAB: Boolean = false

    private var inputManager: InputManager? = null
    private var listener: InputManager.InputDeviceListener? = null

    // Active remap capture (Settings rebind flow): the descriptor we're capturing
    // for + a one-shot callback fed the next input (a keycode or a trigger sentinel)
    // that originates from a device with that descriptor. Non-null swallows that
    // input so it isn't ALSO routed as gameplay.
    @Volatile
    private var captureDescriptor: String? = null
    @Volatile
    private var captureCallback: ((Int) -> Unit)? = null

    /** Begin capturing the next input from [descriptor]; [onInput] gets its keycode
     *  (or a [ControllerProfile.AXIS_*] sentinel). One-shot. */
    fun beginCapture(descriptor: String, onInput: (Int) -> Unit) {
        captureDescriptor = descriptor
        captureCallback = onInput
    }

    fun cancelCapture() {
        captureDescriptor = null
        captureCallback = null
    }

    private fun tryCapture(deviceId: Int, input: Int): Boolean {
        val want = captureDescriptor ?: return false
        if (deviceDescriptors[deviceId] != want) return false
        val cb = captureCallback ?: return false
        captureDescriptor = null
        captureCallback = null
        cb(input)
        return true
    }

    /** Observers (the Settings "Controllers" screen) notified when the device list
     *  or port assignment changes, so the UI re-reads [connectedPads]. */
    private val changeListeners = mutableListOf<() -> Unit>()

    fun addChangeListener(l: () -> Unit) { changeListeners.add(l) }
    fun removeChangeListener(l: () -> Unit) { changeListeners.remove(l) }
    private fun notifyChanged() { changeListeners.toList().forEach { it() } }

    // ---- lifecycle ---------------------------------------------------------

    /** Register the hot-plug listener + enumerate already-connected pads. Call from
     *  the Activity's `onResume`. Idempotent. */
    fun register() {
        if (inputManager != null) return
        val im = context.getSystemService(Context.INPUT_SERVICE) as InputManager
        inputManager = im
        val l = object : InputManager.InputDeviceListener {
            override fun onInputDeviceAdded(deviceId: Int) { addDevice(deviceId); notifyChanged() }
            override fun onInputDeviceRemoved(deviceId: Int) { removeDevice(deviceId); notifyChanged() }
            override fun onInputDeviceChanged(deviceId: Int) { addDevice(deviceId); notifyChanged() }
        }
        im.registerInputDeviceListener(l, null)
        listener = l
        // Seed with whatever is already plugged in.
        for (id in im.inputDeviceIds) addDevice(id)
        notifyChanged()
    }

    /** Unregister the hot-plug listener. Call from the Activity's `onPause`. */
    fun unregister() {
        listener?.let { inputManager?.unregisterInputDeviceListener(it) }
        listener = null
        inputManager = null
    }

    // ---- device assignment -------------------------------------------------

    private fun isGamepad(dev: InputDevice?): Boolean {
        if (dev == null || dev.isVirtual) return false
        val s = dev.sources
        return (s and InputDevice.SOURCE_GAMEPAD) == InputDevice.SOURCE_GAMEPAD ||
            (s and InputDevice.SOURCE_JOYSTICK) == InputDevice.SOURCE_JOYSTICK ||
            (s and InputDevice.SOURCE_DPAD) == InputDevice.SOURCE_DPAD
    }

    private fun addDevice(deviceId: Int) {
        val dev = InputDevice.getDevice(deviceId) ?: return
        if (!isGamepad(dev)) return
        deviceDescriptors[deviceId] = dev.descriptor
        remaps.getOrPut(dev.descriptor) { ControllerProfile.default() }
        if (!deviceToPort.containsKey(deviceId)) {
            val port = firstFreePort() ?: return // all four ports taken; ignore extra pads
            deviceToPort[deviceId] = port
            updateFourScore()
        }
    }

    private fun removeDevice(deviceId: Int) {
        val port = deviceToPort.remove(deviceId) ?: return
        deviceDescriptors.remove(deviceId)
        // Clear that port's contribution so a yanked pad doesn't leave buttons held.
        baseMask[port] = 0
        turboBits[port] = 0
        emulator.setGamepadMask(port, 0)
        updateFourScore()
    }

    private fun firstFreePort(): Int? {
        val used = deviceToPort.values.toSet()
        return (0..3).firstOrNull { it !in used }
    }

    /** Reassign a device to an explicit port (Settings port picker). If another pad
     *  holds that port, the two swap so no port is double-booked. */
    fun assignPort(deviceId: Int, newPort: Int) {
        if (newPort !in 0..3) return
        val oldPort = deviceToPort[deviceId] ?: return
        if (oldPort == newPort) return
        val other = deviceToPort.entries.firstOrNull { it.value == newPort }?.key
        deviceToPort[deviceId] = newPort
        if (other != null) deviceToPort[other] = oldPort
        // Recompute both affected ports' masks from scratch (held state is dropped —
        // a reassign mid-press is rare and a clean slate is the safe choice).
        for (p in setOf(oldPort, newPort)) {
            baseMask[p] = 0
            turboBits[p] = 0
            emulator.setGamepadMask(p, 0)
        }
        updateFourScore()
        notifyChanged()
    }

    private fun updateFourScore() {
        emulator.controller?.setFourScore(deviceToPort.size >= 3)
    }

    /** A fresh [NesController] just loaded: re-assert Four Score for the current pad
     *  count. (Per-port masks are re-pushed separately by the loop.) Called from the
     *  emulation loop the first frame a new controller is seen. */
    fun onControllerReady() = updateFourScore()

    // ---- input routing -----------------------------------------------------

    /**
     * Route a hardware [KeyEvent]. Returns true if it was a mapped gamepad button
     * (consumed). A key NOT from a gamepad source, or from an unknown device, is
     * left for the Activity's keyboard fallback (which feeds P1 as a default
     * profile). DPAD keys count as gamepad input regardless of source flags.
     */
    fun onKey(event: KeyEvent): Boolean {
        val deviceId = event.deviceId
        // Remap capture: grab the keycode on key-down + swallow it (don't also play).
        if (captureDescriptor != null && event.action == KeyEvent.ACTION_DOWN) {
            if (tryCapture(deviceId, event.keyCode)) return true
        }
        val port = deviceToPort[deviceId] ?: return false
        val descriptor = deviceDescriptors[deviceId]
        val profile = descriptor?.let { remaps[it] } ?: ControllerProfile.default()
        val action = profile.actionForKey(event.keyCode) ?: return false
        applyAction(port, action, event.action == KeyEvent.ACTION_DOWN)
        return true
    }

    /**
     * Route a hardware [MotionEvent] (joystick). Decodes the d-pad-as-HAT axis, the
     * left/right analog sticks (stick→d-pad past the flat dead-zone), and the L/R
     * trigger axes. Processes any batched historical samples so fast motion isn't
     * dropped. Returns true if from a joystick source.
     */
    fun onMotion(event: MotionEvent): Boolean {
        if ((event.source and InputDevice.SOURCE_JOYSTICK) != InputDevice.SOURCE_JOYSTICK &&
            (event.source and InputDevice.SOURCE_GAMEPAD) != InputDevice.SOURCE_GAMEPAD
        ) {
            return false
        }
        if (event.action != MotionEvent.ACTION_MOVE) return false
        // Remap capture: a trigger pressed past half-travel binds that sentinel.
        if (captureDescriptor != null && deviceDescriptors[event.deviceId] == captureDescriptor) {
            val lt = maxOf(event.getAxisValue(MotionEvent.AXIS_LTRIGGER), event.getAxisValue(MotionEvent.AXIS_BRAKE))
            val rt = maxOf(event.getAxisValue(MotionEvent.AXIS_RTRIGGER), event.getAxisValue(MotionEvent.AXIS_GAS))
            if (lt > 0.7f && tryCapture(event.deviceId, ControllerProfile.AXIS_LTRIGGER)) return true
            if (rt > 0.7f && tryCapture(event.deviceId, ControllerProfile.AXIS_RTRIGGER)) return true
        }
        val port = deviceToPort[event.deviceId] ?: return false
        val dev = event.device
        val descriptor = deviceDescriptors[event.deviceId]
        val profile = descriptor?.let { remaps[it] } ?: ControllerProfile.default()
        // Process historical batched samples then the current one (last wins).
        val hist = event.historySize
        for (i in 0 until hist) decodeMotion(port, event, dev, profile, i)
        decodeMotion(port, event, dev, profile, -1)
        return true
    }

    private fun decodeMotion(
        port: Int,
        event: MotionEvent,
        dev: InputDevice?,
        profile: ControllerProfile,
        histPos: Int,
    ) {
        fun axis(a: Int): Float =
            if (histPos < 0) event.getAxisValue(a) else event.getHistoricalAxisValue(a, histPos)

        // The d-pad as a HAT axis (the key fix): -1 / 0 / +1 on HAT_X / HAT_Y.
        val hatX = axis(MotionEvent.AXIS_HAT_X)
        val hatY = axis(MotionEvent.AXIS_HAT_Y)
        // Left stick -> d-pad, past the device's flat (dead) zone (getCenteredAxis).
        val lx = centeredAxis(axis(MotionEvent.AXIS_X), dev, MotionEvent.AXIS_X, event.source)
        val ly = centeredAxis(axis(MotionEvent.AXIS_Y), dev, MotionEvent.AXIS_Y, event.source)

        val left = hatX < -0.5f || lx < -STICK_THRESHOLD
        val right = hatX > 0.5f || lx > STICK_THRESHOLD
        val up = hatY < -0.5f || ly < -STICK_THRESHOLD
        val down = hatY > 0.5f || ly > STICK_THRESHOLD

        setBit(port, NesBit.LEFT, left)
        setBit(port, NesBit.RIGHT, right)
        setBit(port, NesBit.UP, up)
        setBit(port, NesBit.DOWN, down)

        // Analog triggers can be remapped (default: unused — the shoulder buttons
        // already map to Select/Start). A trigger past half-travel acts as pressed.
        profile.triggerAction(left = true)?.let { a ->
            applyAction(port, a, axis(MotionEvent.AXIS_LTRIGGER).coerceAtLeast(axis(MotionEvent.AXIS_BRAKE)) > 0.5f)
        }
        profile.triggerAction(left = false)?.let { a ->
            applyAction(port, a, axis(MotionEvent.AXIS_RTRIGGER).coerceAtLeast(axis(MotionEvent.AXIS_GAS)) > 0.5f)
        }
    }

    /** Apply the device's per-axis flat (dead) zone; returns 0 inside it. */
    private fun centeredAxis(value: Float, dev: InputDevice?, axis: Int, source: Int): Float {
        val range = dev?.getMotionRange(axis, source) ?: return value
        val flat = range.flat
        return if (kotlin.math.abs(value) > flat) value else 0f
    }

    // ---- action application -----------------------------------------------

    private fun applyAction(port: Int, action: NesAction, pressed: Boolean) {
        when (action.kind) {
            ActionKind.BUTTON -> {
                // A plain A/B button honours the global autofire toggle.
                val turbo = action.turbo ||
                    (autofireAB && (action.bit == NesBit.A || action.bit == NesBit.B))
                if (turbo) {
                    setTurbo(port, action.bit, pressed)
                } else {
                    setBit(port, action.bit, pressed)
                }
            }
        }
    }

    private fun setBit(port: Int, bit: Int, on: Boolean) {
        val nv = if (on) baseMask[port] or bit else baseMask[port] and bit.inv()
        if (nv != baseMask[port]) {
            baseMask[port] = nv
            pushPort(port)
        }
    }

    private fun setTurbo(port: Int, bit: Int, on: Boolean) {
        val nv = if (on) turboBits[port] or bit else turboBits[port] and bit.inv()
        if (nv != turboBits[port]) {
            turboBits[port] = nv
            // Release: drop the bit immediately so it doesn't stick.
            if (!on) { baseMask[port] = baseMask[port] and bit.inv(); pushPort(port) }
        }
    }

    /** Combine the base mask with the current turbo-pulse phase and push the port. */
    private fun pushPort(port: Int) {
        val mask = baseMask[port] or (turboBits[port] and turboPhaseMask)
        emulator.setGamepadMask(port, mask)
    }

    // ---- turbo pulse -------------------------------------------------------

    private var frameCount = 0

    /** When the pulse mask is "all on" this frame the turbo bits are pressed; when
     *  "all off" they're released. Toggled every [TURBO_PERIOD_FRAMES] frames. */
    @Volatile
    private var turboPhaseMask = 0

    /**
     * Called once per emulated frame from the loop. Advances the ~15 Hz turbo pulse
     * (toggle every [TURBO_PERIOD_FRAMES] frames) and re-pushes any port that has
     * held turbo inputs so the pulse reaches the core. Cheap (a no-op when nothing
     * is in turbo).
     */
    fun onFrameTick() {
        frameCount++
        if (frameCount % TURBO_PERIOD_FRAMES != 0) return
        turboPhaseMask = turboPhaseMask.inv() and 0xFF
        for (port in 0..3) {
            if (turboBits[port] != 0) pushPort(port)
        }
    }

    // ---- Settings / remap support -----------------------------------------

    data class PadInfo(
        val deviceId: Int,
        val name: String,
        val descriptor: String,
        val port: Int,
    )

    /** Snapshot of currently-assigned pads for the Settings "Controllers" screen. */
    fun connectedPads(): List<PadInfo> = deviceToPort.entries
        .sortedBy { it.value }
        .mapNotNull { (id, port) ->
            val name = InputDevice.getDevice(id)?.name ?: "Controller"
            val desc = deviceDescriptors[id] ?: return@mapNotNull null
            PadInfo(id, name, desc, port)
        }

    fun profileFor(descriptor: String): ControllerProfile =
        remaps.getOrPut(descriptor) { ControllerProfile.default() }

    /** Bind [input] (a keycode, or [ControllerProfile.AXIS_*] sentinel) on the given
     *  descriptor's profile to [action], persisting the whole table. */
    fun remap(descriptor: String, input: Int, action: NesAction, settings: AppSettings) {
        profileFor(descriptor).bind(input, action)
        persist(settings)
        notifyChanged()
    }

    fun resetProfile(descriptor: String, settings: AppSettings) {
        remaps[descriptor] = ControllerProfile.default()
        persist(settings)
        notifyChanged()
    }

    /** Load the saved remap JSON (descriptor -> profile) from settings. */
    fun loadRemaps(settings: AppSettings) {
        val json = settings.gamepadRemaps
        if (json.isBlank()) return
        runCatching {
            val root = JSONObject(json)
            for (desc in root.keys()) {
                remaps[desc] = ControllerProfile.fromJson(root.getJSONObject(desc))
            }
        }
        autofireAB = settings.autofireAB
    }

    private fun persist(settings: AppSettings) {
        val root = JSONObject()
        for ((desc, prof) in remaps) root.put(desc, prof.toJson())
        settings.gamepadRemaps = root.toString()
    }

    fun setAutofire(on: Boolean, settings: AppSettings) {
        autofireAB = on
        settings.autofireAB = on
    }

    companion object {
        /** Left-stick magnitude past which it counts as a d-pad press. */
        private const val STICK_THRESHOLD = 0.5f

        /** Turbo pulse: toggle every 2 frames ≈ 15 Hz (on for 2, off for 2). */
        private const val TURBO_PERIOD_FRAMES = 2
    }
}

/** What a mapped input does on the NES pad. */
enum class ActionKind { BUTTON }

/**
 * A NES action a physical input maps to: a button [bit], optionally as a turbo
 * (autofire) variant. (Kind exists so the model can grow, e.g. macros, later.)
 */
data class NesAction(
    val kind: ActionKind,
    val bit: Int,
    val turbo: Boolean = false,
) {
    fun serialize(): String = "${bit}:${if (turbo) 1 else 0}"

    companion object {
        fun button(bit: Int) = NesAction(ActionKind.BUTTON, bit, false)
        fun turbo(bit: Int) = NesAction(ActionKind.BUTTON, bit, true)
        fun parse(s: String): NesAction? {
            val parts = s.split(":")
            if (parts.size != 2) return null
            val bit = parts[0].toIntOrNull() ?: return null
            return NesAction(ActionKind.BUTTON, bit, parts[1] == "1")
        }

        /** Stable label for a NES bit / turbo action (Settings UI). */
        fun label(action: NesAction): String {
            val base = when (action.bit) {
                NesBit.A -> "A"
                NesBit.B -> "B"
                NesBit.SELECT -> "Select"
                NesBit.START -> "Start"
                NesBit.UP -> "Up"
                NesBit.DOWN -> "Down"
                NesBit.LEFT -> "Left"
                NesBit.RIGHT -> "Right"
                else -> "?"
            }
            return if (action.turbo) "Turbo $base" else base
        }
    }
}

/**
 * Per-controller remap table, keyed by [InputDevice.getDescriptor] (stable across
 * reconnects). Maps raw keycodes (and the two analog-trigger sentinels) to a
 * [NesAction]. Defaults to a standard Xbox/SDL layout so an unmapped pad just works.
 */
class ControllerProfile private constructor(
    private val keyToAction: HashMap<Int, NesAction>,
) {
    fun actionForKey(keyCode: Int): NesAction? = keyToAction[keyCode]

    /** Action bound to the left/right analog trigger (sentinel inputs), or null. */
    fun triggerAction(left: Boolean): NesAction? =
        keyToAction[if (left) AXIS_LTRIGGER else AXIS_RTRIGGER]

    fun bind(input: Int, action: NesAction) { keyToAction[input] = action }

    fun toJson(): JSONObject {
        val o = JSONObject()
        for ((k, v) in keyToAction) o.put(k.toString(), v.serialize())
        return o
    }

    /** A human-readable map of NES action -> the inputs bound to it (Settings UI). */
    fun bindings(): Map<Int, NesAction> = keyToAction.toMap()

    companion object {
        // Sentinel "keycodes" for the two analog triggers, well outside the real
        // KeyEvent range so they can live in the same table.
        const val AXIS_LTRIGGER = 100_001
        const val AXIS_RTRIGGER = 100_002

        /** The default Xbox/SDL-standard layout feeding a single NES pad. */
        fun default(): ControllerProfile {
            val m = HashMap<Int, NesAction>()
            // Face buttons: South=A, West=B (the comfortable NES-on-Xbox layout).
            m[KeyEvent.KEYCODE_BUTTON_A] = NesAction.button(NesBit.A)
            m[KeyEvent.KEYCODE_BUTTON_B] = NesAction.button(NesBit.B)
            // X/Y as turbo variants so a stock pad gets autofire out of the box.
            m[KeyEvent.KEYCODE_BUTTON_X] = NesAction.turbo(NesBit.B)
            m[KeyEvent.KEYCODE_BUTTON_Y] = NesAction.turbo(NesBit.A)
            m[KeyEvent.KEYCODE_BUTTON_START] = NesAction.button(NesBit.START)
            m[KeyEvent.KEYCODE_BUTTON_SELECT] = NesAction.button(NesBit.SELECT)
            // Shoulder buttons -> Select/Start as well (many pads lack Select).
            m[KeyEvent.KEYCODE_BUTTON_L1] = NesAction.button(NesBit.SELECT)
            m[KeyEvent.KEYCODE_BUTTON_R1] = NesAction.button(NesBit.START)
            // D-pad as KeyEvents (the other half of the dual d-pad form).
            m[KeyEvent.KEYCODE_DPAD_UP] = NesAction.button(NesBit.UP)
            m[KeyEvent.KEYCODE_DPAD_DOWN] = NesAction.button(NesBit.DOWN)
            m[KeyEvent.KEYCODE_DPAD_LEFT] = NesAction.button(NesBit.LEFT)
            m[KeyEvent.KEYCODE_DPAD_RIGHT] = NesAction.button(NesBit.RIGHT)
            m[KeyEvent.KEYCODE_DPAD_CENTER] = NesAction.button(NesBit.A)
            return ControllerProfile(m)
        }

        fun fromJson(o: JSONObject): ControllerProfile {
            val m = HashMap<Int, NesAction>()
            for (key in o.keys()) {
                val k = key.toIntOrNull() ?: continue
                NesAction.parse(o.getString(key))?.let { m[k] = it }
            }
            // An empty/garbage table falls back to the default so the pad still works.
            return if (m.isEmpty()) default() else ControllerProfile(m)
        }
    }
}
