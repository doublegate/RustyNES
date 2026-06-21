# RustyNES Chromecast Web Receiver (v1.8.7, #38)

This folder holds the custom **Cast Application Framework (CAF) Web Receiver** that
the Android app's `ChromecastSender` streams gameplay frames to. It is the receiver
half of an experimental **spectator mirror** (a ~20-30fps view on the TV, *not* a
play surface — Cast custom-message latency is multi-hundred-ms).

The whole feature ships **disabled by default** behind the `CHROMECAST_ENABLED`
`BuildConfig` flag in `android/app/build.gradle.kts`. Nothing here runs and no Cast
button appears in normal builds. The primary low-latency cast remains the
Presentation API path (`android/app/.../Cast.kt`), which is unaffected.

## Why a custom receiver is required

Google removed the **Cast Remote Display API** (~2019). To send a live emulator
picture to a Chromecast today you need:

1. an Android **Sender** — `ChromecastSender.kt` (already in the repo), and
2. a custom **Web Receiver** — `index.html` in this folder — that you **host over
   HTTPS** and **register for a Receiver App ID**.

The sender 2x down-samples the native 256x240 frame to a **128x120** 6-bit
colour-index plane (one byte/pixel = 15,360 bytes raw → ~20,480 base64 chars,
comfortably under the Cast custom-message **64 KB cap**) and sends it as a JSON
message on `urn:x-cast:com.doublegate.rustynes.fb`. `index.html` decodes it, maps
each index through the standard NES palette to RGBA, draws to a 128x120 canvas, and
lets CSS nearest-neighbour upscale it full-screen. (The full 256x240 plane would
base64 to ~82 KB, over the cap — hence the down-sample.)

## Frame protocol

Custom message namespace: `urn:x-cast:com.doublegate.rustynes.fb`

```json
{ "w": 128, "h": 120, "fmt": "i6", "data": "<base64 of a w*h 1-byte/pixel 6-bit colour-index plane>" }
```

The receiver reads `w`/`h` from the message and resizes its canvas accordingly, so
the resolution can change without a receiver update.

The palette table in `index.html` (`NES_PALETTE`) is byte-identical to the core's
`crates/rustynes-ppu/src/palette.rs` `NES_PALETTE` and the app's
`NetplayPalette.BASE`. Keep them in sync if the core palette ever changes.

## Maintainer setup (the deferred ops)

Before this works live, the maintainer must:

1. **Get a Cast Developer Console account** (one-time ~$5 registration fee):
   <https://cast.google.com/publish>.
2. **Host `index.html` over HTTPS.** GitHub Pages works
   (e.g. `https://doublegate.github.io/RustyNES/cast-receiver/`).
3. **Register a Custom Receiver App ID** in the Cast console pointing at that HTTPS
   URL. You get an 8-hex-digit App ID.
4. **Paste the App ID** into `ChromecastConstants.APP_ID`
   (`android/app/.../ChromecastSender.kt`), replacing the `"RUSTYNES0"` placeholder.
5. **Register your test Chromecast's serial** as a dev device in the Cast console
   (required to test an unpublished receiver), then reboot the device.
6. **Flip the flag:** set `CHROMECAST_ENABLED` to `true` in
   `android/app/build.gradle.kts` and rebuild. A Cast button labelled
   "Cast to TV (spectator ~20-30fps)" then appears in the control bar.

Until step 6, the `play-services-cast-framework` dependency is linked but dormant
(no `CastContext` is ever initialized), and the placeholder App ID is harmless.

## Notes / limitations

- **Spectator only.** Latency makes this a TV mirror, not a control surface. The
  phone keeps the controller and remains the authoritative play surface.
- **Emphasis is dropped** by the sender (to stay under the 64 KB cap); the picture
  stays faithful for the vast majority of frames.
- Frames are internally throttled to ~25fps (`MIN_FRAME_INTERVAL_MS = 40`) and any
  frame whose encoded message would exceed 64 KB is skipped (the receiver keeps the
  previous picture).
