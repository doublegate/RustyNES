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
        // A property's `didSet` does NOT run for in-init assignment, so sync the
        // haptics engine to the persisted value explicitly (otherwise a stored
        // `true` would leave the generator unprepared until the user re-toggles).
        haptics.isEnabled = hapticsEnabled

        audioSession.onShouldPause = { [weak self] in self?.emulator?.pause() }
        audioSession.onShouldResume = { [weak self] in
            if self?.emulator != nil { self?.emulator?.resume() }
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
        emulator?.setFilter(filter)
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

    // MARK: - App lifecycle (foreground/background pause)

    func handleScenePhase(_ active: Bool) {
        if active {
            emulator?.resume()
        } else {
            // The emulator pauses on background (we deliberately declare NO
            // background-audio mode).
            emulator?.pause()
        }
    }
}
