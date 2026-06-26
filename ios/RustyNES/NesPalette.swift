//
//  NesPalette.swift
//
//  Converts the NES palette-index framebuffer to RGBA, for the netplay present path
//  (v1.9.6). During a netplay session the frame loop drives the core via
//  `npAdvanceFrame` (rollback owns pacing), which advances the emulator but does NOT
//  return a framebuffer; calling `runFrame` a second time to fetch RGBA would advance
//  the core again and desync the rollback. So we read the just-produced frame through
//  the NON-advancing `indexFramebufferBytes()` path (the same approach the Android
//  netplay path uses with its composite LUT) and expand the palette indices to RGBA
//  here, on the host, with the active palette.
//
//  The index plane is `256*240` little-endian `u16`s, each `(emphasis << 6) | colour`.
//  We map `colour = value & 0x3F` through a 64-entry RGB table. The emphasis bits are
//  ignored for this first netplay cut (the colour-dimming emphasis variants are an
//  on-device-validation carryover); the non-netplay path is unaffected (it presents
//  the core's own RGBA framebuffer).
//
//  Palette selection: when a custom `.pal` is active (the same bytes fed to the core
//  via `loadPalette`), its first 64 RGB triples are used so the netplay picture
//  matches the single-player look; otherwise the built-in 2C02 master palette below.
//

import Foundation

enum NesPalette {
    /// The visible NES framebuffer dimensions.
    static let width = 256
    static let height = 240
    static let pixelCount = width * height

    /// The canonical 64-colour 2C02 master palette (the widely-used Nestopia/FCEUX
    /// RGB set), as packed `R,G,B` triples (192 bytes). Used when no custom `.pal`
    /// is active.
    static let masterRGB: [UInt8] = [
        0x66, 0x66, 0x66, 0x00, 0x2A, 0x88, 0x14, 0x12, 0xA7, 0x3B, 0x00, 0xA4,
        0x5C, 0x00, 0x7E, 0x6E, 0x00, 0x40, 0x6C, 0x06, 0x00, 0x56, 0x1D, 0x00,
        0x33, 0x35, 0x00, 0x0B, 0x48, 0x00, 0x00, 0x52, 0x00, 0x00, 0x4F, 0x08,
        0x00, 0x40, 0x4D, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0xAD, 0xAD, 0xAD, 0x15, 0x5F, 0xD9, 0x42, 0x40, 0xFF, 0x75, 0x27, 0xFE,
        0xA0, 0x1A, 0xCC, 0xB7, 0x1E, 0x7B, 0xB5, 0x31, 0x20, 0x99, 0x4E, 0x00,
        0x6B, 0x6D, 0x00, 0x38, 0x87, 0x00, 0x0C, 0x93, 0x00, 0x00, 0x8F, 0x32,
        0x00, 0x7C, 0x8D, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0xFF, 0xFE, 0xFF, 0x64, 0xB0, 0xFF, 0x92, 0x90, 0xFF, 0xC6, 0x76, 0xFF,
        0xF3, 0x6A, 0xFF, 0xFE, 0x6E, 0xCC, 0xFE, 0x81, 0x70, 0xEA, 0x9E, 0x22,
        0xBC, 0xBE, 0x00, 0x88, 0xD8, 0x00, 0x5C, 0xE4, 0x30, 0x45, 0xE0, 0x82,
        0x48, 0xCD, 0xDE, 0x4F, 0x4F, 0x4F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0xFF, 0xFE, 0xFF, 0xC0, 0xDF, 0xFF, 0xD3, 0xD2, 0xFF, 0xE8, 0xC8, 0xFF,
        0xFB, 0xC2, 0xFF, 0xFE, 0xC4, 0xEA, 0xFE, 0xCC, 0xC5, 0xF7, 0xD8, 0xA5,
        0xE4, 0xE5, 0x94, 0xCF, 0xEF, 0x96, 0xBD, 0xF4, 0xAB, 0xB3, 0xF3, 0xCC,
        0xB5, 0xEB, 0xF2, 0xB8, 0xB8, 0xB8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]

    /// Expand a `256*240` little-endian `u16` palette-index plane into a `256*240*4`
    /// RGBA8888 buffer, reusing `out` (sized to `pixelCount*4`) to avoid a per-frame
    /// allocation. `customPaletteRGB`, when non-nil and at least 192 bytes, supplies
    /// the 64 RGB triples (matching the active `.pal`); otherwise `masterRGB` is used.
    /// Returns `true` if the index plane was the expected size and `out` was filled.
    @discardableResult
    static func expand(index: Data, customPaletteRGB: Data?, into out: inout [UInt8]) -> Bool {
        guard index.count == pixelCount * 2 else { return false }
        if out.count != pixelCount * 4 {
            out = [UInt8](repeating: 0, count: pixelCount * 4)
        }
        // Pick the palette source: a valid custom .pal, else the built-in master.
        let usingCustom = (customPaletteRGB?.count ?? 0) >= 192
        index.withUnsafeBytes { (src: UnsafeRawBufferPointer) in
            let idx = src.bindMemory(to: UInt8.self)
            out.withUnsafeMutableBufferPointer { dst in
                func writePixels(_ pal: UnsafePointer<UInt8>) {
                    var p = 0
                    while p < pixelCount {
                        // Little-endian u16: low byte holds the colour; the high byte
                        // (emphasis) is masked off (emphasis dimming is a carryover).
                        let colour = Int(idx[p * 2]) & 0x3F
                        let s = colour * 3
                        let d = p * 4
                        dst[d] = pal[s]
                        dst[d + 1] = pal[s + 1]
                        dst[d + 2] = pal[s + 2]
                        dst[d + 3] = 0xFF
                        p += 1
                    }
                }
                if usingCustom, let custom = customPaletteRGB {
                    custom.withUnsafeBytes { (pal: UnsafeRawBufferPointer) in
                        writePixels(pal.bindMemory(to: UInt8.self).baseAddress!)
                    }
                } else {
                    masterRGB.withUnsafeBufferPointer { writePixels($0.baseAddress!) }
                }
            }
        }
        return true
    }
}
