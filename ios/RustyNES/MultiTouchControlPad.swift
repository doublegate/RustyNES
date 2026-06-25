//
//  MultiTouchControlPad.swift
//
//  The v1.9.2 TRUE multi-touch on-screen NES-001 control pad — the replacement for
//  the v1.9.0 `TouchControlsOverlay`, whose single SwiftUI `DragGesture` could only
//  surface the primary touch and so never saw two fingers far apart (e.g. the D-pad
//  AND the A button at once). Here the touch layer is a UIKit `UIView`
//  (`isMultipleTouchEnabled = true`) that tracks EVERY active touch; on each touch
//  change it recomputes the combined `NesButtonMask` by OR-ing each touch's location
//  against the shared `ControlPadLayout` regions, then reports the byte up the exact
//  same path the old overlay used (`onMaskChanged(UInt8)` -> `AppModel.setTouchMask`
//  -> `EmulatorCore.setButtons(port:0:)`). The core and its late-latch are untouched.
//
//  Composition: the translucent NES-001 button VISUALS are drawn in SwiftUI (crisp at
//  any scale, and they carry the VoiceOver accessibility labels). A transparent
//  `UIViewRepresentable` touch layer sits on top of them in the ZStack to capture the
//  raw multi-touch; it is marked `accessibilityHidden` so VoiceOver reads the labelled
//  SwiftUI shapes beneath it. (If on-device testing shows the touch layer shadowing
//  VoiceOver, move the labels into the UIView via `accessibilityElements` — noted for
//  the device dev.)
//
//  The visuals are a tasteful SwiftUI-shapes homage to the grey NES-001 pad: a black
//  cross D-pad, dark-red round A/B buttons, and dark rounded SELECT/START pills. No
//  binary image assets (icon-grade art is a separate maintainer asset task).
//

import SwiftUI
import UIKit

/// The true multi-touch on-screen pad. Calls `onMaskChanged` with the live combined
/// touch mask whenever the held set changes (0 when nothing is held).
struct MultiTouchControlPad: View {
    /// Reports the current touch-only mask; the parent ORs it with any hardware-pad
    /// mask before forwarding to the core (same contract as `TouchControlsOverlay`).
    let onMaskChanged: (UInt8) -> Void

    var body: some View {
        GeometryReader { geo in
            let layout = ControlPadLayout.make(in: geo.size, idiom: UIDevice.current.userInterfaceIdiom)
            ZStack {
                // Visual layer (SwiftUI shapes + VoiceOver labels).
                ForEach(layout) { button in
                    NesPadButtonView(button: button)
                }
                // Touch layer (UIKit multi-touch), transparent and on top.
                MultiTouchSurface(layout: layout, onMaskChanged: onMaskChanged)
                    .accessibilityHidden(true)
            }
        }
        .allowsHitTesting(true)
    }
}

// MARK: - Visuals (NES-001 homage, SwiftUI shapes only)

/// One styled, translucent pad button. Drawn to evoke the NES-001 controller while
/// staying legible over gameplay. Carries the spoken accessibility name.
private struct NesPadButtonView: View {
    let button: PadButton

    var body: some View {
        shape
            .overlay(label)
            .frame(width: button.frame.width, height: button.frame.height)
            .position(x: button.frame.midX, y: button.frame.midY)
            .accessibilityLabel(button.accessibilityName)
    }

    @ViewBuilder
    private var shape: some View {
        switch button.button {
        case .a, .b:
            // Dark-red round face buttons (the NES-001 A/B).
            Circle()
                .fill(
                    LinearGradient(
                        colors: [
                            Color(red: 0.74, green: 0.13, blue: 0.18).opacity(0.85),
                            Color(red: 0.52, green: 0.07, blue: 0.11).opacity(0.85),
                        ],
                        startPoint: .top, endPoint: .bottom
                    )
                )
                .overlay(Circle().stroke(Color.black.opacity(0.45), lineWidth: 2))
        case .select, .start:
            // Dark rounded pills (the NES-001 SELECT/START).
            Capsule()
                .fill(
                    LinearGradient(
                        colors: [
                            Color(white: 0.22).opacity(0.82),
                            Color(white: 0.10).opacity(0.82),
                        ],
                        startPoint: .top, endPoint: .bottom
                    )
                )
                .overlay(Capsule().stroke(Color.white.opacity(0.30), lineWidth: 1.5))
        default:
            // Near-black cross-arm D-pad cells.
            RoundedRectangle(cornerRadius: 6)
                .fill(
                    LinearGradient(
                        colors: [
                            Color(white: 0.20).opacity(0.82),
                            Color(white: 0.06).opacity(0.82),
                        ],
                        startPoint: .top, endPoint: .bottom
                    )
                )
                .overlay(RoundedRectangle(cornerRadius: 6).stroke(Color.white.opacity(0.22), lineWidth: 1.5))
        }
    }

    @ViewBuilder
    private var label: some View {
        if !button.label.isEmpty {
            Text(button.label)
                .font(.system(size: button.button == .a || button.button == .b ? 15 : 11, weight: .bold))
                .foregroundColor(.white.opacity(0.85))
        } else {
            // A faint directional chevron on each D-pad arm for legibility.
            Image(systemName: dpadGlyph)
                .font(.system(size: 12, weight: .bold))
                .foregroundColor(.white.opacity(0.55))
        }
    }

    private var dpadGlyph: String {
        switch button.button {
        case .up: return "chevron.up"
        case .down: return "chevron.down"
        case .left: return "chevron.left"
        case .right: return "chevron.right"
        default: return "circle"
        }
    }
}

// MARK: - Touch layer (UIKit true multi-touch)

/// Bridges a transparent multi-touch `UIView` into SwiftUI. The view tracks all
/// active touches and reports the combined mask; the SwiftUI side keeps the visuals.
private struct MultiTouchSurface: UIViewRepresentable {
    let layout: [PadButton]
    let onMaskChanged: (UInt8) -> Void

    func makeUIView(context: Context) -> MultiTouchPadView {
        let view = MultiTouchPadView()
        view.layout = layout
        view.onMaskChanged = onMaskChanged
        return view
    }

    func updateUIView(_ view: MultiTouchPadView, context: Context) {
        // Re-push the latest layout (it changes on resize/rotation) and callback.
        view.layout = layout
        view.onMaskChanged = onMaskChanged
    }
}

/// A transparent view that sees every finger. On any touch phase change it recomputes
/// the combined NES mask from ALL touches currently down on it. This is the core of
/// the v1.9.2 multi-touch fix: two fingers far apart are both honoured.
final class MultiTouchPadView: UIView {
    var layout: [PadButton] = []
    var onMaskChanged: ((UInt8) -> Void)?

    private var lastBits: UInt8 = 0

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

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) { recompute(event) }
    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) { recompute(event) }
    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) { recompute(event) }
    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) { recompute(event) }

    /// Rebuild the mask from every live touch on this view. Using `event.allTouches`
    /// (filtered to non-lifted phases and to touches owned by this view) avoids
    /// retaining `UITouch` objects across the sequence.
    private func recompute(_ event: UIEvent?) {
        var mask = NesButtonMask()
        for touch in event?.allTouches ?? [] where touch.view === self {
            switch touch.phase {
            case .began, .moved, .stationary:
                mask.formUnion(ControlPadLayout.hitTest(touch.location(in: self), in: layout))
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
