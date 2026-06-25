//
//  SettingsView.swift
//
//  The settings sheet: the video filter picker (None / Scanlines / CRT / NTSC ->
//  the gfx FFI set_filter), an audio mute toggle, the v1.9.2 Controls (haptics) +
//  Controllers (P1-P4 port assignment + button remap) sections, and an About section
//  noting the user-provided-ROM-only / no-bundled-content posture. Mirrors the
//  Android Settings.kt scope.
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

                Section {
                    Toggle("Haptic feedback", isOn: $model.hapticsEnabled)
                        .disabled(!model.hapticsSupported)
                } header: {
                    Text("Controls")
                } footer: {
                    Text(
                        model.hapticsSupported
                            ? "Light vibration when you press an on-screen button. Off by default."
                            : "This device does not support haptics."
                    )
                }

                ControllersSection(manager: model.gamepads)

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

// MARK: - Controllers

/// Lists connected hardware controllers, lets the user pick which NES port each
/// drives (P1-P4), and links to the shared button-remap editor.
private struct ControllersSection: View {
    @ObservedObject var manager: GameControllerManager

    var body: some View {
        Section {
            if manager.connected.isEmpty {
                Text("No controllers connected. Pair an MFi / Xbox / PlayStation controller in Settings.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            } else {
                ForEach(manager.connected) { controller in
                    HStack {
                        Image(systemName: "gamecontroller")
                            .foregroundStyle(.secondary)
                        Text(controller.name)
                            .lineLimit(1)
                        Spacer()
                        Picker("Player", selection: portBinding(for: controller)) {
                            ForEach(0..<GameControllerManager.maxPlayers, id: \.self) { port in
                                Text("P\(port + 1)").tag(port)
                            }
                        }
                        .pickerStyle(.menu)
                        .labelsHidden()
                    }
                }
                NavigationLink("Button mapping") {
                    ControllerMappingView(manager: manager)
                }
            }
        } header: {
            Text("Controllers")
        } footer: {
            Text("Up to four controllers map to NES ports P1-P4.")
        }
    }

    private func portBinding(for controller: ConnectedController) -> Binding<Int> {
        Binding(
            get: { controller.port },
            set: { manager.assign(controllerID: controller.id, toPort: $0) }
        )
    }
}

/// Edits the shared physical-button -> NES-input remap profile. One profile applies
/// to every connected controller (persisted in UserDefaults by the manager).
private struct ControllerMappingView: View {
    @ObservedObject var manager: GameControllerManager

    var body: some View {
        Form {
            Section {
                Text(
                    "Map each controller button to a NES input. Turbo A / Turbo B "
                    + "auto-fire while held. Applies to all controllers."
                )
                .font(.footnote)
                .foregroundStyle(.secondary)
            }

            Section("Buttons") {
                ForEach(PhysicalButton.allCases) { physical in
                    Picker(physical.label, selection: targetBinding(for: physical)) {
                        ForEach(ControllerInput.allCases) { input in
                            Text(input.label).tag(input)
                        }
                    }
                }
            }

            Section {
                Button("Reset to defaults") { manager.remap = .standard }
            }
        }
        .navigationTitle("Button Mapping")
    }

    private func targetBinding(for physical: PhysicalButton) -> Binding<ControllerInput> {
        Binding(
            get: { manager.remap.target(for: physical) },
            set: { manager.remap.mapping[physical] = $0 }
        )
    }
}
