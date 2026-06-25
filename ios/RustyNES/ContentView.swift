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
                if let url = urls.first { model.importAndOpen(url) }
            case .failure(let error):
                model.errorMessage = error.localizedDescription
            }
        }
        .sheet(isPresented: $showingSettings) {
            SettingsView()
        }
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
    }

    /// The importable UTTypes — ONLY the ROM / archive types, never `.data`
    /// (`public.data` would let the picker select any file and then fail at load).
    /// The custom .nes/.fds/.nsf types are declared in Info.plist
    /// (UTImportedTypeDeclarations); resolve them by extension so the picker shows
    /// them even before the system fully indexes the declarations.
    private var importableTypes: [UTType] {
        var types: [UTType] = [.zip]
        for ext in ["nes", "fds", "nsf"] {
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
                                    .onTapGesture { model.openGame(entry) }
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
                if entry.favorite {
                    VStack {
                        HStack {
                            Spacer()
                            Image(systemName: "star.fill")
                                .foregroundStyle(.yellow)
                                .padding(8)
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
