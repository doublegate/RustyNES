//
//  EmulatorCore.swift
//
//  The Swift-side owner of one running emulation session: the UniFFI
//  `NesController` (the typed control surface) plus the opaque Metal-renderer and
//  CoreAudio-sink FFI handles. It is the iOS analogue of the desktop
//  `Arc<Mutex<EmuCore>>` + the Android `EmulatorHandle`.
//
//  Threading: `NesController` is internally synchronised (a Rust `Mutex`), so the
//  CADisplayLink frame loop (MetalGameView) and the SwiftUI UI thread can both call
//  it. The gfx/audio FFI handles are touched only from `tick()` (the display-link
//  thread) and the main-thread lifecycle calls, never concurrently.
//
//  Determinism is untouched: every method forwards straight into the byte-identical
//  core; pacing/resampling lives in the cpal sink, never in the synthesis.
//

import Foundation
import QuartzCore

/// The video filter the renderer applies. Raw values match the gfx FFI's filter
/// id (0 none, 1 scanlines, 2 CRT, 3 NTSC, 4 Bisqwit).
enum VideoFilter: UInt8, CaseIterable, Identifiable {
    case none = 0
    case scanlines = 1
    case crt = 2
    case ntsc = 3

    var id: UInt8 { rawValue }

    var label: String {
        switch self {
        case .none: return "None"
        case .scanlines: return "Scanlines"
        case .crt: return "CRT"
        case .ntsc: return "NTSC"
        }
    }
}

/// Owns a loaded ROM's emulation. Create via `EmulatorCore(romData:)`; drive the
/// frame loop via `attach(view:)` + the CADisplayLink in MetalGameView.
final class EmulatorCore {
    /// The NES visible framebuffer dimensions (matches FRAME_WIDTH/FRAME_HEIGHT).
    static let frameWidth: UInt32 = 256
    static let frameHeight: UInt32 = 240

    private let controller: NesController
    private var gfx: OpaquePointer?
    private var audio: OpaquePointer?

    /// True while the loop should advance the core (false when paused/backgrounded).
    private(set) var isRunning = false
    /// Suppress audio push without tearing the sink down (a user mute toggle).
    var isMuted = false

    /// The host sample rate negotiated for this session (the core synthesises for it).
    let sampleRate: UInt32

    /// Metadata for the loaded cartridge.
    let info: RomInfo

    /// Construct from raw iNES/NES 2.0 ROM bytes. Opens the audio sink first so the
    /// core can synthesise for the device's real sample rate.
    /// - Throws: `MobileError` if the bytes are not a loadable cartridge.
    init(romData: Data) throws {
        // Open the cpal sink first to learn the device sample rate. If it fails to
        // open we fall back to the bridge default so the core still runs (silent).
        let sink = rustynes_ios_audio_new()
        let rate = sink.map { rustynes_ios_audio_sample_rate($0) } ?? 0
        let effectiveRate: UInt32 = rate != 0 ? rate : 48_000

        // UniFFI: `NesController(rom:sampleRate:)` is the generated throwing
        // constructor over the Rust `new(rom, sample_rate)`.
        self.controller = try NesController(rom: romData, sampleRate: effectiveRate)
        self.audio = sink
        self.sampleRate = effectiveRate
        self.info = controller.info()
    }

    // MARK: - Surface lifecycle

    /// Build the wgpu/Metal renderer for the host MTKView at the given drawable
    /// size. The view must outlive the renderer.
    func attach(view: UnsafeMutableRawPointer, width: UInt32, height: UInt32) {
        if gfx != nil { detachRenderer() }
        gfx = rustynes_ios_gfx_init(view, width, height)
    }

    /// Reconfigure the renderer for a new drawable size (rotation / Stage Manager).
    func resize(width: UInt32, height: UInt32) {
        guard let gfx else { return }
        rustynes_ios_gfx_resize(gfx, width, height)
    }

    /// Apply a video filter (and its up-to-four shader params).
    func setFilter(_ filter: VideoFilter, p0: Float = 0, p1: Float = 0, p2: Float = 0, p3: Float = 0) {
        guard let gfx else { return }
        rustynes_ios_gfx_set_filter(gfx, filter.rawValue, p0, p1, p2, p3)
    }

    private func detachRenderer() {
        if let gfx {
            rustynes_ios_gfx_destroy(gfx)
            self.gfx = nil
        }
    }

    // MARK: - Frame loop

    /// Advance one frame, present it through the renderer, and drain audio to the
    /// sink. Called from the CADisplayLink tick. No-op while paused.
    func tick() {
        guard isRunning, let gfx else { return }

        // Run a frame and hand the RGBA framebuffer straight to wgpu (which
        // presents). UniFFI marshals `run_frame()` as a Swift `Data`.
        let frame = controller.runFrame()
        frame.withUnsafeBytes { raw in
            if let base = raw.baseAddress {
                rustynes_ios_gfx_render(gfx, base.assumingMemoryBound(to: UInt8.self), raw.count)
            }
        }

        // Drain mono f32 audio and enqueue it (unless muted). The sink's DRC
        // absorbs the console-rate <-> device-rate beat.
        if !isMuted, let audio {
            let samples = controller.drainAudio()
            if !samples.isEmpty {
                samples.withUnsafeBufferPointer { buf in
                    if let base = buf.baseAddress {
                        rustynes_ios_audio_push(audio, base, buf.count)
                    }
                }
            }
        }
    }

    // MARK: - Run state

    func start() {
        isRunning = true
        if let audio { rustynes_ios_audio_resume(audio) }
    }

    func pause() {
        isRunning = false
        if let audio { rustynes_ios_audio_pause(audio) }
    }

    /// Resume after a pause/background return.
    func resume() { start() }

    // MARK: - Input

    /// Set the full 8-bit controller mask for a port (0-3).
    func setButtons(port: UInt32, mask: UInt8) {
        try? controller.setButtons(port: port, mask: mask)
    }

    // MARK: - Reset / power

    func reset() { controller.reset() }
    func powerCycle() { controller.powerCycle() }

    // MARK: - Save states

    /// Encode the full emulator state to a `.rns` blob.
    func saveState() -> Data { controller.saveState() }

    /// Restore from a `.rns` blob.
    /// - Throws: `MobileError` if malformed or from a different ROM.
    func loadState(_ data: Data) throws { try controller.loadState(data: data) }

    /// The frame index since power-on.
    func frame() -> UInt64 { controller.frame() }

    // MARK: - Teardown

    /// Drop the renderer + audio sink and stop the loop. The `NesController` is
    /// freed when this object deinits (UniFFI ARC).
    func shutdown() {
        pause()
        detachRenderer()
        if let audio {
            rustynes_ios_audio_destroy(audio)
            self.audio = nil
        }
    }

    deinit {
        shutdown()
    }
}
