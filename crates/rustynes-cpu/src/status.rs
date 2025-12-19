//! CPU Status Register (P) implementation.
//!
//! The 6502 status register contains 8 flags that track the processor state.
//! Bit layout: NV-BDIZC (where - is the unused bit, always set to 1)

use bitflags::bitflags;

bitflags! {
    /// CPU Status Register Flags (P register)
    ///
    /// The status register contains flags that reflect the state of the CPU
    /// and control certain behaviors like interrupt handling.
    ///
    /// # Flag Bits (NV-BDIZC)
    ///
    /// - **N (Negative)**: Set if result is negative (bit 7 = 1)
    /// - **V (Overflow)**: Set if signed overflow occurred
    /// - **U (Unused)**: Always 1 when pushed to stack
    /// - **B (Break)**: Distinguishes BRK from IRQ (stack only)
    /// - **D (Decimal)**: Decimal mode flag (ignored on NES)
    /// - **I (Interrupt Disable)**: When set, IRQ interrupts are masked
    /// - **Z (Zero)**: Set if result is zero
    /// - **C (Carry)**: Set if carry/borrow occurred
    ///
    /// # Example
    ///
    /// ```
    /// use rustynes_cpu::StatusFlags;
    ///
    /// let mut flags = StatusFlags::default();
    /// flags.insert(StatusFlags::CARRY);
    /// assert!(flags.contains(StatusFlags::CARRY));
    ///
    /// // Set N and Z based on a value
    /// let value = 0x00;
    /// flags.set_zn(value);
    /// assert!(flags.contains(StatusFlags::ZERO));
    /// assert!(!flags.contains(StatusFlags::NEGATIVE));
    /// ```
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct StatusFlags: u8 {
        /// Carry flag (bit 0)
        const CARRY             = 0b0000_0001;
        /// Zero flag (bit 1)
        const ZERO              = 0b0000_0010;
        /// Interrupt Disable flag (bit 2)
        const INTERRUPT_DISABLE = 0b0000_0100;
        /// Decimal Mode flag (bit 3) - ignored on NES but still functional
        const DECIMAL           = 0b0000_1000;
        /// Break flag (bit 4) - only set when pushed to stack from PHP/BRK
        const BREAK             = 0b0001_0000;
        /// Unused flag (bit 5) - always 1 when pushed to stack
        const UNUSED            = 0b0010_0000;
        /// Overflow flag (bit 6)
        const OVERFLOW          = 0b0100_0000;
        /// Negative flag (bit 7)
        const NEGATIVE          = 0b1000_0000;
    }
}

impl Default for StatusFlags {
    /// Creates status register with default power-on state
    ///
    /// Default state: Interrupt Disable = 1, Unused = 1
    fn default() -> Self {
        Self::INTERRUPT_DISABLE | Self::UNUSED
    }
}

impl StatusFlags {
    /// Update Zero and Negative flags based on a value
    ///
    /// # Arguments
    ///
    /// * `value` - The 8-bit value to test
    ///
    /// # Flag Behavior
    ///
    /// - Z is set if `value == 0`
    /// - N is set if `value & 0x80 != 0` (bit 7 set)
    ///
    /// # Example
    ///
    /// ```
    /// use rustynes_cpu::StatusFlags;
    ///
    /// let mut flags = StatusFlags::default();
    ///
    /// flags.set_zn(0x00);
    /// assert!(flags.contains(StatusFlags::ZERO));
    /// assert!(!flags.contains(StatusFlags::NEGATIVE));
    ///
    /// flags.set_zn(0x80);
    /// assert!(!flags.contains(StatusFlags::ZERO));
    /// assert!(flags.contains(StatusFlags::NEGATIVE));
    /// ```
    #[inline]
    pub fn set_zn(&mut self, value: u8) {
        self.set(Self::ZERO, value == 0);
        self.set(Self::NEGATIVE, value & 0x80 != 0);
    }

    /// Convert status to byte for pushing to stack
    ///
    /// When status is pushed to stack (PHP, BRK, interrupts), the U bit
    /// is always set and the B bit depends on the source.
    ///
    /// # Arguments
    ///
    /// * `brk` - If true, set B flag (from BRK/PHP). If false, clear B flag (from IRQ/NMI)
    ///
    /// # Returns
    ///
    /// 8-bit value with U=1 and B set according to `brk`
    ///
    /// # Example
    ///
    /// ```
    /// use rustynes_cpu::StatusFlags;
    ///
    /// let flags = StatusFlags::CARRY | StatusFlags::ZERO;
    ///
    /// // From BRK/PHP: B=1, U=1
    /// let stack_byte_brk = flags.to_stack_byte(true);
    /// assert_eq!(stack_byte_brk & 0b0011_0000, 0b0011_0000);
    ///
    /// // From IRQ/NMI: B=0, U=1
    /// let stack_byte_int = flags.to_stack_byte(false);
    /// assert_eq!(stack_byte_int & 0b0011_0000, 0b0010_0000);
    /// ```
    #[inline]
    #[must_use]
    pub fn to_stack_byte(&self, brk: bool) -> u8 {
        let mut byte = self.bits();
        byte |= Self::UNUSED.bits();
        if brk {
            byte |= Self::BREAK.bits();
        } else {
            byte &= !Self::BREAK.bits();
        }
        byte
    }

    /// Convert byte from stack to status flags
    ///
    /// When pulling status from stack (PLP, RTI), the B flag is ignored
    /// and U is always set.
    ///
    /// # Arguments
    ///
    /// * `byte` - The 8-bit value pulled from stack
    ///
    /// # Returns
    ///
    /// StatusFlags with U=1 and other flags set from `byte`
    ///
    /// # Example
    ///
    /// ```
    /// use rustynes_cpu::StatusFlags;
    ///
    /// // Stack byte with B=1 (from BRK)
    /// let stack_byte = 0b0011_0011; // B=1, U=1, C=1, Z=1
    /// let flags = StatusFlags::from_stack_byte(stack_byte);
    ///
    /// // B flag is ignored when pulling from stack
    /// assert!(!flags.contains(StatusFlags::BREAK));
    /// // U is always set
    /// assert!(flags.contains(StatusFlags::UNUSED));
    /// // Other flags preserved
    /// assert!(flags.contains(StatusFlags::CARRY));
    /// assert!(flags.contains(StatusFlags::ZERO));
    /// ```
    #[inline]
    #[must_use]
    pub fn from_stack_byte(byte: u8) -> Self {
        (Self::from_bits_truncate(byte) & !Self::BREAK) | Self::UNUSED
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_status() {
        let status = StatusFlags::default();
        assert!(status.contains(StatusFlags::INTERRUPT_DISABLE));
        assert!(status.contains(StatusFlags::UNUSED));
        assert!(!status.contains(StatusFlags::CARRY));
        assert!(!status.contains(StatusFlags::ZERO));
    }

    #[test]
    fn test_set_zn_zero() {
        let mut status = StatusFlags::default();
        status.set_zn(0x00);
        assert!(status.contains(StatusFlags::ZERO));
        assert!(!status.contains(StatusFlags::NEGATIVE));
    }

    #[test]
    fn test_set_zn_negative() {
        let mut status = StatusFlags::default();
        status.set_zn(0x80);
        assert!(!status.contains(StatusFlags::ZERO));
        assert!(status.contains(StatusFlags::NEGATIVE));
    }

    #[test]
    fn test_set_zn_positive_nonzero() {
        let mut status = StatusFlags::default();
        status.set_zn(0x42);
        assert!(!status.contains(StatusFlags::ZERO));
        assert!(!status.contains(StatusFlags::NEGATIVE));
    }

    #[test]
    fn test_to_stack_byte_brk() {
        let status = StatusFlags::CARRY | StatusFlags::ZERO;
        let byte = status.to_stack_byte(true);

        // B and U should be set
        assert_eq!(byte & 0b0011_0000, 0b0011_0000);
        // C and Z should be preserved
        assert_eq!(byte & 0b0000_0011, 0b0000_0011);
    }

    #[test]
    fn test_to_stack_byte_interrupt() {
        let status = StatusFlags::CARRY | StatusFlags::ZERO;
        let byte = status.to_stack_byte(false);

        // Only U should be set, not B
        assert_eq!(byte & 0b0011_0000, 0b0010_0000);
        // C and Z should be preserved
        assert_eq!(byte & 0b0000_0011, 0b0000_0011);
    }

    #[test]
    fn test_from_stack_byte() {
        // Byte with B=1, U=1, C=1, Z=1
        let byte = 0b0011_0011;
        let status = StatusFlags::from_stack_byte(byte);

        // B should be ignored (cleared)
        assert!(!status.contains(StatusFlags::BREAK));
        // U should always be set
        assert!(status.contains(StatusFlags::UNUSED));
        // Other flags preserved
        assert!(status.contains(StatusFlags::CARRY));
        assert!(status.contains(StatusFlags::ZERO));
    }

    #[test]
    fn test_all_flags() {
        let all = StatusFlags::CARRY
            | StatusFlags::ZERO
            | StatusFlags::INTERRUPT_DISABLE
            | StatusFlags::DECIMAL
            | StatusFlags::BREAK
            | StatusFlags::UNUSED
            | StatusFlags::OVERFLOW
            | StatusFlags::NEGATIVE;

        assert_eq!(all.bits(), 0xFF);
    }
}
