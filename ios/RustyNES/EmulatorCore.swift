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
import UIKit

/// The video filter the renderer applies. Raw values match the gfx FFI's filter
/// id (0 none, 1 scanlines, 2 CRT, 3 NTSC, 4 Bisqwit).
enum VideoFilter: UInt8, CaseIterable, Identifiable {
    case none = 0
    case scanlines = 1
    case crt = 2
    case ntsc = 3
    // Bisqwit composite NTSC: the renderer's pipeline reads the palette-index frame
    // (an R16Uint texture) + the NTSC phase, fed each frame via
    // `rustynes_ios_gfx_set_index_frame` while this filter is active. Raw 4 matches
    // the gfx FFI / gfx_metal.rs filter numbering and the Android ordinal.
    case bisqwit = 4

    var id: UInt8 { rawValue }

    var label: String {
        switch self {
        case .none: return "None"
        case .scanlines: return "Scanlines"
        case .crt: return "CRT"
        case .ntsc: return "NTSC"
        case .bisqwit: return "Bisqwit NTSC"
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

    /// The filter currently applied to the renderer. Tracked here (not just inside
    /// the opaque gfx handle) so `tick()` knows when to feed the Bisqwit pass its
    /// per-frame palette-index frame + NTSC phase — an extra copy kept off the
    /// common (RGBA-only) path. Updated by `setFilter`.
    private(set) var activeFilter: VideoFilter = .none

    /// True while an HD-pack is loaded in the core (v1.9.5). When set, `tick()`
    /// composites the HD frame (`compositeHdFrame()` at `hdpackDimensions()`) and
    /// presents it through the renderer's HD path instead of the 256x240 frame.
    private(set) var hdPackLoaded = false

    /// True while the loop should advance the core (false when paused/backgrounded).
    private(set) var isRunning = false
    /// Suppress audio push without tearing the sink down (a user mute toggle).
    var isMuted = false

    /// The host sample rate negotiated for this session (the core synthesises for it).
    let sampleRate: UInt32

    /// The most recently presented RGBA8888 framebuffer (256x240), retained so the
    /// save-state layer can derive a slot thumbnail without re-running a frame. nil
    /// until the first `tick()`. Written on the CADisplayLink/emulation thread and
    /// read on the main actor (`snapshotPNG`), so all access goes through
    /// `frameLock` — `Data` is copy-on-write and not safe for concurrent read/write.
    private var _lastFrame: Data?
    private let frameLock = NSLock()

    /// Thread-safe snapshot of the latest framebuffer (a cheap COW reference copy).
    var lastFrame: Data? {
        frameLock.lock()
        defer { frameLock.unlock() }
        return _lastFrame
    }

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
        activeFilter = filter
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

        // HD-pack path (v1.9.5): when a pack is loaded, present the composited HD
        // frame (which can exceed 256x240) instead of the stock framebuffer. The
        // pack supplies the final look, so no on-screen filter is layered on top.
        // Fall back to the standard path if the composite is unexpectedly empty.
        var presentedHD = false
        if hdPackLoaded {
            let dims = controller.hdpackDimensions()
            if dims.count == 2, dims[0] > 0, dims[1] > 0 {
                let hd = controller.compositeHdFrame()
                if hd.count == Int(dims[0]) * Int(dims[1]) * 4 {
                    hd.withUnsafeBytes { raw in
                        if let base = raw.baseAddress {
                            rustynes_ios_gfx_render_hd(
                                gfx, base.assumingMemoryBound(to: UInt8.self), raw.count,
                                dims[0], dims[1]
                            )
                        }
                    }
                    presentedHD = true
                }
            }
        }

        if !presentedHD {
            presentStandard(frame, gfx: gfx)
        }

        // Retain the (stock 256x240) frame for save-state thumbnail capture (cheap:
        // one COW Data ref, swapped each tick; touched only here + on the main
        // thread snapshot). Locked because this runs off the main actor and
        // `snapshotPNG` reads it there. We keep the stock frame even on the HD path
        // so the thumbnail stays a fixed 256x240 RGBA.
        frameLock.lock()
        _lastFrame = frame
        frameLock.unlock()

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

    /// Present the stock 256x240 RGBA frame (the non-HD path). Feeds the Bisqwit
    /// composite its palette-index frame + NTSC phase first when that filter is
    /// active, then uploads + presents the RGBA frame.
    private func presentStandard(_ frame: Data, gfx: OpaquePointer) {
        // Bisqwit (filter 4) is a composite-NTSC pipeline that samples the
        // palette-index frame (an R16Uint texture), not the RGBA frame, so it needs
        // the index bytes + NTSC phase uploaded each frame. `set_index_frame` only
        // uploads (it does not present), so we still call `gfx_render` below to
        // present. Only fetch the index bytes while Bisqwit is active — it is an
        // extra per-frame copy we keep off the common RGBA-only path.
        if activeFilter == .bisqwit {
            let index = controller.indexFramebufferBytes()
            let phase = controller.ntscPhase()
            index.withUnsafeBytes { raw in
                if let base = raw.baseAddress {
                    rustynes_ios_gfx_set_index_frame(
                        gfx, base.assumingMemoryBound(to: UInt8.self), raw.count, phase
                    )
                }
            }
        }

        frame.withUnsafeBytes { raw in
            if let base = raw.baseAddress {
                rustynes_ios_gfx_render(gfx, base.assumingMemoryBound(to: UInt8.self), raw.count)
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

    /// A PNG of the most-recent framebuffer, for use as a save-slot thumbnail.
    /// Returns nil before the first frame or if the buffer is not the expected
    /// 256x240 RGBA8888 size. The NES emits opaque pixels (alpha 255), so the
    /// straight RGBA bytes render correctly under a premultiplied-last context.
    func snapshotPNG() -> Data? {
        guard let frame = lastFrame else { return nil }
        let w = Int(Self.frameWidth)
        let h = Int(Self.frameHeight)
        let bytesPerPixel = 4
        let bytesPerRow = w * bytesPerPixel
        guard frame.count >= bytesPerRow * h else { return nil }

        var pixels = frame
        let colorSpace = CGColorSpaceCreateDeviceRGB()
        // The buffer is R,G,B,A in memory order. `premultipliedLast` alone leaves the
        // byte order as the little-endian default (which reads as BGRA on iOS and
        // swaps red/blue); OR in `byteOrder32Big` so the 32-bit pixel's first byte is
        // R -> the components are read as R,G,B,A. (NES pixels are opaque, so
        // premultiplied vs straight alpha is moot.)
        let bitmapInfo = CGImageAlphaInfo.premultipliedLast.rawValue
            | CGBitmapInfo.byteOrder32Big.rawValue
        let cgImage: CGImage? = pixels.withUnsafeMutableBytes { raw in
            guard let base = raw.baseAddress,
                  let ctx = CGContext(
                      data: base,
                      width: w,
                      height: h,
                      bitsPerComponent: 8,
                      bytesPerRow: bytesPerRow,
                      space: colorSpace,
                      bitmapInfo: bitmapInfo
                  )
            else { return nil }
            return ctx.makeImage()
        }
        guard let cg = cgImage else { return nil }
        return UIImage(cgImage: cg).pngData()
    }

    // MARK: - TAS movies (.rnm) (v1.9.5)

    /// Start recording a movie from a fresh power-on (the core power-cycles so a
    /// later replay reconstructs from the identical state). Determinism preserved:
    /// the core records the input stream.
    func movieRecordFromPowerOn() { controller.movieRecordFromPowerOn() }

    /// Start recording a movie branching from the current state (embeds a state).
    func movieRecordFromHere() { controller.movieRecordFromHere() }

    /// Stop recording and return the serialized `.rnm` bytes (empty if not
    /// recording). The host writes them to the sandbox.
    func movieStopRecording() -> Data { controller.movieStopRecording() }

    /// Load + play a `.rnm` movie (drives input from the recorded stream).
    /// - Throws: `MobileError` if the bytes are not a valid movie / wrong ROM.
    func moviePlay(_ data: Data) throws { try controller.moviePlay(bytes: data) }

    /// Stop any active recording or playback.
    func movieStop() { controller.movieStop() }

    /// Whether a movie is being recorded.
    var movieIsRecording: Bool { controller.movieIsRecording() }

    /// Whether a movie is playing back.
    var movieIsPlaying: Bool { controller.movieIsPlaying() }

    // MARK: - Custom palette (.pal) (v1.9.5)

    /// Apply a custom 64-colour palette from `.pal` bytes (>= 192 bytes).
    /// Presentation only; byte-identical to the built-in palette once cleared.
    /// - Throws: `MobileError.palette` if fewer than 192 bytes were supplied.
    func loadPalette(_ data: Data) throws { try controller.loadPalette(bytes: data) }

    /// Restore the built-in NES palette.
    func clearPalette() { controller.clearPalette() }

    // MARK: - HD-pack (v1.9.5)

    /// Load an HD-pack from `.zip` bytes. On success the frame loop switches to the
    /// HD composite path. Replaces any active pack.
    /// - Throws: `MobileError.hdPack` if the bytes are not a valid HD-pack archive.
    func loadHDPack(_ data: Data) throws {
        try controller.loadHdpackFromZipBytes(bytes: data)
        hdPackLoaded = true
    }

    /// Unload the active HD-pack (revert to the stock 256x240 framebuffer).
    func unloadHDPack() {
        controller.unloadHdpack()
        hdPackLoaded = false
    }

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
