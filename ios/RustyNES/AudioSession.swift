//
//  AudioSession.swift
//
//  AVAudioSession configuration + interruption / route-change handling for the
//  RustyNES iOS host. The Rust cpal sink (rustynes_ios_audio_*) owns the output
//  stream; this Swift side owns the *session policy* — the category, activation,
//  and the system-event observers that pause/resume the sink + the emulator.
//
//  Per the verified facts: NO `UIBackgroundModes audio` is declared. The emulator
//  pauses on background, so requesting background audio would only invite App
//  Review rejection. On an interruption (phone call, Siri) or a route change
//  (headphones unplugged) we pause; on the matching "ended/resume" notification we
//  resume only if the app is still active.
//

import AVFoundation
import Foundation

/// Owns the AVAudioSession lifecycle and forwards interruption / route-change
/// events to a delegate (the EmulatorCore) so audio + emulation pause together.
final class AudioSession {
    /// Called when the session is interrupted or a route change requires silence.
    var onShouldPause: (() -> Void)?
    /// Called when an interruption ends with the "should resume" option.
    var onShouldResume: (() -> Void)?
    /// v2.7.4: called when the audio route went away (headphones unplugged). A
    /// separate reason from an interruption, because no "resume" event ever
    /// follows it -- the host clears it on the player's next menu close.
    var onRouteLost: (() -> Void)?
    /// v2.7.4 (frontend audit IOS-08): called after the system's media services
    /// were reset. Every audio object is invalid afterwards; the host must
    /// re-configure the session and rebuild its output sink.
    var onMediaServicesReset: (() -> Void)?

    private let session = AVAudioSession.sharedInstance()
    private var observersInstalled = false

    /// Configure for game playback (`.playback`: plays even with the ringer
    /// silenced, the expected behaviour for a game) and activate the session.
    func configure() {
        do {
            try session.setCategory(.playback, mode: .default, options: [])
            try session.setActive(true)
        } catch {
            // Non-fatal: the cpal sink still opens; audio just may route oddly.
            NSLog("RustyNES: AVAudioSession configure failed: \(error)")
        }
        installObservers()
    }

    /// Deactivate the session (on teardown). Best-effort.
    func deactivate() {
        try? session.setActive(false, options: [.notifyOthersOnDeactivation])
    }

    private func installObservers() {
        guard !observersInstalled else { return }
        observersInstalled = true
        let center = NotificationCenter.default
        center.addObserver(
            self,
            selector: #selector(handleInterruption(_:)),
            name: AVAudioSession.interruptionNotification,
            object: session
        )
        center.addObserver(
            self,
            selector: #selector(handleRouteChange(_:)),
            name: AVAudioSession.routeChangeNotification,
            object: session
        )
        // v2.7.4 (IOS-08): before this nothing observed a media-services reset,
        // and the output stream stayed silent until the game was closed and
        // reopened. The Rust sink also flags the dead stream itself
        // (`rustynes_ios_audio_is_invalid`), which the core polls each frame.
        center.addObserver(
            self,
            selector: #selector(handleMediaServicesReset(_:)),
            name: AVAudioSession.mediaServicesWereResetNotification,
            object: session
        )
    }

    @objc private func handleMediaServicesReset(_ note: Notification) {
        onMediaServicesReset?()
    }

    @objc private func handleInterruption(_ note: Notification) {
        guard
            let info = note.userInfo,
            let raw = info[AVAudioSessionInterruptionTypeKey] as? UInt,
            let type = AVAudioSession.InterruptionType(rawValue: raw)
        else { return }

        switch type {
        case .began:
            onShouldPause?()
        case .ended:
            // Re-activate the session, then resume only if the system says we may.
            try? session.setActive(true)
            if let optRaw = info[AVAudioSessionInterruptionOptionKey] as? UInt {
                let options = AVAudioSession.InterruptionOptions(rawValue: optRaw)
                if options.contains(.shouldResume) {
                    onShouldResume?()
                }
            }
        @unknown default:
            break
        }
    }

    @objc private func handleRouteChange(_ note: Notification) {
        guard
            let info = note.userInfo,
            let raw = info[AVAudioSessionRouteChangeReasonKey] as? UInt,
            let reason = AVAudioSession.RouteChangeReason(rawValue: raw)
        else { return }

        // The classic "headphones unplugged" case: pause so audio does not
        // suddenly blast from the speaker. v2.7.4: reported as its own reason --
        // it used to share the interruption flag, which only a background /
        // foreground cycle cleared, so the game stayed frozen in the app.
        if reason == .oldDeviceUnavailable {
            onRouteLost?()
        }
    }

    deinit {
        NotificationCenter.default.removeObserver(self)
    }
}
