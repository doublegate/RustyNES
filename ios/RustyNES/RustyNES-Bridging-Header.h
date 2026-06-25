//
//  RustyNES-Bridging-Header.h
//
//  Exposes the hand-written RustyNES iOS C glue (the Metal renderer + CoreAudio
//  sink FFI) to Swift. The typed emulator control surface (load ROM, set input,
//  run frame, save state) is the UniFFI-generated `NesController` in
//  Generated/RustyNESCore.swift — drive the core through that, NOT through C.
//
//  This header is packaged into RustyNESFFI.xcframework by
//  scripts/build-ios-xcframework.sh; importing it here is what lets Swift call
//  `rustynes_ios_gfx_*` / `rustynes_ios_audio_*`.
//

#import "rustynes_ios.h"
