//
//  ControlPadLayout.swift
//
//  The shared geometry for the on-screen NES control pad: where each translucent
//  button sits for a given canvas size, and the point->mask hit test. Factored out
//  of the original `TouchControlsOverlay` so both the (now-superseded) single-touch
//  overlay AND the v1.9.2 `MultiTouchControlPad` (the true multi-touch responder)
//  resolve the same regions from one source of truth.
//
//  Layout is computed entirely from the available size (a GeometryReader size), so
//  it letterboxes/scales cleanly across iPhone/iPad, portrait/landscape, split view
//  and Stage Manager. The idiom only nudges the proportions (wider spacing + larger
//  minimum touch targets on iPad); there are no fixed pixel sizes.
//
//  The mask bit order is `NesButton` (A=0x01 ... Right=0x80) — the exact order the
//  core's `Buttons` bitflag uses (see NesButtons.swift). Every input path lands on
//  the same late-latched bitmask, so determinism is untouched.
//

import CoreGraphics
import UIKit

/// One translucent button hit region of the on-screen pad.
struct PadButton: Identifiable {
    let button: NesButton
    let frame: CGRect
    /// The visible glyph ("A"/"B"/"SEL"/"STA"); empty for the D-pad arms.
    let label: String
    let isCircle: Bool

    /// Stable identity (a control appears at most once per layout).
    var id: UInt8 { button.rawValue }

    /// The VoiceOver-spoken name (the visible glyph is empty for the D-pad arms and
    /// terse for the rest), preserved from the original overlay.
    var accessibilityName: String {
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
}

/// Pure geometry for the on-screen pad — no view state, so it is trivially shared
/// between the SwiftUI visual layer and the UIKit touch responder.
enum ControlPadLayout {
    /// Build the button regions for the available size. The D-pad sits bottom-left,
    /// A/B bottom-right, Select/Start centred along the bottom. On iPad the clusters
    /// spread a little wider and the targets carry larger minimum sizes.
    static func make(in size: CGSize, idiom: UIUserInterfaceIdiom = .phone) -> [PadButton] {
        let w = size.width
        let h = size.height
        let m = min(w, h)
        let pad = idiom == .pad

        // D-pad cluster (left).
        let dCx = w * (pad ? 0.14 : 0.16)
        let dCy = h * (pad ? 0.74 : 0.72)
        let arm = max(m * (pad ? 0.085 : 0.10), pad ? 52 : 32)

        // A/B cluster (right). B left, A right (NES layout).
        let abY = h * (pad ? 0.74 : 0.72)
        let bCx = w * (pad ? 0.80 : 0.78)
        let aCx = w * (pad ? 0.92 : 0.90)
        let r = max(m * (pad ? 0.065 : 0.075), pad ? 40 : 26)

        // Select/Start (bottom centre, NES-001 pills).
        let ssY = h * (pad ? 0.93 : 0.92)
        let selX = w * (pad ? 0.44 : 0.42)
        let staX = w * (pad ? 0.56 : 0.58)
        let pillW = max(w * (pad ? 0.09 : 0.10), 64)
        let pillH = max(h * (pad ? 0.055 : 0.06), 28)

        func rect(_ cx: CGFloat, _ cy: CGFloat, _ rw: CGFloat, _ rh: CGFloat) -> CGRect {
            CGRect(x: cx - rw / 2, y: cy - rh / 2, width: rw, height: rh)
        }

        return [
            PadButton(button: .up, frame: rect(dCx, dCy - arm, arm, arm), label: "", isCircle: false),
            PadButton(button: .down, frame: rect(dCx, dCy + arm, arm, arm), label: "", isCircle: false),
            PadButton(button: .left, frame: rect(dCx - arm, dCy, arm, arm), label: "", isCircle: false),
            PadButton(button: .right, frame: rect(dCx + arm, dCy, arm, arm), label: "", isCircle: false),
            PadButton(button: .b, frame: rect(bCx, abY, r * 2, r * 2), label: "B", isCircle: true),
            PadButton(button: .a, frame: rect(aCx, abY, r * 2, r * 2), label: "A", isCircle: true),
            PadButton(button: .select, frame: rect(selX, ssY, pillW, pillH), label: "SEL", isCircle: false),
            PadButton(button: .start, frame: rect(staX, ssY, pillW, pillH), label: "STA", isCircle: false),
        ]
    }

    /// OR together every region a point falls in. A small inset makes the D-pad
    /// diagonals (a finger straddling two adjacent arms) and edge presses forgiving.
    static func hitTest(_ point: CGPoint, in layout: [PadButton]) -> NesButtonMask {
        var mask = NesButtonMask()
        for b in layout where b.frame.insetBy(dx: -6, dy: -6).contains(point) {
            mask.set(b.button, pressed: true)
        }
        return mask
    }
}
