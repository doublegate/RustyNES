//
//  AudioDepth.swift
//
//  The host audio-depth (EQ / pan / reverb / crossfeed) settings model (v1.9.9
//  "Workshop"). These are an output-only DSP stage applied in the CoreAudio sink
//  AFTER the core's mono master is drained (see crates/rustynes-ios/src/audio_dsp.rs):
//  the deterministic core synthesis is untouched, so save-states / TAS / netplay
//  and the accuracy audio oracle are unaffected. Off by default; a disabled or
//  flat / centered config is a bit-exact passthrough.
//

import Combine
import Foundation

/// A flat config snapshot handed to `EmulatorCore.setAudioDepth` (which marshals it
/// over the C ABI to the cpal sink). `eqDb` is up to 5 band gains (dB); `pan` is up
/// to 6 per-APU-channel positions (-1 left .. +1 right) the Rust stage averages.
struct AudioDepthConfig {
    var enabled: Bool
    var eqDb: [Float]
    var pan: [Float]
    var reverbMix: Float
    var reverbRoom: Float
    var crossfeed: Float
}

/// Observable, persisted audio-depth settings. AppModel applies them to the running
/// core (on change + on game open). Off by default.
@MainActor
final class AudioDepthModel: ObservableObject {
    /// The 5 EQ band center-frequency labels, parallel to `eqDb`.
    static let bandLabels = ["60 Hz", "240 Hz", "1 kHz", "3.8 kHz", "12 kHz"]
    /// Number of EQ bands (matches `EQ_BAND_COUNT` in audio_dsp.rs).
    static let bandCount = 5
    /// Number of per-APU-channel pan slots (matches `PAN_COUNT` in audio_dsp.rs).
    static let panCount = 6

    /// Re-applied to the running core whenever any setting changes.
    var onChange: (() -> Void)?

    @Published var enabled: Bool {
        didSet { persistBool(enabled, "audioDepthEnabled"); notify() }
    }
    /// Per-band EQ gains in dB (-12..=12), length `bandCount`.
    @Published var eqDb: [Float] {
        didSet { persistFloats(eqDb, "audioDepthEq"); notify() }
    }
    /// Master pan (-1 left .. +1 right), applied to every channel slot.
    @Published var pan: Float {
        didSet { persistFloat(pan, "audioDepthPan"); notify() }
    }
    @Published var reverbMix: Float {
        didSet { persistFloat(reverbMix, "audioDepthReverbMix"); notify() }
    }
    @Published var reverbRoom: Float {
        didSet { persistFloat(reverbRoom, "audioDepthReverbRoom"); notify() }
    }
    @Published var crossfeed: Float {
        didSet { persistFloat(crossfeed, "audioDepthCrossfeed"); notify() }
    }

    init() {
        let d = UserDefaults.standard
        enabled = d.bool(forKey: "audioDepthEnabled")
        eqDb = Self.loadFloats("audioDepthEq", count: Self.bandCount, fallback: 0)
        pan = d.object(forKey: "audioDepthPan") == nil ? 0 : d.float(forKey: "audioDepthPan")
        reverbMix = d.float(forKey: "audioDepthReverbMix")
        reverbRoom = d.object(forKey: "audioDepthReverbRoom") == nil
            ? 0.5 : d.float(forKey: "audioDepthReverbRoom")
        crossfeed = d.float(forKey: "audioDepthCrossfeed")
    }

    /// The flat config to hand to the core. The single master `pan` fills all
    /// `panCount` slots (the Rust stage averages them, so this is one master image).
    var config: AudioDepthConfig {
        AudioDepthConfig(
            enabled: enabled,
            eqDb: eqDb,
            pan: Array(repeating: pan, count: Self.panCount),
            reverbMix: reverbMix,
            reverbRoom: reverbRoom,
            crossfeed: crossfeed
        )
    }

    /// Reset every band / stage to its neutral (bypass) value.
    func resetToFlat() {
        eqDb = Array(repeating: 0, count: Self.bandCount)
        pan = 0
        reverbMix = 0
        reverbRoom = 0.5
        crossfeed = 0
    }

    /// Apply the current config to a running core (no-op when nil).
    func apply(to core: EmulatorCore?) {
        core?.setAudioDepth(config)
    }

    private func notify() { onChange?() }

    private func persistBool(_ v: Bool, _ key: String) {
        UserDefaults.standard.set(v, forKey: key)
    }

    private func persistFloat(_ v: Float, _ key: String) {
        UserDefaults.standard.set(v, forKey: key)
    }

    private func persistFloats(_ v: [Float], _ key: String) {
        UserDefaults.standard.set(v.map { Double($0) }, forKey: key)
    }

    private static func loadFloats(_ key: String, count: Int, fallback: Float) -> [Float] {
        let stored = (UserDefaults.standard.array(forKey: key) as? [Double])?.map { Float($0) }
            ?? []
        var out = Array(repeating: fallback, count: count)
        for i in 0..<min(count, stored.count) { out[i] = stored[i] }
        return out
    }
}
