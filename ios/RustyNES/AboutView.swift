//
//  AboutView.swift
//
//  The About screen: app name + version, the bring-your-own-ROM ownership notice
//  (App Review Guideline 4.7), attribution/credits (the bundled OFL-1.1 Press
//  Start 2P font + the pure-Rust emulation core), and a link to the project.
//  Reachable from Settings.
//

import SwiftUI

/// App identity read from the bundle (single source of truth for the version
/// strings shown in Settings + About).
enum AppInfo {
    /// e.g. "1.9.3".
    static var marketingVersion: String {
        Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "1.9.3"
    }

    /// The build number (CI overrides it per upload).
    static var buildNumber: String {
        Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "1"
    }

    /// "1.9.3 (1)" — the user-facing version label.
    static var displayVersion: String {
        "\(marketingVersion) (\(buildNumber))"
    }

    static let projectURL = URL(string: "https://github.com/doublegate/RustyNES")!
}

struct AboutView: View {
    var body: some View {
        List {
            Section {
                VStack(spacing: 8) {
                    Image(systemName: "gamecontroller.fill")
                        .font(.system(size: 44))
                        .foregroundStyle(.tint)
                    Text("RustyNES")
                        .font(.title2.bold())
                    Text("Version \(AppInfo.displayVersion)")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 8)
            }
            .listRowBackground(Color.clear)

            Section {
                // A single string literal (not a `+` concatenation) so it is a
                // `LocalizedStringKey` and localizes via the String Catalog. This is the
                // App Review Guideline 4.7 ownership reaffirmation.
                Text("RustyNES is a cycle-accurate Nintendo Entertainment System emulator. It plays only ROM files you supply from your own device. No game content is bundled with or downloaded by the app. You must own the games you play.")
                    .font(.footnote)
            } header: {
                Text("Bring your own ROMs")
            }

            Section("Credits") {
                LabeledContent("Emulation core", value: "Pure Rust (RustyNES)")
                LabeledContent("Label font", value: "Press Start 2P (OFL-1.1)")
                Text("Made by DoubleGate. Licensed MIT OR Apache-2.0.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }

            Section {
                Link(destination: AppInfo.projectURL) {
                    Label("Project on GitHub", systemImage: "link")
                }
            }
        }
        .navigationTitle("About")
        .navigationBarTitleDisplayMode(.inline)
    }
}
