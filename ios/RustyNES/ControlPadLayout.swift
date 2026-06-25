//
//  ControlPadLayout.swift
//
//  The shared geometry for the on-screen NES control pad: where each control sits
//  for a given canvas size, and the point->mask hit test. Factored out of the
//  original `TouchControlsOverlay` so both the (now-superseded) single-touch overlay
//  AND the v1.9.2 `MultiTouchControlPad` (the true multi-touch responder) resolve the
//  same regions from one source of truth.
//
//  v1.9.2 visual-parity pass: the geometry below is a faithful port of the Android
//  `VirtualController.kt` constants (the measured NES-001 / NES-004 layout) so the
//  iOS art and hit regions match the Android render exactly. The canvas is drawn at
//  the SAME 123:53 aspect ratio Android uses (`MainActivity.kt`:
//  `.aspectRatio(123f / 53f)`), so every fraction-of-(w,h) constant maps 1:1.
//
//  As on Android, the drawn art (in `MultiTouchControlPad`) AND the hit regions both
//  derive from these same constants, so a resize rescales and remaps them in lockstep
//  -- they can never desync.
//
//  The mask bit order is `NesButton` (A=0x01 ... Right=0x80) -- the exact order the
//  core's `Buttons` bitflag uses (see NesButtons.swift). Every input path lands on
//  the same late-latched bitmask, so determinism is untouched.
//

import CoreGraphics
import Foundation
import UIKit

/// One control hit region of the on-screen pad (used for VoiceOver framing and by the
/// now-superseded single-touch overlay; the live multi-touch hit test uses the shared
/// `ControlPadLayout.hitTest(_:in:)` Android-geometry port directly).
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

/// Pure geometry for the on-screen pad -- no view state, so it is trivially shared
/// between the SwiftUI visual layer and the UIKit touch responder.
enum ControlPadLayout {

    // MARK: - Shared geometry (fractions of w,h)
    //
    // Ported verbatim from Android `VirtualController.kt` (~lines 212-225): measured
    // from the real NES-004 layout, used by BOTH the art and the hit-test so they can
    // never desync. The canvas is the 123:53 aspect Android draws the pad at.

    /// The canvas aspect ratio (width / height) the pad is drawn at -- the NES-001
    /// proportions Android uses (`MainActivity.kt`: `.aspectRatio(123f / 53f)`).
    static let aspectRatio: CGFloat = 123.0 / 53.0

    static let DPAD_CX: CGFloat = 0.165
    static let DPAD_CY: CGFloat = 0.59
    static let SS_CX: CGFloat = 0.461   // SELECT/START white housing centre x
    static let SS_CY: CGFloat = 0.708   // ...and centre y
    static let SS_SELX: CGFloat = 0.393
    static let SS_STAX: CGFloat = 0.530
    static let SS_LABELY: CGFloat = 0.505 // the red SELECT/START labels (their grey stripe)
    static let AB_CY: CGFloat = 0.707
    static let AB_BX: CGFloat = 0.703
    static let AB_AX: CGFloat = 0.831
    static let AB_LABELY: CGFloat = 0.89
    static let RUSTY_CY: CGFloat = 0.351 // the grey "RustyNES" stripe, above SELECT/START

    // MARK: - Accessibility / overlay frames

    /// Build the control regions for the available size, derived from the same measured
    /// geometry as the drawn art. Used for the VoiceOver accessibility frames (and by
    /// the now-superseded single-touch overlay). The live multi-touch responder uses
    /// `hitTest(_:in:)` (the Android-geometry port) for its generous touch slop.
    ///
    /// `idiom` is retained for source compatibility; the proportions are now fixed
    /// fractions (the whole pad is aspect-locked to 123:53 and scales uniformly).
    static func make(in size: CGSize, idiom: UIUserInterfaceIdiom = .phone) -> [PadButton] {
        let w = size.width
        let h = size.height

        // D-pad cross arms (match the drawn black cross).
        let dCx = DPAD_CX * w
        let dCy = DPAD_CY * h
        let daL = 0.216 * h // half-length of an arm
        let daT = 0.07 * h  // half-thickness of an arm

        // A / B drawn circles.
        let abY = AB_CY * h
        let br = 0.046 * w

        // SELECT / START drawn pills.
        let ssY = SS_CY * h
        let pw = 0.079 * w
        let ph = 0.072 * h

        func rect(_ cx: CGFloat, _ cy: CGFloat, _ rw: CGFloat, _ rh: CGFloat) -> CGRect {
            CGRect(x: cx - rw / 2, y: cy - rh / 2, width: rw, height: rh)
        }

        return [
            PadButton(button: .up, frame: CGRect(x: dCx - daT, y: dCy - daL, width: 2 * daT, height: daL), label: "", isCircle: false),
            PadButton(button: .down, frame: CGRect(x: dCx - daT, y: dCy, width: 2 * daT, height: daL), label: "", isCircle: false),
            PadButton(button: .left, frame: CGRect(x: dCx - daL, y: dCy - daT, width: daL, height: 2 * daT), label: "", isCircle: false),
            PadButton(button: .right, frame: CGRect(x: dCx, y: dCy - daT, width: daL, height: 2 * daT), label: "", isCircle: false),
            PadButton(button: .b, frame: rect(AB_BX * w, abY, 2 * br, 2 * br), label: "B", isCircle: true),
            PadButton(button: .a, frame: rect(AB_AX * w, abY, 2 * br, 2 * br), label: "A", isCircle: true),
            PadButton(button: .select, frame: rect(SS_SELX * w, ssY, pw, ph), label: "SEL", isCircle: false),
            PadButton(button: .start, frame: rect(SS_STAX * w, ssY, pw, ph), label: "STA", isCircle: false),
        ]
    }

    /// The red racetrack "MENU" pill -- shared by the art and the menu-toggle hit-test.
    /// Sits where the real controller's wordmark is (top-right, above the A/B buttons).
    /// Ported from Android `logoPillRect` (~lines 428-434).
    static func logoPillRect(in size: CGSize) -> CGRect {
        let w = size.width
        let h = size.height
        let lw = 0.185 * w
        let lh = 0.105 * h
        let cx = (AB_BX + AB_AX) / 2 * w // centred left-right above the A/B squares
        let cy = 0.33 * h                // the real controller's wordmark y (measured 0.330)
        return CGRect(x: cx - lw / 2, y: cy - lh / 2, width: lw, height: lh)
    }

    // MARK: - Hit testing

    /// The live multi-touch hit test: a faithful port of Android `hitTest`
    /// (`VirtualController.kt` ~lines 178-210). Derives every region from the SAME
    /// fractional geometry as the art, with the same generous touch slop, and OR's
    /// together every region the point falls in. The D-pad is a square active area
    /// around the cross with a small deadzone, so a finger toward a corner registers a
    /// diagonal (two bits).
    static func hitTest(_ point: CGPoint, in size: CGSize) -> NesButtonMask {
        let w = size.width
        let h = size.height
        let px = point.x
        let py = point.y
        var mask = NesButtonMask()

        // D-pad: square active area around the cross; direction from the offset.
        let dCx = DPAD_CX * w
        let dCy = DPAD_CY * h
        let dHalf = 0.225 * h
        if abs(px - dCx) < dHalf && abs(py - dCy) < dHalf {
            let dz = 0.05 * h
            let dx = px - dCx
            let dy = py - dCy
            if dy < -dz { mask.set(.up, pressed: true) }
            if dy > dz { mask.set(.down, pressed: true) }
            if dx < -dz { mask.set(.left, pressed: true) }
            if dx > dz { mask.set(.right, pressed: true) }
        }

        // A / B: circles (with touch slop). NES layout: B left, A right.
        let abY = AB_CY * h
        let br = 0.082 * w
        if hypot(px - AB_AX * w, py - abY) < br { mask.set(.a, pressed: true) }
        if hypot(px - AB_BX * w, py - abY) < br { mask.set(.b, pressed: true) }

        // Select / Start: rounded rects (generous hit area for the small black pills).
        let ssY = SS_CY * h
        let sHw = 0.055 * w
        let sHh = 0.075 * h
        if abs(px - SS_SELX * w) < sHw && abs(py - ssY) < sHh { mask.set(.select, pressed: true) }
        if abs(px - SS_STAX * w) < sHw && abs(py - ssY) < sHh { mask.set(.start, pressed: true) }

        return mask
    }

    /// Rect-based hit test retained for the now-superseded single-touch
    /// `TouchControlsOverlay` (which passes a `[PadButton]` layout). New code uses the
    /// `hitTest(_:in:)` size overload above (the Android-geometry port).
    static func hitTest(_ point: CGPoint, in layout: [PadButton]) -> NesButtonMask {
        var mask = NesButtonMask()
        for b in layout where b.frame.insetBy(dx: -6, dy: -6).contains(point) {
            mask.set(b.button, pressed: true)
        }
        return mask
    }
}
