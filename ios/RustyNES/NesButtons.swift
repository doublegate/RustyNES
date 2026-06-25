//
//  NesButtons.swift
//
//  The standard NES controller button bitmask, shared by the on-screen touch
//  overlay and the hardware-gamepad mapper. Both feed `NesController.setButtons(
//  port:mask:)`, so the bit order MUST match the core exactly.
//
//  Source of truth: crates/rustynes-core/src/controller.rs — the `Buttons` bitflag
//  is ordered LSB-first to match the 4016/4017 shift-out order
//  (A, B, Select, Start, Up, Down, Left, Right):
//
//      A      = 1 << 0   (0x01)
//      B      = 1 << 1   (0x02)
//      Select = 1 << 2   (0x04)
//      Start  = 1 << 3   (0x08)
//      Up     = 1 << 4   (0x10)
//      Down   = 1 << 5   (0x20)
//      Left   = 1 << 6   (0x40)
//      Right  = 1 << 7   (0x80)
//
//  The Rust FFI `set_buttons(port, mask)` calls `Buttons::from_bits_truncate(mask)`,
//  so these constants are the wire format the core expects.
//

import Foundation

/// One standard NES controller button as its single-bit mask value.
enum NesButton: UInt8, CaseIterable {
    case a = 0x01
    case b = 0x02
    case select = 0x04
    case start = 0x08
    case up = 0x10
    case down = 0x20
    case left = 0x40
    case right = 0x80
}

/// A live 8-bit controller mask (a set of pressed `NesButton`s) for one port.
struct NesButtonMask {
    private(set) var bits: UInt8 = 0

    init(bits: UInt8 = 0) { self.bits = bits }

    /// Press or release a single button, preserving the others.
    mutating func set(_ button: NesButton, pressed: Bool) {
        if pressed {
            bits |= button.rawValue
        } else {
            bits &= ~button.rawValue
        }
    }

    /// Whether a button is currently held.
    func contains(_ button: NesButton) -> Bool {
        bits & button.rawValue != 0
    }

    /// Merge another mask in (OR of both sets of held buttons).
    mutating func formUnion(_ other: NesButtonMask) {
        bits |= other.bits
    }

    /// Clear all buttons.
    mutating func clear() { bits = 0 }
}
