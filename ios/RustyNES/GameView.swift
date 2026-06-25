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
    @State private var showingControls = false
    @State private var showingStates = false

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()

            if let emulator = model.emulator {
                // The picture fills the available space; wgpu pillarboxes the NES
                // image to the correct pixel aspect inside the drawable.
                MetalGameView(emulator: emulator)
                    .ignoresSafeArea()

                // On-screen controls overlay (feeds the touch mask into the model,
                // which ORs it with any hardware-pad mask).
                TouchControlsOverlay { mask in
                    model.setTouchMask(mask)
                }
                .ignoresSafeArea()
                .allowsHitTesting(true)
            }

            // A small top bar: menu toggle + game title.
            VStack {
                topBar
                Spacer()
            }
        }
        .sheet(isPresented: $showingStates) {
            SaveStatesSheet()
        }
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

/// The save-state slot picker sheet (save into / load from one of N slots).
private struct SaveStatesSheet: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List(model.slots()) { slot in
                HStack {
                    VStack(alignment: .leading) {
                        Text("Slot \(slot.index + 1)")
                            .font(.headline)
                        Text(slot.isEmpty ? "Empty" : "Saved \(slot.savedAt!.formatted())")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button("Save") { model.saveSlot(slot.index) }
                        .buttonStyle(.bordered)
                    Button("Load") { model.loadSlot(slot.index); dismiss() }
                        .buttonStyle(.borderedProminent)
                        .disabled(slot.isEmpty)
                }
            }
            .navigationTitle("Save States")
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
        }
    }
}
