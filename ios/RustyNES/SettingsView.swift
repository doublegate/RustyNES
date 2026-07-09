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
import UIKit
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

                // Host audio-depth DSP (v1.9.9): EQ / pan / reverb / crossfeed.
                AudioDepthSection(depth: model.audioDepth)

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

                // Netplay endpoints (room-code / CGNAT play, v1.9.7).
                Section {
                    NavigationLink {
                        NetplaySettingsView(netplay: model.netplay)
                    } label: {
                        LabeledContent(
                            "Netplay",
                            value: model.netplay.signalingConfigured ? "Configured" : "Setup needed"
                        )
                    }
                } header: {
                    Text("Netplay")
                } footer: {
                    Text("Same-Wi-Fi play works out of the box. Room-code (internet) play needs a signaling relay (and optional TURN server) the maintainer hosts.")
                }

                // iCloud save-state sync (v1.9.7).
                CloudSyncSection(cloud: model.cloudSaveStates)

                // Game Center sign-in (v1.9.8). Opt-in / off by default.
                GameCenterSection(model: model.gameCenter)

                // Screen recording (v1.9.8, ReplayKit).
                RecordingSection(recorder: model.recorder)

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

                // Opt-in crash reporting (v2.0.6 "Parity"). Off by default; local-only.
                Section {
                    Toggle("Crash reporting", isOn: $model.crashReportingEnabled)
                    NavigationLink {
                        CrashLogsView()
                    } label: {
                        Label("Crash logs", systemImage: "ladybug")
                    }
                } header: {
                    Text("Diagnostics")
                } footer: {
                    Text("Off by default. When on, uncaught-exception reports are written to a local file you can read and copy — nothing is uploaded.")
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
    // `LocalizedStringKey` so `Text(title)` uses the localizing initializer: the
    // shader-param + audio-depth labels resolve through the String Catalog.
    // Unit-style band labels ("60 Hz") have no catalog entry and render verbatim.
    let title: LocalizedStringKey
    @Binding var value: Float
    let range: ClosedRange<Float>

    init(_ title: LocalizedStringKey, value: Binding<Float>, range: ClosedRange<Float>) {
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

// MARK: - Audio depth (v1.9.9)

/// The host audio-depth controls: a master enable, a 5-band EQ, a stereo pan, a
/// Schroeder reverb, and a headphone crossfeed. All output-only (post-mix), so a
/// flat / disabled config is a bit-exact passthrough. Observes the model so the
/// per-band sliders bind live and re-apply to the running core.
private struct AudioDepthSection: View {
    @ObservedObject var depth: AudioDepthModel

    var body: some View {
        Section {
            Toggle("Enable audio depth", isOn: $depth.enabled)
            if depth.enabled {
                ForEach(0..<AudioDepthModel.bandCount, id: \.self) { i in
                    // Band labels are frequency units ("60 Hz"); wrap the runtime
                    // String as a key (no catalog entry → renders verbatim).
                    ParamSlider(
                        LocalizedStringKey(AudioDepthModel.bandLabels[i]),
                        value: bandBinding(i),
                        range: -12...12
                    )
                }
                ParamSlider("Pan", value: $depth.pan, range: -1...1)
                ParamSlider("Reverb mix", value: $depth.reverbMix, range: 0...1)
                ParamSlider("Reverb room", value: $depth.reverbRoom, range: 0...1)
                ParamSlider("Crossfeed", value: $depth.crossfeed, range: 0...1)
                Button("Reset to flat") { depth.resetToFlat() }
            }
        } header: {
            Text("Audio depth")
        } footer: {
            Text("Output-only EQ, stereo pan, reverb, and headphone crossfeed. Off by default; flat = unchanged sound.")
        }
    }

    private func bandBinding(_ i: Int) -> Binding<Float> {
        Binding(
            get: { i < depth.eqDb.count ? depth.eqDb[i] : 0 },
            set: { var copy = depth.eqDb; if i < copy.count { copy[i] = $0; depth.eqDb = copy } }
        )
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

// MARK: - Netplay endpoints (v1.9.7)

/// Edits the room-code netplay endpoints: a REQUIRED signaling relay URL, an optional
/// STUN list, and an optional TURN trio. Persisted by NetplayModel into UserDefaults.
private struct NetplaySettingsView: View {
    @ObservedObject var netplay: NetplayModel

    var body: some View {
        Form {
            Section {
                TextField("wss://relay.example.com", text: $netplay.signalingURL)
                    .keyboardType(.URL)
                    .autocorrectionDisabled()
                    .textInputAutocapitalization(.never)
                if !netplay.signalingConfigured {
                    Label("Enter a relay URL (wss://; ws:// only for local testing).", systemImage: "exclamationmark.triangle")
                        .font(.caption)
                        .foregroundStyle(.orange)
                }
            } header: {
                Text("Signaling relay (required)")
            } footer: {
                Text("Room-code play relays the initial handshake through this server. The maintainer must deploy and host it (a documented carryover mirroring the Android deploy bundle). Same-Wi-Fi play does not use it.")
            }

            Section {
                TextField("stun:stun.example.com:3478", text: $netplay.stunServers, axis: .vertical)
                    .lineLimit(1...4)
                    .autocorrectionDisabled()
                    .textInputAutocapitalization(.never)
            } header: {
                Text("STUN servers (optional)")
            } footer: {
                Text("One host:port per line. Leave empty to use the built-in defaults.")
            }

            Section {
                TextField("turn:turn.example.com:3478", text: $netplay.turnURL)
                    .autocorrectionDisabled()
                    .textInputAutocapitalization(.never)
                TextField("TURN username", text: $netplay.turnUser)
                    .autocorrectionDisabled()
                    .textInputAutocapitalization(.never)
                SecureField("TURN secret", text: $netplay.turnSecret)
            } header: {
                Text("TURN relay (optional)")
            } footer: {
                Text("A TURN server lets strict (symmetric) NATs fall back to a relay. All three fields are needed together; otherwise room-code play is punch-or-fail.")
            }
        }
        .navigationTitle("Netplay")
        .navigationBarTitleDisplayMode(.inline)
    }
}

// MARK: - Game Center (v1.9.8)

/// The opt-in Game Center sign-in toggle + status. Auth + presence only — RustyNES
/// defines no leaderboards or achievements of its own (RetroAchievements is the real
/// achievement system); this is a complementary Apple-account sign-in. Observes the
/// model so the status line updates after authentication completes.
private struct GameCenterSection: View {
    @ObservedObject var model: GameCenterModel

    var body: some View {
        Section {
            Toggle("Enable Game Center", isOn: $model.enabled)
                .accessibilityHint(Text("Sign in to your Apple Game Center account"))
            if model.enabled {
                LabeledContent(
                    "Status",
                    value: model.isAuthenticated
                        ? (model.playerName ?? String(localized: "Signed in"))
                        : String(localized: "Not signed in")
                )
                .foregroundStyle(model.isAuthenticated ? .primary : .secondary)
            }
        } header: {
            Text("Game Center")
        } footer: {
            Text("Off by default. Signs in to your Apple Game Center account for presence. RustyNES has no leaderboards or achievements of its own \u{2014} RetroAchievements is the achievement system.")
        }
        .alert(
            "Game Center",
            isPresented: Binding(get: { model.lastError != nil }, set: { if !$0 { model.lastError = nil } }),
            actions: { Button("OK", role: .cancel) {} },
            message: { Text(model.lastError ?? "") }
        )
    }
}

// MARK: - Screen recording (v1.9.8, ReplayKit)

/// A Record-screen toggle backed by ReplayKit. Disabled (with an explanation) when the
/// device can't record. Also reachable from the in-game pill menu.
private struct RecordingSection: View {
    @ObservedObject var recorder: ScreenRecorder

    var body: some View {
        Section {
            Toggle("Record screen", isOn: Binding(
                get: { recorder.isRecording },
                set: { $0 ? recorder.startRecording() : recorder.stopRecording() }
            ))
            .disabled(!recorder.isAvailable && !recorder.isRecording)
            .accessibilityHint(Text("Capture gameplay video to save or share"))
        } header: {
            Text("Screen recording")
        } footer: {
            Text(recorder.isAvailable || recorder.isRecording
                ? "Record gameplay, then save or share the clip. Also on the in-game menu."
                : "Screen recording isn't available on this device right now.")
        }
        .alert(
            "Screen recording",
            isPresented: Binding(get: { recorder.lastError != nil }, set: { if !$0 { recorder.lastError = nil } }),
            actions: { Button("OK", role: .cancel) {} },
            message: { Text(recorder.lastError ?? "") }
        )
    }
}

// MARK: - iCloud save-state sync (v1.9.7)

/// The opt-in CloudKit save-state sync toggle + status. Observes the sync model so the
/// availability line updates after the account check.
private struct CloudSyncSection: View {
    @ObservedObject var cloud: CloudSaveStateSync

    var body: some View {
        Section {
            Toggle("Sync save states via iCloud", isOn: $cloud.enabled)
            if cloud.enabled {
                LabeledContent("iCloud account", value: cloud.accountAvailable ? "Available" : "Unavailable")
                    .foregroundStyle(cloud.accountAvailable ? .primary : .secondary)
            }
        } header: {
            Text("Save-state sync")
        } footer: {
            Text(cloud.enabled && !cloud.accountAvailable
                ? "Sign in to iCloud (Settings > Apple ID) to sync. Save states still work locally."
                : "Mirror your four per-game save slots across your devices through your private iCloud. Off by default; local save/load always works.")
        }
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
            // Built-in accessibility palettes (v1.9.8): high-contrast + Okabe-Ito
            // colourblind, the iOS counterpart of the desktop accessibility palettes.
            ForEach(AccessibilityPalettes.all) { palette in
                paletteButton(title: palette.name, id: palette.id)
            }
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
            if case .success(let urls) = result, let url = urls.first {
                Task {
                    if let id = try? await manager.importPalette(from: url) {
                        selectedId = id
                    }
                }
            }
        }
    }

    private func paletteButton(title: String, id: String) -> some View {
        let isSelected = selectedId == id
        // Title is rendered through `LocalizedStringKey` so the built-in palette names
        // ("Default (built-in)", the accessibility palettes) localize via the String
        // Catalog; an imported `.pal` stem has no catalog entry and renders verbatim.
        return Button {
            selectedId = id
        } label: {
            HStack {
                Text(LocalizedStringKey(title)).foregroundStyle(.primary)
                Spacer()
                if isSelected {
                    Image(systemName: "checkmark").foregroundStyle(.tint)
                }
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel(Text(LocalizedStringKey(title)))
        .accessibilityHint(Text("Use this palette"))
        .accessibilityAddTraits(isSelected ? [.isSelected, .isButton] : .isButton)
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

// MARK: - Crash logs (v2.0.6 "Parity")

/// A read-only viewer for the opt-in crash logs: list newest-first, tap to read the
/// full trace and copy it, or clear them all. Local-only — nothing is uploaded (the
/// logs are written by `CrashReporter` only when the user has opted in).
private struct CrashLogsView: View {
    @State private var logs: [URL] = CrashReporter.savedLogs()

    var body: some View {
        List {
            if logs.isEmpty {
                Text("No crash logs.")
                    .foregroundStyle(.secondary)
            } else {
                ForEach(logs, id: \.self) { url in
                    NavigationLink {
                        CrashLogDetailView(url: url)
                    } label: {
                        Text(url.deletingPathExtension().lastPathComponent)
                            .font(.system(.body, design: .monospaced))
                    }
                }
            }
        }
        .navigationTitle("Crash logs")
        .toolbar {
            if !logs.isEmpty {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Clear", role: .destructive) {
                        CrashReporter.clear()
                        logs = []
                    }
                }
            }
        }
    }
}

/// The full text of one crash log, with a copy-to-clipboard action.
private struct CrashLogDetailView: View {
    let url: URL

    private var text: String {
        (try? String(contentsOf: url, encoding: .utf8)) ?? "(unreadable)"
    }

    var body: some View {
        ScrollView {
            Text(text)
                .font(.system(.footnote, design: .monospaced))
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding()
        }
        .navigationTitle(url.deletingPathExtension().lastPathComponent)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    UIPasteboard.general.string = text
                } label: {
                    Label("Copy", systemImage: "doc.on.doc")
                }
            }
        }
    }
}
