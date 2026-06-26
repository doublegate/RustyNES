//
//  RetroAchievementsView.swift
//
//  The RetroAchievements (RA) settings screen (v1.9.6), pushed from Settings ->
//  RetroAchievements. Opt-in / off by default. Username + password sign-in forwards
//  to the bridge; on success the returned token is stored in the Keychain (NOT
//  UserDefaults) and auto-logs-in on the next game launch. Hardcore mode is a toggle
//  with its save-state caveat. Rich presence is shown subtly; the per-game achievement
//  list is a sub-screen.
//
//  Privacy: RA is an account login to a third party. The privacy manifest must
//  disclose this before shipping (a maintainer carryover); the feature is opt-in.
//
//  iOS note: the RA session lives on the per-game controller, so sign-in requires a
//  running game; the auto token re-login on each game launch keeps the user signed in
//  thereafter. The poll pumps a paused login (behind this sheet) so it still completes.
//

import SwiftUI

struct RetroAchievementsView: View {
    @EnvironmentObject private var model: AppModel
    @ObservedObject private var ra: RetroAchievementsModel

    @State private var username = ""
    @State private var password = ""

    init(ra: RetroAchievementsModel) {
        self._ra = ObservedObject(wrappedValue: ra)
    }

    var body: some View {
        Form {
            Section {
                Toggle("Enable RetroAchievements", isOn: $ra.enabled)
            } footer: {
                Text("Off by default. RetroAchievements is an account login to a third-party service; sign-in shares your credentials with retroachievements.org.")
            }

            if ra.enabled {
                accountSection
                hardcoreSection

                if !ra.richPresence.isEmpty {
                    Section {
                        Text(ra.richPresence)
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                    } header: {
                        Text("Now playing")
                    }
                }

                if ra.isLoggedIn {
                    Section {
                        NavigationLink {
                            AchievementsView(ra: ra)
                        } label: {
                            LabeledContent("Achievements", value: "\(ra.earned) / \(ra.total)")
                        }
                    }
                }
            }
        }
        .navigationTitle("RetroAchievements")
        .navigationBarTitleDisplayMode(.inline)
        .onAppear { username = ra.username }
        .alert(
            "RetroAchievements",
            isPresented: Binding(get: { ra.lastError != nil }, set: { if !$0 { ra.lastError = nil } }),
            actions: { Button("OK", role: .cancel) {} },
            message: { Text(ra.lastError ?? "") }
        )
    }

    @ViewBuilder
    private var accountSection: some View {
        if ra.isLoggedIn, let user = ra.user {
            Section {
                LabeledContent("User", value: user.displayName)
                LabeledContent("Points", value: "\(user.score)")
                Button("Sign out", role: .destructive) { ra.logout() }
            } header: {
                Text("Account")
            }
        } else {
            Section {
                TextField("Username", text: $username)
                    .textContentType(.username)
                    .autocorrectionDisabled()
                    .textInputAutocapitalization(.never)
                SecureField("Password", text: $password)
                    .textContentType(.password)
                Button("Sign in") {
                    ra.loginPassword(user: username, password: password)
                    password = ""
                }
                .disabled(username.isEmpty || password.isEmpty || model.emulator == nil)
                if ra.status == .loggingIn {
                    HStack {
                        ProgressView()
                        Text("Signing in...").foregroundStyle(.secondary)
                    }
                }
            } header: {
                Text("Sign in")
            } footer: {
                Text(model.emulator == nil
                    ? "Open a game first; RetroAchievements signs in per game session. Your token is saved to the Keychain and reused automatically."
                    : "Your token is saved to the Keychain (not iCloud) and reused on the next launch. Your password is never stored.")
            }
        }
    }

    private var hardcoreSection: some View {
        Section {
            Toggle("Hardcore mode", isOn: $ra.hardcore)
        } header: {
            Text("Hardcore")
        } footer: {
            Text("Hardcore mode earns full points but disables loading save-states (and rewind). Turn it off for casual play with save-states.")
        }
    }
}
