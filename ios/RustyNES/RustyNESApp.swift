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
    @Environment(\.scenePhase) private var scenePhase

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(model)
                // Open a ROM dragged onto the app / shared from Files (.nes/.fds/
                // .nsf are registered in Info.plist).
                .onOpenURL { url in
                    model.importAndOpen(url)
                }
        }
        .onChange(of: scenePhase) { phase in
            model.handleScenePhase(phase == .active)
        }
    }
}
