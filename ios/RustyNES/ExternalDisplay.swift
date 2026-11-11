//
//  ExternalDisplay.swift
//
//  v3.7.0 (D24, `T-MOBILE-EXTRAS` row 2): the game on an external display (a cable or
//  AirPlay), with the phone as the controller. The iOS twin of Android's
//  `CastManager` (Cast.kt), which puts the picture on a `Presentation`.
//
//  UNCOMPILED on the Linux host this was written on: no Swift toolchain. The iOS CI
//  Simulator build is the first compile; the device run-sheet rows E1-E4
//  (docs/mobile-v2.9.3-run-sheet.md) are the first run.
//
//  How the scene arrives, which changed across iOS versions (Apple, "Presenting
//  content on a connected display"):
//
//  - iOS 16-26: the system connects a scene with the role
//    `UISceneSession.Role.windowExternalDisplayNonInteractive` (named
//    `windowExternalDisplay` before iOS 16) on its own when a display attaches; the
//    app supplies content by answering `application(_:configurationForConnecting:
//    options:)` with a configuration whose delegate attaches a window, and declines
//    by answering with no delegate (the system then mirrors the phone).
//  - iOS 27 and later: that scene arrives only after the app registers a
//    `UISceneAccessory` (`.externalNonInteractive(sceneConfiguration:)`) from a
//    presented view controller. That API is in the iOS 27 SDK; CI builds with the
//    Xcode 26 / iOS 26 SDK (.github/workflows/ios.yml), where the symbol does not
//    exist, so the registration is NOT written here: on iOS 27 the display mirrors
//    the phone, as before v3.7.0. The run sheet's row E4 and the plan carry it to the
//    first release built with the iOS 27 SDK.
//
//  What the display shows: the latest presented frame (`EmulatorCore.lastFrame`, the
//  RGBA8 buffer the save-state thumbnails already read), redrawn on a display link.
//  The phone keeps running the game (its Metal view drives `tick()`), covers the
//  picture with a "playing on" card and keeps the on-screen controls: the phone is
//  the controller. Nothing here touches the core: the picture is a copy of a frame
//  the core already produced.
//

import SwiftUI
import UIKit

/// The shared link between the app's model and the external-display scene, which
/// UIKit creates on its own (outside SwiftUI's environment). Main actor only.
@MainActor
final class ExternalDisplayHub: ObservableObject {
    static let shared = ExternalDisplayHub()

    /// Whether an external-display scene is connected and showing the game.
    @Published private(set) var isConnected = false

    /// Supplies the latest RGBA8 frame (256 x 240); set by `AppModel` to read the
    /// running core's `lastFrame`. `nil` with no game, and the display goes black.
    var frameSource: (() -> Data?)?

    fileprivate func connected() { isConnected = true }

    fileprivate func disconnected() { isConnected = false }
}

/// The app delegate SwiftUI adopts (`@UIApplicationDelegateAdaptor` in
/// `RustyNESApp`) for one purpose: to give the external-display scene a delegate.
/// Every other scene role gets the default configuration SwiftUI builds.
final class RustyNESAppDelegate: NSObject, UIApplicationDelegate {
    func application(
        _ application: UIApplication,
        configurationForConnecting connectingSceneSession: UISceneSession,
        options: UIScene.ConnectionOptions
    ) -> UISceneConfiguration {
        let config = UISceneConfiguration(
            name: nil,
            sessionRole: connectingSceneSession.role
        )
        if connectingSceneSession.role == .windowExternalDisplayNonInteractive {
            config.delegateClass = ExternalDisplaySceneDelegate.self
        }
        return config
    }
}

/// Attaches a full-screen window with the game picture to the external display's
/// scene, and tells the hub so the phone switches to controller mode.
final class ExternalDisplaySceneDelegate: NSObject, UIWindowSceneDelegate {
    var window: UIWindow?

    func scene(
        _ scene: UIScene,
        willConnectTo session: UISceneSession,
        options connectionOptions: UIScene.ConnectionOptions
    ) {
        guard let windowScene = scene as? UIWindowScene else { return }
        let window = UIWindow(windowScene: windowScene)
        window.rootViewController = UIHostingController(rootView: ExternalGameView())
        window.isHidden = false
        self.window = window
        Task { @MainActor in ExternalDisplayHub.shared.connected() }
    }

    func sceneDidDisconnect(_ scene: UIScene) {
        window = nil
        Task { @MainActor in ExternalDisplayHub.shared.disconnected() }
    }
}

/// The external display's content: the NES picture, letterboxed on black at the
/// NES's 8:7 pixel aspect, nearest-neighbour scaled. Redrawn every display frame.
private struct ExternalGameView: View {
    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()
            TimelineView(.animation) { _ in
                if let image = ExternalGameView.currentImage() {
                    Image(decorative: image, scale: 1)
                        .interpolation(.none)
                        .resizable()
                        // 256 x 240 storage pixels at the NES's 8:7 pixel aspect.
                        .aspectRatio(256.0 * 8.0 / 7.0 / 240.0, contentMode: .fit)
                }
            }
        }
    }

    /// The latest frame as a `CGImage`, or nil before the first frame.
    @MainActor
    static func currentImage() -> CGImage? {
        guard let frame = ExternalDisplayHub.shared.frameSource?() else { return nil }
        return ExternalFrame.image(rgba: frame, width: 256, height: 240)
    }
}

/// RGBA8 bytes to a `CGImage` (the same byte-order setup `EmulatorCore.snapshotPNG`
/// uses: R,G,B,A in memory, opaque NES pixels).
enum ExternalFrame {
    static func image(rgba: Data, width: Int, height: Int) -> CGImage? {
        let bytesPerRow = width * 4
        guard rgba.count >= bytesPerRow * height,
              let provider = CGDataProvider(data: rgba as CFData)
        else { return nil }
        let info = CGBitmapInfo(
            rawValue: CGImageAlphaInfo.premultipliedLast.rawValue
                | CGBitmapInfo.byteOrder32Big.rawValue
        )
        return CGImage(
            width: width,
            height: height,
            bitsPerComponent: 8,
            bitsPerPixel: 32,
            bytesPerRow: bytesPerRow,
            space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: info,
            provider: provider,
            decode: nil,
            shouldInterpolate: false,
            intent: .defaultIntent
        )
    }
}

/// The phone's picture area while the game is on the external display: a card that
/// says where the picture went. The on-screen controls stay above it (GameView).
struct ExternalDisplayCard: View {
    @ObservedObject var hub: ExternalDisplayHub

    var body: some View {
        if hub.isConnected {
            ZStack {
                Color.black.ignoresSafeArea()
                VStack(spacing: 8) {
                    Image(systemName: "tv")
                        .font(.system(size: 40))
                        .foregroundStyle(.secondary)
                        .accessibilityHidden(true)
                    Text("Playing on the external display")
                        .font(.headline)
                        .foregroundStyle(.white)
                }
                .padding(.bottom, 120)
            }
            .accessibilityElement(children: .combine)
        }
    }
}
