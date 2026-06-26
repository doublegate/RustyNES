//
//  AchievementsView.swift
//
//  The loaded game's RetroAchievements list (v1.9.6): each achievement's badge, title,
//  description, points, and earned state, with an earned/total + points summary header
//  (from `raGameSummary`). Reachable from Settings -> RetroAchievements -> Achievements
//  and from the in-game pill menu while signed in. Live-updates as the poll refreshes
//  the cached list + summary.
//

import SwiftUI

struct AchievementsView: View {
    @ObservedObject private var ra: RetroAchievementsModel

    init(ra: RetroAchievementsModel) {
        self._ra = ObservedObject(wrappedValue: ra)
    }

    var body: some View {
        List {
            Section {
                summaryRow
            }
            if ra.achievements.isEmpty {
                Section {
                    Text("No achievements loaded. Sign in and load a supported game.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            } else {
                Section {
                    ForEach(ra.achievements, id: \.id) { ach in
                        AchievementRow(achievement: ach)
                    }
                } header: {
                    Text("Achievements")
                }
            }
        }
        .navigationTitle("Achievements")
        .navigationBarTitleDisplayMode(.inline)
    }

    private var summaryRow: some View {
        VStack(alignment: .leading, spacing: 4) {
            if !ra.richPresence.isEmpty {
                Text(ra.richPresence)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
            HStack {
                Label("\(ra.earned) / \(ra.total)", systemImage: "trophy.fill")
                    .foregroundStyle(.yellow)
                Spacer()
                if ra.summary.count > 5 {
                    Text("\(ra.summary[5]) / \(ra.summary[4]) pts")
                        .font(.subheadline.monospacedDigit())
                        .foregroundStyle(.secondary)
                }
            }
        }
    }
}

/// One achievement row: badge (locked vs unlocked URL), title, description, points,
/// and the measured-progress percent for in-progress achievements.
private struct AchievementRow: View {
    let achievement: RaAchievementInfo

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            badge
            VStack(alignment: .leading, spacing: 2) {
                HStack {
                    Text(achievement.title)
                        .font(.subheadline.bold())
                    Spacer()
                    Text("\(achievement.points)")
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                }
                Text(achievement.description)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                if !achievement.unlocked, achievement.measuredPercent > 0 {
                    ProgressView(value: Double(achievement.measuredPercent), total: 100)
                        .tint(.yellow)
                }
            }
        }
        .opacity(achievement.unlocked ? 1.0 : 0.6)
    }

    private var badge: some View {
        let urlString = achievement.unlocked ? achievement.badgeUrl : achievement.badgeLockedUrl
        return AsyncImage(url: URL(string: urlString)) { phase in
            switch phase {
            case .success(let image):
                image.resizable().scaledToFit()
            default:
                RoundedRectangle(cornerRadius: 6)
                    .fill(Color.secondary.opacity(0.2))
                    .overlay(
                        Image(systemName: achievement.unlocked ? "trophy.fill" : "lock.fill")
                            .foregroundStyle(.secondary)
                    )
            }
        }
        .frame(width: 48, height: 48)
        .clipShape(RoundedRectangle(cornerRadius: 6))
        .grayscale(achievement.unlocked ? 0 : 1)
    }
}
