//
//  ScreenRecorder.swift  (v1.9.8 "Horizon")
//
//  Gameplay capture via ReplayKit's `RPScreenRecorder`. A Record toggle (the in-game
//  pill menu + a Settings row) starts/stops an in-app recording; on stop ReplayKit
//  hands back an `RPPreviewViewController` the user can trim, save to Photos, or share
//  (ReplayKit's own preview owns the share sheet, so there is no `UIActivityViewController`
//  popover for the host to anchor). The not-available / permission-denied cases surface
//  a message and leave the emulator untouched.
//
//  Concurrency: `RPScreenRecorder`'s completion handlers are not guaranteed to land on
//  the main thread, so every state mutation hops back to the @MainActor via `Task`.
//
//  Picture-in-Picture follow-up (documented, NOT built this release): the game view is a
//  Metal/wgpu surface, not an `AVPlayerLayer`, so system PiP would require feeding frames
//  through a custom `AVSampleBufferDisplayLayer` + `AVPictureInPictureController` with a
//  `AVSampleBufferDisplayLayer` content source — a substantial, separate workstream
//  (pixel-buffer pool, CMSampleBuffer timing, audio routing). It is deferred to a later
//  iOS release; ReplayKit capture here covers the "record + share a clip" use case.
//

import Foundation
import ReplayKit
import SwiftUI

/// A pending preview controller to present (Identifiable so SwiftUI's
/// `.fullScreenCover(item:)` can drive it).
struct RecordingPreviewRequest: Identifiable {
    let id = UUID()
    let controller: RPPreviewViewController
}

@MainActor
final class ScreenRecorder: NSObject, ObservableObject {
    /// True while a recording is in progress (drives the pill's red indicator).
    @Published private(set) var isRecording = false
    /// A transient error surfaced to the UI (not available / denied / stop failed).
    @Published var lastError: String?
    /// Set when ReplayKit returns a preview to present; cleared when it is dismissed.
    @Published var preview: RecordingPreviewRequest?

    private let recorder = RPScreenRecorder.shared()

    /// Whether the device currently allows recording (false in the Simulator, while a
    /// system capture is already active, or when the user has disabled Screen Recording).
    var isAvailable: Bool { recorder.isAvailable }

    /// Start an in-app recording (gameplay video only; the microphone stays off).
    func startRecording() {
        guard !isRecording else { return }
        guard recorder.isAvailable else {
            lastError = String(localized: "Screen recording isn't available right now.")
            return
        }
        recorder.isMicrophoneEnabled = false
        recorder.startRecording { [weak self] error in
            Task { @MainActor in
                guard let self else { return }
                if let error {
                    self.isRecording = false
                    self.lastError = error.localizedDescription
                } else {
                    self.isRecording = true
                }
            }
        }
    }

    /// Stop recording and present ReplayKit's preview for save / share.
    func stopRecording() {
        guard isRecording else { return }
        recorder.stopRecording { [weak self] previewController, error in
            Task { @MainActor in
                guard let self else { return }
                self.isRecording = false
                if let error {
                    self.lastError = error.localizedDescription
                    return
                }
                guard let previewController else { return }
                previewController.previewControllerDelegate = self
                self.preview = RecordingPreviewRequest(controller: previewController)
            }
        }
    }

    func toggle() {
        if isRecording { stopRecording() } else { startRecording() }
    }
}

extension ScreenRecorder: RPPreviewViewControllerDelegate {
    nonisolated func previewControllerDidFinish(_ previewController: RPPreviewViewController) {
        Task { @MainActor in self.preview = nil }
    }
}

/// Hosts the ReplayKit preview controller inside a SwiftUI cover.
struct ScreenRecordingPreview: UIViewControllerRepresentable {
    let controller: RPPreviewViewController
    func makeUIViewController(context: Context) -> RPPreviewViewController { controller }
    func updateUIViewController(_ uiViewController: RPPreviewViewController, context: Context) {}
}

/// An invisible host that presents the recorder's preview when one is available. Drop
/// it into the game view's `ZStack`; it observes the recorder so the cover follows the
/// published `preview`.
struct RecordingPreviewHost: View {
    @ObservedObject var recorder: ScreenRecorder

    var body: some View {
        Color.clear
            .allowsHitTesting(false)
            .fullScreenCover(item: $recorder.preview) { request in
                ScreenRecordingPreview(controller: request.controller)
                    .ignoresSafeArea()
            }
    }
}
