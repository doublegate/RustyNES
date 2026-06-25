//
//  TouchControlsOverlay.swift
//
//  SUPERSEDED (v1.9.0): the original single-touch on-screen pad, kept for reference
//  and as a fallback. A single container `DragGesture` reports one primary touch
//  location; `hitTest` OR's together every region that location falls in, so a D-pad
//  *diagonal* combines, but two fingers far apart (the D-pad AND A at once) are NOT
//  both seen, because `DragGesture` surfaces only the primary touch. v1.9.2 replaces
//  this in `GameView` with `MultiTouchControlPad` (a UIView-backed true multi-touch
//  responder). Both share `ControlPadLayout`, so the regions stay identical.
//
//  The mask bit order is NesButton (A=0x01 ... Right=0x80) — the exact order the
//  core's `Buttons` bitflag uses (see NesButtons.swift). Touch input flows through
//  the same late-latched mask path, so determinism is untouched.
//

import SwiftUI

/// Translucent on-screen controller. Calls `onMaskChanged` with the live touch
/// mask whenever the held set changes; the parent ORs it with any hardware-pad
/// mask before forwarding to the core.
struct TouchControlsOverlay: View {
    /// Reports the current touch-only mask (0 when nothing is held).
    let onMaskChanged: (UInt8) -> Void

    var body: some View {
        GeometryReader { geo in
            let layout = ControlPadLayout.make(in: geo.size)
            ZStack {
                ForEach(layout) { b in
                    buttonShape(b)
                }
            }
            // One container-level gesture: recompute the whole mask from every
            // active touch each change, mirroring the Android pointer loop.
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { value in
                        let mask = ControlPadLayout.hitTest(value.location, in: layout)
                        onMaskChanged(mask.bits)
                    }
                    .onEnded { _ in
                        onMaskChanged(0)
                    }
            )
        }
        .allowsHitTesting(true)
    }

    @ViewBuilder
    private func buttonShape(_ b: PadButton) -> some View {
        let fill = Color.white.opacity(0.14)
        let stroke = Color.white.opacity(0.45)
        Group {
            if b.isCircle {
                Circle()
                    .fill(fill)
                    .overlay(Circle().stroke(stroke, lineWidth: 2))
            } else {
                RoundedRectangle(cornerRadius: 8)
                    .fill(fill)
                    .overlay(RoundedRectangle(cornerRadius: 8).stroke(stroke, lineWidth: 2))
            }
        }
        .overlay(
            Text(b.label)
                .font(.system(size: 13, weight: .bold))
                .foregroundColor(.white.opacity(0.7))
        )
        .frame(width: b.frame.width, height: b.frame.height)
        .position(x: b.frame.midX, y: b.frame.midY)
        // Use a spoken name (not the visible `label`, which is empty for the
        // D-pad arms) so VoiceOver announces every control.
        .accessibilityLabel(b.accessibilityName)
    }
}
