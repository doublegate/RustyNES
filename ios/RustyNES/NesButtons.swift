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
enum NesButtonBit: UInt8, CaseIterable {
    case a = 0x01
    case b = 0x02
    case select = 0x04
    case start = 0x08
    case up = 0x10
    case down = 0x20
    case left = 0x40
    case right = 0x80
}

/// A live 8-bit controller mask (a set of pressed `NesButtonBit`s) for one port.
struct NesButtonMask {
    private(set) var bits: UInt8 = 0

    init(bits: UInt8 = 0) { self.bits = bits }

    /// Press or release a single button, preserving the others.
    mutating func set(_ button: NesButtonBit, pressed: Bool) {
        if pressed {
            bits |= button.rawValue
        } else {
            bits &= ~button.rawValue
        }
    }

    /// Whether a button is currently held.
    func contains(_ button: NesButtonBit) -> Bool {
        bits & button.rawValue != 0
    }

    /// Merge another mask in (OR of both sets of held buttons).
    mutating func formUnion(_ other: NesButtonMask) {
        bits |= other.bits
    }

    /// Clear all buttons.
    mutating func clear() { bits = 0 }

    /// v2.9.2 (audit AUD-10) -- neutral SOCD (simultaneous opposing cardinal
    /// directions) cleaning: Up + Down together become neither, and so do
    /// Left + Right, each axis on its own; every other bit passes through. The
    /// same rule as the desktop's `input::socd_neutral` and Android's
    /// `socdNeutral`. A real NES pad's rocking cross cannot report opposites; two
    /// fingers on the on-screen D-pad can. UNCOMPILED at v2.9.2 (no Swift
    /// toolchain on the Linux build host) -- see the device checklist.
    ///
    /// v2.9.7 "Tandem" (plan item 8): `enabled` is the user's "Cancel opposite
    /// directions" setting (`AppModel.cancelOpposites`), default on -- the
    /// desktop's `[input] allow_opposing_directions = false`. Off leaves the mask
    /// untouched. UNCOMPILED at v2.9.7 as well; see the run sheet.
    mutating func cancelOpposingDirections(enabled: Bool = true) {
        guard enabled else { return }
        let vertical = NesButtonBit.up.rawValue | NesButtonBit.down.rawValue
        let horizontal = NesButtonBit.left.rawValue | NesButtonBit.right.rawValue
        if bits & vertical == vertical { bits &= ~vertical }
        if bits & horizontal == horizontal { bits &= ~horizontal }
    }
}
