//
//  GameCenterModel.swift  (v1.9.8 "Horizon")
//
//  Opt-in, light Game Center integration: `GKLocalPlayer` sign-in (off by default,
//  behind a Settings toggle) plus the `GKAccessPoint` presence badge. It is auth +
//  presence only — RustyNES defines NO leaderboards or achievements of its own, so
//  there is nothing to submit here. The real achievement system is RetroAchievements
//  (v1.9.6, `RetroAchievementsModel`); Game Center is a complementary, optional Apple-
//  account sign-in. A `GKSavedGame` hook is noted below as a documented FUTURE
//  alternative to the v1.9.7 CloudKit save-state sync (`CloudSaveStateSync`), not wired
//  this release.
//
//  Privacy: signing in to Game Center authenticates the user's Apple Game Center
//  account (an Apple service). It is opt-in / default-off and discloses an "Apple
//  account" data use in `PrivacyInfo.xcprivacy`. The base app, with this toggle off,
//  makes no Game Center calls.
//
//  Concurrency: GameKit may invoke `authenticateHandler` on a background thread, so the
//  @MainActor state is only mutated inside a `Task { @MainActor in }` hop. When Game
//  Center is unavailable (no account / Simulator / network) the handler reports an
//  error and we no-op gracefully.
//

import Foundation
import GameKit
import SwiftUI

/// A pending Game Center sign-in controller to present.
struct GameCenterAuthRequest: Identifiable {
    let id = UUID()
    let controller: UIViewController
}

@MainActor
final class GameCenterModel: NSObject, ObservableObject {
    /// Master opt-in. Off by default; when off, no Game Center calls are made and the
    /// access point is hidden.
    @Published var enabled: Bool {
        didSet {
            UserDefaults.standard.set(enabled, forKey: "gameCenterEnabled")
            if enabled {
                authenticate()
            } else {
                // Tear down so a previously-installed handler can't fire later
                // (e.g. after returning from Settings) and re-present UI or mutate
                // published state. Clear the system handler, hide the access point,
                // and reset published state so the UI updates immediately.
                GKLocalPlayer.local.authenticateHandler = nil
                GKAccessPoint.shared.isActive = false
                authRequest = nil
                isAuthenticated = false
                playerName = nil
                lastError = nil
            }
        }
    }

    @Published private(set) var isAuthenticated = false
    @Published private(set) var playerName: String?
    @Published var lastError: String?
    /// Set when GameKit asks us to present its sign-in UI; cleared once presented.
    @Published var authRequest: GameCenterAuthRequest?

    override init() {
        enabled = UserDefaults.standard.bool(forKey: "gameCenterEnabled")
        super.init()
    }

    /// Authenticate at launch if the user previously opted in. A no-op when disabled.
    func start() {
        guard enabled else { return }
        authenticate()
    }

    /// Begin (or refresh) Game Center authentication. Installing the handler triggers
    /// the system flow; GameKit may call it again later (e.g. after returning from
    /// Settings), so it is safe to set more than once.
    func authenticate() {
        let local = GKLocalPlayer.local
        local.authenticateHandler = { [weak self] viewController, error in
            Task { @MainActor in
                guard let self else { return }
                // The user may have toggled Game Center off while a system-triggered
                // auth callback was in flight; bail before presenting UI or touching
                // published state so a disabled toggle stays disabled.
                guard self.enabled else { return }
                if let viewController {
                    // GameKit needs the host to present its sign-in UI.
                    self.authRequest = GameCenterAuthRequest(controller: viewController)
                    return
                }
                if let error {
                    self.isAuthenticated = false
                    self.playerName = nil
                    self.lastError = error.localizedDescription
                    return
                }
                self.isAuthenticated = local.isAuthenticated
                self.playerName = local.isAuthenticated ? local.displayName : nil
                if local.isAuthenticated { self.showAccessPoint() }
            }
        }
    }

    /// Show the Game Center access point (the small profile badge) while signed in and
    /// opted in. Top-trailing so it does not collide with the in-game pill menu.
    private func showAccessPoint() {
        guard enabled else { return }
        GKAccessPoint.shared.location = .topTrailing
        GKAccessPoint.shared.showHighlights = false
        GKAccessPoint.shared.isActive = true
    }

    // FUTURE (documented, not wired this release): `GKSavedGame` would let signed-in
    // players sync save-states through their Game Center account as an ALTERNATIVE to
    // the v1.9.7 CloudKit path (`CloudSaveStateSync`). It is intentionally left as a
    // future option so this release stays "auth + presence only".
}

/// An invisible host that presents the Game Center sign-in controller when GameKit
/// requests it. Drop it into the root view; it observes the model so the cover follows
/// the published `authRequest`.
struct GameCenterAuthHost: View {
    @ObservedObject var model: GameCenterModel

    var body: some View {
        Color.clear
            .allowsHitTesting(false)
            .fullScreenCover(item: $model.authRequest) { request in
                GameCenterAuthController(controller: request.controller)
                    .ignoresSafeArea()
            }
    }
}

/// Hosts the GameKit-provided sign-in `UIViewController` inside a SwiftUI cover.
private struct GameCenterAuthController: UIViewControllerRepresentable {
    let controller: UIViewController
    func makeUIViewController(context: Context) -> UIViewController { controller }
    func updateUIViewController(_ uiViewController: UIViewController, context: Context) {}
}
