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
//  Pacing (v2.7.4, frontend audit IOS-01). The display link now asks for exactly
//  60 Hz. Before, it asked for 60-120 Hz (preferring 120) and paced the core by a
//  wall-clock accumulator at the console's 60.0988 Hz; because 60.0988 does not
//  divide either refresh, a console frame periodically got one vsync instead of
//  two at 120 Hz (every ~5 s), or two frames landed in one vsync at 60 Hz (every
//  ~10 s) -- a visible hitch either way. Now, when the link really runs at ~60 Hz,
//  exactly ONE console frame runs per vsync (display sync, as on the desktop): no
//  frame is ever doubled or dropped. The core then runs 0.16% slow, well inside
//  the +/-1% the sink's rate control absorbs (v2.7.4, IOS-02), so audio neither
//  underruns nor drifts. On any other refresh (a ProMotion override, an external
//  display) the accumulator below remains the fallback.
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
        /// A display interval within this of 1/60 s counts as a 60 Hz display, where
        /// the loop runs exactly one console frame per vsync (IOS-01).
        private static let vsyncLockTolerance: CFTimeInterval = 0.0015
        /// Cap the catch-up burst per callback so a hitch can't spiral into a flood
        /// of emulated frames (the audio rate control absorbs the steady residual).
        private static let maxFramesPerCallback = 2
        /// Cap the retained pacing debt (in console frames) so a transient stutter is
        /// caught up over the next callbacks but a sustained slow patch can't grow an
        /// unbounded backlog.
        private static let maxBacklogFrames = 4

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
            // v2.7.4 (IOS-01): exactly 60 Hz. 120 Hz bought nothing for a 60 Hz
            // picture and made the pacing hitch (see the file header).
            link.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: 60, preferred: 60)
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

            // Display sync (IOS-01): on a ~60 Hz link, one console frame per vsync.
            // `targetTimestamp - timestamp` is the link's actual frame interval.
            let interval = link.targetTimestamp - link.timestamp
            if abs(interval - 1.0 / 60.0) < Self.vsyncLockTolerance {
                emulator.tick()
                // Keep the fallback's clock current, so switching to it (a refresh
                // change) does not replay the time spent here as frame debt.
                lastTimestamp = link.timestamp
                frameAccumulator = 0
                return
            }

            // Fallback: pace the core to the console rate by elapsed time, NOT
            // once per vsync (which would double-speed on a 120 Hz panel).
            // `link.timestamp` is the time the current frame is displayed.
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
            // Keep a small amount of unspent debt so a transient stutter (a few
            // frames late) is caught up smoothly over the next callbacks instead of
            // permanently dropped, but CLAMP it so a sustained slow patch can't grow
            // an unbounded backlog (which would otherwise spiral on resume).
            let maxBacklog = Self.consoleFramePeriod * Double(Self.maxBacklogFrames)
            if frameAccumulator > maxBacklog {
                frameAccumulator = maxBacklog
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
