# 24. A shared `rustynes-mobile` UniFFI bridge with a hybrid Android host

Date: 2026-06-19

## Status

Accepted (v1.8.0 "Android", Workstream A — foundation & build).

## Context

RustyNES is a pure-Rust workspace: a `#![no_std]`+alloc cycle-accurate chip stack
(`rustynes-{cpu,ppu,apu,mappers,core}`) wrapped by hosted frontends — the desktop
`rustynes-frontend` (winit + wgpu + cpal + egui) and a WebAssembly build. v1.8.0
adds the first *platform* (not accuracy) target: a complete, shippable Android
app. The core compiles to AArch64 and produces **bit-identical** framebuffer +
audio on ARM, so the entire emulator value (accuracy, shaders, save-states, TAS)
ports for free; only the host shell — lifecycle, render surface, audio sink,
touch UI, file access, packaging — is new.

Two questions had to be settled before any Android code was written:

1. **How does Kotlin call the Rust core?** Options: hand-write JNI for the whole
   control surface; generate the binding with UniFFI; or embed the existing winit
   app via `android-activity`.
2. **What renders the NES image?** Options: (a) compile the existing
   winit/wgpu/egui frontend to Android; (b) a pure-Compose host with no wgpu
   (re-implement blitting in Kotlin); (c) a hybrid — keep wgpu rendering onto an
   Android `SurfaceView`, with Jetpack Compose owning the chrome.

A v1.9.0 iOS port is planned, so whatever binding strategy we pick should serve
both mobile targets.

## Decision

**Bindings — a shared `rustynes-mobile` crate generated with UniFFI, plus a thin
`rustynes-android` crate for the hot glue UniFFI can't express.**

`rustynes-mobile` owns the typed control surface over the core — load a ROM from
a *byte buffer* (never a path), set the per-port `Buttons` mask, run a frame,
borrow the framebuffer/audio, save/restore state, query metadata — annotated with
`#[uniffi::export]`. UniFFI generates the Kotlin (Android) and Swift (iOS)
bindings, so the foreign-language surface is type-checked and identical across
both mobile hosts. The crate is `std`, host-testable (`cargo test --workspace`
exercises the control surface on host CI), and adds **no new determinism
surface**: every method forwards directly into `rustynes_core::Nes` with no
timing feedback or wall-clock dependence.

`rustynes-android` carries only the narrow, hot, hand-rolled glue UniFFI cannot
express: handing the native surface handle (`ANativeWindow`, from an
`android.view.Surface`) to wgpu, and the audio sink. Everything Android-specific
is behind `#[cfg(target_os = "android")]`, so on a host build the crate is a near-
empty shell that exists to be linted; the real `.so` is produced by `cargo-ndk`.
`unsafe` is confined here (each site with a `// SAFETY:` note), the same exemption
`rustynes-cheevos` and `rustynes-frontend` already take.

**Rendering — Option (c), Hybrid.** Keep wgpu rendering the NES image (full
shader stack: NTSC/CRT/Bisqwit + the 8:7-PAR/overscan/letterbox blit + the wgpu
present path) onto a `SurfaceView`; Jetpack Compose owns the chrome (top bar,
settings, SAF ROM picker, touch overlay, save-state manager). Touch/gamepad
events publish into the existing late-latched `Buttons` mask — the exact pattern
the wasm touch overlay already proved — so TAS/netplay/rollback see input
identically.

**beta.1 spike — Option (a) as a proof, not the ship path.** The existing winit +
wgpu + egui `App` can compile to Android via `android-activity` (the `android_main`
entry in `rustynes-android`), the fastest proof the core renders + sounds on real
ARM; winit 0.30's `resumed`/`suspended` already models the surface-loss contract.
The first-boot Compose shell, however, drives the core through the pure UniFFI
bridge (a simple RGBA→`Bitmap` blit); beta.2 swaps that blit for the wgpu
`SurfaceView` shader pipeline.

**Toolchain.** Rust targets `aarch64-linux-android` (ship) + `x86_64-linux-android`
(emulator/CI), built with `cargo-ndk` against NDK r27+ (16 KB page alignment, a
Play requirement for Android 15+), packaged as an **AAB** (not `cargo-apk`, which
is deprecated + APK-only). `minSdk 26` (AAudio floor), `targetSdk 35` (Play
mandate). The Gradle module invokes `cargo ndk` and `uniffi-bindgen` as build
steps so the `.so` + generated Kotlin are always in sync with the Rust source.

## Consequences

- **Positive.** The hand-rolled `unsafe` FFI shrinks to the surface/audio handoff;
  the entire control surface is type-checked and shared verbatim with iOS
  (v1.9.0's `rustynes-ios` wraps the same `rustynes-mobile`). The shader/PAR/
  overscan render crown jewels port unchanged. Host CI still owns accuracy; the
  Android CI only proves the build *links* + a smoke boot, and the cross-build is
  verified end-to-end (host build + tests, Kotlin generation, NDK cross-compile to
  arm64). A save-state written on desktop loads on Android and a `.rnm` replays
  bit-identically, so desktop⇄Android cross-play stays valid.
- **Negative.** UniFFI adds a build-time dependency + a binding-generation step,
  and its Kotlin runtime pulls JNA (`@aar`). The hybrid surface handoff means the
  surface-loss lifecycle (drop the `Surface` on `surfaceDestroyed`, recreate on
  `surfaceCreated`) must be handled correctly — the keystone risk Workstream B
  retires. Marshalling a full framebuffer across the FFI each frame would be
  wasteful, so the hot path borrows the framebuffer pointer in `rustynes-android`
  and bypasses the owned-`Vec` `run_frame` (kept for the spike + copy callers).
- **Rejected — Option (b), pure-Compose-no-wgpu.** Discards the shader stack + the
  PAR/overscan blit + the wgpu present path for Material polish: an XL rewrite at
  lower fidelity.
- **Rejected — hand-written JNI for the whole surface.** Maximises `unsafe`,
  duplicates the entire API by hand for iOS, and is not type-checked.
