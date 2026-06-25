//
//  SettingsView.swift
//
//  The settings sheet: the video filter picker (None / Scanlines / CRT / NTSC ->
//  the gfx FFI set_filter), an audio mute toggle, and an About section noting the
//  user-provided-ROM-only / no-bundled-content posture. Mirrors the Android
//  Settings.kt scope (the MVP subset).
//

import SwiftUI

struct SettingsView: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            Form {
                Section("Video") {
                    Picker("Filter", selection: $model.filter) {
                        ForEach(VideoFilter.allCases) { filter in
                            Text(filter.label).tag(filter)
                        }
                    }
                }

                Section("Audio") {
                    Toggle("Mute", isOn: $model.muted)
                }

                Section("About") {
                    LabeledContent("Version", value: appVersion)
                    Text(
                        "RustyNES is a cycle-accurate NES emulator. It plays only "
                        + "ROM files you supply from your device. No game content is "
                        + "bundled or downloaded."
                    )
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                }
            }
            .navigationTitle("Settings")
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
        }
    }

    private var appVersion: String {
        let v = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "1.9.0"
        let b = Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "1"
        return "\(v) (\(b))"
    }
}
