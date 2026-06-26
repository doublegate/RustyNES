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
import UniformTypeIdentifiers

struct SettingsView: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.dismiss) private var dismiss
    @State private var showingLua = false

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Picker("Filter", selection: $model.filter) {
                        ForEach(VideoFilter.allCases) { filter in
                            Text(filter.label).tag(filter)
                        }
                    }
                    // Tuning sliders for the ACTIVE filter only (None / Bisqwit have
                    // none). They drive the renderer's shader params live and persist.
                    FilterParamSliders(model: model)
                } header: {
                    Text("Video")
                } footer: {
                    Text("The picture filter the renderer applies. None is the raw, pixel-exact image.")
                }

                // The global default palette (per-game overrides can pick another).
                PalettePickerSection(
                    manager: model.palettes,
                    selectedId: $model.globalPaletteId,
                    footer: "Import a .pal file to recolour the NES palette. Default is the built-in palette."
                )

                Section {
                    Toggle("Mute", isOn: $model.muted)
                } header: {
                    Text("Audio")
                } footer: {
                    Text("Silence the emulator without pausing it.")
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

                // Connectivity & scripting (v1.9.6).
                Section {
                    NavigationLink {
                        RetroAchievementsView(ra: model.ra)
                    } label: {
                        LabeledContent(
                            "RetroAchievements",
                            value: model.ra.enabled
                                ? (model.ra.isLoggedIn ? (model.ra.user?.displayName ?? "Signed in") : "On")
                                : "Off"
                        )
                    }
                } header: {
                    Text("RetroAchievements")
                } footer: {
                    Text("Opt-in achievement tracking. Off by default.")
                }

                Section {
                    Button {
                        showingLua = true
                    } label: {
                        Label("Lua console", systemImage: "curlybraces")
                    }
                    .disabled(model.emulator == nil)
                } header: {
                    Text("Developer")
                } footer: {
                    Text("Run a sandboxed Lua script against the running game. Also reachable from the in-game menu (where it runs live).")
                }

                // Per-game display overrides (only when a game is running).
                if model.currentEntry != nil {
                    Section {
                        NavigationLink {
                            GameSettingsView()
                        } label: {
                            LabeledContent(
                                "This game",
                                value: model.currentGameHasOverride ? "Custom" : "Global defaults"
                            )
                        }
                    } header: {
                        Text("Per-game settings")
                    } footer: {
                        Text("Give this game its own filter, palette, and HD-pack.")
                    }
                }

                Section {
                    NavigationLink {
                        AboutView()
                    } label: {
                        LabeledContent("About", value: AppInfo.marketingVersion)
                    }
                }
            }
            .navigationTitle("Settings")
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
            .sheet(isPresented: $showingLua) {
                LuaConsoleView()
            }
        }
    }
}

// MARK: - Filter parameters

/// The per-filter shader-param sliders, shown only for the currently-selected
/// filter (None and Bisqwit have no host-tunable knobs). Each binds to an AppModel
/// param, which re-applies live to the renderer and persists to UserDefaults.
private struct FilterParamSliders: View {
    @ObservedObject var model: AppModel

    var body: some View {
        switch model.filter {
        case .none, .bisqwit:
            EmptyView()
        case .scanlines:
            ParamSlider("Scanline intensity", value: $model.scanlineIntensity, range: 0...1)
        case .crt:
            ParamSlider("Scanline intensity", value: $model.scanlineIntensity, range: 0...1)
            ParamSlider("Aperture mask", value: $model.crtMask, range: 0...0.5)
        case .ntsc:
            ParamSlider("Saturation", value: $model.ntscSaturation, range: 0...2)
            ParamSlider("Sharpness", value: $model.ntscSharpness, range: 0...1)
            ParamSlider("Tint", value: $model.ntscTint, range: -0.5...0.5)
            ParamSlider("Phase", value: $model.ntscPhase, range: 0...1)
        }
    }
}

/// A labelled `Slider` with a live numeric readout, for one shader parameter.
private struct ParamSlider: View {
    let title: String
    @Binding var value: Float
    let range: ClosedRange<Float>

    init(_ title: String, value: Binding<Float>, range: ClosedRange<Float>) {
        self.title = title
        self._value = value
        self.range = range
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text(title)
                Spacer()
                Text(String(format: "%.2f", value))
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
            }
            Slider(value: $value, in: range)
        }
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

// MARK: - Palette / HD-pack importable UTTypes (v1.9.5)

/// The `.pal` UTType for palette import (declared in Info.plist). Resolved by
/// extension so the picker shows it before the system indexes the declaration.
enum PaletteTypes {
    static var importable: [UTType] {
        if let t = UTType(filenameExtension: "pal") { return [t] }
        return [.data]
    }
}

/// HD-packs are `.zip` archives (the same `public.zip-archive` the ROM importer
/// allows). The core extracts the pack from the archive bytes.
enum HDPackTypes {
    static var importable: [UTType] { [.zip] }
}

// MARK: - Palette picker (global + per-game)

/// A reusable Settings `Section` for choosing the active palette: "Default
/// (built-in)", the imported `.pal` files, and an import button. Binds to a
/// palette-id string ("" = built-in).
struct PalettePickerSection: View {
    @ObservedObject var manager: PaletteManager
    @Binding var selectedId: String
    var footer: String?
    @State private var showingImporter = false

    var body: some View {
        Section {
            paletteButton(title: "Default (built-in)", id: "")
            ForEach(manager.palettes) { palette in
                paletteButton(title: palette.name, id: palette.id)
            }
            Button { showingImporter = true } label: {
                Label("Import .pal", systemImage: "plus")
            }
        } header: {
            Text("Palette")
        } footer: {
            if let footer { Text(footer) }
        }
        .fileImporter(
            isPresented: $showingImporter,
            allowedContentTypes: PaletteTypes.importable,
            allowsMultipleSelection: false
        ) { result in
            if case .success(let urls) = result, let url = urls.first,
               let id = try? manager.importPalette(from: url) {
                selectedId = id
            }
        }
    }

    private func paletteButton(title: String, id: String) -> some View {
        Button {
            selectedId = id
        } label: {
            HStack {
                Text(title).foregroundStyle(.primary)
                Spacer()
                if selectedId == id {
                    Image(systemName: "checkmark").foregroundStyle(.tint)
                }
            }
        }
    }
}

// MARK: - HD-pack picker (per-game)

/// A reusable Settings `Section` for choosing an HD-pack: "None", the imported
/// packs, and an import button. Binds to a pack-id string ("" = none).
struct HDPackPickerSection: View {
    @ObservedObject var manager: HDPackStore
    @Binding var selectedId: String
    @State private var showingImporter = false

    var body: some View {
        Section {
            packButton(title: "None", id: "")
            ForEach(manager.packs) { pack in
                packButton(title: pack.name, id: pack.id)
            }
            Button { showingImporter = true } label: {
                Label("Import HD-pack (.zip)", systemImage: "plus")
            }
        } header: {
            Text("HD-pack")
        } footer: {
            Text("Loads a Mesen-format HD-pack. The composited high-resolution frame replaces the picture.")
        }
        .fileImporter(
            isPresented: $showingImporter,
            allowedContentTypes: HDPackTypes.importable,
            allowsMultipleSelection: false
        ) { result in
            if case .success(let urls) = result, let url = urls.first {
                Task {
                    if let id = try? await manager.importPack(from: url) {
                        selectedId = id
                    }
                }
            }
        }
    }

    private func packButton(title: String, id: String) -> some View {
        Button {
            selectedId = id
        } label: {
            HStack {
                Text(title).foregroundStyle(.primary)
                Spacer()
                if selectedId == id {
                    Image(systemName: "checkmark").foregroundStyle(.tint)
                }
            }
        }
    }
}

// MARK: - Per-game settings editor (v1.9.5)

/// Edits the running game's per-game display override: a master toggle, then (when
/// on) the filter + shader params + palette + HD-pack, all live-applied and
/// persisted under the ROM's SHA-256. With the toggle off, the game uses the global
/// defaults.
struct GameSettingsView: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        Form {
            if let override = model.currentGameOverride {
                Section {
                    Toggle("Custom settings for this game", isOn: Binding(
                        get: { true },
                        set: { if !$0 { model.clearCurrentGameOverride() } }
                    ))
                }

                Section {
                    Picker("Filter", selection: filterBinding(override)) {
                        ForEach(VideoFilter.allCases) { filter in
                            Text(filter.label).tag(filter)
                        }
                    }
                    overrideSliders(override)
                } header: {
                    Text("Video")
                }

                PalettePickerSection(
                    manager: model.palettes,
                    selectedId: paletteBinding(override),
                    footer: nil
                )

                HDPackPickerSection(
                    manager: model.hdpacks,
                    selectedId: hdpackBinding(override)
                )

                Section {
                    Button("Reset to global defaults", role: .destructive) {
                        model.clearCurrentGameOverride()
                    }
                }
            } else {
                Section {
                    Toggle("Custom settings for this game", isOn: Binding(
                        get: { false },
                        set: { if $0 { model.enableCurrentGameOverride() } }
                    ))
                } footer: {
                    Text("When on, this game remembers its own filter, palette, and HD-pack, independent of the global defaults.")
                }
            }
        }
        .navigationTitle(model.currentEntry?.name ?? "This Game")
    }

    @ViewBuilder
    private func overrideSliders(_ override: GameDisplaySettings) -> some View {
        switch VideoFilter(rawValue: override.filter) ?? .none {
        case .none, .bisqwit:
            EmptyView()
        case .scanlines:
            ParamSlider("Scanline intensity", value: floatBinding(override, \.scanlineIntensity), range: 0...1)
        case .crt:
            ParamSlider("Scanline intensity", value: floatBinding(override, \.scanlineIntensity), range: 0...1)
            ParamSlider("Aperture mask", value: floatBinding(override, \.crtMask), range: 0...0.5)
        case .ntsc:
            ParamSlider("Saturation", value: floatBinding(override, \.ntscSaturation), range: 0...2)
            ParamSlider("Sharpness", value: floatBinding(override, \.ntscSharpness), range: 0...1)
            ParamSlider("Tint", value: floatBinding(override, \.ntscTint), range: -0.5...0.5)
            ParamSlider("Phase", value: floatBinding(override, \.ntscPhase), range: 0...1)
        }
    }

    private func filterBinding(_ override: GameDisplaySettings) -> Binding<VideoFilter> {
        Binding(
            get: { VideoFilter(rawValue: override.filter) ?? .none },
            set: { var copy = override; copy.filter = $0.rawValue; model.updateCurrentGameOverride(copy) }
        )
    }

    private func floatBinding(
        _ override: GameDisplaySettings,
        _ keyPath: WritableKeyPath<GameDisplaySettings, Float>
    ) -> Binding<Float> {
        Binding(
            get: { override[keyPath: keyPath] },
            set: { var copy = override; copy[keyPath: keyPath] = $0; model.updateCurrentGameOverride(copy) }
        )
    }

    private func paletteBinding(_ override: GameDisplaySettings) -> Binding<String> {
        Binding(
            get: { override.paletteId },
            set: { var copy = override; copy.paletteId = $0; model.updateCurrentGameOverride(copy) }
        )
    }

    private func hdpackBinding(_ override: GameDisplaySettings) -> Binding<String> {
        Binding(
            get: { override.hdpackId },
            set: { var copy = override; copy.hdpackId = $0; model.updateCurrentGameOverride(copy) }
        )
    }
}
