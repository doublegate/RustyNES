//
//  MoviePanelView.swift
//
//  The in-game TAS / movie control surface (v1.9.5): record a `.rnm` movie (from
//  power-on or from the current state), stop + save it to the sandbox, play a saved
//  or imported movie, and stop playback. Saved movies can be shared (export) via the
//  system share sheet, and external `.rnm` files imported through the document
//  picker. Reached from the in-game pill menu.
//

import SwiftUI
import UniformTypeIdentifiers

struct MoviePanelView: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.dismiss) private var dismiss

    @State private var showingImporter = false
    @State private var shareItem: ShareItem?

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    statusRow
                } header: {
                    Text("Status")
                }

                Section {
                    Button {
                        model.startMovieRecordFromPowerOn()
                    } label: {
                        Label("Record from power-on", systemImage: "record.circle")
                    }
                    .disabled(model.movieRecording)

                    Button {
                        model.startMovieRecordFromHere()
                    } label: {
                        Label("Record from here", systemImage: "record.circle.fill")
                    }
                    .disabled(model.movieRecording)

                    Button {
                        model.stopAndSaveMovie()
                    } label: {
                        Label("Stop & save recording", systemImage: "stop.circle")
                    }
                    .disabled(!model.movieRecording)
                } header: {
                    Text("Record")
                } footer: {
                    Text("Recording captures the input stream; replay is deterministic.")
                }

                Section {
                    Button {
                        showingImporter = true
                    } label: {
                        Label("Import & play a .rnm file", systemImage: "square.and.arrow.down")
                    }
                    Button {
                        model.stopMovie()
                    } label: {
                        Label("Stop playback", systemImage: "stop.fill")
                    }
                    .disabled(!model.moviePlaying)
                } header: {
                    Text("Play")
                }

                SavedMoviesSection(manager: model.movies, shareItem: $shareItem)
            }
            .navigationTitle("TAS / Movies")
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
            .fileImporter(
                isPresented: $showingImporter,
                allowedContentTypes: MovieTypes.importable,
                allowsMultipleSelection: false
            ) { result in
                if case .success(let urls) = result, let url = urls.first {
                    model.playMovie(at: url)
                }
            }
            .sheet(item: $shareItem) { item in
                ShareSheet(items: [item.url])
            }
        }
    }

    private var statusRow: some View {
        HStack {
            if model.movieRecording {
                Image(systemName: "record.circle").foregroundStyle(.red)
                Text("Recording")
            } else if model.moviePlaying {
                Image(systemName: "play.circle").foregroundStyle(.green)
                Text("Playing")
            } else {
                Image(systemName: "circle").foregroundStyle(.secondary)
                Text("Idle")
            }
            Spacer()
        }
    }

}

/// The saved-movies list. A child view so it observes `MovieManager` directly
/// (AppModel holds it but does not re-publish its changes), refreshing when a
/// recording is saved or a movie is deleted.
private struct SavedMoviesSection: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.dismiss) private var dismiss
    @ObservedObject var manager: MovieManager
    @Binding var shareItem: ShareItem?

    var body: some View {
        if !manager.movies.isEmpty {
            Section {
                ForEach(manager.movies) { movie in
                    movieRow(movie)
                }
            } header: {
                Text("Saved movies")
            }
        }
    }

    private func movieRow(_ movie: MovieFile) -> some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(movie.name).lineLimit(1)
                if let date = movie.savedAt {
                    Text(date.formatted(date: .abbreviated, time: .shortened))
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }
            Spacer()
            Button {
                model.playMovie(at: movie.url)
                dismiss()
            } label: {
                Image(systemName: "play.fill")
            }
            .buttonStyle(.borderless)
            Button {
                shareItem = ShareItem(url: movie.url)
            } label: {
                Image(systemName: "square.and.arrow.up")
            }
            .buttonStyle(.borderless)
        }
        .swipeActions {
            Button(role: .destructive) {
                manager.remove(at: movie.url)
            } label: {
                Label("Delete", systemImage: "trash")
            }
        }
    }
}

/// The `.rnm` UTType for movie import (declared in Info.plist), resolved by
/// extension so the picker shows it even before the system indexes the declaration.
enum MovieTypes {
    static var importable: [UTType] {
        if let t = UTType(filenameExtension: "rnm") { return [t] }
        return [.data]
    }
}

/// An Identifiable wrapper so a file URL can drive `.sheet(item:)` without adding a
/// global `URL: Identifiable` conformance (which can clash with SDK conformances).
struct ShareItem: Identifiable {
    let id = UUID()
    let url: URL
}

/// A `UIActivityViewController` wrapper for exporting a saved `.rnm` (iOS 15
/// compatible — `ShareLink` is iOS 16+).
struct ShareSheet: UIViewControllerRepresentable {
    let items: [Any]

    func makeUIViewController(context: Context) -> UIActivityViewController {
        let controller = UIActivityViewController(activityItems: items, applicationActivities: nil)
        // On iPad the activity sheet is presented as a popover; presenting one
        // without a non-nil `sourceView` (+ `sourceRect`) raises an exception.
        // Anchor it to the controller's own view, centered, with no arrow.
        if let popover = controller.popoverPresentationController {
            popover.sourceView = controller.view
            popover.sourceRect = CGRect(
                x: controller.view.bounds.midX, y: controller.view.bounds.midY,
                width: 0, height: 0
            )
            popover.permittedArrowDirections = []
        }
        return controller
    }

    func updateUIViewController(_ controller: UIActivityViewController, context: Context) {}
}
