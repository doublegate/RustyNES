//
//  TouchControlsOverlay.swift
//
//  The on-screen NES control pad: a translucent D-pad on the left and the A/B +
//  Select/Start buttons on the right, overlaid on the game view. A single
//  container `DragGesture` reports one primary touch location; `hitTest` OR's
//  together every region that location falls in, so a D-pad *diagonal* (the
//  finger straddling two adjacent arms) combines, and a face button under the
//  same finger combines. NOTE: this is NOT full multi-touch — two fingers far
//  apart (e.g. the D-pad AND the A button simultaneously) are not both seen,
//  because SwiftUI's `DragGesture` surfaces only the primary touch. A hardware
//  controller (GameControllerManager) is the path for true simultaneous input;
//  a `UIView`-backed multi-touch overlay is a documented v1.9.x follow-up.
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

    /// One translucent button hit region.
    private struct PadButton: Identifiable {
        let id = UUID()
        let button: NesButton
        let frame: CGRect
        let label: String
        let isCircle: Bool
    }

    var body: some View {
        GeometryReader { geo in
            let layout = makeLayout(in: geo.size)
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
                        let mask = hitTest(value.location, in: layout)
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
        .accessibilityLabel(accessibilityName(b.button))
    }

    /// The VoiceOver-spoken name for a control (the visible glyph/label is empty
    /// for the D-pad arms and terse for the rest).
    private func accessibilityName(_ button: NesButton) -> String {
        switch button {
        case .up: return "D-pad Up"
        case .down: return "D-pad Down"
        case .left: return "D-pad Left"
        case .right: return "D-pad Right"
        case .a: return "A button"
        case .b: return "B button"
        case .select: return "Select"
        case .start: return "Start"
        }
    }

    // MARK: - Layout

    /// Build the button regions for the available size. The D-pad sits bottom-left,
    /// A/B bottom-right, Select/Start centred along the bottom.
    private func makeLayout(in size: CGSize) -> [PadButton] {
        let w = size.width
        let h = size.height
        // D-pad cluster (left).
        let dCx = w * 0.16
        let dCy = h * 0.72
        let arm: CGFloat = min(w, h) * 0.10
        // A/B cluster (right). B left, A right (NES layout).
        let abY = h * 0.72
        let bCx = w * 0.78
        let aCx = w * 0.90
        let r = min(w, h) * 0.075
        // Select/Start (bottom centre).
        let ssY = h * 0.92
        let selX = w * 0.42
        let staX = w * 0.58
        let pillW = w * 0.10
        let pillH = h * 0.06

        func rect(center: CGPoint, w: CGFloat, h: CGFloat) -> CGRect {
            CGRect(x: center.x - w / 2, y: center.y - h / 2, width: w, height: h)
        }

        return [
            PadButton(button: .up, frame: rect(center: CGPoint(x: dCx, y: dCy - arm), w: arm, h: arm), label: "", isCircle: false),
            PadButton(button: .down, frame: rect(center: CGPoint(x: dCx, y: dCy + arm), w: arm, h: arm), label: "", isCircle: false),
            PadButton(button: .left, frame: rect(center: CGPoint(x: dCx - arm, y: dCy), w: arm, h: arm), label: "", isCircle: false),
            PadButton(button: .right, frame: rect(center: CGPoint(x: dCx + arm, y: dCy), w: arm, h: arm), label: "", isCircle: false),
            PadButton(button: .b, frame: rect(center: CGPoint(x: bCx, y: abY), w: r * 2, h: r * 2), label: "B", isCircle: true),
            PadButton(button: .a, frame: rect(center: CGPoint(x: aCx, y: abY), w: r * 2, h: r * 2), label: "A", isCircle: true),
            PadButton(button: .select, frame: rect(center: CGPoint(x: selX, y: ssY), w: pillW, h: pillH), label: "SEL", isCircle: false),
            PadButton(button: .start, frame: rect(center: CGPoint(x: staX, y: ssY), w: pillW, h: pillH), label: "STA", isCircle: false),
        ]
    }

    /// Recompute the pressed mask from a single active touch point. SwiftUI's
    /// DragGesture surfaces one primary location; for true multi-touch the parent
    /// can layer multiple overlays, but each region OR's independently so adjacent
    /// D-pad cells (diagonals) and a face button under the same finger combine.
    private func hitTest(_ point: CGPoint, in layout: [PadButton]) -> NesButtonMask {
        var mask = NesButtonMask()
        for b in layout where b.frame.insetBy(dx: -6, dy: -6).contains(point) {
            mask.set(b.button, pressed: true)
        }
        return mask
    }
}
