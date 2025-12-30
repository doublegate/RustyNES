//! CPU Status Register (P register) flags.
//!
//! The 6502 status register is an 8-bit register that contains various flags
//! reflecting the state of the processor:
//!
//! ```text
//! 7  6  5  4  3  2  1  0
//! N  V  U  B  D  I  Z  C
//! │  │  │  │  │  │  │  └─ Carry
//! │  │  │  │  │  │  └──── Zero
//! │  │  │  │  │  └─────── Interrupt Disable
//! │  │  │  │  └────────── Decimal Mode (not used in NES but still functional)
//! │  │  │  └───────────── Break (1 when pushed from PHP/BRK, 0 from IRQ/NMI)
//! │  │  └──────────────── Unused (always 1 when pushed to stack)
//! │  └─────────────────── Overflow
//! └────────────────────── Negative
//! ```

use bitflags::bitflags;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

bitflags! {
    /// CPU Status Register flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    #[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
    pub struct Status: u8 {
        /// Carry flag - Set if the last operation caused an overflow from bit 7
        /// or an underflow from bit 0.
        const C = 1 << 0;

        /// Zero flag - Set if the result of the last operation was zero.
        const Z = 1 << 1;

        /// Interrupt Disable flag - When set, IRQ interrupts are disabled.
        /// NMI interrupts are not affected.
        const I = 1 << 2;

        /// Decimal Mode flag - When set, arithmetic operations use BCD.
        /// Note: The NES CPU lacks BCD support, but this flag still functions.
        const D = 1 << 3;

        /// Break flag - Distinguishes hardware interrupts from BRK instructions.
        /// Set to 1 when pushed by PHP or BRK, 0 when pushed by IRQ or NMI.
        const B = 1 << 4;

        /// Unused flag - Always set to 1 when status is pushed to the stack.
        const U = 1 << 5;

        /// Overflow flag - Set if the last operation caused a signed overflow.
        const V = 1 << 6;

        /// Negative flag - Set if bit 7 of the result is set.
        const N = 1 << 7;
    }
}

impl Status {
    /// Initial status after power-on.
    /// I flag is set, U flag is always 1.
    pub const POWER_ON: Self = Self::I.union(Self::U);

    /// Mask for flags that can be set by PLP instruction.
    /// The B and U flags are not affected by PLP.
    pub const PLP_MASK: Self = Self::C
        .union(Self::Z)
        .union(Self::I)
        .union(Self::D)
        .union(Self::V)
        .union(Self::N);

    /// Creates a new Status register with default flags (I and U set).
    #[must_use]
    pub const fn new() -> Self {
        Self::POWER_ON
    }

    /// Sets or clears the Zero and Negative flags based on a value.
    #[inline]
    pub fn set_zn(&mut self, value: u8) {
        self.set_flag(Self::Z, value == 0);
        self.set_flag(Self::N, value & 0x80 != 0);
    }

    /// Sets or clears a flag.
    #[inline]
    pub fn set_flag(&mut self, flag: Self, value: bool) {
        if value {
            *self |= flag;
        } else {
            *self &= !flag;
        }
    }

    /// Converts the status register to a byte for pushing to stack.
    /// The U flag is always set when pushing.
    #[inline]
    #[must_use]
    pub const fn to_stack_byte(self, brk: bool) -> u8 {
        let mut value = self.bits() | Self::U.bits();
        if brk {
            value |= Self::B.bits();
        }
        value
    }

    /// Creates a status register from a byte pulled from the stack.
    /// The B flag is ignored and U is always set.
    #[inline]
    #[must_use]
    pub fn from_stack_byte(value: u8) -> Self {
        // Clear B, set U
        Self::from_bits_truncate((value & !Self::B.bits()) | Self::U.bits())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_power_on_status() {
        let status = Status::new();
        assert!(status.contains(Status::I));
        assert!(status.contains(Status::U));
        assert!(!status.contains(Status::C));
        assert!(!status.contains(Status::Z));
        assert!(!status.contains(Status::N));
        assert!(!status.contains(Status::V));
    }

    #[test]
    fn test_set_zn_zero() {
        let mut status = Status::empty();
        status.set_zn(0);
        assert!(status.contains(Status::Z));
        assert!(!status.contains(Status::N));
    }

    #[test]
    fn test_set_zn_negative() {
        let mut status = Status::empty();
        status.set_zn(0x80);
        assert!(!status.contains(Status::Z));
        assert!(status.contains(Status::N));
    }

    #[test]
    fn test_set_zn_positive() {
        let mut status = Status::empty();
        status.set_zn(0x42);
        assert!(!status.contains(Status::Z));
        assert!(!status.contains(Status::N));
    }

    #[test]
    fn test_to_stack_byte_with_brk() {
        let status = Status::C | Status::Z;
        let byte = status.to_stack_byte(true);
        assert_eq!(byte & Status::B.bits(), Status::B.bits());
        assert_eq!(byte & Status::U.bits(), Status::U.bits());
    }

    #[test]
    fn test_to_stack_byte_without_brk() {
        let status = Status::C | Status::Z;
        let byte = status.to_stack_byte(false);
        assert_eq!(byte & Status::B.bits(), 0);
        assert_eq!(byte & Status::U.bits(), Status::U.bits());
    }

    #[test]
    fn test_from_stack_byte() {
        // B flag should be cleared, U should be set
        let status = Status::from_stack_byte(0xFF);
        assert!(!status.contains(Status::B));
        assert!(status.contains(Status::U));
        assert!(status.contains(Status::C));
        assert!(status.contains(Status::Z));
        assert!(status.contains(Status::I));
        assert!(status.contains(Status::D));
        assert!(status.contains(Status::V));
        assert!(status.contains(Status::N));
    }
}
