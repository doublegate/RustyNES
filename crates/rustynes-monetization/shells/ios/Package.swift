// swift-tools-version:5.9
//
// Package.swift — SwiftPM wiring for the iOS app's dependencies.
//
// Three dependencies are declared:
//   • RustyNesMonetization  — the Rust core, packaged as a Swift package by `cargo swift`
//                     (see README). It contains the generated RustyNesMonetization.swift plus
//                     the librustynes_monetization xcframework as a binaryTarget.
//   • RevenueCat    — purchases-ios SDK (entitlement source of truth).
//   • AppLovinSDK   — MAX mediation SDK.
//
// In practice many teams add RevenueCat and AppLovinSDK through Xcode's SPM UI on the
// app target and keep RustyNesMonetization as a local package. This manifest shows the
// all-SPM arrangement for a self-contained reference.
//
// NOTE: AppLovin distributes AppLovinSDK via SPM at the URL below; pin to the newest
// 13.x tag. RevenueCat's SPM package is purchases-ios.

import PackageDescription

let package = Package(
    name: "RustyNesApp",
    platforms: [
        .iOS(.v14) // AppLovin MAX 13 / RevenueCat current minimums
    ],
    products: [
        .library(name: "RustyNesApp", targets: ["RustyNesApp"])
    ],
    dependencies: [
        // Local package generated from the Rust core by `cargo swift package`.
        .package(path: "../RustyNesMonetization"),
        // RevenueCat — pin to the newest tag before release.
        .package(url: "https://github.com/RevenueCat/purchases-ios.git", from: "5.0.0"),
        // AppLovin MAX — pin to the newest 13.x tag before release.
        .package(url: "https://github.com/AppLovin/AppLovin-MAX-Swift-Package.git", from: "13.0.0"),
    ],
    targets: [
        .target(
            name: "RustyNesApp",
            dependencies: [
                .product(name: "RustyNesMonetization", package: "RustyNesMonetization"),
                .product(name: "RevenueCat", package: "purchases-ios"),
                .product(name: "AppLovinSDK", package: "AppLovin-MAX-Swift-Package"),
            ],
            path: "Sources/RustyNesApp"
        )
    ]
)
