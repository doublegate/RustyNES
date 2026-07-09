//
//  ContentView.swift
//
//  Top-level navigation: the library (when no game is running) and the player
//  (when one is). Mirrors the Android adaptive shell — a library grid as the idle
//  screen, and the game view when a ROM is loaded.
//

import SwiftUI
import UniformTypeIdentifiers

struct ContentView: View {
    @EnvironmentObject private var model: AppModel
    @State private var showingImporter = false
    @State private var showingSettings = false
    // First-run onboarding gate: set true by "Get Started" / "Skip", never shown
    // again once true (persisted in UserDefaults).
    @AppStorage("didOnboard") private var didOnboard = false

    var body: some View {
        Group {
            if model.emulator != nil {
                GameView()
            } else {
                LibraryScreen(
                    showingImporter: $showingImporter,
                    showingSettings: $showingSettings
                )
            }
        }
        // ROM import: .nes / .fds / .nsf plus zip archives (the same set the
        // Info.plist document types register). User-provided ROMs ONLY.
        .fileImporter(
            isPresented: $showingImporter,
            allowedContentTypes: importableTypes,
            allowsMultipleSelection: false
        ) { result in
            switch result {
            case .success(let urls):
                if let url = urls.first { Task { await model.importAndOpen(url) } }
            case .failure(let error):
                model.errorMessage = error.localizedDescription
            }
        }
        .sheet(isPresented: $showingSettings) {
            SettingsView()
        }
        // Presents the GameKit sign-in controller when Game Center requests it (v1.9.8),
        // available from the library and in-game alike.
        .background(GameCenterAuthHost(model: model.gameCenter))
        .fullScreenCover(isPresented: Binding(
            // Symmetric: presenting (true) means not-yet-onboarded; any dismissal
            // (false) marks onboarding done, so the cover can't re-present in a loop.
            get: { !didOnboard },
            set: { didOnboard = !$0 }
        )) {
            OnboardingView { didOnboard = true }
                // Completion is via Skip / Get Started, not an accidental swipe.
                .interactiveDismissDisabled()
        }
        .alert(
            "RustyNES",
            isPresented: Binding(
                get: { model.errorMessage != nil },
                set: { if !$0 { model.errorMessage = nil } }
            ),
            actions: { Button("OK", role: .cancel) {} },
            message: { Text(model.errorMessage ?? "") }
        )
        // A distinct, non-blocking notice channel (v2.0.5): a succeeded operation
        // the host wants to caveat — currently a pre-Timebase `.rnm` load warning.
        // Separate from the error alert so a warning never reads as a failure.
        .alert(
            "RustyNES",
            isPresented: Binding(
                get: { model.warningMessage != nil },
                set: { if !$0 { model.warningMessage = nil } }
            ),
            actions: { Button("OK", role: .cancel) {} },
            message: { Text(model.warningMessage ?? "") }
        )
    }

    /// The importable UTTypes — ONLY the ROM / archive types, never `.data`
    /// (`public.data` would let the picker select any file and then fail at load).
    /// The mobile bridge is iNES/NES 2.0-only (no FDS/NSF load path), so the picker
    /// advertises only `.nes` (+ `.zip`) — advertising `.fds`/`.nsf` would let the
    /// user pick a file the core cannot load. The custom `.nes` type is declared in
    /// Info.plist (UTImportedTypeDeclarations); resolve it by extension so the picker
    /// shows it even before the system fully indexes the declarations.
    private var importableTypes: [UTType] {
        var types: [UTType] = [.zip]
        for ext in ["nes"] {
            if let t = UTType(filenameExtension: ext) { types.append(t) }
        }
        return types
    }
}

/// The library grid + the empty-state import prompt.
private struct LibraryScreen: View {
    @EnvironmentObject private var model: AppModel
    @Binding var showingImporter: Bool
    @Binding var showingSettings: Bool

    private let columns = [GridItem(.adaptive(minimum: 140), spacing: 16)]

    var body: some View {
        NavigationStack {
            Group {
                if model.library.entries.isEmpty {
                    emptyState
                } else {
                    ScrollView {
                        LazyVGrid(columns: columns, spacing: 16) {
                            ForEach(model.library.entries) { entry in
                                LibraryCell(entry: entry)
                                    .onTapGesture { Task { await model.openGame(entry) } }
                                    // One VoiceOver element per tile (the placeholder art
                                    // is decorative), activatable to open the game.
                                    .accessibilityElement(children: .combine)
                                    .accessibilityAddTraits(.isButton)
                                    .accessibilityHint(Text("Opens this game"))
                                    .contextMenu {
                                        Button(entry.favorite ? "Unfavorite" : "Favorite") {
                                            model.library.toggleFavorite(entry.sha)
                                        }
                                        Button("Remove", role: .destructive) {
                                            model.library.remove(entry.sha)
                                        }
                                    }
                            }
                        }
                        .padding()
                    }
                }
            }
            .navigationTitle("RustyNES")
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button { showingImporter = true } label: {
                        Image(systemName: "plus")
                    }
                    .accessibilityLabel("Import ROM")
                }
                ToolbarItem(placement: .topBarLeading) {
                    Button { showingSettings = true } label: {
                        Image(systemName: "gearshape")
                    }
                    .accessibilityLabel("Settings")
                }
            }
        }
    }

    private var emptyState: some View {
        VStack(spacing: 16) {
            Image(systemName: "gamecontroller")
                .font(.system(size: 56))
                .foregroundStyle(.secondary)
            Text("No games yet")
                .font(.title2.bold())
            Text("Import a .nes ROM you own from Files to start playing.")
                .font(.body)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 40)
            Button("Import ROM") { showingImporter = true }
                .buttonStyle(.borderedProminent)
        }
    }
}

/// One library tile (placeholder art + name; box art is a future maintainer task).
private struct LibraryCell: View {
    let entry: LibraryEntry

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ZStack {
                RoundedRectangle(cornerRadius: 10)
                    .fill(Color.secondary.opacity(0.18))
                    .aspectRatio(1.0, contentMode: .fit)
                Image(systemName: "tv")
                    .font(.system(size: 34))
                    .foregroundStyle(.secondary)
                    .accessibilityHidden(true)
                if entry.favorite {
                    VStack {
                        HStack {
                            Spacer()
                            Image(systemName: "star.fill")
                                .foregroundStyle(.yellow)
                                .padding(8)
                                .accessibilityLabel("Favorite")
                        }
                        Spacer()
                    }
                }
            }
            Text(entry.name)
                .font(.subheadline)
                .lineLimit(1)
            if entry.mapper >= 0 {
                Text("Mapper \(entry.mapper) - \(entry.region)")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
        }
    }
}
