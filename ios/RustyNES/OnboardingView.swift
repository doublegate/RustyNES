//
//  OnboardingView.swift
//
//  The first-launch flow: a few paged cards that explain RustyNES is a
//  bring-your-own-ROM emulator (App Review Guideline 4.7 ownership notice), how to
//  import a ROM (Files / share sheet), and a quick tour of the controls (the
//  on-screen pad + MFi controllers). Shown once, gated by the `didOnboard`
//  UserDefaults flag (see ContentView); "Get Started" sets it so it never returns.
//

import SwiftUI

struct OnboardingView: View {
    /// Called when the user finishes or skips; the caller flips `didOnboard`.
    var onFinish: () -> Void

    @State private var page = 0

    // Each title/body is a single string literal so it is a `LocalizedStringKey` and
    // localizes via the String Catalog (see `PageView`, which renders the stored String
    // values through `LocalizedStringKey`).
    private let pages: [OnboardingPage] = [
        OnboardingPage(
            systemImage: "gamecontroller.fill",
            title: "Welcome to RustyNES",
            body: "A cycle-accurate NES emulator for iPhone and iPad. RustyNES plays only the ROM files you supply from your own device. No games are bundled or downloaded \u{2014} you must own the games you play."
        ),
        OnboardingPage(
            systemImage: "square.and.arrow.down.on.square",
            title: "Import a ROM",
            body: "Tap the + button in the library to import a .nes file from Files, or share a ROM to RustyNES from another app. Your games stay on your device."
        ),
        OnboardingPage(
            systemImage: "dpad",
            title: "Play your way",
            body: "Use the on-screen controller, or pair an MFi / Xbox / PlayStation controller for up to four players. Tap the MENU pill on the pad to show the in-game menu, save states, and settings."
        )
    ]

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Spacer()
                Button("Skip", action: onFinish)
                    .padding()
            }

            TabView(selection: $page) {
                ForEach(pages.indices, id: \.self) { i in
                    PageView(page: pages[i])
                        .tag(i)
                }
            }
            .tabViewStyle(.page(indexDisplayMode: .always))

            Button(action: advance) {
                Text(page == pages.count - 1 ? "Get Started" : "Next")
                    .font(.headline)
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .padding()
        }
    }

    private func advance() {
        if page < pages.count - 1 {
            withAnimation { page += 1 }
        } else {
            onFinish()
        }
    }
}

private struct OnboardingPage {
    let systemImage: String
    let title: String
    let body: String
}

private struct PageView: View {
    let page: OnboardingPage

    var body: some View {
        VStack(spacing: 24) {
            Image(systemName: page.systemImage)
                .font(.system(size: 72))
                .foregroundStyle(.tint)
                .accessibilityHidden(true) // decorative; the title/body carry the meaning
            // The stored String values are looked up as catalog keys via
            // `LocalizedStringKey` (a plain `Text(String)` would render verbatim).
            Text(LocalizedStringKey(page.title))
                .font(.title.bold())
                .multilineTextAlignment(.center)
            Text(LocalizedStringKey(page.body))
                .font(.body)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 32)
        }
        .padding()
    }
}
