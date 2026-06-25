//
//  HapticsManager.swift
//
//  Optional light haptic feedback for on-screen (and, optionally, controller) button
//  presses, via Core Haptics. OFF by default; the toggle lives in SettingsView and is
//  persisted by AppModel. Gracefully no-ops on devices without a haptic engine
//  (`CHHapticEngine.capabilitiesForHardware().supportsHaptics`). Haptics NEVER block
//  or gate input — every failure path silently returns, and the engine is built
//  lazily so a device that never enables haptics pays nothing.
//

import CoreHaptics
import Foundation

/// A tiny wrapper around a transient Core Haptics engine for one-shot taps.
final class HapticsManager {
    /// Whether the hardware can play haptics at all (false on iPad / older devices).
    let isSupported: Bool

    /// User preference; when false, `tap()` is a no-op. Toggled from Settings.
    var isEnabled: Bool = false

    private var engine: CHHapticEngine?

    init() {
        isSupported = CHHapticEngine.capabilitiesForHardware().supportsHaptics
    }

    /// Build and start the engine (idempotent). Safe to call when unsupported — it
    /// simply does nothing. Called when the user first enables haptics.
    func prepare() {
        guard isSupported, engine == nil else { return }
        do {
            let engine = try CHHapticEngine()
            engine.isAutoShutdownEnabled = true
            // Restart transparently if the system resets the engine (e.g. after an
            // audio-session interruption) so the next tap still plays.
            engine.resetHandler = { [weak self] in
                try? self?.engine?.start()
            }
            engine.stoppedHandler = { _ in }
            try engine.start()
            self.engine = engine
        } catch {
            // Leave the engine nil; taps will no-op. Never surface this to the user.
            engine = nil
        }
    }

    /// Play a single light tap. No-op unless enabled, supported, and the engine is up.
    func tap(intensity: Float = 0.5, sharpness: Float = 0.45) {
        guard isEnabled, isSupported else { return }
        if engine == nil { prepare() }
        guard let engine else { return }
        do {
            let event = CHHapticEvent(
                eventType: .hapticTransient,
                parameters: [
                    CHHapticEventParameter(parameterID: .hapticIntensity, value: intensity),
                    CHHapticEventParameter(parameterID: .hapticSharpness, value: sharpness),
                ],
                relativeTime: 0
            )
            let pattern = try CHHapticPattern(events: [event], parameters: [])
            let player = try engine.makePlayer(with: pattern)
            try player.start(atTime: CHHapticTimeImmediate)
        } catch {
            // Swallow: input must never depend on a haptic succeeding.
        }
    }
}
