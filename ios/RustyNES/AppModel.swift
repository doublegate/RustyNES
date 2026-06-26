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

/// Small app-level errors surfaced to the user (v1.9.6).
enum AppError: LocalizedError {
    case noGame
    var errorDescription: String? {
        switch self {
        case .noGame: return "No game is running."
        }
    }
}

@MainActor
final class AppModel: ObservableObject {
    let library = ROMLibrary()
    let saveStates = SaveStateManager()
    let audioSession = AudioSession()

    /// iCloud (CloudKit) save-state sync (v1.9.7). Opt-in / off by default; gracefully
    /// no-ops when iCloud is unavailable. Mirrors `.rns` slots across the user's
    /// devices without ever blocking local save/load.
    lazy var cloudSaveStates = CloudSaveStateSync(saveStates: saveStates)

    // Power-user stores (v1.9.5).
    let palettes = PaletteManager()
    let hdpacks = HDPackStore()
    let movies = MovieManager()
    let overrides = GameOverrides()

    // Connectivity & scripting (v1.9.6). Both opt-in / off by default.
    let ra = RetroAchievementsModel()
    let netplay = NetplayModel()

    /// The currently-running game, or nil when on the library screen.
    @Published var emulator: EmulatorCore?
    /// The library entry backing `emulator` (for save-state keying / metadata).
    @Published var currentEntry: LibraryEntry?
    /// A transient error to surface to the user (load/import failures).
    @Published var errorMessage: String?

    // Settings (persisted lightly via UserDefaults + mirrored to iCloud, v1.9.5).
    @Published var filter: VideoFilter = .none {
        didSet {
            // During a cloud pull each property is set in turn; defer the (expensive)
            // re-apply to a single call at the end of `pullCloudConfig()`.
            if !isApplyingCloud { applyDisplaySettings() }
            UserDefaults.standard.set(Int(filter.rawValue), forKey: "filter")
            if !isApplyingCloud { cloud.setInt(Int(filter.rawValue), forKey: "filter") }
        }
    }
    @Published var muted: Bool = false {
        didSet {
            emulator?.isMuted = muted
            UserDefaults.standard.set(muted, forKey: "muted")
            if !isApplyingCloud { cloud.setBool(muted, forKey: "muted") }
        }
    }
    /// The global default palette id (an imported `.pal` stem), or "" for the
    /// built-in NES palette. Per-game overrides can pick a different palette.
    @Published var globalPaletteId: String = "" {
        didSet {
            if !isApplyingCloud { applyDisplaySettings() }
            UserDefaults.standard.set(globalPaletteId, forKey: "paletteId")
            if !isApplyingCloud { cloud.setString(globalPaletteId, forKey: "paletteId") }
        }
    }
    /// Whether a TAS movie is being recorded / played (reactive mirror of the core's
    /// `movieIsRecording` / `movieIsPlaying`, updated by the movie control methods).
    @Published var movieRecording = false
    @Published var moviePlaying = false

    // Per-filter shader parameters (the tunable knobs the renderer's filter pipelines
    // read). Each maps into the gfx `p0..p3` per `filterParams(_:)`, matching the
    // gfx_metal.rs param layout (Scanlines [intensity]; CRT [intensity, mask]; NTSC
    // [saturation, sharpness, tint, phase]). Defaults match the Android knob set.
    // `didSet` re-applies live to the running renderer and persists to UserDefaults.
    @Published var scanlineIntensity: Float = 0.5 {
        didSet { persistParam(scanlineIntensity, key: "scanlineIntensity") }
    }
    @Published var crtMask: Float = 0.10 {
        didSet { persistParam(crtMask, key: "crtMask") }
    }
    @Published var ntscSaturation: Float = 0.55 {
        didSet { persistParam(ntscSaturation, key: "ntscSaturation") }
    }
    @Published var ntscSharpness: Float = 0.08 {
        didSet { persistParam(ntscSharpness, key: "ntscSharpness") }
    }
    @Published var ntscTint: Float = 0.0 {
        didSet { persistParam(ntscTint, key: "ntscTint") }
    }
    @Published var ntscPhase: Float = 0.0 {
        didSet { persistParam(ntscPhase, key: "ntscPhase") }
    }
    /// Optional Core Haptics feedback on on-screen button presses (OFF by default).
    @Published var hapticsEnabled: Bool = false {
        didSet {
            haptics.isEnabled = hapticsEnabled
            if hapticsEnabled { haptics.prepare() }
            UserDefaults.standard.set(hapticsEnabled, forKey: "hapticsEnabled")
            if !isApplyingCloud { cloud.setBool(hapticsEnabled, forKey: "hapticsEnabled") }
        }
    }

    /// Persist one shader param: re-apply to the renderer, write UserDefaults, and
    /// mirror to iCloud (unless we are currently applying a remote change).
    private func persistParam(_ value: Float, key: String) {
        // Skip the per-knob re-apply during a cloud pull; `pullCloudConfig()`
        // applies once after all values are set.
        if !isApplyingCloud { applyDisplaySettings() }
        UserDefaults.standard.set(value, forKey: key)
        if !isApplyingCloud { cloud.setFloat(value, forKey: key) }
    }

    /// True while applying a remote iCloud change, so the property `didSet`s don't
    /// echo it back to the cloud store (which would fight a concurrent remote edit).
    private var isApplyingCloud = false

    /// The config keys mirrored to iCloud (the global Settings scalars — NOT
    /// save-states or ROMs, and NOT per-game overrides, which stay local).
    private static let cloudKeys = [
        "filter", "muted", "hapticsEnabled", "paletteId",
        "scanlineIntensity", "crtMask",
        "ntscSaturation", "ntscSharpness", "ntscTint", "ntscPhase",
    ]

    /// The iCloud key-value sync (v1.9.5). Lazy so it is created after the stored
    /// managers; pulls remote config on an external change. Gracefully no-ops when
    /// iCloud is unavailable.
    private lazy var cloud: CloudConfigSync = {
        let sync = CloudConfigSync(keys: Self.cloudKeys)
        // The store posts its change notification on the main queue, but hop to the
        // main actor explicitly (matching the audio-session callbacks) so the
        // `@MainActor` pull is well-formed under any concurrency checking.
        sync.onExternalChange = { [weak self] in
            Task { @MainActor in self?.pullCloudConfig() }
        }
        return sync
    }()

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
        // Suppress cloud echo while restoring the persisted local values (the
        // property `didSet`s would otherwise push the just-restored values back).
        isApplyingCloud = true
        if let raw = UserDefaults.standard.object(forKey: "filter") as? Int,
           let f = VideoFilter(rawValue: UInt8(raw)) {
            filter = f
        }
        muted = UserDefaults.standard.bool(forKey: "muted")
        hapticsEnabled = UserDefaults.standard.bool(forKey: "hapticsEnabled")
        globalPaletteId = UserDefaults.standard.string(forKey: "paletteId") ?? ""
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
        isApplyingCloud = false

        // Reconcile with iCloud at launch: pull any values another device wrote
        // (last-writer-wins — the cloud value wins when present). A no-op when
        // iCloud is unavailable. Done after local restore so a fresh install
        // adopts the user's synced config.
        pullCloudConfig()

        // Audio interruptions (phone call / Siri) and route changes (headphones
        // unplugged) flow through the run-state composition so they don't fight the
        // scene/menu pause gates: set a sticky `audioInterrupted` flag and recompute.
        // AVAudioSession interruption / route-change notifications are delivered on
        // an arbitrary (often background) thread, so hop to the main actor before
        // touching this `@MainActor` model.
        audioSession.onShouldPause = { [weak self] in
            Task { @MainActor in
                self?.audioInterrupted = true
                self?.applyRunState()
            }
        }
        audioSession.onShouldResume = { [weak self] in
            Task { @MainActor in
                self?.audioInterrupted = false
                self?.applyRunState()
            }
        }
        gamepads.onMaskChanged = { [weak self] port, mask in
            guard let self else { return }
            let p = Int(port)
            guard p >= 0, p < self.padMasks.count else { return }
            self.padMasks[p] = mask
            self.pushInput(port: p)
        }
        gamepads.start()

        // Check the iCloud account status at launch so the save-state sync indicator
        // and reconciliation are ready by the time a game opens (a no-op when the
        // sync toggle is off / iCloud is unavailable).
        cloudSaveStates.start()
    }

    // MARK: - Session lifecycle

    /// Open a library entry: read its bytes, build a fresh EmulatorCore, and start.
    func openGame(_ entry: LibraryEntry) {
        do {
            audioSession.configure()
            let data = try library.romData(for: entry)
            let core = try EmulatorCore(romData: data)
            core.isMuted = muted
            // Tear any prior session's connectivity state down before freeing its core
            // (RA persists per-game progress; netplay ends on a ROM swap).
            ra.detachFromGame()
            netplay.detach()
            emulator?.shutdown()
            emulator = core
            currentEntry = entry
            library.markPlayed(entry.sha, info: core.info)
            // Apply this game's per-game overrides if any, else the global defaults
            // (filter + shader params + palette + HD-pack).
            applyDisplaySettings()
            syncMovieState()
            // Wire connectivity & scripting into the fresh core (no-ops when disabled).
            ra.attach(core: core, romData: data, sha: entry.sha)
            netplay.attach(core: core)
            // Reconcile this game's cloud save-states (pull any newer-remote slots).
            cloudSaveStates.setCurrentGame(sha: entry.sha)
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
        // Persist RA progress + end any netplay session while the core is still alive.
        ra.detachFromGame()
        netplay.detach()
        cloudSaveStates.setCurrentGame(sha: nil)
        emulator?.shutdown()
        emulator = nil
        currentEntry = nil
        movieRecording = false
        moviePlaying = false
        audioSession.deactivate()
    }

    // MARK: - Lua scripting (v1.9.6)

    /// The last script text entered in the Lua console, persisted for convenience.
    var lastLuaScript: String {
        get { UserDefaults.standard.string(forKey: "luaLastScript") ?? "" }
        set { UserDefaults.standard.set(newValue, forKey: "luaLastScript") }
    }

    /// Load a Lua script into the running game (persisting the text).
    /// - Throws: `MobileError.script` if it fails to compile / load.
    func loadLuaScript(_ src: String) throws {
        guard let emulator else { throw AppError.noGame }
        // Persist the text BEFORE attempting the load so a failed compile (syntax
        // error) still keeps the user's edit; the load error is rethrown so the
        // caller can surface it.
        lastLuaScript = src
        try emulator.loadScript(src)
    }

    func unloadLuaScript() { emulator?.unloadScript() }
    var luaIsLoaded: Bool { emulator?.scriptIsLoaded ?? false }
    func drainLuaLog() -> [String] { emulator?.drainScriptLog() ?? [] }

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

    /// Apply the effective display settings (filter + shader params + palette +
    /// HD-pack) for the running game to the renderer/core: the per-game override if
    /// one exists, else the global defaults. Idempotent — safe to call after any
    /// settings change. A no-op when no game is running.
    func applyDisplaySettings() {
        guard let emulator else { return }
        let e = effectiveDisplay()
        emulator.setFilter(e.filter, p0: e.params.0, p1: e.params.1, p2: e.params.2, p3: e.params.3)
        // Palette: "" means the built-in NES palette; an unknown/missing id also
        // falls back to it.
        if e.paletteId.isEmpty {
            emulator.clearPalette()
        } else if let bytes = palettes.bytes(id: e.paletteId) {
            do {
                try emulator.loadPalette(bytes)
            } catch {
                // Revert to a known renderer state (built-in palette) so the UI
                // selection doesn't claim a palette that isn't actually loaded.
                emulator.clearPalette()
                errorMessage = "Could not load palette: \(error.localizedDescription)"
            }
        } else {
            emulator.clearPalette()
        }
        // HD-pack: "" means none; an unknown/missing id unloads.
        if e.hdpackId.isEmpty {
            emulator.unloadHDPack()
        } else if let bytes = hdpacks.bytes(id: e.hdpackId) {
            do {
                try emulator.loadHDPack(bytes)
            } catch {
                // Unload so a stale/failed pack isn't left active, and surface it.
                emulator.unloadHDPack()
                errorMessage = "Could not load HD-pack: \(error.localizedDescription)"
            }
        } else {
            emulator.unloadHDPack()
        }
    }

    /// The resolved display settings for the running game: the per-game override if
    /// present, else the global defaults.
    private func effectiveDisplay()
        -> (filter: VideoFilter, params: (Float, Float, Float, Float), paletteId: String, hdpackId: String) {
        if let sha = currentEntry?.sha, let o = overrides.settings(for: sha) {
            let f = VideoFilter(rawValue: o.filter) ?? .none
            return (
                f,
                Self.params(
                    for: f, scan: o.scanlineIntensity, mask: o.crtMask,
                    sat: o.ntscSaturation, sharp: o.ntscSharpness, tint: o.ntscTint, phase: o.ntscPhase
                ),
                o.paletteId, o.hdpackId
            )
        }
        return (filter, filterParams(filter), globalPaletteId, "")
    }

    /// The four `p0..p3` shader params for `filter`, in the order the renderer's
    /// pipelines expect (gfx_metal.rs): Scanlines = [intensity]; CRT = [intensity,
    /// mask]; NTSC = [saturation, sharpness, tint, phase]. None / Bisqwit run at
    /// neutral params (Bisqwit's per-frame phase + picture knobs are handled in the
    /// renderer via `set_index_frame` / aux, so it has no host-tunable sliders).
    func filterParams(_ filter: VideoFilter) -> (Float, Float, Float, Float) {
        Self.params(
            for: filter, scan: scanlineIntensity, mask: crtMask,
            sat: ntscSaturation, sharp: ntscSharpness, tint: ntscTint, phase: ntscPhase
        )
    }

    /// Pure mapping of named knob values to the renderer's `p0..p3` layout (shared
    /// by the global defaults and the per-game overrides).
    static func params(
        for filter: VideoFilter,
        scan: Float, mask: Float, sat: Float, sharp: Float, tint: Float, phase: Float
    ) -> (Float, Float, Float, Float) {
        switch filter {
        case .none: return (0, 0, 0, 0)
        case .scanlines: return (scan, 0, 0, 0)
        case .crt: return (scan, mask, 0, 0)
        case .ntsc: return (sat, sharp, tint, phase)
        case .bisqwit: return (0, 0, 0, 0)
        }
    }

    // MARK: - Per-game overrides (v1.9.5)

    /// Whether the running game has a per-game override.
    var currentGameHasOverride: Bool {
        guard let sha = currentEntry?.sha else { return false }
        return overrides.has(sha)
    }

    /// The running game's override, or nil.
    var currentGameOverride: GameDisplaySettings? {
        guard let sha = currentEntry?.sha else { return nil }
        return overrides.settings(for: sha)
    }

    /// Enable a per-game override for the running game, seeded from the current
    /// global defaults, then apply it.
    func enableCurrentGameOverride() {
        guard let sha = currentEntry?.sha else { return }
        let seed = GameDisplaySettings(
            filter: filter.rawValue,
            scanlineIntensity: scanlineIntensity,
            crtMask: crtMask,
            ntscSaturation: ntscSaturation,
            ntscSharpness: ntscSharpness,
            ntscTint: ntscTint,
            ntscPhase: ntscPhase,
            paletteId: globalPaletteId,
            hdpackId: ""
        )
        overrides.set(seed, for: sha)
        applyDisplaySettings()
    }

    /// Replace the running game's override and re-apply.
    func updateCurrentGameOverride(_ settings: GameDisplaySettings) {
        guard let sha = currentEntry?.sha else { return }
        overrides.set(settings, for: sha)
        applyDisplaySettings()
    }

    /// Remove the running game's override (revert to the global defaults) and
    /// re-apply.
    func clearCurrentGameOverride() {
        guard let sha = currentEntry?.sha else { return }
        overrides.clear(for: sha)
        applyDisplaySettings()
    }

    // MARK: - TAS movies (v1.9.5)

    func startMovieRecordFromPowerOn() {
        emulator?.movieRecordFromPowerOn()
        syncMovieState()
    }

    func startMovieRecordFromHere() {
        emulator?.movieRecordFromHere()
        syncMovieState()
    }

    /// Stop recording, save the `.rnm` to the sandbox under the game's name.
    func stopAndSaveMovie() {
        guard let emulator else { return }
        let bytes = emulator.movieStopRecording()
        syncMovieState()
        guard !bytes.isEmpty else { return }
        let name = currentEntry?.name ?? "movie"
        do {
            try movies.save(bytes, gameName: name)
        } catch {
            errorMessage = "Could not save movie: \(error.localizedDescription)"
        }
    }

    /// Load + play a saved or imported `.rnm` at `url`. The file read can block on a
    /// large or security-scoped file, so it runs off the main actor; the core call +
    /// state update hop back to the main actor.
    func playMovie(at url: URL) {
        Task {
            let scoped = url.startAccessingSecurityScopedResource()
            let data: Data
            do {
                data = try await Task.detached(priority: .userInitiated) {
                    try Data(contentsOf: url)
                }.value
            } catch {
                if scoped { url.stopAccessingSecurityScopedResource() }
                self.errorMessage = "Could not read movie file: \(error.localizedDescription)"
                return
            }
            if scoped { url.stopAccessingSecurityScopedResource() }
            do {
                try self.emulator?.moviePlay(data)
                self.syncMovieState()
            } catch {
                self.errorMessage = "Movie playback failed: \(error.localizedDescription)"
            }
        }
    }

    func stopMovie() {
        emulator?.movieStop()
        syncMovieState()
    }

    /// Refresh the reactive `movieRecording` / `moviePlaying` mirrors from the core.
    private func syncMovieState() {
        movieRecording = emulator?.movieIsRecording ?? false
        moviePlaying = emulator?.movieIsPlaying ?? false
    }

    // MARK: - iCloud config sync (v1.9.5)

    /// Pull the cloud config values into the local model (last-writer-wins: a
    /// present cloud value overrides the local one). Guarded so the resulting
    /// property `didSet`s don't echo back to the cloud store.
    private func pullCloudConfig() {
        isApplyingCloud = true
        if let raw = cloud.int(forKey: "filter"), let f = VideoFilter(rawValue: UInt8(clamping: raw)) {
            filter = f
            UserDefaults.standard.set(raw, forKey: "filter")
        }
        if let v = cloud.bool(forKey: "muted") {
            muted = v
            UserDefaults.standard.set(v, forKey: "muted")
        }
        if let v = cloud.bool(forKey: "hapticsEnabled") {
            hapticsEnabled = v
            UserDefaults.standard.set(v, forKey: "hapticsEnabled")
        }
        if let v = cloud.string(forKey: "paletteId") {
            globalPaletteId = v
            UserDefaults.standard.set(v, forKey: "paletteId")
        }
        func pullFloat(_ key: String, _ assign: (Float) -> Void) {
            if let v = cloud.float(forKey: key) {
                assign(v)
                UserDefaults.standard.set(v, forKey: key)
            }
        }
        pullFloat("scanlineIntensity") { scanlineIntensity = $0 }
        pullFloat("crtMask") { crtMask = $0 }
        pullFloat("ntscSaturation") { ntscSaturation = $0 }
        pullFloat("ntscSharpness") { ntscSharpness = $0 }
        pullFloat("ntscTint") { ntscTint = $0 }
        pullFloat("ntscPhase") { ntscPhase = $0 }
        // Clear the guard, then apply the merged settings to a running game ONCE
        // (the per-property `didSet`s skipped their own re-apply above).
        isApplyingCloud = false
        applyDisplaySettings()
    }

    // MARK: - Save states

    func saveSlot(_ slot: Int) {
        guard let emulator, let sha = currentEntry?.sha else { return }
        let blob = emulator.saveState()
        let frame = emulator.frame()
        let thumbnail = emulator.snapshotPNG()
        try? saveStates.write(blob, sha: sha, slot: slot, frame: frame, thumbnailPNG: thumbnail)
        // Mirror to iCloud in the background (no-op when sync is off / unavailable).
        cloudSaveStates.upload(sha: sha, slot: slot)
    }

    func loadSlot(_ slot: Int) {
        // RetroAchievements hardcore mode forbids loading save-states (it would
        // invalidate a hardcore run). Refuse and explain, matching the desktop rule.
        if ra.enabled, ra.hardcore, ra.isLoggedIn {
            errorMessage = "Loading save-states is disabled in RetroAchievements hardcore mode."
            return
        }
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
        // Remove the cloud record too (best-effort; no-op when sync is off).
        cloudSaveStates.delete(sha: sha, slot: slot)
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
