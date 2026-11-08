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
    // v3.7.0 (D24 row 2): gives the external-display scene its delegate
    // (ExternalDisplay.swift); every other scene keeps SwiftUI's configuration.
    @UIApplicationDelegateAdaptor(RustyNESAppDelegate.self) private var appDelegate
    @Environment(\.scenePhase) private var scenePhase

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(model)
                // Open a ROM dragged onto the app / shared from Files (.nes, .fds,
                // .nsf / .nsfe and .zip are registered in Info.plist).
                .onOpenURL { url in
                    // v3.7.0 (D24 row 1): the home-screen widget's deep link
                    // (`rustynes://resume`) reopens the last-played game.
                    if ResumeWidgetLink.isResume(url) {
                        Task { await model.resumeLastPlayed() }
                    } else {
                        Task { await model.importAndOpen(url) }
                    }
                }
        }
        .onChange(of: scenePhase) { phase in
            model.handleScenePhase(phase == .active)
        }
    }
}
