//
//  GameView.swift
//
//  The in-game screen: the Metal-backed picture (with 8:7-corrected NES aspect
//  letterboxing via the renderer), the translucent on-screen controls overlaid on
//  top, and a slide-out control bar (close, reset, save/load slots). Mirrors the
//  Android in-game shell.
//

import SwiftUI

struct GameView: View {
    @EnvironmentObject private var model: AppModel
    @State private var showingStates = false
    @State private var showingSettings = false
    @State private var showingMovies = false
    @State private var showingNetplay = false
    @State private var showingAchievements = false
    @State private var showingLua = false
    // The top bar + pill menu visibility, toggled by the on-screen MENU pill (mirrors
    // the Android MENU pill toggling `controlsVisible`).
    @State private var menuVisible = true

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()

            if let emulator = model.emulator {
                // The picture fills the available space; wgpu pillarboxes the NES
                // image to the correct pixel aspect inside the drawable.
                MetalGameView(emulator: emulator)
                    .ignoresSafeArea()

                // On-screen controls: the v1.9.2 true multi-touch NES-001 pad (the
                // replacement for the v1.9.0 single-DragGesture TouchControlsOverlay).
                // It feeds the combined touch mask into the model, which ORs it with
                // the P1 hardware-pad mask. Drawn at the Android NES-001 aspect ratio
                // (123:53, `ControlPadLayout.aspectRatio`), width-driven and anchored
                // to the bottom safe area so the proportions match the Android render.
                VStack(spacing: 0) {
                    Spacer(minLength: 0)
                    MultiTouchControlPad(
                        onMaskChanged: { mask in model.setTouchMask(mask) },
                        onLogoTap: { menuVisible.toggle() }
                    )
                    .aspectRatio(ControlPadLayout.aspectRatio, contentMode: .fit)
                    .frame(maxWidth: .infinity)
                }
                .padding(.horizontal)
                .padding(.bottom, 8)
            }

            // A small top bar: menu toggle + game title (toggled by the MENU pill).
            VStack {
                if menuVisible {
                    topBar
                }
                Spacer()
            }

            // The floating pill menu: one-handed quick access to the save-state
            // manager, settings, reset, power, and the library. Trailing-edge,
            // vertically centred, auto-hiding with the rest of the chrome.
            if menuVisible {
                HStack {
                    Spacer()
                    PillMenu(
                        recorder: model.recorder,
                        onLibrary: { model.closeGame() },
                        onStates: { showingStates = true },
                        onMovies: { showingMovies = true },
                        onNetplay: { showingNetplay = true },
                        onAchievements: { showingAchievements = true },
                        onLua: { showingLua = true },
                        onRecord: { model.recorder.toggle() },
                        onSettings: { showingSettings = true },
                        onReset: { model.emulator?.reset() },
                        onPower: { model.emulator?.powerCycle() }
                    )
                    .padding(.trailing, 8)
                }
                .transition(.move(edge: .trailing).combined(with: .opacity))
            }

            // RetroAchievements unlock / login toasts (v1.9.6) + the netplay status
            // chip. These observe their nested ObservableObjects directly so they
            // re-render when the poll timers update them (AppModel doesn't republish
            // nested-model changes).
            AchievementToastOverlay(ra: model.ra, topInset: menuVisible ? 56 : 12)
            NetplayOverlay(netplay: model.netplay)
            // Live hardware-controller indicator: appears/updates as pads connect or
            // disconnect mid-game (observes the manager's @Published list directly).
            ControllerIndicatorOverlay(manager: model.gamepads, topInset: menuVisible ? 56 : 12)

            // Presents ReplayKit's preview controller after a recording stops (v1.9.8).
            RecordingPreviewHost(recorder: model.recorder)
        }
        .animation(.easeInOut(duration: 0.2), value: menuVisible)
        .sheet(isPresented: $showingStates) {
            SaveStatesView()
        }
        .sheet(isPresented: $showingSettings) {
            SettingsView()
        }
        .sheet(isPresented: $showingMovies) {
            MoviePanelView()
        }
        .sheet(isPresented: $showingNetplay) {
            NetplayView(netplay: model.netplay)
        }
        .sheet(isPresented: $showingAchievements) {
            NavigationStack {
                AchievementsView(ra: model.ra)
                    .toolbar {
                        ToolbarItem(placement: .topBarTrailing) {
                            Button("Done") { showingAchievements = false }
                        }
                    }
            }
        }
        .sheet(isPresented: $showingLua) {
            LuaConsoleView()
        }
        // Pause the emulator while a menu/sheet is open so the player doesn't lose
        // progress or hear audio behind it; resume once all are dismissed. The TAS /
        // Movies panel is the exception: recording / playback must keep the core
        // running, so it does NOT pause emulation.
        .onChange(of: showingStates) { _ in model.setMenuPaused(showingStates || showingSettings) }
        .onChange(of: showingSettings) { _ in model.setMenuPaused(showingStates || showingSettings) }
        .statusBarHidden(true)
    }

    private var topBar: some View {
        HStack(spacing: 16) {
            Button { model.closeGame() } label: {
                Image(systemName: "chevron.left")
                    .font(.headline)
            }
            .accessibilityLabel("Close game")
            .accessibilityHint("Return to the game library")

            Text(model.currentEntry?.name ?? "RustyNES")
                .font(.subheadline.bold())
                .lineLimit(1)
                .foregroundColor(.white.opacity(0.85))
                .accessibilityAddTraits(.isHeader)

            Spacer()

            Button { model.emulator?.reset() } label: {
                Image(systemName: "arrow.counterclockwise")
            }
            .accessibilityLabel("Reset")
            .accessibilityHint("Soft-reset the console")

            Button { showingStates = true } label: {
                Image(systemName: "tray.and.arrow.down")
            }
            .accessibilityLabel("Save states")
            .accessibilityHint("Open the save-state manager")

            Button { model.muted.toggle() } label: {
                Image(systemName: model.muted ? "speaker.slash" : "speaker.wave.2")
            }
            .accessibilityLabel(model.muted ? Text("Unmute") : Text("Mute"))
            .accessibilityValue(model.muted ? Text("Muted") : Text("On"))
        }
        .foregroundColor(.white)
        .padding(.horizontal)
        .padding(.vertical, 8)
        .background(Color.black.opacity(0.35))
    }
}

/// Observes the RA model and renders its live toast banners at the top of the screen.
private struct AchievementToastOverlay: View {
    @ObservedObject var ra: RetroAchievementsModel
    let topInset: CGFloat

    var body: some View {
        if !ra.toasts.isEmpty {
            VStack(spacing: 6) {
                ForEach(Array(ra.toasts.enumerated()), id: \.offset) { _, toast in
                    AchievementToast(toast: toast)
                }
                Spacer()
            }
            .padding(.top, topInset)
            .padding(.horizontal)
            .allowsHitTesting(false)
            .transition(.move(edge: .top).combined(with: .opacity))
            .animation(.easeInOut(duration: 0.2), value: ra.toasts.count)
        }
    }
}

/// Observes the netplay model and shows the connection / desync chip while active.
private struct NetplayOverlay: View {
    @ObservedObject var netplay: NetplayModel

    var body: some View {
        if let status = netplay.status, netplay.isActive {
            VStack {
                Spacer()
                HStack {
                    NetplayStatusChip(status: status)
                    Spacer()
                }
            }
            .padding(.leading)
            .padding(.bottom, 90)
            .allowsHitTesting(false)
        }
    }
}

/// A small top-trailing chip showing how many hardware controllers are connected.
/// Observes the manager so it appears / updates live on hot-plug + disconnect.
private struct ControllerIndicatorOverlay: View {
    @ObservedObject var manager: GameControllerManager
    let topInset: CGFloat

    var body: some View {
        if !manager.connected.isEmpty {
            VStack {
                HStack {
                    Spacer()
                    HStack(spacing: 4) {
                        Image(systemName: "gamecontroller.fill")
                        Text("\(manager.connected.count)")
                            .font(.caption.monospacedDigit().bold())
                    }
                    .font(.caption)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 5)
                    .background(.ultraThinMaterial, in: Capsule())
                }
                Spacer()
            }
            .padding(.trailing, 12)
            .padding(.top, topInset)
            .allowsHitTesting(false)
            .accessibilityLabel("\(manager.connected.count) controllers connected")
        }
    }
}

/// A transient RetroAchievements toast banner (unlock / login / server message).
private struct AchievementToast: View {
    let toast: RaToast

    var body: some View {
        HStack(spacing: 10) {
            badge
            VStack(alignment: .leading, spacing: 1) {
                Text(toast.title)
                    .font(.subheadline.bold())
                    .lineLimit(1)
                if !toast.detail.isEmpty {
                    Text(toast.detail)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(10)
        .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 12))
        .overlay(
            RoundedRectangle(cornerRadius: 12)
                .strokeBorder(toast.isError ? Color.red.opacity(0.5) : Color.yellow.opacity(0.4))
        )
        .shadow(radius: 6, y: 2)
    }

    @ViewBuilder
    private var badge: some View {
        if toast.isError {
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundStyle(.red)
                .frame(width: 36, height: 36)
        } else if let url = URL(string: toast.badgeUrl), !toast.badgeUrl.isEmpty {
            AsyncImage(url: url) { image in
                image.resizable().scaledToFit()
            } placeholder: {
                Image(systemName: "trophy.fill").foregroundStyle(.yellow)
            }
            .frame(width: 36, height: 36)
            .clipShape(RoundedRectangle(cornerRadius: 6))
        } else {
            Image(systemName: "trophy.fill")
                .foregroundStyle(.yellow)
                .frame(width: 36, height: 36)
        }
    }
}

/// A small netplay status chip (connection phase / desync) shown in-game.
private struct NetplayStatusChip: View {
    let status: NpStatus

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: icon)
                .foregroundStyle(tint)
            Text(label)
                .font(.caption.bold())
            if let ping = status.pingMs {
                Text("\(ping)ms")
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(.ultraThinMaterial, in: Capsule())
        .overlay(Capsule().strokeBorder(tint.opacity(0.4)))
    }

    private var icon: String {
        if status.desync { return "exclamationmark.triangle.fill" }
        switch status.phase {
        case .inGame: return status.stalled ? "clock.arrow.circlepath" : "wifi"
        case .connecting, .negotiating: return "wifi.exclamationmark"
        case .error: return "wifi.slash"
        case .idle: return "wifi"
        }
    }

    private var tint: Color {
        if status.desync || status.phase == .error { return .red }
        if status.stalled || status.phase != .inGame { return .orange }
        return .green
    }

    private var label: String {
        if status.desync { return "Desync" }
        switch status.phase {
        case .inGame: return status.stalled ? "Re-syncing" : "Netplay"
        case .connecting: return "Connecting"
        case .negotiating: return "Negotiating"
        case .error: return "Disconnected"
        case .idle: return "Idle"
        }
    }
}
