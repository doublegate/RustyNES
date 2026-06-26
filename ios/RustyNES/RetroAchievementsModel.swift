//
//  RetroAchievementsModel.swift
//
//  The RetroAchievements (RA) session state + lifecycle (v1.9.6), opt-in and OFF by
//  default. RA is an account login to a third-party service; the login token is kept
//  in the Keychain (never UserDefaults / iCloud), and the feature is gated behind an
//  explicit `enabled` toggle.
//
//  Privacy (maintainer carryover): RA is an account login + data exchange with a
//  third party. Before this ships, PrivacyInfo.xcprivacy must disclose the account-
//  login data collection. The feature stays opt-in / default-off until then.
//
//  iOS lifecycle note: the `NesController` (and thus the RA session) is rebuilt per
//  game here, unlike Android's long-lived controller, so the login does not survive a
//  game swap. We re-establish it on every `openGame`: token re-login from the Keychain
//  + `raLoadGame`. RA reconciliation (login completion, unlock checks, toast / rich-
//  presence refresh) runs inside the core's `post_frame_ra`, which only ticks while
//  the emulator runs; the poll timer pumps a paused login (Settings sheet) via
//  `EmulatorCore.pumpForLogin`.
//

import Combine
import Foundation

@MainActor
final class RetroAchievementsModel: ObservableObject {
    /// Master opt-in. Off by default; when off, no RA calls are made.
    @Published var enabled: Bool {
        didSet { UserDefaults.standard.set(enabled, forKey: "raEnabled") }
    }
    /// Hardcore mode (no save-state loading, no rewind). Off by default.
    @Published var hardcore: Bool {
        didSet {
            UserDefaults.standard.set(hardcore, forKey: "raHardcore")
            core?.raSetHardcore(hardcore)
        }
    }

    /// The login username (persisted for token re-login). Display info comes from `user`.
    @Published private(set) var username: String
    @Published private(set) var status: RaLoginStatus = .loggedOut
    @Published private(set) var user: RaUserInfo?
    /// The live HUD toasts (the bridge TTLs these out; assign unconditionally).
    @Published private(set) var toasts: [RaToast] = []
    @Published private(set) var richPresence: String = ""
    @Published private(set) var achievements: [RaAchievementInfo] = []
    /// Flat `[core, unofficial, unlocked, unsupported, pointsCore, pointsUnlocked]`.
    @Published private(set) var summary: [UInt32] = []
    /// A transient error surfaced to the RA UI (e.g. a failed login).
    @Published var lastError: String?

    private weak var core: EmulatorCore?
    private var currentSha: String?
    /// The current ROM's bytes, retained so a mid-session login can load its
    /// achievement set (`loadGame`) without reopening the game. Cleared on detach.
    private var currentRomData: Data?
    /// True once `raLoadGame` has succeeded for the current ROM. Drives teardown so
    /// `detachFromGame` unloads + persists regardless of the live `enabled` toggle
    /// (RA disabled mid-game must still flush + unload the loaded set).
    private var isGameLoaded = false
    /// The token currently persisted to the Keychain / UserDefaults. The 4 Hz poll
    /// only re-persists when the token actually changes, avoiding a per-tick Keychain
    /// write storm (delete+add = disk I/O + security-daemon IPC).
    private var activeToken: String?
    private var pollTimer: Timer?

    private let tokenAccount = "retroachievements.token"
    private let fileManager = FileManager.default

    var hasStoredToken: Bool { Keychain.has(account: tokenAccount) }
    var isLoggedIn: Bool { status == .loggedIn }
    /// Earned / total core achievements (from `raGameSummary`).
    var earned: Int { summary.count > 2 ? Int(summary[2]) : 0 }
    var total: Int { summary.count > 0 ? Int(summary[0]) : 0 }

    init() {
        enabled = UserDefaults.standard.bool(forKey: "raEnabled")
        hardcore = UserDefaults.standard.bool(forKey: "raHardcore")
        username = UserDefaults.standard.string(forKey: "raUsername") ?? ""
    }

    // MARK: - Session lifecycle (driven by AppModel.openGame / closeGame)

    /// Wire RA into a freshly-opened game: seed hardcore, re-login from the stored
    /// token (if any), identify the ROM, and start polling. A no-op when disabled.
    func attach(core: EmulatorCore, romData: Data, sha: String) {
        self.core = core
        currentSha = sha
        currentRomData = romData
        guard enabled else { stopPolling(); return }
        core.raInit(hardcore: hardcore)
        if let token = Keychain.get(account: tokenAccount), !username.isEmpty {
            // Already in the Keychain — record it as the active token so the poll
            // doesn't re-persist it every tick.
            activeToken = token
            core.raLoginToken(user: username, token: token)
        }
        loadGame(romData: romData, sha: sha)
        startPolling()
    }

    /// Begin identifying + loading the achievement set for the current ROM.
    private func loadGame(romData: Data, sha: String) {
        guard enabled, let core else { return }
        let digest = RomIdentity.sha256Bytes(romData)
        let sidecar = readSidecar(sha: sha) ?? Data()
        do {
            try core.raLoadGame(rom: romData, sha256: digest, sidecar: sidecar)
            isGameLoaded = true
        } catch {
            lastError = "RetroAchievements could not load this game: \(error.localizedDescription)"
        }
    }

    /// Persist progress + unload the game's set before the core is torn down.
    func detachFromGame() {
        // Persist + unload whenever a set was actually loaded, regardless of the live
        // `enabled` toggle — disabling RA mid-game must still flush progress + unload.
        if isGameLoaded, let core, let sha = currentSha {
            let blob = core.raSerializeProgress()
            if !blob.isEmpty { writeSidecar(blob, sha: sha) }
            core.raUnloadGame()
        }
        isGameLoaded = false
        stopPolling()
        core = nil
        currentSha = nil
        currentRomData = nil
        // Clear per-game live views (login state stays as-is for the next game).
        toasts = []
        achievements = []
        summary = []
        richPresence = ""
    }

    // MARK: - Login

    /// Begin a username + password login (requires a running game on iOS, since the
    /// RA session lives on the per-game controller). On success the poll stores the
    /// returned token in the Keychain.
    func loginPassword(user: String, password: String) {
        guard let core else {
            lastError = "Open a game first to sign in to RetroAchievements."
            return
        }
        enabled = true // didSet persists raEnabled
        username = user
        UserDefaults.standard.set(user, forKey: "raUsername")
        core.raInit(hardcore: hardcore)
        core.raLoginPassword(user: user, password: password)
        // Load the current ROM's achievement set if it wasn't loaded at attach time
        // (RA was disabled / not signed in then). The unlock state reconciles once the
        // login lands inside `post_frame_ra`; reuse the same `raLoadGame` as `attach`.
        if !isGameLoaded, let romData = currentRomData, let sha = currentSha {
            loadGame(romData: romData, sha: sha)
        }
        startPolling()
    }

    /// Log out, clear the stored token, and reset the displayed login state.
    func logout() {
        core?.raLogout()
        Keychain.delete(account: tokenAccount)
        activeToken = nil
        UserDefaults.standard.removeObject(forKey: "raUsername")
        username = ""
        user = nil
        status = .loggedOut
        achievements = []
        summary = []
        richPresence = ""
        toasts = []
    }

    // MARK: - Polling

    private func startPolling() {
        guard pollTimer == nil else { return }
        // ~4 Hz: cheap snapshot reads; the bridge does the work each frame.
        let timer = Timer(timeInterval: 0.25, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.poll() }
        }
        RunLoop.main.add(timer, forMode: .common)
        pollTimer = timer
    }

    private func stopPolling() {
        pollTimer?.invalidate()
        pollTimer = nil
    }

    private func poll() {
        guard enabled, let core else { stopPolling(); return }
        status = core.raLoginStatus()
        user = core.raUser()
        toasts = core.raPollToasts()
        richPresence = core.raRichPresence()
        achievements = core.raAchievementList()
        summary = core.raGameSummary()

        // While a login is in flight behind a paused Settings sheet, pump the core so
        // `post_frame_ra` can reconcile it (the display link is paused, so no tick
        // otherwise). Bounded to the brief login handshake.
        if status == .loggingIn, !core.isRunning {
            core.pumpForLogin()
        }

        // On a completed login, persist the returned token for token re-login next
        // game — but ONLY when it changes, not every poll tick (a Keychain delete+add
        // each tick is disk I/O + security-daemon IPC).
        if status == .loggedIn, let token = core.raToken(), token != activeToken {
            Keychain.set(token, account: tokenAccount)
            if let u = user { username = u.username }
            UserDefaults.standard.set(username, forKey: "raUsername")
            activeToken = token
        }
    }

    // MARK: - Per-game progress sidecar

    private var sidecarDir: URL {
        let base = fileManager.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent("RustyNES/ra-progress", isDirectory: true)
    }

    private func sidecarURL(sha: String) -> URL {
        sidecarDir.appendingPathComponent("\(sha).bin")
    }

    private func readSidecar(sha: String) -> Data? {
        try? Data(contentsOf: sidecarURL(sha: sha))
    }

    private func writeSidecar(_ data: Data, sha: String) {
        try? fileManager.createDirectory(at: sidecarDir, withIntermediateDirectories: true)
        try? data.write(to: sidecarURL(sha: sha), options: .atomic)
    }
}
