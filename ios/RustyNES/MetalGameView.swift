//
//  MetalGameView.swift
//
//  Hosts the Metal layer the Rust wgpu renderer draws into, and drives the
//  emulation frame loop from a CADisplayLink.
//
//  Per the verified facts: an MTKView is a UIView whose backing layer is a
//  CAMetalLayer. We pass the MTKView pointer to `rustynes_ios_gfx_init`, and Rust
//  wgpu reads `view.layer` (the CAMetalLayer) to build its surface. wgpu owns the
//  drawable and presents, so the MTKView is just the layer host: we set
//  `isPaused = true` + `enableSetNeedsDisplay = false` and never draw in an
//  MTKViewDelegate. The CADisplayLink ticks the EmulatorCore, which runs a frame
//  and presents via the gfx FFI.
//
//  ProMotion: the display link requests 60-120 Hz, so its callback can fire up to
//  120x/sec. The console runs at ~60.0988 Hz, so we must NOT advance one emulated
//  frame per vsync (that would run at double speed on a 120 Hz ProMotion panel).
//  Instead we pace by elapsed wall time: a frame accumulator emits a console frame
//  only when a console-frame period has elapsed (one frame every other vsync at
//  120 Hz, ~one per vsync at 60 Hz), and the audio sink/DRC absorbs the residual
//  60↔120 beat. `CADisableMinimumFrameDurationOnPhone` (Info.plist) unlocks 120 Hz.
//
//  Lifecycle: the CADisplayLink is paused on background (we must not pump frames
//  into a backgrounded CAMetalLayer; the emulator itself is also paused via
//  ScenePhase) and resumed on foreground, re-syncing the drawable size and resetting
//  the pacing clock so the background gap is not replayed as frame debt. The
//  renderer (owned by EmulatorCore) survives a brief background — wgpu tolerates a
//  transient Lost/Outdated surface — so we only gate the frame pump, never the
//  renderer handle (which stays balanced gfx_init/gfx_destroy in EmulatorCore).
//

import MetalKit
import SwiftUI
import UIKit

/// A SwiftUI wrapper over the Metal-backed game view + its frame loop.
struct MetalGameView: UIViewRepresentable {
    let emulator: EmulatorCore

    func makeCoordinator() -> Coordinator {
        Coordinator(emulator: emulator)
    }

    func makeUIView(context: Context) -> MTKView {
        let view = MTKView()
        view.device = MTLCreateSystemDefaultDevice()
        // wgpu owns the drawable and presents; the MTKView is only the layer host.
        view.isPaused = true
        view.enableSetNeedsDisplay = false
        view.framebufferOnly = true
        // Nearest-neighbour-ish: the NES image is upscaled by wgpu, so keep the
        // host layer opaque and let the renderer fill it.
        view.isOpaque = true
        view.delegate = context.coordinator
        context.coordinator.view = view
        context.coordinator.attachAndStart()
        return view
    }

    func updateUIView(_ uiView: MTKView, context: Context) {
        // Sizing is reactive via layoutSubviews; nothing to push on SwiftUI updates.
    }

    static func dismantleUIView(_ uiView: MTKView, coordinator: Coordinator) {
        coordinator.stop()
    }

    /// Owns the CADisplayLink and bridges the MTKView's drawable-size changes into
    /// the renderer. Also the (no-op) MTKViewDelegate, since wgpu does the drawing.
    final class Coordinator: NSObject, MTKViewDelegate {
        /// The NES frame period (1 / 60.0988 Hz). The pacing clock advances the core
        /// at this cadence regardless of the display's 60-120 Hz refresh.
        private static let consoleFramePeriod: CFTimeInterval = 1.0 / 60.0988
        /// Cap the catch-up burst per callback so a hitch can't spiral into a flood
        /// of emulated frames (the audio DRC absorbs the small steady-state residual).
        private static let maxFramesPerCallback = 2

        private let emulator: EmulatorCore
        weak var view: MTKView?
        private var displayLink: CADisplayLink?
        private var attached = false
        private var lastDrawableSize: CGSize = .zero

        // Pacing state: accumulate elapsed wall time and emit a console frame per
        // elapsed period. `lastTimestamp == 0` means "uninitialised" (first tick or
        // after a resume) so the first delta isn't a huge jump.
        private var frameAccumulator: CFTimeInterval = 0
        private var lastTimestamp: CFTimeInterval = 0

        init(emulator: EmulatorCore) {
            self.emulator = emulator
            super.init()
            registerLifecycleObservers()
        }

        deinit {
            NotificationCenter.default.removeObserver(self)
        }

        /// Build the renderer for the current drawable and start the loop.
        func attachAndStart() {
            guard let view, !attached else { return }
            let size = view.drawableSize
            guard size.width > 0, size.height > 0 else {
                // The drawable is not sized yet; defer to the first delegate call.
                return
            }
            let ptr = Unmanaged.passUnretained(view).toOpaque()
            emulator.attach(view: ptr, width: UInt32(size.width), height: UInt32(size.height))
            lastDrawableSize = size
            attached = true
            emulator.start()
            startDisplayLink()
        }

        private func startDisplayLink() {
            guard displayLink == nil else { return }
            let link = CADisplayLink(target: self, selector: #selector(step(_:)))
            link.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: 120, preferred: 120)
            link.add(to: .main, forMode: .common)
            displayLink = link
        }

        @objc private func step(_ link: CADisplayLink) {
            // If the drawable resized (rotation / Stage Manager), reconfigure first.
            if let view, view.drawableSize != lastDrawableSize {
                let size = view.drawableSize
                if size.width > 0, size.height > 0 {
                    if attached {
                        emulator.resize(width: UInt32(size.width), height: UInt32(size.height))
                    } else {
                        attachAndStart()
                    }
                    lastDrawableSize = size
                }
            }

            // Pace the core to the console rate by elapsed time, NOT once per vsync
            // (which double-speeds on a 120 Hz ProMotion panel). `link.timestamp` is
            // the time the current frame is displayed.
            if lastTimestamp == 0 { lastTimestamp = link.timestamp }
            var delta = link.timestamp - lastTimestamp
            lastTimestamp = link.timestamp
            // Guard against a large jump (a stall, or a missed pause) replaying as a
            // burst of frames: treat anything implausibly large as a single period.
            if delta > 0.25 || delta < 0 { delta = Self.consoleFramePeriod }
            frameAccumulator += delta

            // Emit due console frames, capped to bound any catch-up burst.
            var budget = Self.maxFramesPerCallback
            while frameAccumulator >= Self.consoleFramePeriod, budget > 0 {
                emulator.tick()
                frameAccumulator -= Self.consoleFramePeriod
                budget -= 1
            }
            // Drop unspent debt beyond one period so a sustained slow patch can't
            // accumulate an unbounded backlog.
            if frameAccumulator > Self.consoleFramePeriod {
                frameAccumulator = frameAccumulator.truncatingRemainder(dividingBy: Self.consoleFramePeriod)
            }
        }

        /// Reset the pacing clock so a wall-time gap (a background pause, a resume)
        /// is not replayed as a flood of catch-up frames.
        private func resetPacing() {
            lastTimestamp = 0
            frameAccumulator = 0
        }

        func stop() {
            NotificationCenter.default.removeObserver(self)
            displayLink?.invalidate()
            displayLink = nil
            emulator.pause()
        }

        // MARK: Scene background / foreground

        private func registerLifecycleObservers() {
            let center = NotificationCenter.default
            center.addObserver(
                self,
                selector: #selector(appDidEnterBackground),
                name: UIApplication.didEnterBackgroundNotification,
                object: nil
            )
            center.addObserver(
                self,
                selector: #selector(appWillEnterForeground),
                name: UIApplication.willEnterForegroundNotification,
                object: nil
            )
        }

        /// Stop driving frames into the backgrounded CAMetalLayer. Pause (not
        /// invalidate) so we keep the link for a clean resume. The EmulatorCore is
        /// independently paused via AppModel's ScenePhase handling.
        @objc private func appDidEnterBackground() {
            displayLink?.isPaused = true
        }

        /// Rebuild on return: handle the deferred-init case (drawable unsized at
        /// first), re-sync a drawable-size change that happened while backgrounded,
        /// reset the pacing clock, then resume the link.
        @objc private func appWillEnterForeground() {
            guard attached else {
                // The renderer was never built (drawable was 0 at makeUIView); try now.
                attachAndStart()
                return
            }
            if let view {
                let size = view.drawableSize
                if size.width > 0, size.height > 0, size != lastDrawableSize {
                    emulator.resize(width: UInt32(size.width), height: UInt32(size.height))
                    lastDrawableSize = size
                }
            }
            resetPacing()
            displayLink?.isPaused = false
        }

        // MARK: MTKViewDelegate (wgpu owns drawing; these are intentionally inert).

        func mtkView(_ view: MTKView, drawableSizeWillChange size: CGSize) {
            // Handled in `step` against `view.drawableSize`; nothing to do here.
        }

        func draw(in view: MTKView) {
            // No-op: wgpu presents from the gfx FFI, not from this delegate.
        }
    }
}
