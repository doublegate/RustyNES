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
    var onLibrary: () -> Void
    var onStates: () -> Void
    var onMovies: () -> Void
    var onSettings: () -> Void
    var onReset: () -> Void
    var onPower: () -> Void

    var body: some View {
        VStack(spacing: 4) {
            pillButton("rectangle.stack", "Library", action: onLibrary)
            pillButton("tray.full", "Save states", action: onStates)
            pillButton("film", "TAS / Movies", action: onMovies)
            pillButton("gearshape", "Settings", action: onSettings)
            pillButton("arrow.counterclockwise", "Reset", action: onReset)
            pillButton("power", "Power cycle", action: onPower)
        }
        .padding(.vertical, 10)
        .padding(.horizontal, 6)
        .background(.ultraThinMaterial, in: Capsule())
        .overlay(Capsule().strokeBorder(Color.white.opacity(0.12)))
        .shadow(radius: 8, y: 2)
    }

    private func pillButton(_ systemImage: String, _ label: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: systemImage)
                .font(.system(size: 18, weight: .semibold))
                .frame(width: 44, height: 44)
                .contentShape(Rectangle())
        }
        .foregroundStyle(.primary)
        .accessibilityLabel(label)
    }
}
