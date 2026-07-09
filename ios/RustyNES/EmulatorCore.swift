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

    /// The live P1 (port 0) controller mask, cached from `setButtons` so the netplay
    /// frame loop can feed it to `npAdvanceFrame(localMask:)` (v1.9.6). The bridge
    /// maps this peer's local mask onto its own player slot internally (host = P1,
    /// joiner = P2); the remote player's input arrives over the wire. Written on the
    /// main thread (`setButtons`) and read on the CADisplayLink/emulation thread
    /// (`tickNetplay`), so all access goes through `frameLock`.
    private var _localMask: UInt8 = 0

    /// The active custom palette's RGB bytes (the same `.pal` fed to the core via
    /// `loadPalette`), cached so the netplay index->RGBA path (`NesPalette.expand`)
    /// matches the single-player look. `nil` => the built-in master palette. Written
    /// on the main thread (`loadPalette`/`clearPalette`) and read on the emulation
    /// thread (`tickNetplay`), so all access goes through `frameLock` — `Data` is
    /// copy-on-write and not safe for concurrent read/write.
    private var _customPaletteRGB: Data?

    /// Reused RGBA staging buffer for the netplay present path (filled by
    /// `NesPalette.expand`), kept off the per-frame allocation hot path.
    private var netplayRGBA: [UInt8] = []

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

    /// Thread-safe accessors for the cross-thread netplay inputs (`_localMask`,
    /// `_customPaletteRGB`). Each holds `frameLock` only for the field access, so the
    /// critical section stays tiny — the caller snapshots into a local before use.
    private func loadLocalMask() -> UInt8 {
        frameLock.lock()
        defer { frameLock.unlock() }
        return _localMask
    }

    private func storeLocalMask(_ mask: UInt8) {
        frameLock.lock()
        _localMask = mask
        frameLock.unlock()
    }

    private func loadCustomPaletteRGB() -> Data? {
        frameLock.lock()
        defer { frameLock.unlock() }
        return _customPaletteRGB
    }

    private func storeCustomPaletteRGB(_ rgb: Data?) {
        frameLock.lock()
        _customPaletteRGB = rgb
        frameLock.unlock()
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

        // Netplay (v1.9.6): while a session is active the rollback core owns pacing,
        // so the loop advances via `npAdvanceFrame` instead of `runFrame` (calling
        // `runFrame` would advance the core a second time and desync rollback). Handled
        // entirely in `tickNetplay`; the single-player path below is skipped.
        if controller.npIsActive() {
            tickNetplay(gfx: gfx)
            return
        }

        // TAStudio (v1.9.9): when a scripted playback is active, inject this
        // frame's P1 mask before advancing. No-op otherwise (live input path).
        tasAdvanceIfActive()

        // Run a frame and hand the RGBA framebuffer straight to wgpu (which
        // presents). UniFFI marshals `run_frame()` as a Swift `Data`.
        let frame = controller.runFrame()

        // TAStudio export (v1.9.9): if this frame exhausted the authored table,
        // stop the recorder NOW (before any idle frames are recorded).
        tasFinalizeExportIfPending()

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

    /// Advance one netplay frame (the `runFrame` replacement during a session) and
    /// present it. `npAdvanceFrame` feeds this peer's local P1 mask into the rollback
    /// session, advances (re-simulating on rollback), and reports whether a frame was
    /// actually produced. A non-produced tick is a time-sync stall / connecting /
    /// error tick: we drain + discard audio (so the ring doesn't back up) and skip the
    /// present this tick. On a produced frame we read the framebuffer through the
    /// NON-advancing index path and expand it to RGBA (see `NesPalette`), since
    /// `npAdvanceFrame` does not return pixels and re-running `runFrame` would desync.
    private func tickNetplay(gfx: OpaquePointer) {
        // Snapshot the cross-thread inputs under `frameLock` (they are mutated on the
        // main thread); use the locals for the rest of the tick.
        let localMask = loadLocalMask()
        let customPaletteRGB = loadCustomPaletteRGB()
        let result = controller.npAdvanceFrame(localMask: localMask)
        guard result.producedFrame else {
            // Stall / connecting / error: keep the audio ring from backing up, no present.
            _ = controller.drainAudio()
            return
        }

        // The just-produced frame, read without advancing the core again.
        let index = controller.indexFramebufferBytes()
        if NesPalette.expand(index: index, customPaletteRGB: customPaletteRGB, into: &netplayRGBA) {
            netplayRGBA.withUnsafeBytes { raw in
                if let base = raw.baseAddress {
                    rustynes_ios_gfx_render(gfx, base.assumingMemoryBound(to: UInt8.self), raw.count)
                }
            }
            // Retain a 256x240 RGBA copy for save-slot thumbnails, mirroring the
            // single-player path (locked: read on the main actor in `snapshotPNG`).
            let copy = Data(netplayRGBA)
            frameLock.lock()
            _lastFrame = copy
            frameLock.unlock()
        }

        // Drain this tick's audio (rollback produced exactly one frame's worth); play
        // it unless muted (a muted/sinkless drain still empties the ring).
        let samples = controller.drainAudio()
        if !isMuted, let audio, !samples.isEmpty {
            samples.withUnsafeBufferPointer { buf in
                if let base = buf.baseAddress {
                    rustynes_ios_audio_push(audio, base, buf.count)
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

    /// Set the full 8-bit controller mask for a port (0-3). Caches the P1 (port 0)
    /// mask so the netplay loop can feed it to `npAdvanceFrame`.
    func setButtons(port: UInt32, mask: UInt8) {
        if port == 0 { storeLocalMask(mask) }
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

    /// Drain the host-facing warning codes the core queued during the last
    /// operation — currently a pre-v2.0.0 `.rnm` movie whose input stream still
    /// replays on the v2.0.0 "Timebase" core, but whose byte-exact framebuffer/audio
    /// reproduction is not guaranteed across the ADR-0028 timebase change — each
    /// mapped to a device-locale string. v2.0.5 "Landfall": the iOS analogue of the
    /// Android v2.0.4 `drainWarningCodes()` surfacing. The bridge stores
    /// machine-readable `HostWarning` codes (v2.0.3) rather than pre-baked English, so
    /// the host owns the localization; draining is idempotent (the queue empties on
    /// read), so a caller surfaces each warning exactly once.
    func drainWarnings() -> [String] {
        controller.drainWarningCodes().map(Self.warningText)
    }

    /// Map one `HostWarning` code to its localized presentation string. The English
    /// key is byte-identical to the Android `host_warning_pre_timebase_movie`
    /// resource so both platforms surface the same wording and share the ES copy.
    private static func warningText(_ warning: HostWarning) -> String {
        switch warning {
        case .preTimebaseMovie:
            return String(
                localized: "This movie was recorded on a pre-v2.0.0 build. Input replay proceeds, but exact framebuffer/audio reproduction is not guaranteed across the engine-timebase change (ADR 0028)."
            )
        }
    }

    // MARK: - Custom palette (.pal) (v1.9.5)

    /// Apply a custom 64-colour palette from `.pal` bytes (>= 192 bytes).
    /// Presentation only; byte-identical to the built-in palette once cleared.
    /// - Throws: `MobileError.palette` if fewer than 192 bytes were supplied.
    func loadPalette(_ data: Data) throws {
        try controller.loadPalette(bytes: data)
        // Cache for the netplay index->RGBA path so it matches the single-player look.
        // Locked: read on the emulation thread in `tickNetplay`.
        storeCustomPaletteRGB(data)
    }

    /// Restore the built-in NES palette.
    func clearPalette() {
        controller.clearPalette()
        storeCustomPaletteRGB(nil)
    }

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

    // MARK: - Lua scripting (v1.9.6)

    /// Load + start a sandboxed Lua script (its `on_frame` runs each frame after the
    /// tick). Replaces any active script.
    /// - Throws: `MobileError.script` if it fails to compile / load.
    func loadScript(_ src: String) throws { try controller.loadScript(src: src) }

    /// Unload the active script.
    func unloadScript() { controller.unloadScript() }

    /// Whether a script is loaded.
    var scriptIsLoaded: Bool { controller.scriptIsLoaded() }

    /// Drain the script's `print` / `emu.log` output since the last call.
    func drainScriptLog() -> [String] { controller.drainScriptLog() }

    // MARK: - RetroAchievements (v1.9.6)
    //
    // The RA session lives on the `NesController`. On iOS the controller is rebuilt
    // per game (unlike Android's long-lived one), so the RA login does NOT persist
    // across games here: `RetroAchievementsModel` re-establishes it (token re-login +
    // `raLoadGame`) each time a game opens. All reconciliation (login completion,
    // unlock checks, toast/rich-presence refresh) happens inside `post_frame_ra`,
    // which only runs while the core ticks (`runFrame`/`stepFrame`).

    func raInit(hardcore: Bool) { controller.raInit(hardcore: hardcore) }
    func raLoginPassword(user: String, password: String) {
        controller.raLoginPassword(user: user, password: password)
    }
    func raLoginToken(user: String, token: String) {
        controller.raLoginToken(user: user, token: token)
    }
    func raLogout() { controller.raLogout() }
    func raLoginStatus() -> RaLoginStatus { controller.raLoginStatus() }
    func raUser() -> RaUserInfo? { controller.raUser() }
    func raToken() -> String? { controller.raToken() }
    func raSetHardcore(_ hardcore: Bool) { controller.raSetHardcore(hardcore: hardcore) }
    func raHardcore() -> Bool { controller.raHardcore() }
    /// Identify + load the achievement set for the loaded ROM. `sha256` is the raw
    /// 32-byte digest; `sidecar` is previously-saved progress ("" / empty if none).
    /// - Throws: `MobileError.saveState` if `sha256` is not 32 bytes.
    func raLoadGame(rom: Data, sha256: Data, sidecar: Data) throws {
        try controller.raLoadGame(rom: rom, sha256: sha256, sidecar: sidecar)
    }
    func raUnloadGame() { controller.raUnloadGame() }
    func raSerializeProgress() -> Data { controller.raSerializeProgress() }
    func raPollToasts() -> [RaToast] { controller.raPollToasts() }
    func raRichPresence() -> String { controller.raRichPresence() }
    func raAchievementList() -> [RaAchievementInfo] { controller.raAchievementList() }
    func raGameSummary() -> [UInt32] { controller.raGameSummary() }

    /// Pump the core one frame WITHOUT presenting, to let `post_frame_ra` reconcile an
    /// in-flight RA login while the emulator is paused behind the Settings sheet. The
    /// audio is drained + discarded so a paused login doesn't leak sound. Only intended
    /// to be called while paused (no display-link tick) and a login is pending; calling
    /// it while running would double-advance the core.
    func pumpForLogin() {
        controller.stepFrame()
        _ = controller.drainAudio()
    }

    // MARK: - Netplay (direct-IP / LAN) (v1.9.6)
    //
    // Host/Join start a session; the frame loop then advances via `npAdvanceFrame`
    // (see `tickNetplay`) for as long as `npIsActive()`. This is true for BOTH the
    // direct-IP / LAN path (`npHost`/`npJoin`, v1.9.6) and the room-code / CGNAT path
    // (`npHostRoom`/`npJoinRoom`, v1.9.7): once a session is started the downstream
    // `npAdvanceFrame` loop is identical; only the connection establishment differs.

    /// Host a session: bind `0.0.0.0:localPort` (pass 0 to let the OS pick) and listen
    /// as P1. Returns the actual bound port to share with the joiner.
    /// - Throws: `MobileError.netplay` if the socket bind fails.
    func npHost(localPort: UInt16, numPlayers: UInt8) throws -> UInt16 {
        try controller.npHost(localPort: localPort, numPlayers: numPlayers)
    }
    /// Join a session at `address` ("ip:port") as P2.
    /// - Throws: `MobileError.netplay` on a bad address or bind/connect failure.
    func npJoin(address: String) throws { try controller.npJoin(address: address) }
    /// Host a room-code (internet / CGNAT) session: register with the signaling relay
    /// in `cfg`, begin NAT traversal, and return the room code to share with the peer.
    /// The session then advances via `npAdvanceFrame` exactly like the direct-IP path.
    /// - Throws: `MobileError.netplay` if the local socket bind fails. A bad relay /
    ///   failed traversal surfaces later as the session moving to the `error` phase.
    func npHostRoom(numPlayers: UInt8, cfg: NpNetConfig) throws -> String {
        try controller.npHostRoom(numPlayers: numPlayers, cfg: cfg)
    }
    /// Join a room-code (internet / CGNAT) session by its `roomCode` using the relay /
    /// STUN / TURN endpoints in `cfg`. Drive `npAdvanceFrame` and poll `npStatus` for
    /// the `negotiating` sub-step; on success it converges on `connecting` -> `inGame`.
    /// - Throws: `MobileError.netplay` if the local socket bind fails.
    func npJoinRoom(roomCode: String, cfg: NpNetConfig) throws {
        try controller.npJoinRoom(roomCode: roomCode, cfg: cfg)
    }
    /// Tear down any session and return to single-player.
    func npLeave() { controller.npLeave() }
    /// Whether a session is active / connecting (the loop drives via `npAdvanceFrame`).
    func npIsActive() -> Bool { controller.npIsActive() }
    /// A status snapshot for the netplay panel/HUD.
    func npStatus() -> NpStatus { controller.npStatus() }

    // MARK: - Cheats (v1.9.9 "Workshop")

    /// Add a Game Genie code (6 or 8 characters). The core applies it on every
    /// PRG read until removed.
    /// - Throws: `MobileError.cheat` if the code is malformed.
    func cheatAddGenie(_ code: String) throws { try controller.cheatAddGenie(code: code) }
    /// Remove a Game Genie code (no-op if not present).
    func cheatRemoveGenie(_ code: String) { controller.cheatRemoveGenie(code: code) }
    /// Remove every active Game Genie code.
    func cheatClearGenie() { controller.cheatClearGenie() }
    /// The currently-active Game Genie codes.
    func cheatGenieCodes() -> [GenieCodeInfo] { controller.cheatGenieCodes() }
    /// Write one byte into CPU RAM ($0000..=$1FFF) via the core's `poke_ram`.
    func pokeRam(addr: UInt16, value: UInt8) { controller.pokeRam(addr: addr, value: value) }
    /// Read one byte from the CPU bus, side-effect-free.
    func peekByte(addr: UInt16) -> UInt8 { controller.peekByte(addr: addr) }

    // MARK: - Read-only debugger inspector (v1.9.9 "Workshop")

    /// A read-only snapshot of the CPU registers (does not advance the core).
    func debugCpuState() -> CpuRegs { controller.debugCpuState() }
    /// Read `len` bytes from the CPU bus starting at `start` (side-effect-free).
    func debugReadMemory(start: UInt16, len: UInt32) -> Data {
        controller.debugReadMemory(start: start, len: len)
    }
    /// Disassemble `count` instructions starting at `pc`.
    func debugDisassemble(pc: UInt16, count: UInt32) -> [DisasmRow] {
        controller.debugDisassemble(pc: pc, count: count)
    }

    /// Advance one frame WITHOUT presenting, for the debugger's single-step while
    /// the emulator is paused behind the inspector sheet. Drains + discards the
    /// frame's audio so a paused step doesn't leak sound. Only meaningful while
    /// paused (no display-link tick); calling it while running double-advances.
    func debugStep() {
        controller.stepFrame()
        _ = controller.drainAudio()
    }

    // MARK: - Foreign movie import (v1.9.9 "Workshop")
    //
    // Each importer transcodes a foreign movie into native `.rnm` bytes stamped
    // with THIS game's ROM hash; the caller plays them via `moviePlay` and/or
    // saves them as a `.rnm`.

    /// Import an FCEUX `.fm2` movie and return native `.rnm` bytes.
    /// - Throws: `MobileError.movie` if the bytes are not a parseable `.fm2`.
    func movieImportFm2(_ data: Data) throws -> Data { try controller.movieImportFm2(bytes: data) }
    /// Import a BizHawk `.bk2` movie and return native `.rnm` bytes.
    /// - Throws: `MobileError.movie` if the archive / input log is malformed.
    func movieImportBk2(_ data: Data) throws -> Data { try controller.movieImportBk2(bytes: data) }
    /// Import a Nestopia `.fcm` movie and return native `.rnm` bytes.
    /// - Throws: `MobileError.movie` if the bytes are not a parseable `.fcm`.
    func movieImportFcm(_ data: Data) throws -> Data { try controller.movieImportFcm(bytes: data) }
    /// Import a Famtasia `.fmv` movie and return native `.rnm` bytes.
    /// - Throws: `MobileError.movie` if the bytes are not a parseable `.fmv`.
    func movieImportFmv(_ data: Data) throws -> Data { try controller.movieImportFmv(bytes: data) }
    /// Import a VirtuaNES `.vmv` movie and return native `.rnm` bytes.
    /// - Throws: `MobileError.movie` if the bytes are not a parseable `.vmv`.
    func movieImportVmv(_ data: Data) throws -> Data { try controller.movieImportVmv(bytes: data) }

    // MARK: - Audio-depth DSP (v1.9.9 "Workshop")

    /// Publish the live host audio-depth (EQ / pan / reverb / crossfeed) config to
    /// the CoreAudio sink. Off / flat = bit-exact passthrough. No-op if the sink
    /// failed to open.
    func setAudioDepth(_ config: AudioDepthConfig) {
        guard let audio else { return }
        config.eqDb.withUnsafeBufferPointer { eqBuf in
            config.pan.withUnsafeBufferPointer { panBuf in
                rustynes_ios_audio_set_depth(
                    audio,
                    config.enabled ? 1 : 0,
                    eqBuf.baseAddress, eqBuf.count,
                    panBuf.baseAddress, panBuf.count,
                    config.reverbMix, config.reverbRoom, config.crossfeed
                )
            }
        }
    }

    // MARK: - TAStudio scripted input (v1.9.9 "Workshop")
    //
    // A pragmatic touch piano-roll authors a host-side table of per-frame P1
    // masks. Playback injects one mask per frame through the EXISTING bridge
    // (`setButtons` + `runFrame`), so it is deterministic; with a recording armed
    // (`movieRecordFromPowerOn`) the core's recorder captures the same input,
    // yielding a real `.rnm`. Off by default — the normal input path is untouched
    // unless a playback is active.

    /// Per-frame P1 masks to inject, the read cursor, and the active flag. Written
    /// on the main thread (`tasStartPlayback`/`tasStop`) and read on the
    /// CADisplayLink thread (`tasAdvanceLocked`), so all access goes through
    /// `frameLock`.
    private var _tasFrames: [UInt8] = []
    private var _tasCursor = 0
    private var _tasActive = false
    /// While true, the active playback is being captured to a `.rnm`; the recorder
    /// is stopped at the exact frame the authored table is exhausted (no trailing
    /// idle frames). Guarded by `frameLock`.
    private var _tasExporting = false
    /// Set on the emu thread when the last authored frame has just been injected, so
    /// `tick()` stops the recorder immediately after running that frame.
    private var _tasFinalizeExport = false
    /// The finished export bytes, captured on the emu thread and taken by the host.
    private var _tasExportedMovie: Data?

    /// Whether a scripted TAStudio playback is currently running.
    var tasIsActive: Bool {
        frameLock.lock()
        defer { frameLock.unlock() }
        return _tasActive
    }

    /// Begin injecting `p1Masks` one-per-frame (from the current state). Pair with
    /// `movieRecordFromPowerOn()` first to capture the run as a `.rnm`.
    func tasStartPlayback(p1Masks: [UInt8]) {
        frameLock.lock()
        _tasFrames = p1Masks
        _tasCursor = 0
        _tasActive = !p1Masks.isEmpty
        _tasExporting = false
        frameLock.unlock()
    }

    /// Arm a power-on recording AND begin scripted playback of `p1Masks` as a single
    /// export: the recorder is stopped at the exact last authored frame (rather than
    /// on the next host poll tick), so the saved `.rnm` carries no trailing idle
    /// frames. The finished bytes are retrieved via `tasTakeExportedMovie()`.
    func tasStartExport(p1Masks: [UInt8]) {
        // An empty table would arm the recorder with no frames to inject: playback
        // never activates, so the finalize path never fires and the recorder is left
        // armed. Refuse to start instead, so it is never left in that state.
        guard !p1Masks.isEmpty else { return }
        controller.movieRecordFromPowerOn()
        frameLock.lock()
        _tasFrames = p1Masks
        _tasCursor = 0
        _tasActive = !p1Masks.isEmpty
        _tasExporting = !p1Masks.isEmpty
        _tasFinalizeExport = false
        _tasExportedMovie = nil
        frameLock.unlock()
    }

    /// Take (and clear) the finished export movie bytes, or nil if an export is
    /// still running / none is pending. Polled by the host after `tasStartExport`.
    func tasTakeExportedMovie() -> Data? {
        frameLock.lock()
        defer { frameLock.unlock() }
        let bytes = _tasExportedMovie
        _tasExportedMovie = nil
        return bytes
    }

    /// Stop scripted playback (returns to live input). If an export was in flight,
    /// finalize it on the next tick so the partial recording is still captured.
    func tasStop() {
        frameLock.lock()
        let wasExporting = _tasExporting && _tasActive
        _tasActive = false
        if wasExporting { _tasFinalizeExport = true }
        frameLock.unlock()
    }

    /// If a scripted playback is active, take the next P1 mask (advancing the
    /// cursor, deactivating at the end) and feed it to port 0 before the frame.
    /// Snapshots under `frameLock`, then calls `setButtons` OUTSIDE the lock
    /// (`setButtons` re-locks; NSLock is not reentrant).
    private func tasAdvanceIfActive() {
        frameLock.lock()
        guard _tasActive else {
            frameLock.unlock()
            return
        }
        let mask = _tasCursor < _tasFrames.count ? _tasFrames[_tasCursor] : 0
        _tasCursor += 1
        if _tasCursor >= _tasFrames.count {
            _tasActive = false
            // This is the last authored frame; finalize the export right after the
            // frame is run (in `tick`) so no idle frames are recorded past it.
            if _tasExporting { _tasFinalizeExport = true }
        }
        frameLock.unlock()
        setButtons(port: 0, mask: mask)
    }

    /// Stop the export recorder immediately if the authored table was just
    /// exhausted (called from `tick` right after the last authored frame ran).
    private func tasFinalizeExportIfPending() {
        frameLock.lock()
        guard _tasFinalizeExport else {
            frameLock.unlock()
            return
        }
        _tasFinalizeExport = false
        _tasExporting = false
        frameLock.unlock()
        // `movieStopRecording` re-locks the bridge; call it outside `frameLock`.
        let bytes = controller.movieStopRecording()
        frameLock.lock()
        _tasExportedMovie = bytes
        frameLock.unlock()
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
