//
//  RustyNESWidget.swift
//
//  v3.7.0 (D24, `T-MOBILE-EXTRAS` row 1): the home-screen widget, the iOS twin of
//  Android's `ResumeWidget.kt`. It shows the last-played game (its box art when one
//  was found, its name, when it was played) and a tap opens the app on
//  `rustynes://resume`, which reopens that game (RustyNESApp's `onOpenURL`).
//
//  A WidgetKit extension: a `StaticConfiguration` with a `TimelineProvider` that
//  reads the App Group the app writes (ResumeWidgetShared.swift). The app reloads
//  the timeline when a game opens, so the provider's own policy is `.never`. Small
//  and medium families; nothing here runs the emulator.
//
//  UNCOMPILED on the Linux host this was written on (no Swift toolchain); the iOS CI
//  Simulator build compiles it (target RustyNESWidget in project.yml). Device rows
//  W1-W3 of docs/mobile-v2.9.3-run-sheet.md are its first run.
//

import SwiftUI
import UIKit
import WidgetKit

struct ResumeEntry: TimelineEntry {
    let date: Date
    let game: ResumeWidgetShared.LastPlayed?
}

struct ResumeProvider: TimelineProvider {
    func placeholder(in context: Context) -> ResumeEntry {
        ResumeEntry(date: Date(), game: nil)
    }

    func getSnapshot(in context: Context, completion: @escaping (ResumeEntry) -> Void) {
        completion(ResumeEntry(date: Date(), game: ResumeWidgetShared.read()))
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<ResumeEntry>) -> Void) {
        let entry = ResumeEntry(date: Date(), game: ResumeWidgetShared.read())
        // The app calls `WidgetCenter.reloadTimelines(ofKind:)` when a game opens.
        completion(Timeline(entries: [entry], policy: .never))
    }
}

struct ResumeWidgetView: View {
    let entry: ResumeEntry

    var body: some View {
        Group {
            if let game = entry.game {
                HStack(spacing: 10) {
                    cover(for: game)
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Resume")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        Text(game.name)
                            .font(.headline)
                            .lineLimit(2)
                        Text(game.played, style: .relative)
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                    Spacer(minLength: 0)
                }
            } else {
                VStack(spacing: 6) {
                    Image(systemName: "gamecontroller")
                        .font(.title)
                        .accessibilityHidden(true)
                    Text("Open a game in RustyNES to resume it here.")
                        .font(.caption)
                        .multilineTextAlignment(.center)
                }
            }
        }
        .widgetURL(ResumeWidgetLink.url)
        .containerBackground(.fill.tertiary, for: .widget)
    }

    @ViewBuilder
    private func cover(for game: ResumeWidgetShared.LastPlayed) -> some View {
        if let url = ResumeWidgetShared.boxArtURL(sha: game.sha),
           let image = UIImage(contentsOfFile: url.path) {
            Image(uiImage: image)
                .resizable()
                .aspectRatio(contentMode: .fit)
                .clipShape(RoundedRectangle(cornerRadius: 6))
                .accessibilityHidden(true)
        } else {
            Image(systemName: "tv")
                .font(.largeTitle)
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)
        }
    }
}

@main
struct RustyNESWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: ResumeWidgetShared.kind, provider: ResumeProvider()) { entry in
            ResumeWidgetView(entry: entry)
        }
        .configurationDisplayName("Resume")
        .description("The last game you played, one tap away.")
        .supportedFamilies([.systemSmall, .systemMedium])
    }
}
