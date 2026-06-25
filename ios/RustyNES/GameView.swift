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
                        onLibrary: { model.closeGame() },
                        onStates: { showingStates = true },
                        onSettings: { showingSettings = true },
                        onReset: { model.emulator?.reset() },
                        onPower: { model.emulator?.powerCycle() }
                    )
                    .padding(.trailing, 8)
                }
                .transition(.move(edge: .trailing).combined(with: .opacity))
            }
        }
        .animation(.easeInOut(duration: 0.2), value: menuVisible)
        .sheet(isPresented: $showingStates) {
            SaveStatesView()
        }
        .sheet(isPresented: $showingSettings) {
            SettingsView()
        }
        // Pause the emulator while a menu/sheet is open so the player doesn't lose
        // progress or hear audio behind it; resume once both are dismissed.
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

            Text(model.currentEntry?.name ?? "RustyNES")
                .font(.subheadline.bold())
                .lineLimit(1)
                .foregroundColor(.white.opacity(0.85))

            Spacer()

            Button { model.emulator?.reset() } label: {
                Image(systemName: "arrow.counterclockwise")
            }
            .accessibilityLabel("Reset")

            Button { showingStates = true } label: {
                Image(systemName: "tray.and.arrow.down")
            }
            .accessibilityLabel("Save states")

            Button { model.muted.toggle() } label: {
                Image(systemName: model.muted ? "speaker.slash" : "speaker.wave.2")
            }
            .accessibilityLabel(model.muted ? "Unmute" : "Mute")
        }
        .foregroundColor(.white)
        .padding(.horizontal)
        .padding(.vertical, 8)
        .background(Color.black.opacity(0.35))
    }
}
