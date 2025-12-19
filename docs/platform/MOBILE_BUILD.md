# RustyNES Mobile Build Guide

Considerations and architecture for potential mobile platform support (iOS, Android).

## Overview

While RustyNES is primarily designed for desktop and web platforms, the modular architecture allows for potential mobile ports. This document outlines the considerations and approaches for mobile development.

## Architecture Considerations

### Core Separation

The `rustynes-core` crate is designed to be platform-agnostic:

```rust
// rustynes-core is no_std compatible
#![no_std]

extern crate alloc;
use alloc::vec::Vec;

pub struct Emulator {
    cpu: Cpu,
    ppu: Ppu,
    apu: Apu,
    bus: Bus,
}

impl Emulator {
    /// Run one frame of emulation
    /// Returns framebuffer and audio samples
    pub fn run_frame(&mut self) -> FrameResult {
        // Platform-independent emulation logic
    }
}
```

### Platform Abstraction Layer

```rust
/// Platform abstraction for input/output
pub trait Platform {
    type AudioOutput: AudioSink;
    type VideoOutput: VideoSink;
    type InputSource: InputProvider;

    fn audio(&mut self) -> &mut Self::AudioOutput;
    fn video(&mut self) -> &mut Self::VideoOutput;
    fn input(&self) -> &Self::InputSource;
}

pub trait AudioSink {
    fn queue_samples(&mut self, samples: &[f32]);
    fn sample_rate(&self) -> u32;
}

pub trait VideoSink {
    fn render_frame(&mut self, framebuffer: &[u32], width: u32, height: u32);
}

pub trait InputProvider {
    fn controller_state(&self, player: u8) -> u8;
}
```

## Android

### Project Structure

```
rustynes-android/
├── app/
│   ├── src/main/
│   │   ├── java/com/rustynes/
│   │   │   ├── MainActivity.kt
│   │   │   ├── EmulatorView.kt
│   │   │   ├── AudioPlayer.kt
│   │   │   └── TouchController.kt
│   │   ├── jniLibs/
│   │   │   ├── arm64-v8a/librustynes.so
│   │   │   ├── armeabi-v7a/librustynes.so
│   │   │   └── x86_64/librustynes.so
│   │   └── res/
│   └── build.gradle
└── rust/
    ├── Cargo.toml
    └── src/
        └── lib.rs  # JNI bindings
```

### JNI Bindings

```rust
// rust/src/lib.rs
use jni::JNIEnv;
use jni::objects::{JClass, JByteArray, JObject};
use jni::sys::{jlong, jint, jbyteArray};
use rustynes_core::Emulator;
use std::ptr;

/// Create new emulator instance
#[no_mangle]
pub extern "system" fn Java_com_rustynes_NativeEmulator_create(
    _env: JNIEnv,
    _class: JClass,
) -> jlong {
    let emulator = Box::new(Emulator::new());
    Box::into_raw(emulator) as jlong
}

/// Destroy emulator instance
#[no_mangle]
pub extern "system" fn Java_com_rustynes_NativeEmulator_destroy(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if handle != 0 {
        unsafe {
            let _ = Box::from_raw(handle as *mut Emulator);
        }
    }
}

/// Load ROM from byte array
#[no_mangle]
pub extern "system" fn Java_com_rustynes_NativeEmulator_loadRom(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    rom_data: JByteArray,
) -> jint {
    if handle == 0 {
        return -1;
    }

    let emulator = unsafe { &mut *(handle as *mut Emulator) };

    let data = match env.convert_byte_array(&rom_data) {
        Ok(d) => d,
        Err(_) => return -2,
    };

    match emulator.load_rom(&data) {
        Ok(_) => 0,
        Err(_) => -3,
    }
}

/// Run single frame, return framebuffer
#[no_mangle]
pub extern "system" fn Java_com_rustynes_NativeEmulator_runFrame(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
    input_p1: jint,
    input_p2: jint,
    framebuffer: JByteArray,
) -> jint {
    if handle == 0 {
        return -1;
    }

    let emulator = unsafe { &mut *(handle as *mut Emulator) };

    emulator.set_controller_state(0, input_p1 as u8);
    emulator.set_controller_state(1, input_p2 as u8);

    emulator.run_frame();

    // Copy framebuffer (RGBA format)
    let fb = emulator.framebuffer();
    let mut rgba = Vec::with_capacity(256 * 240 * 4);
    for &pixel in fb {
        rgba.push(((pixel >> 16) & 0xFF) as i8);
        rgba.push(((pixel >> 8) & 0xFF) as i8);
        rgba.push((pixel & 0xFF) as i8);
        rgba.push(-1i8); // 255 as signed byte
    }

    let _ = env.set_byte_array_region(&framebuffer, 0, &rgba);

    0
}

/// Get audio samples
#[no_mangle]
pub extern "system" fn Java_com_rustynes_NativeEmulator_getAudioSamples(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
    buffer: JByteArray,
) -> jint {
    if handle == 0 {
        return 0;
    }

    let emulator = unsafe { &mut *(handle as *mut Emulator) };
    let samples = emulator.audio_samples();

    // Convert f32 samples to i16
    let mut i16_samples: Vec<i8> = Vec::with_capacity(samples.len() * 2);
    for &sample in samples {
        let i16_val = (sample * 32767.0) as i16;
        i16_samples.push((i16_val & 0xFF) as i8);
        i16_samples.push((i16_val >> 8) as i8);
    }

    let _ = env.set_byte_array_region(&buffer, 0, &i16_samples);

    samples.len() as jint
}
```

### Kotlin Interface

```kotlin
// NativeEmulator.kt
package com.rustynes

class NativeEmulator {
    private var handle: Long = 0

    init {
        System.loadLibrary("rustynes")
        handle = create()
    }

    fun loadRom(data: ByteArray): Boolean {
        return loadRom(handle, data) == 0
    }

    fun runFrame(inputP1: Int, inputP2: Int, framebuffer: ByteArray) {
        runFrame(handle, inputP1, inputP2, framebuffer)
    }

    fun getAudioSamples(buffer: ByteArray): Int {
        return getAudioSamples(handle, buffer)
    }

    fun destroy() {
        if (handle != 0L) {
            destroy(handle)
            handle = 0
        }
    }

    protected fun finalize() {
        destroy()
    }

    private external fun create(): Long
    private external fun destroy(handle: Long)
    private external fun loadRom(handle: Long, data: ByteArray): Int
    private external fun runFrame(handle: Long, inputP1: Int, inputP2: Int, framebuffer: ByteArray): Int
    private external fun getAudioSamples(handle: Long, buffer: ByteArray): Int
}
```

### Build Configuration

```toml
# rust/Cargo.toml
[package]
name = "rustynes-android"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
rustynes-core = { path = "../../crates/rustynes-core" }
jni = "0.21"

[profile.release]
opt-level = 3
lto = true
```

### Build Script

```bash
#!/bin/bash
# build-android.sh

set -e

# Install Android targets
rustup target add aarch64-linux-android
rustup target add armv7-linux-androideabi
rustup target add x86_64-linux-android

# Set NDK path
export ANDROID_NDK_HOME=${ANDROID_NDK_HOME:-$HOME/Android/Sdk/ndk/25.0.8775105}

# Build for each architecture
for TARGET in aarch64-linux-android armv7-linux-androideabi x86_64-linux-android; do
    echo "Building for $TARGET..."

    # Set up linker
    case $TARGET in
        aarch64-linux-android)
            export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android21-clang"
            JNI_DIR="arm64-v8a"
            ;;
        armv7-linux-androideabi)
            export CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_LINKER="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/armv7a-linux-androideabi21-clang"
            JNI_DIR="armeabi-v7a"
            ;;
        x86_64-linux-android)
            export CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/x86_64-linux-android21-clang"
            JNI_DIR="x86_64"
            ;;
    esac

    cargo build --target $TARGET --release -p rustynes-android

    # Copy to jniLibs
    mkdir -p app/src/main/jniLibs/$JNI_DIR
    cp target/$TARGET/release/librustynes_android.so app/src/main/jniLibs/$JNI_DIR/librustynes.so
done

echo "Build complete!"
```

## iOS

### Project Structure

```
rustynes-ios/
├── RustyNES/
│   ├── RustyNESApp.swift
│   ├── ContentView.swift
│   ├── EmulatorView.swift
│   ├── AudioPlayer.swift
│   └── TouchController.swift
├── RustyNESCore/
│   ├── RustyNESCore.h
│   └── module.modulemap
└── rust/
    ├── Cargo.toml
    └── src/
        └── lib.rs  # C FFI bindings
```

### C FFI Bindings

```rust
// rust/src/lib.rs
use rustynes_core::Emulator;
use std::ffi::c_void;
use std::ptr;
use std::slice;

/// Opaque emulator handle
pub struct EmulatorHandle(Emulator);

/// Create new emulator
#[no_mangle]
pub extern "C" fn rustynes_create() -> *mut EmulatorHandle {
    let emulator = Box::new(EmulatorHandle(Emulator::new()));
    Box::into_raw(emulator)
}

/// Destroy emulator
#[no_mangle]
pub extern "C" fn rustynes_destroy(handle: *mut EmulatorHandle) {
    if !handle.is_null() {
        unsafe {
            let _ = Box::from_raw(handle);
        }
    }
}

/// Load ROM from data
#[no_mangle]
pub extern "C" fn rustynes_load_rom(
    handle: *mut EmulatorHandle,
    data: *const u8,
    len: usize,
) -> i32 {
    if handle.is_null() || data.is_null() {
        return -1;
    }

    let emulator = unsafe { &mut (*handle).0 };
    let rom_data = unsafe { slice::from_raw_parts(data, len) };

    match emulator.load_rom(rom_data) {
        Ok(_) => 0,
        Err(_) => -2,
    }
}

/// Run single frame
#[no_mangle]
pub extern "C" fn rustynes_run_frame(
    handle: *mut EmulatorHandle,
    input_p1: u8,
    input_p2: u8,
) {
    if handle.is_null() {
        return;
    }

    let emulator = unsafe { &mut (*handle).0 };
    emulator.set_controller_state(0, input_p1);
    emulator.set_controller_state(1, input_p2);
    emulator.run_frame();
}

/// Get framebuffer pointer
#[no_mangle]
pub extern "C" fn rustynes_get_framebuffer(
    handle: *mut EmulatorHandle,
) -> *const u32 {
    if handle.is_null() {
        return ptr::null();
    }

    let emulator = unsafe { &(*handle).0 };
    emulator.framebuffer().as_ptr()
}

/// Get audio samples
#[no_mangle]
pub extern "C" fn rustynes_get_audio_samples(
    handle: *mut EmulatorHandle,
    buffer: *mut f32,
    max_samples: usize,
) -> usize {
    if handle.is_null() || buffer.is_null() {
        return 0;
    }

    let emulator = unsafe { &mut (*handle).0 };
    let samples = emulator.audio_samples();
    let count = samples.len().min(max_samples);

    unsafe {
        ptr::copy_nonoverlapping(samples.as_ptr(), buffer, count);
    }

    count
}

/// Reset emulator
#[no_mangle]
pub extern "C" fn rustynes_reset(handle: *mut EmulatorHandle) {
    if !handle.is_null() {
        let emulator = unsafe { &mut (*handle).0 };
        emulator.reset();
    }
}
```

### Swift Wrapper

```swift
// EmulatorWrapper.swift
import Foundation

class EmulatorWrapper {
    private var handle: OpaquePointer?
    private var framebufferData = [UInt32](repeating: 0, count: 256 * 240)
    private var audioBuffer = [Float](repeating: 0, count: 4096)

    init() {
        handle = rustynes_create()
    }

    deinit {
        if let h = handle {
            rustynes_destroy(h)
        }
    }

    func loadRom(data: Data) -> Bool {
        guard let h = handle else { return false }

        return data.withUnsafeBytes { (ptr: UnsafeRawBufferPointer) -> Bool in
            let result = rustynes_load_rom(
                h,
                ptr.baseAddress?.assumingMemoryBound(to: UInt8.self),
                data.count
            )
            return result == 0
        }
    }

    func runFrame(inputP1: UInt8, inputP2: UInt8) {
        guard let h = handle else { return }
        rustynes_run_frame(h, inputP1, inputP2)
    }

    func getFramebuffer() -> [UInt32] {
        guard let h = handle else { return framebufferData }

        let ptr = rustynes_get_framebuffer(h)
        if let ptr = ptr {
            framebufferData.withUnsafeMutableBufferPointer { dest in
                dest.baseAddress?.initialize(from: ptr, count: 256 * 240)
            }
        }

        return framebufferData
    }

    func getAudioSamples() -> [Float] {
        guard let h = handle else { return [] }

        let count = audioBuffer.withUnsafeMutableBufferPointer { buffer -> Int in
            Int(rustynes_get_audio_samples(h, buffer.baseAddress, buffer.count))
        }

        return Array(audioBuffer.prefix(count))
    }

    func reset() {
        guard let h = handle else { return }
        rustynes_reset(h)
    }
}
```

### Build Configuration

```toml
# rust/Cargo.toml
[package]
name = "rustynes-ios"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["staticlib"]

[dependencies]
rustynes-core = { path = "../../crates/rustynes-core" }

[profile.release]
opt-level = 3
lto = true
```

### Build Script

```bash
#!/bin/bash
# build-ios.sh

set -e

# Install iOS targets
rustup target add aarch64-apple-ios
rustup target add aarch64-apple-ios-sim
rustup target add x86_64-apple-ios

# Build for device
cargo build --target aarch64-apple-ios --release -p rustynes-ios

# Build for simulator (Apple Silicon)
cargo build --target aarch64-apple-ios-sim --release -p rustynes-ios

# Build for simulator (Intel)
cargo build --target x86_64-apple-ios --release -p rustynes-ios

# Create XCFramework
xcodebuild -create-xcframework \
    -library target/aarch64-apple-ios/release/librustynes_ios.a -headers rust/include \
    -library target/aarch64-apple-ios-sim/release/librustynes_ios.a -headers rust/include \
    -output RustyNESCore.xcframework

echo "Build complete: RustyNESCore.xcframework"
```

## Mobile UI Considerations

### Touch Controls

```swift
// TouchControllerView.swift
import SwiftUI

struct TouchControllerView: View {
    @Binding var inputState: UInt8

    var body: some View {
        HStack {
            // D-Pad
            DPadView(inputState: $inputState)
                .frame(width: 120, height: 120)

            Spacer()

            // Action buttons
            VStack {
                HStack {
                    ButtonView(label: "A", bit: 0x01, inputState: $inputState)
                    ButtonView(label: "B", bit: 0x02, inputState: $inputState)
                }
                HStack {
                    ButtonView(label: "SELECT", bit: 0x04, inputState: $inputState)
                        .frame(width: 50, height: 25)
                    ButtonView(label: "START", bit: 0x08, inputState: $inputState)
                        .frame(width: 50, height: 25)
                }
            }
        }
        .padding()
    }
}
```

### Haptic Feedback

```swift
// HapticManager.swift
import UIKit

class HapticManager {
    private let impactLight = UIImpactFeedbackGenerator(style: .light)
    private let impactMedium = UIImpactFeedbackGenerator(style: .medium)

    func buttonPress() {
        impactLight.impactOccurred()
    }

    func collision() {
        impactMedium.impactOccurred()
    }
}
```

### Battery Optimization

```swift
// PowerManager.swift
import UIKit

class PowerManager {
    var thermalState: ProcessInfo.ThermalState {
        ProcessInfo.processInfo.thermalState
    }

    func adjustForThermalState() -> EmulationSettings {
        switch thermalState {
        case .nominal, .fair:
            return EmulationSettings(
                targetFPS: 60,
                enableFilters: true,
                audioQuality: .high
            )
        case .serious:
            return EmulationSettings(
                targetFPS: 60,
                enableFilters: false,
                audioQuality: .medium
            )
        case .critical:
            return EmulationSettings(
                targetFPS: 30,
                enableFilters: false,
                audioQuality: .low
            )
        @unknown default:
            return EmulationSettings(
                targetFPS: 60,
                enableFilters: true,
                audioQuality: .high
            )
        }
    }
}
```

## Performance Considerations

### Frame Timing

Mobile devices may have variable refresh rates. Use platform APIs for optimal frame pacing:

```swift
// iOS: CADisplayLink
class EmulatorLoop {
    private var displayLink: CADisplayLink?
    private var emulator: EmulatorWrapper

    func start() {
        displayLink = CADisplayLink(target: self, selector: #selector(runFrame))
        displayLink?.preferredFrameRateRange = CAFrameRateRange(
            minimum: 60, maximum: 60, preferred: 60
        )
        displayLink?.add(to: .main, forMode: .common)
    }

    @objc func runFrame() {
        emulator.runFrame(inputP1: currentInput, inputP2: 0)
        updateDisplay()
    }
}
```

```kotlin
// Android: Choreographer
class EmulatorLoop(private val emulator: NativeEmulator) {
    private val choreographer = Choreographer.getInstance()
    private val frameCallback = object : Choreographer.FrameCallback {
        override fun doFrame(frameTimeNanos: Long) {
            emulator.runFrame(currentInput, 0, framebuffer)
            updateDisplay()
            choreographer.postFrameCallback(this)
        }
    }

    fun start() {
        choreographer.postFrameCallback(frameCallback)
    }

    fun stop() {
        choreographer.removeFrameCallback(frameCallback)
    }
}
```

### Memory Management

- Limit save state size for mobile storage constraints
- Use memory-mapped I/O for ROM loading where possible
- Implement proper cleanup when app backgrounds

### Audio Latency

Mobile audio APIs typically have higher latency than desktop:

| Platform | Typical Latency |
|----------|-----------------|
| iOS (AudioUnit) | 5-15ms |
| Android (Oboe) | 10-50ms |
| Android (AudioTrack) | 50-200ms |

## Platform Status

| Platform | Status | Notes |
|----------|--------|-------|
| Android | Planned | JNI bindings designed |
| iOS | Planned | C FFI designed |
| iPadOS | Planned | Same as iOS |
| Android TV | Future | Gamepad-focused UI |
| tvOS | Future | Apple TV support |

## References

- [Mozilla/rust-android-gradle](https://github.com/aspect-dev/rules_rust_android)
- [cargo-ndk](https://github.com/nickelc/cargo-ndk)
- [Rust on iOS](https://mozilla.github.io/firefox-browser-architecture/experiments/2017-09-21-rust-on-ios.html)
- [Android NDK](https://developer.android.com/ndk)
- [Oboe Audio Library](https://github.com/google/oboe)
