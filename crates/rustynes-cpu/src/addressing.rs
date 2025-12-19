//! CPU Addressing Modes
//!
//! The 6502 has 13 addressing modes that determine how instructions access memory.
//! Each mode has specific cycle timings and page-crossing behavior.

use crate::bus::Bus;

/// CPU Addressing Modes
///
/// Defines how an instruction's operand is accessed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AddressingMode {
    /// No operand (instruction operates on CPU state)
    ///
    /// Example: `NOP`, `CLC`, `DEX`
    Implied,

    /// Operand is the accumulator register
    ///
    /// Example: `ASL A`, `ROL A`
    Accumulator,

    /// Operand is the next byte after the opcode
    ///
    /// Example: `LDA #$42` loads the value $42
    Immediate,

    /// Operand is in zero page ($00-$FF)
    ///
    /// Example: `LDA $80` reads from address $0080
    ZeroPage,

    /// Zero page address + X register (wraps within zero page)
    ///
    /// Example: `LDA $80,X` with X=$05 reads from $0085
    ZeroPageX,

    /// Zero page address + Y register (wraps within zero page)
    ///
    /// Example: `LDX $80,Y` with Y=$05 reads from $0085
    ZeroPageY,

    /// Operand is a 16-bit absolute address
    ///
    /// Example: `LDA $1234` reads from address $1234
    Absolute,

    /// Absolute address + X register
    ///
    /// Example: `LDA $1234,X` with X=$10 reads from $1244
    AbsoluteX,

    /// Absolute address + Y register
    ///
    /// Example: `LDA $1234,Y` with Y=$10 reads from $1244
    AbsoluteY,

    /// Indirect addressing (JMP only)
    ///
    /// Example: `JMP ($1234)` reads target address from $1234-$1235
    Indirect,

    /// Indexed Indirect: (Zero Page + X), then dereference
    ///
    /// Example: `LDA ($80,X)` with X=$05:
    /// 1. Calculate pointer address: $80 + $05 = $85
    /// 2. Read pointer: [$85] = $20, [$86] = $30
    /// 3. Read from $3020
    IndexedIndirectX,

    /// Indirect Indexed: Dereference Zero Page, then + Y
    ///
    /// Example: `LDA ($80),Y` with Y=$10:
    /// 1. Read pointer: [$80] = $20, [$81] = $30
    /// 2. Add Y: $3020 + $10 = $3030
    /// 3. Read from $3030
    IndirectIndexedY,

    /// Relative offset for branch instructions
    ///
    /// Example: `BNE $02` branches PC + 2 bytes forward
    Relative,
}

impl AddressingMode {
    /// Get the number of operand bytes for this addressing mode
    ///
    /// # Returns
    ///
    /// - 0 bytes: Implied, Accumulator
    /// - 1 byte: Immediate, Zero Page variants, Indexed Indirect, Indirect Indexed, Relative
    /// - 2 bytes: Absolute variants, Indirect
    #[inline]
    #[must_use]
    pub const fn operand_bytes(self) -> u8 {
        match self {
            Self::Implied | Self::Accumulator => 0,
            Self::Immediate
            | Self::ZeroPage
            | Self::ZeroPageX
            | Self::ZeroPageY
            | Self::IndexedIndirectX
            | Self::IndirectIndexedY
            | Self::Relative => 1,
            Self::Absolute | Self::AbsoluteX | Self::AbsoluteY | Self::Indirect => 2,
        }
    }

    /// Check if this addressing mode can have a page crossing penalty
    ///
    /// # Returns
    ///
    /// `true` for:
    /// - Absolute,X
    /// - Absolute,Y
    /// - (Indirect),Y
    /// - Relative (branches)
    #[inline]
    #[must_use]
    pub const fn can_page_cross(self) -> bool {
        matches!(
            self,
            Self::AbsoluteX | Self::AbsoluteY | Self::IndirectIndexedY | Self::Relative
        )
    }
}

/// Addressing mode resolution result
///
/// Contains the effective address and whether a page boundary was crossed
#[derive(Debug, Clone, Copy)]
pub struct AddressResult {
    /// Effective memory address (or operand value for Immediate mode)
    pub addr: u16,

    /// Whether a page boundary was crossed during address calculation
    ///
    /// Page crossing adds +1 cycle for read operations in certain modes
    pub page_crossed: bool,
}

impl AddressResult {
    /// Create a new address result without page crossing
    #[inline]
    #[must_use]
    pub const fn new(addr: u16) -> Self {
        Self {
            addr,
            page_crossed: false,
        }
    }

    /// Create a new address result with page crossing information
    #[inline]
    #[must_use]
    pub const fn with_page_cross(addr: u16, page_crossed: bool) -> Self {
        Self { addr, page_crossed }
    }
}

/// Check if two addresses are on different pages
///
/// A page is 256 bytes, so addresses differ by page if their high bytes differ.
#[inline]
#[must_use]
pub const fn page_crossed(addr1: u16, addr2: u16) -> bool {
    (addr1 & 0xFF00) != (addr2 & 0xFF00)
}

/// Addressing mode implementations
///
/// These methods are called during instruction execution to resolve operand addresses.
impl AddressingMode {
    /// Resolve the effective address for this addressing mode
    ///
    /// # Arguments
    ///
    /// * `pc` - Current program counter (AFTER opcode fetch)
    /// * `x` - X register value
    /// * `y` - Y register value
    /// * `bus` - Memory bus for reading operands
    ///
    /// # Returns
    ///
    /// `AddressResult` containing the effective address and page crossing status
    ///
    /// # Notes
    ///
    /// - For `Immediate`, `addr` is the value itself
    /// - For `Accumulator` and `Implied`, `addr` is unused
    /// - PC should point to the first operand byte
    pub fn resolve(self, pc: u16, x: u8, y: u8, bus: &mut impl Bus) -> AddressResult {
        match self {
            Self::Implied | Self::Accumulator => AddressResult::new(0),

            Self::Immediate => AddressResult::new(pc),

            Self::ZeroPage => {
                let addr = bus.read(pc) as u16;
                AddressResult::new(addr)
            }

            Self::ZeroPageX => {
                let base = bus.read(pc);
                // Wraps within zero page
                let addr = base.wrapping_add(x) as u16;
                AddressResult::new(addr)
            }

            Self::ZeroPageY => {
                let base = bus.read(pc);
                // Wraps within zero page
                let addr = base.wrapping_add(y) as u16;
                AddressResult::new(addr)
            }

            Self::Absolute => {
                let addr = bus.read_u16(pc);
                AddressResult::new(addr)
            }

            Self::AbsoluteX => {
                let base = bus.read_u16(pc);
                let addr = base.wrapping_add(x as u16);
                let crossed = page_crossed(base, addr);
                AddressResult::with_page_cross(addr, crossed)
            }

            Self::AbsoluteY => {
                let base = bus.read_u16(pc);
                let addr = base.wrapping_add(y as u16);
                let crossed = page_crossed(base, addr);
                AddressResult::with_page_cross(addr, crossed)
            }

            Self::Indirect => {
                let ptr = bus.read_u16(pc);
                // JMP indirect has a page-wrap bug
                let addr = bus.read_u16_wrap(ptr);
                AddressResult::new(addr)
            }

            Self::IndexedIndirectX => {
                let base = bus.read(pc);
                // Zero page pointer + X (wraps)
                let ptr = base.wrapping_add(x);

                // Read 16-bit address from zero page (wraps)
                let lo = bus.read(ptr as u16) as u16;
                let hi = bus.read(ptr.wrapping_add(1) as u16) as u16;
                let addr = (hi << 8) | lo;

                AddressResult::new(addr)
            }

            Self::IndirectIndexedY => {
                let ptr = bus.read(pc);

                // Read base address from zero page (wraps)
                let lo = bus.read(ptr as u16) as u16;
                let hi = bus.read(ptr.wrapping_add(1) as u16) as u16;
                let base = (hi << 8) | lo;

                // Add Y to base address
                let addr = base.wrapping_add(y as u16);
                let crossed = page_crossed(base, addr);

                AddressResult::with_page_cross(addr, crossed)
            }

            Self::Relative => {
                // Read signed offset
                let offset = bus.read(pc) as i8;
                // PC has already advanced past the branch instruction
                let base = pc.wrapping_add(1);
                let addr = base.wrapping_add(offset as u16);

                let crossed = page_crossed(base, addr);
                AddressResult::with_page_cross(addr, crossed)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestBus {
        ram: [u8; 0x10000],
    }

    impl Bus for TestBus {
        fn read(&mut self, addr: u16) -> u8 {
            self.ram[addr as usize]
        }

        fn write(&mut self, addr: u16, value: u8) {
            self.ram[addr as usize] = value;
        }
    }

    #[test]
    fn test_page_crossed_same_page() {
        assert!(!page_crossed(0x1234, 0x1256));
    }

    #[test]
    fn test_page_crossed_different_page() {
        assert!(page_crossed(0x12FF, 0x1300));
    }

    #[test]
    fn test_immediate() {
        let mut bus = TestBus { ram: [0; 0x10000] };
        let result = AddressingMode::Immediate.resolve(0x1000, 0, 0, &mut bus);
        assert_eq!(result.addr, 0x1000);
        assert!(!result.page_crossed);
    }

    #[test]
    fn test_zero_page() {
        let mut bus = TestBus { ram: [0; 0x10000] };
        bus.write(0x1000, 0x42);

        let result = AddressingMode::ZeroPage.resolve(0x1000, 0, 0, &mut bus);
        assert_eq!(result.addr, 0x0042);
    }

    #[test]
    fn test_zero_page_x_wrap() {
        let mut bus = TestBus { ram: [0; 0x10000] };
        bus.write(0x1000, 0xFF);

        // $FF + $05 = $04 (wraps within zero page)
        let result = AddressingMode::ZeroPageX.resolve(0x1000, 0x05, 0, &mut bus);
        assert_eq!(result.addr, 0x0004);
    }

    #[test]
    fn test_absolute() {
        let mut bus = TestBus { ram: [0; 0x10000] };
        bus.write(0x1000, 0x34);
        bus.write(0x1001, 0x12);

        let result = AddressingMode::Absolute.resolve(0x1000, 0, 0, &mut bus);
        assert_eq!(result.addr, 0x1234);
    }

    #[test]
    fn test_absolute_x_no_page_cross() {
        let mut bus = TestBus { ram: [0; 0x10000] };
        bus.write(0x1000, 0x00);
        bus.write(0x1001, 0x12);

        let result = AddressingMode::AbsoluteX.resolve(0x1000, 0x10, 0, &mut bus);
        assert_eq!(result.addr, 0x1210);
        assert!(!result.page_crossed);
    }

    #[test]
    fn test_absolute_x_page_cross() {
        let mut bus = TestBus { ram: [0; 0x10000] };
        bus.write(0x1000, 0xFF);
        bus.write(0x1001, 0x12);

        let result = AddressingMode::AbsoluteX.resolve(0x1000, 0x01, 0, &mut bus);
        assert_eq!(result.addr, 0x1300);
        assert!(result.page_crossed);
    }

    #[test]
    fn test_indexed_indirect_x() {
        let mut bus = TestBus { ram: [0; 0x10000] };

        // Instruction operand: $80
        bus.write(0x1000, 0x80);

        // Pointer at ($80 + X) = $85:  [$85] = $20, [$86] = $30
        bus.write(0x0085, 0x20);
        bus.write(0x0086, 0x30);

        let result = AddressingMode::IndexedIndirectX.resolve(0x1000, 0x05, 0, &mut bus);
        assert_eq!(result.addr, 0x3020);
    }

    #[test]
    fn test_indirect_indexed_y_no_page_cross() {
        let mut bus = TestBus { ram: [0; 0x10000] };

        // Instruction operand: $80
        bus.write(0x1000, 0x80);

        // Pointer at $80: [$80] = $00, [$81] = $30
        bus.write(0x0080, 0x00);
        bus.write(0x0081, 0x30);

        let result = AddressingMode::IndirectIndexedY.resolve(0x1000, 0, 0x10, &mut bus);
        assert_eq!(result.addr, 0x3010);
        assert!(!result.page_crossed);
    }

    #[test]
    fn test_indirect_indexed_y_page_cross() {
        let mut bus = TestBus { ram: [0; 0x10000] };

        // Instruction operand: $80
        bus.write(0x1000, 0x80);

        // Pointer at $80: [$80] = $FF, [$81] = $30
        bus.write(0x0080, 0xFF);
        bus.write(0x0081, 0x30);

        let result = AddressingMode::IndirectIndexedY.resolve(0x1000, 0, 0x01, &mut bus);
        assert_eq!(result.addr, 0x3100);
        assert!(result.page_crossed);
    }

    #[test]
    fn test_relative_forward_same_page() {
        let mut bus = TestBus { ram: [0; 0x10000] };

        // PC at $1000, offset = +$10
        bus.write(0x1000, 0x10);

        let result = AddressingMode::Relative.resolve(0x1000, 0, 0, &mut bus);
        // Base = PC + 1 = $1001, target = $1001 + $10 = $1011
        assert_eq!(result.addr, 0x1011);
        assert!(!result.page_crossed);
    }

    #[test]
    fn test_relative_forward_page_cross() {
        let mut bus = TestBus { ram: [0; 0x10000] };

        // PC at $10F0, offset = +$20
        bus.write(0x10F0, 0x20);

        let result = AddressingMode::Relative.resolve(0x10F0, 0, 0, &mut bus);
        // Base = $10F1, target = $10F1 + $20 = $1111
        assert_eq!(result.addr, 0x1111);
        assert!(result.page_crossed);
    }

    #[test]
    fn test_relative_backward() {
        let mut bus = TestBus { ram: [0; 0x10000] };

        // PC at $1050, offset = -$10 (0xF0 as unsigned)
        bus.write(0x1050, 0xF0);

        let result = AddressingMode::Relative.resolve(0x1050, 0, 0, &mut bus);
        // Base = $1051, target = $1051 - $10 = $1041
        assert_eq!(result.addr, 0x1041);
        assert!(!result.page_crossed);
    }
}
