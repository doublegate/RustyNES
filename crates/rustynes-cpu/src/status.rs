//! Status register flags for the 6502.
//!
//! Per `docs/cpu-6502.md` §State, the bits are: N V _ B D I Z C from MSB to
//! LSB. Bit 5 (`U`) is unused on real hardware but always reads as 1; we
//! set it on power-on to match. The B flag exists only on stack pushes
//! (PHP / BRK push it set; IRQ/NMI sequences push it clear).

use bitflags::bitflags;

bitflags! {
    /// Processor status flags. Bits match the layout pushed onto the stack.
    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    pub struct Status: u8 {
        /// Carry.
        const CARRY = 0b0000_0001;
        /// Zero.
        const ZERO = 0b0000_0010;
        /// IRQ disable.
        const INTERRUPT_DISABLE = 0b0000_0100;
        /// Decimal mode (settable on 2A03 but ignored arithmetically).
        const DECIMAL = 0b0000_1000;
        /// Break flag (only meaningful on stack pushes).
        const BREAK = 0b0001_0000;
        /// Unused bit (always reads 1).
        const UNUSED = 0b0010_0000;
        /// Overflow.
        const OVERFLOW = 0b0100_0000;
        /// Negative.
        const NEGATIVE = 0b1000_0000;
    }
}

impl Status {
    /// Power-on state: I and U set, others clear (bit pattern `$24`).
    #[must_use]
    pub const fn power_on() -> Self {
        Self::from_bits_truncate(0x24)
    }

    /// Set N and Z based on `value`. Used by every load/transfer/arith op
    /// that produces a result observable by the program.
    pub fn set_nz(&mut self, value: u8) {
        self.set(Self::ZERO, value == 0);
        self.set(Self::NEGATIVE, value & 0x80 != 0);
    }
}
