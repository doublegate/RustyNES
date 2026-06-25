//
//  HapticsManager.swift
//
//  Optional light haptic feedback for on-screen button presses. Uses UIKit's
//  `UIImpactFeedbackGenerator` (a `.light` impact) — the simple, reliable,
//  purpose-built API for one-shot UI taps, and the iOS analog of the Android pad's
//  predefined `VibrationEffect` tick. We deliberately avoid `CHHapticEngine` here: a
//  custom haptic engine idles / auto-shuts-down between sparse button presses, so the
//  next tap either drops or stutters while the engine respins — needless lifecycle
//  risk for a plain tap. `CoreHaptics` is imported ONLY for the accurate hardware
//  capability check that drives the Settings toggle's enabled state.
//
//  OFF by default; the toggle lives in SettingsView and is persisted by AppModel.
//  Haptics NEVER block or gate input — a tap on a device without a Taptic Engine
//  simply does nothing.
//

import CoreHaptics
import UIKit

/// A tiny wrapper around a light `UIImpactFeedbackGenerator` for one-shot taps.
@MainActor
final class HapticsManager {
    /// Whether the hardware can play haptics at all (false on iPad / older devices).
    /// Used to gray out the Settings toggle; the generator itself no-ops elsewhere.
    let isSupported: Bool = CHHapticEngine.capabilitiesForHardware().supportsHaptics

    /// User preference; when false, `tap()` is a no-op. Toggled from Settings.
    var isEnabled: Bool = false {
        didSet { if isEnabled { generator.prepare() } }
    }

    private let generator = UIImpactFeedbackGenerator(style: .light)

    /// Warm the generator so the first tap has minimal latency. Cheap + idempotent;
    /// safe to call when unsupported (the generator no-ops). Called when the user
    /// enables haptics.
    func prepare() {
        generator.prepare()
    }

    /// Play a single light tap. No-op unless enabled (and a no-op on hardware without
    /// a Taptic Engine). Re-primes so the following press is also low-latency.
    func tap() {
        guard isEnabled, isSupported else { return }
        generator.impactOccurred()
        generator.prepare()
    }
}
