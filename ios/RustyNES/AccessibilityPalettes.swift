//
//  AccessibilityPalettes.swift  (v1.9.8 "Horizon")
//
//  Built-in accessibility NES palettes, the iOS counterpart of the desktop's
//  high-contrast + Okabe-Ito colourblind palettes. They feed the SAME presentation-
//  only `NesController.loadPalette` path as an imported `.pal` (v1.9.5): each is a
//  64-entry RGB table (192 bytes, the minimum `loadPalette` accepts), so selecting
//  one is byte-identical to clearing back to the built-in palette once deselected.
//
//  Palette ids are namespaced ("builtin.*") so they never collide with an imported
//  `.pal` file stem; `AppModel.applyDisplaySettings` resolves a "builtin.*" id here
//  before falling through to the imported-`.pal` store. The RGB tables are derived
//  deterministically (no checked-in 192-byte literal to mistype):
//
//  - High contrast expands the canonical 2C02 master palette's per-channel contrast
//    around mid-grey, so dark colours go darker and bright colours brighter — easier
//    to tell apart for low-vision players.
//  - Colourblind (Okabe-Ito) recolours the NES hue columns onto the eight Okabe-Ito
//    colourblind-safe colours (scaled per luminance row), so deutan/protan/tritan
//    players can still distinguish the palette's hues.
//

import Foundation

/// One built-in accessibility palette. The raw value is the stable, namespaced id
/// persisted in `globalPaletteId` / a per-game override, exactly like an imported
/// `.pal` stem.
enum AccessibilityPalette: String, CaseIterable, Identifiable {
    case highContrast = "builtin.high-contrast"
    case okabeIto = "builtin.okabe-ito"

    var id: String { rawValue }

    /// The display name (a `Localizable.xcstrings` key — see `PalettePickerSection`,
    /// which renders it through `LocalizedStringKey`).
    var name: String {
        switch self {
        case .highContrast: return "High contrast"
        case .okabeIto: return "Colourblind (Okabe-Ito)"
        }
    }

    /// The 64-entry RGB table (192 bytes) fed to `loadPalette`.
    var rgb: Data {
        switch self {
        case .highContrast: return Data(Self.highContrast())
        case .okabeIto: return Data(Self.okabeIto())
        }
    }

    // MARK: - Table generation

    /// High-contrast: expand each channel of the master palette around mid-grey
    /// (128) by 1.5x and clamp. Presentation-only; same 64-entry layout.
    private static func highContrast() -> [UInt8] {
        NesPalette.masterRGB.map { channel in
            let expanded = (Double(channel) - 128.0) * 1.5 + 128.0
            return UInt8(clamping: Int(expanded.rounded()))
        }
    }

    /// Okabe-Ito colourblind-safe recolour. The NES palette is laid out as four
    /// luminance rows of sixteen columns; column 0 (and 0xD) are greys and columns
    /// 0xE/0xF are black, with columns 0x1...0xC the hues. We map each hue column
    /// onto one of the eight Okabe-Ito colours, scaled by the row's brightness (the
    /// palest row mixes toward white), and keep the grey/black columns neutral.
    private static func okabeIto() -> [UInt8] {
        // The eight Okabe-Ito colourblind-safe colours (orange, sky blue, bluish
        // green, yellow, blue, vermillion, reddish purple, plus a neutral grey).
        let hues: [(Double, Double, Double)] = [
            (230, 159, 0), (86, 180, 233), (0, 158, 115), (240, 228, 66),
            (0, 114, 178), (213, 94, 0), (204, 121, 167), (120, 120, 120),
        ]
        // Per-row luminance scale + a white-mix for the palest (top) row.
        let rowLevel: [Double] = [0.45, 0.70, 1.0, 1.0]
        let rowWhiteMix: [Double] = [0.0, 0.0, 0.0, 0.55]
        let rowGrey: [Double] = [70, 130, 190, 235]

        var out = [UInt8]()
        out.reserveCapacity(192)
        for row in 0..<4 {
            for col in 0..<16 {
                var r = 0.0, g = 0.0, b = 0.0
                if col == 0 || col == 0xD {
                    let grey = rowGrey[row]
                    r = grey; g = grey; b = grey
                } else if col >= 0xE {
                    r = 0; g = 0; b = 0 // the canonical "blacker than black" columns
                } else {
                    let h = hues[(col - 1) % hues.count]
                    let level = rowLevel[row]
                    let mix = rowWhiteMix[row]
                    r = h.0 * level; g = h.1 * level; b = h.2 * level
                    r += (255 - r) * mix; g += (255 - g) * mix; b += (255 - b) * mix
                }
                out.append(UInt8(clamping: Int(r.rounded())))
                out.append(UInt8(clamping: Int(g.rounded())))
                out.append(UInt8(clamping: Int(b.rounded())))
            }
        }
        return out
    }
}

/// Resolves the built-in accessibility palettes by id (the lookup `AppModel` consults
/// before the imported-`.pal` store).
enum AccessibilityPalettes {
    static var all: [AccessibilityPalette] { AccessibilityPalette.allCases }

    /// The RGB bytes for a built-in accessibility palette id, or nil if `id` is not
    /// one of ours (so the caller falls through to the imported-`.pal` store).
    static func bytes(id: String) -> Data? { AccessibilityPalette(rawValue: id)?.rgb }

    /// Whether `id` names a built-in accessibility palette.
    static func exists(id: String) -> Bool { AccessibilityPalette(rawValue: id) != nil }
}
