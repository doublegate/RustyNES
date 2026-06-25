//
//  AppModel.swift
//
//  The app-wide observable state: the library, the active emulation session, the
//  selected video filter / mute, and the input plumbing (touch mask OR hardware
//  pad mask -> the core). Held by the App and shared down the view tree.
//

import Combine
import Foundation
import SwiftUI

@MainActor
final class AppModel: ObservableObject {
    let library = ROMLibrary()
    let saveStates = SaveStateManager()
    let audioSession = AudioSession()

    /// The currently-running game, or nil when on the library screen.
    @Published var emulator: EmulatorCore?
    /// The library entry backing `emulator` (for save-state keying / metadata).
    @Published var currentEntry: LibraryEntry?
    /// A transient error to surface to the user (load/import failures).
    @Published var errorMessage: String?

    // Settings (persisted lightly via UserDefaults).
    @Published var filter: VideoFilter = .none {
        didSet { applyFilter(); UserDefaults.standard.set(Int(filter.rawValue), forKey: "filter") }
    }
    @Published var muted: Bool = false {
        didSet { emulator?.isMuted = muted; UserDefaults.standard.set(muted, forKey: "muted") }
    }

    // Per-filter shader parameters (the tunable knobs the renderer's filter pipelines
    // read). Each maps into the gfx `p0..p3` per `filterParams(_:)`, matching the
    // gfx_metal.rs param layout (Scanlines [intensity]; CRT [intensity, mask]; NTSC
    // [saturation, sharpness, tint, phase]). Defaults match the Android knob set.
    // `didSet` re-applies live to the running renderer and persists to UserDefaults.
    @Published var scanlineIntensity: Float = 0.5 {
        didSet { applyFilter(); UserDefaults.standard.set(scanlineIntensity, forKey: "scanlineIntensity") }
    }
    @Published var crtMask: Float = 0.10 {
        didSet { applyFilter(); UserDefaults.standard.set(crtMask, forKey: "crtMask") }
    }
    @Published var ntscSaturation: Float = 0.55 {
        didSet { applyFilter(); UserDefaults.standard.set(ntscSaturation, forKey: "ntscSaturation") }
    }
    @Published var ntscSharpness: Float = 0.08 {
        didSet { applyFilter(); UserDefaults.standard.set(ntscSharpness, forKey: "ntscSharpness") }
    }
    @Published var ntscTint: Float = 0.0 {
        didSet { applyFilter(); UserDefaults.standard.set(ntscTint, forKey: "ntscTint") }
    }
    @Published var ntscPhase: Float = 0.0 {
        didSet { applyFilter(); UserDefaults.standard.set(ntscPhase, forKey: "ntscPhase") }
    }
    /// Optional Core Haptics feedback on on-screen button presses (OFF by default).
    @Published var hapticsEnabled: Bool = false {
        didSet {
            haptics.isEnabled = hapticsEnabled
            if hapticsEnabled { haptics.prepare() }
            UserDefaults.standard.set(hapticsEnabled, forKey: "hapticsEnabled")
        }
    }

    /// The multi-controller (P1-P4) hardware-pad manager. Exposed so SettingsView can
    /// list controllers, reassign ports, and edit the remap.
    let gamepads = GameControllerManager()
    private let haptics = HapticsManager()

    private var touchMask: UInt8 = 0
    /// Per-port hardware-pad masks (P1-P4). Touch input ORs into port 0 only.
    private var padMasks: [UInt8] = Array(repeating: 0, count: GameControllerManager.maxPlayers)

    /// Whether the device can play haptics (drives the Settings toggle's enabled state).
    var hapticsSupported: Bool { haptics.isSupported }

    init() {
        if let raw = UserDefaults.standard.object(forKey: "filter") as? Int,
           let f = VideoFilter(rawValue: UInt8(raw)) {
            filter = f
        }
        muted = UserDefaults.standard.bool(forKey: "muted")
        hapticsEnabled = UserDefaults.standard.bool(forKey: "hapticsEnabled")
        // Restore the persisted shader params, falling back to the defaults above when
        // a key was never written (UserDefaults.float returns 0 for a missing key, so
        // probe `object(forKey:)` to distinguish "unset" from a stored 0).
        let defaults = UserDefaults.standard
        func storedFloat(_ key: String, _ fallback: Float) -> Float {
            defaults.object(forKey: key) == nil ? fallback : defaults.float(forKey: key)
        }
        scanlineIntensity = storedFloat("scanlineIntensity", scanlineIntensity)
        crtMask = storedFloat("crtMask", crtMask)
        ntscSaturation = storedFloat("ntscSaturation", ntscSaturation)
        ntscSharpness = storedFloat("ntscSharpness", ntscSharpness)
        ntscTint = storedFloat("ntscTint", ntscTint)
        ntscPhase = storedFloat("ntscPhase", ntscPhase)
        // A property's `didSet` does NOT run for in-init assignment, so sync the
        // haptics engine to the persisted value explicitly (otherwise a stored
        // `true` would leave the generator unprepared until the user re-toggles).
        haptics.isEnabled = hapticsEnabled

        // Audio interruptions (phone call / Siri) and route changes (headphones
        // unplugged) flow through the run-state composition so they don't fight the
        // scene/menu pause gates: set a sticky `audioInterrupted` flag and recompute.
        audioSession.onShouldPause = { [weak self] in
            self?.audioInterrupted = true
            self?.applyRunState()
        }
        audioSession.onShouldResume = { [weak self] in
            self?.audioInterrupted = false
            self?.applyRunState()
        }
        gamepads.onMaskChanged = { [weak self] port, mask in
            guard let self else { return }
            let p = Int(port)
            guard p >= 0, p < self.padMasks.count else { return }
            self.padMasks[p] = mask
            self.pushInput(port: p)
        }
        gamepads.start()
    }

    // MARK: - Session lifecycle

    /// Open a library entry: read its bytes, build a fresh EmulatorCore, and start.
    func openGame(_ entry: LibraryEntry) {
        do {
            audioSession.configure()
            let data = try library.romData(for: entry)
            let core = try EmulatorCore(romData: data)
            core.isMuted = muted
            emulator?.shutdown()
            emulator = core
            currentEntry = entry
            library.markPlayed(entry.sha, info: core.info)
            applyFilter()
        } catch {
            errorMessage = "Could not load \(entry.name): \(error.localizedDescription)"
        }
    }

    /// Import a ROM from a picked URL and immediately open it.
    func importAndOpen(_ url: URL) {
        do {
            let entry = try library.importROM(from: url)
            openGame(entry)
        } catch {
            errorMessage = "Import failed: \(error.localizedDescription)"
        }
    }

    /// Close the running game and return to the library.
    func closeGame() {
        emulator?.shutdown()
        emulator = nil
        currentEntry = nil
        audioSession.deactivate()
    }

    // MARK: - Input fan-in

    /// The on-screen pad reports its combined multi-touch mask here; merged with the
    /// P1 hardware-pad mask. A rising edge (any newly pressed button) fires a haptic.
    func setTouchMask(_ mask: UInt8) {
        let newlyPressed = mask & ~touchMask
        touchMask = mask
        if newlyPressed != 0 { haptics.tap() }
        pushInput(port: 0)
    }

    /// Forward one port's effective mask to the core. Port 0 is touch OR P1 pad; the
    /// other ports are their pad mask alone.
    private func pushInput(port: Int) {
        guard let emulator else { return }
        if port == 0 {
            emulator.setButtons(port: 0, mask: touchMask | padMasks[0])
        } else {
            emulator.setButtons(port: UInt32(port), mask: padMasks[port])
        }
    }

    // MARK: - Settings application

    private func applyFilter() {
        let p = filterParams(filter)
        emulator?.setFilter(filter, p0: p.0, p1: p.1, p2: p.2, p3: p.3)
    }

    /// The four `p0..p3` shader params for `filter`, in the order the renderer's
    /// pipelines expect (gfx_metal.rs): Scanlines = [intensity]; CRT = [intensity,
    /// mask]; NTSC = [saturation, sharpness, tint, phase]. None / Bisqwit run at
    /// neutral params (Bisqwit's per-frame phase + picture knobs are handled in the
    /// renderer via `set_index_frame` / aux, so it has no host-tunable sliders).
    func filterParams(_ filter: VideoFilter) -> (Float, Float, Float, Float) {
        switch filter {
        case .none: return (0, 0, 0, 0)
        case .scanlines: return (scanlineIntensity, 0, 0, 0)
        case .crt: return (scanlineIntensity, crtMask, 0, 0)
        case .ntsc: return (ntscSaturation, ntscSharpness, ntscTint, ntscPhase)
        case .bisqwit: return (0, 0, 0, 0)
        }
    }

    // MARK: - Save states

    func saveSlot(_ slot: Int) {
        guard let emulator, let sha = currentEntry?.sha else { return }
        let blob = emulator.saveState()
        let frame = emulator.frame()
        let thumbnail = emulator.snapshotPNG()
        try? saveStates.write(blob, sha: sha, slot: slot, frame: frame, thumbnailPNG: thumbnail)
    }

    func loadSlot(_ slot: Int) {
        guard let emulator, let sha = currentEntry?.sha,
              let data = saveStates.read(sha: sha, slot: slot) else { return }
        do {
            try emulator.loadState(data)
        } catch {
            errorMessage = "Load state failed: \(error.localizedDescription)"
        }
    }

    func deleteSlot(_ slot: Int) {
        guard let sha = currentEntry?.sha else { return }
        saveStates.clear(sha: sha, slot: slot)
    }

    func slots() -> [SaveSlot] {
        guard let sha = currentEntry?.sha else { return [] }
        return saveStates.slots(for: sha)
    }

    /// Quick-save / quick-load map to slot 1 (the desktop F1/F4 analogue).
    func quickSave() { saveSlot(SaveStateManager.quickSlot) }
    func quickLoad() { loadSlot(SaveStateManager.quickSlot) }

    // MARK: - App lifecycle (foreground/background + in-game-menu pause)

    /// Whether the scene is foreground-active and whether an in-game menu/sheet is
    /// holding the emulator paused. Tracked separately so neither clobbers the other
    /// (e.g. foregrounding with a sheet still open must NOT resume emulation).
    private var sceneActive = true
    private var menuPaused = false
    /// Sticky while an audio interruption (call/Siri) or a silencing route change is
    /// in effect; cleared by the matching "resume" event or by a fresh foreground.
    private var audioInterrupted = false

    func handleScenePhase(_ active: Bool) {
        sceneActive = active
        // A fresh foreground clears any stale audio gate: a route change (e.g.
        // headphones unplugged) pauses without ever emitting a "resume" event, so
        // without this the emulator could stay wedged after returning to the app.
        if active { audioInterrupted = false }
        applyRunState()
    }

    /// Pause/resume the emulator for an in-game menu (Settings / Save States), so it
    /// doesn't keep running (losing progress / playing audio) behind the sheet.
    func setMenuPaused(_ paused: Bool) {
        menuPaused = paused
        applyRunState()
    }

    /// Resume only when the scene is active AND no menu is open AND no audio
    /// interruption is in effect; otherwise pause. (We deliberately declare NO
    /// background-audio mode, so backgrounding pauses.)
    private func applyRunState() {
        if sceneActive, !menuPaused, !audioInterrupted {
            emulator?.resume()
        } else {
            emulator?.pause()
        }
    }
}
