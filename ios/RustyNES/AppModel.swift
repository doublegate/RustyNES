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

    private let gamepads = GameControllerManager()
    private var touchMask: UInt8 = 0
    private var padMask: UInt8 = 0

    init() {
        if let raw = UserDefaults.standard.object(forKey: "filter") as? Int,
           let f = VideoFilter(rawValue: UInt8(raw)) {
            filter = f
        }
        muted = UserDefaults.standard.bool(forKey: "muted")

        audioSession.onShouldPause = { [weak self] in self?.emulator?.pause() }
        audioSession.onShouldResume = { [weak self] in
            if self?.emulator != nil { self?.emulator?.resume() }
        }
        gamepads.onMaskChanged = { [weak self] _, mask in
            self?.padMask = mask
            self?.pushInput()
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

    /// The touch overlay reports its mask here; merged with the hardware-pad mask.
    func setTouchMask(_ mask: UInt8) {
        touchMask = mask
        pushInput()
    }

    private func pushInput() {
        emulator?.setButtons(port: 0, mask: touchMask | padMask)
    }

    // MARK: - Settings application

    private func applyFilter() {
        emulator?.setFilter(filter)
    }

    // MARK: - Save states

    func saveSlot(_ slot: Int) {
        guard let emulator, let sha = currentEntry?.sha else { return }
        try? saveStates.write(emulator.saveState(), sha: sha, slot: slot)
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

    func slots() -> [SaveSlot] {
        guard let sha = currentEntry?.sha else { return [] }
        return saveStates.slots(for: sha)
    }

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
