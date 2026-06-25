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
//  ProMotion: the display link requests 60-120 Hz; the emulator still advances at
//  the console rate (one `runFrame` per tick), and the audio sink/DRC absorbs the
//  ~60.0988 Hz <-> 120 Hz beat. `CADisableMinimumFrameDurationOnPhone` (Info.plist)
//  unlocks 120 Hz on ProMotion iPhones.
//

import MetalKit
import SwiftUI

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
        private let emulator: EmulatorCore
        weak var view: MTKView?
        private var displayLink: CADisplayLink?
        private var attached = false
        private var lastDrawableSize: CGSize = .zero

        init(emulator: EmulatorCore) {
            self.emulator = emulator
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
            emulator.tick()
        }

        func stop() {
            displayLink?.invalidate()
            displayLink = nil
            emulator.pause()
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
