//
//  RustyNESApp.swift
//
//  The app entry point. Owns the shared AppModel and wires ScenePhase so the
//  emulator pauses when the app is backgrounded and resumes on return (we declare
//  NO background-audio mode by design, so playback stops in the background).
//

import SwiftUI

@main
struct RustyNESApp: App {
    @StateObject private var model = AppModel()
    // v1.9.1: the dormant freemium gate, injected app-wide so the v2.1.0
    // monetization wiring is a drop-in. Fully unlocked through v1.9.x.
    @StateObject private var entitlements = Entitlements()
    @Environment(\.scenePhase) private var scenePhase

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(model)
                .environmentObject(entitlements)
                // Open a ROM dragged onto the app / shared from Files (.nes/.fds/
                // .nsf are registered in Info.plist).
                .onOpenURL { url in
                    model.importAndOpen(url)
                }
                .task { entitlements.refresh() }
        }
        .onChange(of: scenePhase) { phase in
            model.handleScenePhase(phase == .active)
        }
    }
}
