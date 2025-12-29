//! Memory bus trait for CPU communication.
//!
//! The Bus trait defines the interface between the CPU and the rest of the system.
//! All memory reads and writes go through this trait, allowing for flexible
//! implementation of memory mapping, I/O registers, and hardware synchronization.

/// Memory bus interface
///
/// Implementors of this trait provide memory access to the CPU.
/// The CPU calls `read()` and `write()` for all memory operations.
///
/// # Examples
///
/// ## Simple RAM-only bus
///
/// ```
/// use rustynes_cpu::Bus;
///
/// struct SimpleBus {
///     ram: [u8; 0x10000],
/// }
///
/// impl Bus for SimpleBus {
///     fn read(&mut self, addr: u16) -> u8 {
///         self.ram[addr as usize]
///     }
///
///     fn write(&mut self, addr: u16, value: u8) {
///         self.ram[addr as usize] = value;
///     }
/// }
/// ```
///
/// ## NES bus with memory-mapped I/O
///
/// ```
/// use rustynes_cpu::Bus;
///
/// # struct Ppu;
/// # impl Ppu {
/// #     fn read_register(&mut self, _: u16) -> u8 { 0 }
/// #     fn write_register(&mut self, _: u16, _: u8) {}
/// # }
/// struct NesBus {
///     ram: [u8; 0x800],
///     ppu: Ppu,
///     // ... other components
/// }
///
/// impl Bus for NesBus {
///     fn read(&mut self, addr: u16) -> u8 {
///         match addr {
///             0x0000..=0x1FFF => {
///                 // 2KB internal RAM, mirrored 4 times
///                 self.ram[(addr & 0x07FF) as usize]
///             }
///             0x2000..=0x3FFF => {
///                 // PPU registers, mirrored every 8 bytes
///                 self.ppu.read_register(addr & 0x0007)
///             }
///             // ... other address ranges
///             _ => 0,
///         }
///     }
///
///     fn write(&mut self, addr: u16, value: u8) {
///         match addr {
///             0x0000..=0x1FFF => {
///                 self.ram[(addr & 0x07FF) as usize] = value;
///             }
///             0x2000..=0x3FFF => {
///                 self.ppu.write_register(addr & 0x0007, value);
///             }
///             // ... other address ranges
///             _ => {}
///         }
///     }
/// }
/// ```
pub trait Bus {
    /// Read a byte from memory
    ///
    /// # Arguments
    ///
    /// * `addr` - 16-bit memory address to read from
    ///
    /// # Returns
    ///
    /// The 8-bit value at the specified address
    ///
    /// # Notes
    ///
    /// - This function may have side effects (e.g., reading from a hardware register)
    /// - Open bus behavior: return last value on the bus for unmapped addresses
    /// - For debugging, implement `peek()` instead
    fn read(&mut self, addr: u16) -> u8;

    /// Write a byte to memory
    ///
    /// # Arguments
    ///
    /// * `addr` - 16-bit memory address to write to
    /// * `value` - 8-bit value to write
    ///
    /// # Notes
    ///
    /// - This function may have side effects (e.g., triggering DMA)
    /// - Writes to ROM or unmapped areas should be ignored (or logged)
    fn write(&mut self, addr: u16, value: u8);

    /// Read a byte without side effects (for debugging/disassembly)
    ///
    /// Default implementation returns 0. Override for proper debugging support.
    ///
    /// # Arguments
    ///
    /// * `addr` - 16-bit memory address to peek at
    ///
    /// # Returns
    ///
    /// The 8-bit value at the specified address, without triggering side effects
    ///
    /// # Notes
    ///
    /// - This should NOT modify any state (e.g., don't clear IRQ flags)
    /// - Used by debuggers and disassemblers
    /// - Default implementation returns 0 for simplicity
    #[inline]
    fn peek(&self, addr: u16) -> u8 {
        let _ = addr;
        0
    }

    /// Read a 16-bit value in little-endian format
    ///
    /// Reads two consecutive bytes and combines them into a 16-bit value.
    ///
    /// # Arguments
    ///
    /// * `addr` - Address of the low byte
    ///
    /// # Returns
    ///
    /// 16-bit value: `(high << 8) | low`
    ///
    /// # Notes
    ///
    /// - Reads from `addr` (low byte) and `addr + 1` (high byte)
    /// - Addition wraps: `0xFFFF + 1 = 0x0000`
    ///
    /// # Example
    ///
    /// ```
    /// # use rustynes_cpu::Bus;
    /// # struct TestBus { ram: [u8; 0x10000] }
    /// # impl Bus for TestBus {
    /// #     fn read(&mut self, addr: u16) -> u8 { self.ram[addr as usize] }
    /// #     fn write(&mut self, addr: u16, value: u8) { self.ram[addr as usize] = value; }
    /// # }
    /// # let mut bus = TestBus { ram: [0; 0x10000] };
    /// bus.write(0x1000, 0x34); // Low byte
    /// bus.write(0x1001, 0x12); // High byte
    /// assert_eq!(bus.read_u16(0x1000), 0x1234);
    /// ```
    #[inline]
    fn read_u16(&mut self, addr: u16) -> u16 {
        let lo = self.read(addr) as u16;
        let hi = self.read(addr.wrapping_add(1)) as u16;
        (hi << 8) | lo
    }

    /// Read a 16-bit value with page wrap (for JMP indirect bug)
    ///
    /// The 6502 has a bug in JMP indirect where if the low byte is $FF,
    /// the high byte is read from $xx00 instead of $(xx+1)00.
    ///
    /// # Arguments
    ///
    /// * `addr` - Address of the low byte
    ///
    /// # Returns
    ///
    /// 16-bit value with page-wrap behavior
    ///
    /// # Example: JMP ($10FF) Bug
    ///
    /// ```
    /// # use rustynes_cpu::Bus;
    /// # struct TestBus { ram: [u8; 0x10000] }
    /// # impl Bus for TestBus {
    /// #     fn read(&mut self, addr: u16) -> u8 { self.ram[addr as usize] }
    /// #     fn write(&mut self, addr: u16, value: u8) { self.ram[addr as usize] = value; }
    /// # }
    /// # let mut bus = TestBus { ram: [0; 0x10000] };
    /// bus.write(0x10FF, 0x34); // Low byte at $10FF
    /// bus.write(0x1100, 0x56); // Should be high byte (correct)
    /// bus.write(0x1000, 0x12); // Actually read as high byte (bug!)
    ///
    /// // Normal read would give 0x5634
    /// assert_eq!(bus.read_u16(0x10FF), 0x5634);
    ///
    /// // Page-wrap read gives 0x1234 (bug behavior)
    /// assert_eq!(bus.read_u16_wrap(0x10FF), 0x1234);
    /// ```
    #[inline]
    fn read_u16_wrap(&mut self, addr: u16) -> u16 {
        let lo = self.read(addr) as u16;

        // If low byte is at $xxFF, high byte wraps to $xx00
        let hi_addr = if addr & 0xFF == 0xFF {
            addr & 0xFF00
        } else {
            addr.wrapping_add(1)
        };

        let hi = self.read(hi_addr) as u16;
        (hi << 8) | lo
    }
}

/// Cycle-accurate bus interface for sub-cycle PPU/APU synchronization
///
/// This trait extends the basic Bus functionality with cycle callbacks that are
/// invoked BEFORE each memory access. This enables accurate emulation of the
/// NES timing where PPU advances 3 dots per CPU cycle and APU advances 1 cycle.
///
/// # Architecture
///
/// The key insight from accurate emulators (like Pinky) is that `on_cpu_cycle()`
/// must be called at the START of each memory access, BEFORE the actual read/write.
/// This ensures that when the CPU reads $2002 (PPUSTATUS), the PPU has already
/// advanced to the correct state for that exact CPU cycle.
///
/// # Timing Model
///
/// ```text
/// CPU Cycle:  |-------- read --------|
/// PPU Cycles: |--1--|--2--|--3--|
///              ^ on_cpu_cycle() called here (before read)
/// ```
///
/// # Examples
///
/// ## Implementing cycle-accurate bus for NES
///
/// ```
/// use rustynes_cpu::CpuBus;
///
/// # struct Ppu { scanline: u16, dot: u16 }
/// # impl Ppu {
/// #     fn step(&mut self) { self.dot += 1; }
/// #     fn read_register(&mut self, _: u16) -> u8 { 0 }
/// #     fn write_register(&mut self, _: u16, _: u8) {}
/// # }
/// # struct Apu;
/// # impl Apu { fn step(&mut self) {} }
/// struct CycleAccurateBus {
///     ram: [u8; 0x800],
///     ppu: Ppu,
///     apu: Apu,
/// }
///
/// impl CpuBus for CycleAccurateBus {
///     fn read(&mut self, addr: u16) -> u8 {
///         match addr {
///             0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],
///             0x2000..=0x3FFF => self.ppu.read_register(addr & 0x0007),
///             _ => 0,
///         }
///     }
///
///     fn write(&mut self, addr: u16, value: u8) {
///         match addr {
///             0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize] = value,
///             0x2000..=0x3FFF => self.ppu.write_register(addr & 0x0007, value),
///             _ => {}
///         }
///     }
///
///     fn on_cpu_cycle(&mut self) {
///         // Step APU once per CPU cycle
///         self.apu.step();
///
///         // Step PPU 3 times per CPU cycle (3:1 ratio)
///         for _ in 0..3 {
///             self.ppu.step();
///         }
///     }
/// }
/// ```
///
/// # Implementation Notes
///
/// - The CPU's `read_cycle()` and `write_cycle()` methods call `on_cpu_cycle()`
///   before performing the actual memory access
/// - This trait is essential for passing VBlank timing tests that require
///   +/- 2 cycle accuracy when reading $2002 during VBlank transitions
/// - Dummy reads for page boundary crossings also trigger `on_cpu_cycle()`
pub trait CpuBus {
    /// Read a byte from memory
    ///
    /// This is the raw memory read without cycle callback. For cycle-accurate
    /// emulation, use CPU's `read_cycle()` method instead which calls
    /// `on_cpu_cycle()` before this method.
    ///
    /// # Arguments
    ///
    /// * `addr` - 16-bit memory address to read from
    ///
    /// # Returns
    ///
    /// The 8-bit value at the specified address
    fn read(&mut self, addr: u16) -> u8;

    /// Write a byte to memory
    ///
    /// This is the raw memory write without cycle callback. For cycle-accurate
    /// emulation, use CPU's `write_cycle()` method instead which calls
    /// `on_cpu_cycle()` before this method.
    ///
    /// # Arguments
    ///
    /// * `addr` - 16-bit memory address to write to
    /// * `value` - 8-bit value to write
    fn write(&mut self, addr: u16, value: u8);

    /// Called BEFORE each memory access to synchronize PPU/APU
    ///
    /// This callback is the heart of cycle-accurate emulation. It must:
    /// 1. Step the APU once (1:1 ratio with CPU)
    /// 2. Step the PPU three times (3:1 ratio with CPU)
    /// 3. Handle any cycle-based events (IRQ timing, etc.)
    ///
    /// # Critical Timing
    ///
    /// This method is called BEFORE the memory access occurs. This is essential
    /// for accurate $2002 reads during VBlank, where the PPU state must be
    /// updated to the exact cycle before the CPU observes the flags.
    ///
    /// # Example
    ///
    /// ```ignore
    /// fn on_cpu_cycle(&mut self) {
    ///     // Step APU
    ///     self.apu.step();
    ///
    ///     // Step PPU 3 times
    ///     for _ in 0..3 {
    ///         self.ppu.step();
    ///     }
    /// }
    /// ```
    fn on_cpu_cycle(&mut self);

    /// Read a byte without side effects (for debugging/disassembly)
    ///
    /// Default implementation calls read(). Override for proper debugging support
    /// where hardware register reads have side effects.
    ///
    /// # Notes
    ///
    /// - This should NOT call `on_cpu_cycle()`
    /// - This should NOT modify any state (e.g., don't clear IRQ flags)
    /// - Used by debuggers and disassemblers
    #[inline]
    fn peek(&self, addr: u16) -> u8 {
        let _ = addr;
        0
    }

    /// Read a 16-bit value in little-endian format
    ///
    /// Reads two consecutive bytes and combines them into a 16-bit value.
    /// Note: This does NOT call on_cpu_cycle() - use CPU's cycle-aware methods.
    ///
    /// # Arguments
    ///
    /// * `addr` - Address of the low byte
    ///
    /// # Returns
    ///
    /// 16-bit value: `(high << 8) | low`
    #[inline]
    fn read_u16(&mut self, addr: u16) -> u16 {
        let lo = self.read(addr) as u16;
        let hi = self.read(addr.wrapping_add(1)) as u16;
        (hi << 8) | lo
    }

    /// Read a 16-bit value with page wrap (for JMP indirect bug)
    ///
    /// The 6502 has a bug in JMP indirect where if the low byte is $FF,
    /// the high byte is read from $xx00 instead of $(xx+1)00.
    /// Note: This does NOT call on_cpu_cycle() - use CPU's cycle-aware methods.
    #[inline]
    fn read_u16_wrap(&mut self, addr: u16) -> u16 {
        let lo = self.read(addr) as u16;

        // If low byte is at $xxFF, high byte wraps to $xx00
        let hi_addr = if addr & 0xFF == 0xFF {
            addr & 0xFF00
        } else {
            addr.wrapping_add(1)
        };

        let hi = self.read(hi_addr) as u16;
        (hi << 8) | lo
    }
}

/// Blanket implementation: any CpuBus also implements Bus
///
/// This provides backward compatibility - code using the simpler Bus trait
/// will continue to work. The CpuBus trait just adds the on_cpu_cycle() method.
impl<T: CpuBus> Bus for T {
    #[inline]
    fn read(&mut self, addr: u16) -> u8 {
        CpuBus::read(self, addr)
    }

    #[inline]
    fn write(&mut self, addr: u16, value: u8) {
        CpuBus::write(self, addr, value);
    }

    #[inline]
    fn peek(&self, addr: u16) -> u8 {
        CpuBus::peek(self, addr)
    }

    #[inline]
    fn read_u16(&mut self, addr: u16) -> u16 {
        CpuBus::read_u16(self, addr)
    }

    #[inline]
    fn read_u16_wrap(&mut self, addr: u16) -> u16 {
        CpuBus::read_u16_wrap(self, addr)
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

        fn peek(&self, addr: u16) -> u8 {
            self.ram[addr as usize]
        }
    }

    #[test]
    fn test_read_write() {
        let mut bus = TestBus { ram: [0; 0x10000] };

        bus.write(0x1234, 0x42);
        assert_eq!(bus.read(0x1234), 0x42);
    }

    #[test]
    fn test_read_u16() {
        let mut bus = TestBus { ram: [0; 0x10000] };

        bus.write(0x1000, 0x34);
        bus.write(0x1001, 0x12);

        assert_eq!(bus.read_u16(0x1000), 0x1234);
    }

    #[test]
    fn test_read_u16_wrap_no_boundary() {
        let mut bus = TestBus { ram: [0; 0x10000] };

        bus.write(0x1080, 0x34);
        bus.write(0x1081, 0x12);

        // No page boundary, should behave normally
        assert_eq!(bus.read_u16_wrap(0x1080), 0x1234);
    }

    #[test]
    fn test_read_u16_wrap_page_boundary() {
        let mut bus = TestBus { ram: [0; 0x10000] };

        bus.write(0x10FF, 0x34); // Low byte
        bus.write(0x1100, 0x56); // What high byte SHOULD be
        bus.write(0x1000, 0x12); // What high byte ACTUALLY is (bug)

        // Normal read crosses page correctly
        assert_eq!(bus.read_u16(0x10FF), 0x5634);

        // Wrap read triggers the bug
        assert_eq!(bus.read_u16_wrap(0x10FF), 0x1234);
    }

    #[test]
    fn test_peek_no_side_effects() {
        let bus = TestBus {
            ram: [0x42; 0x10000],
        };

        // Peek should not modify state
        assert_eq!(bus.peek(0x1234), 0x42);
    }
}
