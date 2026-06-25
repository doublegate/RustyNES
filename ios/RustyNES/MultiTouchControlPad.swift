//
//  MultiTouchControlPad.swift
//
//  The v1.9.2 TRUE multi-touch on-screen NES-001 control pad -- the replacement for
//  the v1.9.0 `TouchControlsOverlay`, whose single SwiftUI `DragGesture` could only
//  surface the primary touch and so never saw two fingers far apart (e.g. the D-pad
//  AND the A button at once). Here the touch layer is a UIKit `UIView`
//  (`isMultipleTouchEnabled = true`) that tracks EVERY active touch; on each touch
//  change it recomputes the combined `NesButtonMask` by OR-ing each touch's location
//  against the shared `ControlPadLayout` regions, then reports the byte up the exact
//  same path the old overlay used (`onMaskChanged(UInt8)` -> `AppModel.setTouchMask`
//  -> `EmulatorCore.setButtons(port:0:)`). The core and its late-latch are untouched.
//
//  v1.9.2 visual-parity pass: the VISUALS are now a faithful SwiftUI `Canvas` port of
//  the Android `VirtualController.kt` `drawNesController` render (the measured NES-001
//  / NES-004 controller) -- body shell + asymmetric bezel + near-black face + grey
//  decorative stripes + "RustyNES" wordmark + white SELECT/START housing with black
//  inset frame + black pills + the black-cross D-pad (white outline, grey face,
//  outward arrows, centre circle) + the red "MENU" racetrack pill + the two white A/B
//  housings with domed red radial-gradient buttons + all the SELECT/START/A/B/MENU/
//  RustyNES labels. The palette + geometry constants are ported 1:1 from Android, and
//  the canvas is drawn at the SAME 123:53 aspect ratio (see ControlPadLayout).
//
//  Composition (a ZStack): the SwiftUI `Canvas` draws the controller (crisp at any
//  scale and reflecting the LIVE held mask -- pressed pills/arms/A/B light up). An
//  invisible labelled accessibility layer sits over it so VoiceOver still announces
//  each control. A transparent `UIViewRepresentable` multi-touch layer sits on top to
//  capture the raw multi-touch; it is `accessibilityHidden` so VoiceOver reads the
//  labelled layer beneath it.
//
//  Pressed-state lighting: the multi-touch responder publishes the live mask back into
//  a `@State` here (the `onMaskChanged` callback updates `liveMask` in addition to
//  forwarding it to the model), so the `Canvas` redraws with the held buttons lit --
//  the SwiftUI analog of Android's `mutableIntStateOf(mask)` driving the Compose
//  `Canvas`.
//
//  Font: the labels are set in the same two BUNDLED faces the Android pad uses, for
//  glyph parity — "Nes Controller" (Fonts/NESController.ttf) for the button labels
//  (SELECT/START/A/B/MENU) and "Press Start 2P" (OFL-1.1, Fonts/PressStart2P.ttf) for
//  the "RustyNES" wordmark — both registered via Info.plist UIAppFonts, in NES red,
//  each falling back to a bold monospaced system face if its font fails to register.
//  (The "Nes Controller" .ttf's FontCreator default placeholder copyright
//  "(c) (your company). 2009. All Rights Reserved" was stripped on both platforms.)
//

import SwiftUI
import UIKit

/// The true multi-touch on-screen pad. Calls `onMaskChanged` with the live combined
/// touch mask whenever the held set changes (0 when nothing is held); `onLogoTap` fires
/// when the red MENU pill is pressed (the menu toggle).
struct MultiTouchControlPad: View {
    /// Reports the current touch-only mask; the parent ORs it with any hardware-pad
    /// mask before forwarding to the core (same contract as `TouchControlsOverlay`).
    let onMaskChanged: (UInt8) -> Void
    /// Fired when the MENU pill is tapped (the on-screen menu toggle).
    var onLogoTap: () -> Void = {}

    /// The live pressed-button mask, used to light the drawn art. The multi-touch
    /// responder updates this through `onMaskChanged` so held buttons glow.
    @State private var liveMask: UInt8 = 0

    var body: some View {
        GeometryReader { geo in
            let buttons = ControlPadLayout.make(in: geo.size)
            ZStack {
                // Visual layer: the faithful NES-001 render, reflecting the live mask.
                NesControllerCanvas(mask: liveMask)
                    .allowsHitTesting(false)

                // Accessibility layer: invisible labelled rects so VoiceOver still
                // announces each control (the Canvas itself is one opaque node).
                ForEach(buttons) { button in
                    Color.clear
                        .frame(width: button.frame.width, height: button.frame.height)
                        .position(x: button.frame.midX, y: button.frame.midY)
                        .accessibilityElement()
                        .accessibilityLabel(button.accessibilityName)
                        .accessibilityAddTraits(.isButton)
                }

                // Touch layer (UIKit multi-touch), transparent and on top.
                MultiTouchSurface(
                    onMaskChanged: { mask in
                        liveMask = mask
                        onMaskChanged(mask)
                    },
                    onLogoTap: onLogoTap
                )
                .accessibilityHidden(true)
            }
        }
        .allowsHitTesting(true)
    }
}

// MARK: - Visuals (faithful NES-001 render, SwiftUI Canvas)

/// The drawn NES-001 controller. A SwiftUI `Canvas` is the direct analog of Compose's
/// `DrawScope`: it hands a `GraphicsContext` with fill/stroke of `Path`, clipping, and
/// gradient shading. `mask` is the live held-button set; pressed controls are lit.
private struct NesControllerCanvas: View {
    let mask: UInt8

    var body: some View {
        Canvas { context, size in
            NesControllerArt.draw(into: &context, size: size, mask: mask)
        }
    }
}

// MARK: - Palette (ported 1:1 from Android `VirtualController.kt` ~lines 230-245)

private enum NesPad {
    /// Translate an Android `Color(0xFFRRGGBB)` to a SwiftUI `Color` (0-1 channels).
    static func c(_ r: Double, _ g: Double, _ b: Double, _ a: Double = 1) -> Color {
        Color(red: r / 255, green: g / 255, blue: b / 255, opacity: a)
    }

    static let body = c(211, 207, 198)      // 0xFFD3CFC6 light warm-grey plastic shell
    static let bodyEdge = c(142, 139, 130)  // 0xFF8E8B82
    static let face = c(20, 20, 22)         // 0xFF141416 near-black central face
    static let cross = c(20, 20, 22)        // 0xFF141416 D-pad cross (matches the face)
    static let crossOut = c(222, 222, 222)  // 0xFFDEDEDE white cross outline
    static let arrowDk = c(0, 0, 0)         // 0xFF000000 dark directional-arrow outlines
    static let crossFace = c(40, 40, 43)    // 0xFF28282B dark-grey cross face
    static let stripe = c(133, 133, 133)    // 0xFF858585 grey decorative stripes
    static let housingW = c(222, 222, 222)  // 0xFFDEDEDE white SELECT/START + A/B housings
    static let housingE = c(154, 152, 143)  // 0xFF9A988F
    static let pillBlk = c(20, 20, 22)      // 0xFF141416 black SELECT/START pills
    static let btnRed = c(232, 24, 16)      // 0xFFE81810 A/B base red
    static let btnRedHi = c(248, 88, 76)    // 0xFFF8584C domed-button highlight
    static let btnRedLo = c(204, 24, 16)    // 0xFFCC1810 recessed-centre tint
    static let red = c(206, 32, 24)         // 0xFFCE2018 labels + RustyNES + MENU pill
    static let lit = c(255, 255, 255, 0x44 / 255.0) // 0x44FFFFFF pressed-state overlay (~0.27)
}

// MARK: - Drawing helpers (the GraphicsContext analogs of Compose DrawScope calls)

/// `drawRoundRect(color, ..., rr)` fill -> `context.fill(Path(roundedRect:cornerRadius:))`.
private func fillRR(_ ctx: inout GraphicsContext, _ color: Color,
                    _ x: CGFloat, _ y: CGFloat, _ w: CGFloat, _ h: CGFloat, _ r: CGFloat) {
    ctx.fill(Path(roundedRect: CGRect(x: x, y: y, width: w, height: h), cornerRadius: r), with: .color(color))
}

/// `drawRoundRect(..., style = Stroke(lw))` -> `context.stroke(Path(roundedRect:...))`.
private func strokeRR(_ ctx: inout GraphicsContext, _ color: Color,
                      _ x: CGFloat, _ y: CGFloat, _ w: CGFloat, _ h: CGFloat, _ r: CGFloat, _ lw: CGFloat) {
    ctx.stroke(Path(roundedRect: CGRect(x: x, y: y, width: w, height: h), cornerRadius: r), with: .color(color), lineWidth: lw)
}

/// The NES-001 controller art -- a faithful port of Android `drawNesController`
/// (`VirtualController.kt` ~lines 247-423). `ctx` is the Canvas `inout GraphicsContext`
/// (the `(inout GraphicsContext, CGSize)` renderer parameter).
private enum NesControllerArt {
    static func draw(into ctx: inout GraphicsContext, size: CGSize, mask: UInt8) {
        let w = size.width
        let h = size.height

        func has(_ b: NesButton) -> Bool { mask & b.rawValue != 0 }

        // --- Body + edge, then the near-black central face. The white-plastic borders
        //     are asymmetric like the real shell: thick top, thin bottom, thin sides.
        fillRR(&ctx, NesPad.body, 0, 0, w, h, 0.022 * h)
        strokeRR(&ctx, NesPad.bodyEdge, 0, 0, w, h, 0.022 * h, 0.014 * h)
        let faceT = 0.167 * h
        let faceB = 0.939 * h
        // Sharp (un-rounded) corners where the black face meets the white border.
        ctx.fill(Path(CGRect(x: 0.027 * w, y: faceT, width: 0.946 * w, height: faceB - faceT)),
                 with: .color(NesPad.face))

        // --- Four grey decorative stripes down the centre, clipped to the face. The
        //     "RustyNES" stripe is RUSTY_CY, the SELECT/START stripe is SS_LABELY.
        let stL = 0.321 * w
        let stW = 0.28 * w
        do {
            var clipped = ctx
            clipped.clip(to: Path(CGRect(x: 0.027 * w, y: faceT, width: 0.946 * w, height: faceB - faceT)))
            for cyf in [0.199, ControlPadLayout.RUSTY_CY, ControlPadLayout.SS_LABELY, 0.924] as [CGFloat] {
                fillRR(&clipped, NesPad.stripe, stL, cyf * h - 0.059 * h, stW, 0.118 * h, 0.042 * h)
            }
        }

        // --- SELECT / START: a WHITE housing with two black pills (red labels later).
        let ssY = ControlPadLayout.SS_CY * h
        let hsW = 0.28 * w
        let hsH = 0.222 * h
        let hsL = ControlPadLayout.SS_CX * w - hsW / 2
        let hsT = ssY - hsH / 2
        fillRR(&ctx, NesPad.housingW, hsL, hsT, hsW, hsH, 0.055 * h)
        strokeRR(&ctx, NesPad.housingE, hsL, hsT, hsW, hsH, 0.055 * h, 0.006 * h)
        // Black inset border inside the white housing (the recessed SELECT/START plate).
        let ins = 0.016 * w
        strokeRR(&ctx, NesPad.pillBlk, hsL + ins, hsT + 0.022 * h, hsW - 2 * ins, hsH - 0.044 * h, 0.038 * h, 0.007 * h)
        let pw = 0.079 * w
        let ph = 0.072 * h
        let selX = ControlPadLayout.SS_SELX * w
        fillRR(&ctx, NesPad.pillBlk, selX - pw / 2, ssY - ph / 2, pw, ph, ph / 2)
        if has(.select) { fillRR(&ctx, NesPad.lit, selX - pw / 2, ssY - ph / 2, pw, ph, ph / 2) }
        let staX = ControlPadLayout.SS_STAX * w
        fillRR(&ctx, NesPad.pillBlk, staX - pw / 2, ssY - ph / 2, pw, ph, ph / 2)
        if has(.start) { fillRR(&ctx, NesPad.lit, staX - pw / 2, ssY - ph / 2, pw, ph, ph / 2) }

        // --- D-pad: a black cross with a WHITE OUTLINE (draw white bars, then inset
        //     black bars), grey cross face, outward arrows, and a centre circle.
        let dCx = ControlPadLayout.DPAD_CX * w
        let dCy = ControlPadLayout.DPAD_CY * h
        let daL = 0.216 * h // half-length incl. outline
        let daT = 0.07 * h  // half-thickness
        let ow = 0.012 * h
        fillRR(&ctx, NesPad.crossOut, dCx - daT - ow, dCy - daL - ow, 2 * (daT + ow), 2 * (daL + ow), 0.025 * h)
        fillRR(&ctx, NesPad.crossOut, dCx - daL - ow, dCy - daT - ow, 2 * (daL + ow), 2 * (daT + ow), 0.025 * h)
        fillRR(&ctx, NesPad.cross, dCx - daT, dCy - daL, 2 * daT, 2 * daL, 0.02 * h)
        fillRR(&ctx, NesPad.cross, dCx - daL, dCy - daT, 2 * daL, 2 * daT, 0.02 * h)
        // Lighter grey cross FACE inset within the black band.
        let fb = 0.018 * h
        fillRR(&ctx, NesPad.crossFace, dCx - daT + fb, dCy - daL + fb, 2 * (daT - fb), 2 * (daL - fb), 0.014 * h)
        fillRR(&ctx, NesPad.crossFace, dCx - daL + fb, dCy - daT + fb, 2 * (daL - fb), 2 * (daT - fb), 0.014 * h)

        // Dark, head+shaft directional arrows (outline) near each tip, pointing out.
        func dpadArrow(_ dxn: CGFloat, _ dyn: CGFloat) {
            let dist = daL * 0.64
            let hw = 0.045 * h // head half-width
            let hh = 0.052 * h // head (triangle) length
            let sw = 0.023 * h // shaft half-width (~half the head)
            let sh = 0.044 * h // shaft length
            // Same rotation math as Android `pt(along, perp)`.
            func pt(_ along: CGFloat, _ perp: CGFloat) -> CGPoint {
                CGPoint(x: dCx + dxn * along - dyn * perp, y: dCy + dyn * along + dxn * perp)
            }
            let tipA = dist + (hh + sh) / 2
            var p = Path()
            p.move(to: pt(tipA, 0))
            p.addLine(to: pt(tipA - hh, hw))
            p.addLine(to: pt(tipA - hh, sw))
            p.addLine(to: pt(tipA - hh - sh, sw))
            p.addLine(to: pt(tipA - hh - sh, -sw))
            p.addLine(to: pt(tipA - hh, -sw))
            p.addLine(to: pt(tipA - hh, -hw))
            p.closeSubpath()
            ctx.stroke(p, with: .color(NesPad.arrowDk), lineWidth: 0.009 * h)
        }
        dpadArrow(0, -1)
        dpadArrow(0, 1)
        dpadArrow(-1, 0)
        dpadArrow(1, 0)

        // Centre circle -- hollow (grey face fill) with a black outline.
        let cr = 0.05 * h
        let centreRect = CGRect(x: dCx - cr, y: dCy - cr, width: 2 * cr, height: 2 * cr)
        ctx.fill(Path(ellipseIn: centreRect), with: .color(NesPad.crossFace))
        ctx.stroke(Path(ellipseIn: centreRect), with: .color(NesPad.arrowDk), lineWidth: 0.007 * h)

        // Lit arms.
        if has(.up) { fillRR(&ctx, NesPad.lit, dCx - daT, dCy - daL, 2 * daT, daL, 0.018 * h) }
        if has(.down) { fillRR(&ctx, NesPad.lit, dCx - daT, dCy, 2 * daT, daL, 0.018 * h) }
        if has(.left) { fillRR(&ctx, NesPad.lit, dCx - daL, dCy - daT, daL, 2 * daT, 0.018 * h) }
        if has(.right) { fillRR(&ctx, NesPad.lit, dCx, dCy - daT, daL, 2 * daT, 0.018 * h) }

        // --- The red racetrack "MENU" pill (top-right, above A/B; the menu toggle).
        let pill = ControlPadLayout.logoPillRect(in: size)
        strokeRR(&ctx, NesPad.red, pill.minX, pill.minY, pill.width, pill.height, pill.height / 2, 0.016 * h)

        // --- A / B: two WHITE rounded-square housings, each holding a domed red button
        //     (a radial gradient from a recessed centre to a lit rim).
        let abY = ControlPadLayout.AB_CY * h
        let sqW = 0.112 * w
        let sqH = 0.271 * h
        let br = 0.046 * w
        for (bx, bit) in [(ControlPadLayout.AB_BX, NesButton.b), (ControlPadLayout.AB_AX, NesButton.a)] {
            let cx = bx * w
            fillRR(&ctx, NesPad.housingW, cx - sqW / 2, abY - sqH / 2, sqW, sqH, 0.035 * h)
            strokeRR(&ctx, NesPad.housingE, cx - sqW / 2, abY - sqH / 2, sqW, sqH, 0.035 * h, 0.005 * h)
            // CONCAVE dish: the radial gradient centre is nudged up by br*0.22, matching
            // Android's `Brush.radialGradient(center = Offset(cx, abY - br * 0.22))`.
            let buttonRect = CGRect(x: cx - br, y: abY - br, width: 2 * br, height: 2 * br)
            ctx.fill(
                Path(ellipseIn: buttonRect),
                with: .radialGradient(
                    Gradient(colors: [NesPad.btnRedLo, NesPad.btnRed, NesPad.btnRedHi]),
                    center: CGPoint(x: cx, y: abY - br * 0.22),
                    startRadius: 0,
                    endRadius: br * 1.18
                )
            )
            if has(bit) { ctx.fill(Path(ellipseIn: buttonRect), with: .color(NesPad.lit)) }
        }

        // --- Labels, in the BUNDLED NES-era faces for glyph parity with the Android
        //     pad: the button labels (SELECT/START/A/B/MENU) in "Nes Controller" and
        //     the "RustyNES" wordmark in "Press Start 2P" (OFL-1.1) — exactly the two
        //     faces and sizes the Android pad uses. In NES red; each falls back to a
        //     bold monospaced system face if its font failed to register. Sizes are
        //     Android's textSize fractions of h; exact cap-height centring is an
        //     on-device tuning detail for the iOS dev.
        func resolved(_ name: String, _ available: Bool, _ size: CGFloat) -> Font {
            available ? .custom(name, fixedSize: size)
                : .system(size: size, weight: .bold, design: .monospaced)
        }
        func label(_ s: String, _ x: CGFloat, _ y: CGFloat, _ font: Font) {
            ctx.draw(
                Text(verbatim: s).font(font).foregroundColor(NesPad.red),
                at: CGPoint(x: x, y: y), anchor: .center
            )
        }
        func nesFont(_ size: CGFloat) -> Font { resolved(nesControllerName, nesControllerAvailable, size) }
        // Button labels in "Nes Controller" at Android's sizes (SELECT/START 0.087h,
        // B/A 0.104h, MENU 0.087h).
        label("SELECT", ControlPadLayout.SS_SELX * w, ControlPadLayout.SS_LABELY * h, nesFont(0.087 * h))
        label("START", ControlPadLayout.SS_STAX * w, ControlPadLayout.SS_LABELY * h, nesFont(0.087 * h))
        // B/A sit toward the bottom-right of each square (right third), per Android.
        label("B", ControlPadLayout.AB_BX * w + 0.05 * w, ControlPadLayout.AB_LABELY * h, nesFont(0.104 * h))
        label("A", ControlPadLayout.AB_AX * w + 0.05 * w, ControlPadLayout.AB_LABELY * h, nesFont(0.104 * h))
        label("M E N U", pill.midX, pill.midY, nesFont(0.087 * h))
        // "RustyNES" wordmark in Press Start 2P, centred on the SELECT/START housing x.
        label(
            "RustyNES", ControlPadLayout.SS_CX * w, ControlPadLayout.RUSTY_CY * h,
            resolved(pressStart2PName, pressStart2PAvailable, 0.06 * h)
        )
    }
}

/// The two bundled NES-era label faces (registered via Info.plist `UIAppFonts`),
/// matching the Android pad for glyph parity: "Nes Controller" for the button labels,
/// "Press Start 2P" (OFL-1.1) for the wordmark. The family names resolve for both
/// `Font.custom` and the `UIFont` availability probes; each falls back to a bold
/// monospaced system face if its font failed to register.
private let nesControllerName = "Nes Controller"
private let nesControllerAvailable = UIFont(name: nesControllerName, size: 12) != nil
private let pressStart2PName = "Press Start 2P"
private let pressStart2PAvailable = UIFont(name: pressStart2PName, size: 12) != nil

// MARK: - Touch layer (UIKit true multi-touch)

/// Bridges a transparent multi-touch `UIView` into SwiftUI. The view tracks all active
/// touches and reports the combined mask; the SwiftUI side keeps the visuals.
private struct MultiTouchSurface: UIViewRepresentable {
    let onMaskChanged: (UInt8) -> Void
    let onLogoTap: () -> Void

    func makeUIView(context: Context) -> MultiTouchPadView {
        let view = MultiTouchPadView()
        view.onMaskChanged = onMaskChanged
        view.onLogoTap = onLogoTap
        return view
    }

    func updateUIView(_ view: MultiTouchPadView, context: Context) {
        // Re-push the latest callbacks (they capture fresh @State each render).
        view.onMaskChanged = onMaskChanged
        view.onLogoTap = onLogoTap
    }
}

/// A transparent view that sees every finger. On any touch phase change it recomputes
/// the combined NES mask from ALL touches currently down on it, hit-tested against the
/// shared `ControlPadLayout` geometry (sized from this view's own bounds, so the art
/// and hit regions stay in lockstep). This is the core of the v1.9.2 multi-touch fix:
/// two fingers far apart are both honoured.
///
/// The red MENU pill is a tap target (toggle the menu), not an NES button -- a touch
/// that goes down on the pill is "owned" by it for the whole gesture, so dragging out
/// of the pill never presses a button (mirrors Android's `pillPointers` set).
final class MultiTouchPadView: UIView {
    var onMaskChanged: ((UInt8) -> Void)?
    var onLogoTap: (() -> Void)?

    private var lastBits: UInt8 = 0
    private var pillTouches = Set<ObjectIdentifier>()

    override init(frame: CGRect) {
        super.init(frame: frame)
        isMultipleTouchEnabled = true
        isUserInteractionEnabled = true
        backgroundColor = .clear
        isOpaque = false
        // The visuals + VoiceOver labels live in the SwiftUI layer below; this layer
        // is purely for capturing touches, so it stays out of the accessibility tree.
        isAccessibilityElement = false
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        let pill = ControlPadLayout.logoPillRect(in: bounds.size)
        for touch in touches where pill.contains(touch.location(in: self)) {
            onLogoTap?()
            pillTouches.insert(ObjectIdentifier(touch))
        }
        recompute(event)
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) { recompute(event) }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches { pillTouches.remove(ObjectIdentifier(touch)) }
        recompute(event)
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        for touch in touches { pillTouches.remove(ObjectIdentifier(touch)) }
        recompute(event)
    }

    /// Rebuild the mask from every live touch on this view (skipping touches owned by
    /// the MENU pill). Using `event.allTouches` (filtered to non-lifted phases and to
    /// touches owned by this view) avoids retaining `UITouch` objects across the
    /// sequence.
    private func recompute(_ event: UIEvent?) {
        var mask = NesButtonMask()
        for touch in event?.allTouches ?? [] where touch.view === self {
            if pillTouches.contains(ObjectIdentifier(touch)) { continue }
            switch touch.phase {
            case .began, .moved, .stationary:
                mask.formUnion(ControlPadLayout.hitTest(touch.location(in: self), in: bounds.size))
            case .ended, .cancelled:
                break
            @unknown default:
                break
            }
        }
        guard mask.bits != lastBits else { return }
        lastBits = mask.bits
        onMaskChanged?(mask.bits)
    }
}
