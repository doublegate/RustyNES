//
//  PillMenu.swift
//
//  The compact in-game "pill" menu: a floating, auto-hiding control cluster that
//  gives one-handed access to the save-state manager, settings, reset, power-cycle,
//  and returning to the library. It mirrors the Android floating pill menu, and is
//  toggled (shown/hidden) by the on-screen MENU pill (the NES-001 pad logo tap),
//  which already drives `menuVisible` in GameView.
//
//  Anchored to the trailing edge, vertically centred, so the buttons fall under the
//  thumb of a hand holding the device on the right side.
//

import SwiftUI

struct PillMenu: View {
    /// The ReplayKit recorder, observed so the Record pill reflects the live state.
    @ObservedObject var recorder: ScreenRecorder

    var onLibrary: () -> Void
    var onStates: () -> Void
    var onMovies: () -> Void
    var onNetplay: () -> Void
    var onAchievements: () -> Void
    var onLua: () -> Void
    var onRecord: () -> Void
    var onSettings: () -> Void
    var onReset: () -> Void
    var onPower: () -> Void

    var body: some View {
        VStack(spacing: 4) {
            pillButton("rectangle.stack", "Library",
                       hint: "Return to the game library", action: onLibrary)
            pillButton("tray.full", "Save states",
                       hint: "Open the save-state manager", action: onStates)
            pillButton("film", "TAS / Movies",
                       hint: "Record or play a movie", action: onMovies)
            pillButton("wifi", "Netplay",
                       hint: "Host or join an online session", action: onNetplay)
            pillButton("trophy", "Achievements",
                       hint: "View RetroAchievements progress", action: onAchievements)
            pillButton("curlybraces", "Lua console",
                       hint: "Run a script against the game", action: onLua)
            // Record-screen pill: a red dot + "Stop recording" while active.
            pillButton(
                recorder.isRecording ? "stop.circle" : "record.circle",
                recorder.isRecording ? "Stop recording" : "Record screen",
                hint: "Capture gameplay video to save or share",
                tint: recorder.isRecording ? .red : .primary,
                action: onRecord
            )
            .accessibilityValue(Text(recorder.isRecording ? "Recording" : "Not recording"))
            pillButton("gearshape", "Settings",
                       hint: "Open settings", action: onSettings)
            pillButton("arrow.counterclockwise", "Reset",
                       hint: "Soft-reset the console", action: onReset)
            pillButton("power", "Power cycle",
                       hint: "Power-cycle the console", action: onPower)
        }
        .padding(.vertical, 10)
        .padding(.horizontal, 6)
        .background(.ultraThinMaterial, in: Capsule())
        .overlay(Capsule().strokeBorder(Color.white.opacity(0.12)))
        .shadow(radius: 8, y: 2)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(Text("Game menu"))
    }

    private func pillButton(
        _ systemImage: String,
        _ label: LocalizedStringKey,
        hint: LocalizedStringKey,
        tint: Color = .primary,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            Image(systemName: systemImage)
                // A Dynamic-Type-relative font so the glyph tracks the user's text
                // size (capped via the fixed 44pt hit target so the pill stays tidy).
                .font(.title3.weight(.semibold))
                .frame(width: 44, height: 44)
                .contentShape(Rectangle())
        }
        .foregroundStyle(tint)
        .accessibilityLabel(Text(label))
        .accessibilityHint(Text(hint))
    }
}
