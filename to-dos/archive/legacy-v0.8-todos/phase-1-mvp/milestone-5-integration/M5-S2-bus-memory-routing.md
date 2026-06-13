# [Milestone 5] Sprint 5.2: Bus & Memory Routing

**Status:** ✅ COMPLETED
**Started:** December 19, 2025
**Completed:** December 19, 2025
**Duration:** 1 day (part of M5 integration)
**Assignee:** Claude Code / Developer

---

## Overview

Implement the system bus connecting CPU, PPU, APU, and Mapper subsystems with complete memory routing for the NES address space. This sprint establishes the communication backbone for all emulation components.

### Goals

- Complete CPU address space implementation ($0000-$FFFF)
- PPU register routing ($2000-$3FFF)
- APU register routing ($4000-$4017)
- Controller register routing ($4016-$4017)
- Mapper integration ($4020-$FFFF)
- Internal RAM with mirroring ($0000-$1FFF)
- DMA handling (OAM DMA at $4014)
- Zero unsafe code

---

## Acceptance Criteria

- [ ] Bus trait implementation complete
- [ ] CPU address space fully mapped
- [ ] PPU register reads/writes routed correctly
- [ ] APU register reads/writes routed correctly
- [ ] Controller registers functional
- [ ] Mapper PRG space integrated
- [ ] Internal RAM mirrored correctly
- [ ] OAM DMA implemented (513/514 cycle stall)
- [ ] DMC DMA handling (CPU/APU coordination)
- [ ] Comprehensive unit tests
- [ ] Zero unsafe code

---

## Tasks

### Task 1: Define Bus Trait

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 1 hour

**Description:**
Define the Bus trait that provides memory access abstraction for the CPU.

**Files:**

- `crates/rustynes-core/src/bus.rs` - Bus trait definition

**Subtasks:**

- [ ] Define Bus trait with read/write methods
- [ ] Add read_u16 helper for 16-bit reads (little-endian)
- [ ] Document trait requirements
- [ ] Add trait bounds (Send for thread safety)

**Implementation:**

```rust
/// Memory bus abstraction for CPU memory access
pub trait Bus: Send {
    /// Read a byte from the given address
    fn read(&mut self, addr: u16) -> u8;

    /// Write a byte to the given address
    fn write(&mut self, addr: u16, value: u8);

    /// Read a 16-bit value (little-endian) from the given address
    fn read_u16(&mut self, addr: u16) -> u16 {
        let lo = self.read(addr) as u16;
        let hi = self.read(addr.wrapping_add(1)) as u16;
        (hi << 8) | lo
    }

    /// Poll for pending interrupts
    fn poll_nmi(&mut self) -> bool;
    fn poll_irq(&mut self) -> bool;
}
```

---

### Task 2: Implement NES Bus Structure

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 3 hours

**Description:**
Create the main NES Bus struct that coordinates all subsystems.

**Files:**

- `crates/rustynes-core/src/bus.rs` - NesBus implementation

**Subtasks:**

- [ ] Define NesBus struct with all components
- [ ] Implement CPU read routing
- [ ] Implement CPU write routing
- [ ] Add interrupt polling methods
- [ ] Implement Display for debugging

**Implementation:**

```rust
use rustynes_cpu::Cpu;
use rustynes_ppu::Ppu;
use rustynes_apu::Apu;
use rustynes_mappers::Mapper;

pub struct NesBus {
    // Internal RAM (2KB, mirrored to 8KB)
    ram: [u8; 2048],

    // Subsystems
    ppu: Ppu,
    apu: Apu,
    cartridge: Box<dyn Mapper>,

    // Controllers
    controller1: Controller,
    controller2: Controller,

    // DMA state
    dma_pending: bool,
    dma_address: u8,
    dma_data: u8,
    dma_dummy_read: bool,
}

impl NesBus {
    pub fn new(
        ppu: Ppu,
        apu: Apu,
        cartridge: Box<dyn Mapper>,
    ) -> Self {
        Self {
            ram: [0; 2048],
            ppu,
            apu,
            cartridge,
            controller1: Controller::new(),
            controller2: Controller::new(),
            dma_pending: false,
            dma_address: 0,
            dma_data: 0,
            dma_dummy_read: false,
        }
    }
}
```

---

### Task 3: CPU Read Routing

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Implement complete CPU read address space routing.

**Files:**

- `crates/rustynes-core/src/bus.rs` - Bus::read implementation

**Subtasks:**

- [ ] Route $0000-$1FFF to internal RAM (with mirroring)
- [ ] Route $2000-$3FFF to PPU registers (with mirroring)
- [ ] Route $4000-$4015 to APU registers
- [ ] Route $4016 to Controller 1
- [ ] Route $4017 to Controller 2
- [ ] Route $4020-$FFFF to Mapper PRG space
- [ ] Handle open bus for unmapped regions

**Implementation:**

```rust
impl Bus for NesBus {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // Internal RAM (2KB, mirrored 4 times to 8KB)
            0x0000..=0x1FFF => {
                self.ram[(addr & 0x07FF) as usize]
            }

            // PPU Registers (8 registers, mirrored)
            0x2000..=0x3FFF => {
                let register = 0x2000 + (addr & 0x0007);
                self.ppu.read_register(register)
            }

            // APU Registers
            0x4000..=0x4015 => {
                self.apu.read_register(addr)
            }

            // Controller 1
            0x4016 => {
                self.controller1.read()
            }

            // Controller 2 / APU Frame Counter (read: controller)
            0x4017 => {
                self.controller2.read()
            }

            // APU test registers (disabled on retail consoles)
            0x4018..=0x401F => {
                0 // Open bus
            }

            // Cartridge space (Mapper controlled)
            0x4020..=0xFFFF => {
                self.cartridge.read_prg(addr)
            }
        }
    }
}
```

---

### Task 4: CPU Write Routing

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 2 hours

**Description:**
Implement complete CPU write address space routing.

**Files:**

- `crates/rustynes-core/src/bus.rs` - Bus::write implementation

**Subtasks:**

- [ ] Route $0000-$1FFF to internal RAM (with mirroring)
- [ ] Route $2000-$3FFF to PPU registers (with mirroring)
- [ ] Route $4000-$4013 to APU channels
- [ ] Route $4014 to OAM DMA
- [ ] Route $4015 to APU status
- [ ] Route $4016 to Controller strobe
- [ ] Route $4017 to APU frame counter
- [ ] Route $4020-$FFFF to Mapper PRG space

**Implementation:**

```rust
impl Bus for NesBus {
    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            // Internal RAM
            0x0000..=0x1FFF => {
                self.ram[(addr & 0x07FF) as usize] = value;
            }

            // PPU Registers
            0x2000..=0x3FFF => {
                let register = 0x2000 + (addr & 0x0007);
                self.ppu.write_register(register, value);
            }

            // APU Pulse 1, Pulse 2, Triangle, Noise, DMC
            0x4000..=0x4013 => {
                self.apu.write_register(addr, value);
            }

            // OAM DMA
            0x4014 => {
                self.trigger_oam_dma(value);
            }

            // APU Status
            0x4015 => {
                self.apu.write_register(addr, value);
            }

            // Controller Strobe
            0x4016 => {
                self.controller1.write(value);
                self.controller2.write(value);
            }

            // APU Frame Counter
            0x4017 => {
                self.apu.write_register(addr, value);
            }

            // APU test registers (ignored)
            0x4018..=0x401F => {
                // Write has no effect
            }

            // Cartridge space (Mapper controlled)
            0x4020..=0xFFFF => {
                self.cartridge.write_prg(addr, value);
            }
        }
    }
}
```

---

### Task 5: OAM DMA Implementation

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 3 hours

**Description:**
Implement OAM DMA for sprite data transfer with accurate cycle timing.

**Files:**

- `crates/rustynes-core/src/bus.rs` - OAM DMA handling

**Subtasks:**

- [ ] Implement trigger_oam_dma method
- [ ] Add DMA state tracking
- [ ] Implement 513/514 cycle stall (odd/even CPU cycle)
- [ ] Transfer 256 bytes from CPU RAM to PPU OAM
- [ ] Add DMA cycle count to CPU

**Implementation:**

```rust
impl NesBus {
    /// Trigger OAM DMA transfer
    /// Copies 256 bytes from CPU RAM to PPU OAM
    /// Takes 513 or 514 cycles depending on alignment
    fn trigger_oam_dma(&mut self, page: u8) {
        self.dma_pending = true;
        self.dma_address = page;
        self.dma_dummy_read = true;
    }

    /// Execute one DMA cycle
    /// Returns true if DMA is still in progress
    pub fn tick_dma(&mut self, cpu_cycle: u64) -> bool {
        if !self.dma_pending {
            return false;
        }

        // Dummy read on first cycle (even cycles only)
        if self.dma_dummy_read {
            if cpu_cycle % 2 == 1 {
                // Wait for even cycle
                return true;
            }
            self.dma_dummy_read = false;
            return true;
        }

        // Transfer 256 bytes (read on even cycles, write on odd cycles)
        let base_address = (self.dma_address as u16) << 8;
        for offset in 0..256u16 {
            let addr = base_address.wrapping_add(offset);
            let data = self.read(addr);
            self.ppu.write_oam_data(data);
        }

        self.dma_pending = false;
        false
    }
}
```

---

### Task 6: DMC DMA Handling

- **Status:** ⏳ Pending
- **Priority:** Medium
- **Estimated:** 2 hours

**Description:**
Implement DMC (Delta Modulation Channel) DMA with CPU stalling.

**Files:**

- `crates/rustynes-core/src/bus.rs` - DMC DMA support

**Subtasks:**

- [ ] Add DMC DMA state tracking
- [ ] Implement CPU cycle stealing (4 cycles per sample)
- [ ] Handle DMC/OAM DMA conflicts
- [ ] Implement DMC sample fetch
- [ ] Add DMC DMA unit tests

**Implementation:**

```rust
impl NesBus {
    /// Check if DMC needs a sample fetch
    pub fn poll_dmc_dma(&mut self) -> Option<u16> {
        self.apu.poll_dmc_address()
    }

    /// Fetch DMC sample byte
    pub fn fetch_dmc_sample(&mut self, addr: u16) -> u8 {
        // DMC sample fetch takes 4 CPU cycles
        // Conflicts with OAM DMA (OAM DMA takes priority)
        self.read(addr)
    }
}
```

---

### Task 7: Interrupt Polling

- **Status:** ⏳ Pending
- **Priority:** High
- **Estimated:** 1 hour

**Description:**
Implement interrupt polling for NMI and IRQ.

**Files:**

- `crates/rustynes-core/src/bus.rs` - Interrupt methods

**Subtasks:**

- [ ] Implement poll_nmi (from PPU)
- [ ] Implement poll_irq (from APU, Mapper)
- [ ] Clear interrupt flags after polling
- [ ] Add interrupt priority handling

**Implementation:**

```rust
impl Bus for NesBus {
    fn poll_nmi(&mut self) -> bool {
        self.ppu.poll_nmi()
    }

    fn poll_irq(&mut self) -> bool {
        // IRQ can come from APU frame counter or mapper
        self.apu.irq_pending() || self.cartridge.irq_pending()
    }
}
```

---

### Task 8: Controller Integration

- **Status:** ⏳ Pending
- **Priority:** Medium
- **Estimated:** 2 hours

**Description:**
Integrate controller input handling.

**Files:**

- `crates/rustynes-core/src/input.rs` - Controller implementation

**Subtasks:**

- [ ] Define Controller struct
- [ ] Implement strobe signal
- [ ] Implement read sequence (8 bits)
- [ ] Add button state tracking
- [ ] Implement open bus behavior for unused bits

**Implementation:**

```rust
pub struct Controller {
    button_state: u8,
    shift_register: u8,
    strobe: bool,
}

impl Controller {
    pub fn new() -> Self {
        Self {
            button_state: 0,
            shift_register: 0,
            strobe: false,
        }
    }

    pub fn set_button(&mut self, button: Button, pressed: bool) {
        if pressed {
            self.button_state |= button as u8;
        } else {
            self.button_state &= !(button as u8);
        }
    }

    pub fn write(&mut self, value: u8) {
        self.strobe = (value & 0x01) != 0;
        if self.strobe {
            self.shift_register = self.button_state;
        }
    }

    pub fn read(&mut self) -> u8 {
        if self.strobe {
            self.shift_register = self.button_state;
        }

        let bit = self.shift_register & 0x01;
        self.shift_register >>= 1;
        self.shift_register |= 0x80; // Open bus

        bit
    }
}

#[repr(u8)]
pub enum Button {
    A      = 0x01,
    B      = 0x02,
    Select = 0x04,
    Start  = 0x08,
    Up     = 0x10,
    Down   = 0x20,
    Left   = 0x40,
    Right  = 0x80,
}
```

---

### Task 9: Unit Tests

- **Status:** ⏳ Pending
- **Priority:** Medium
- **Estimated:** 3 hours

**Description:**
Create comprehensive unit tests for bus routing.

**Files:**

- `crates/rustynes-core/src/bus.rs` - Test module

**Subtasks:**

- [ ] Test RAM mirroring ($0000-$1FFF)
- [ ] Test PPU register mirroring ($2000-$3FFF)
- [ ] Test APU register routing
- [ ] Test controller read/write
- [ ] Test OAM DMA triggering
- [ ] Test interrupt polling
- [ ] Test Mapper integration

**Tests:**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ram_mirroring() {
        let mut bus = create_test_bus();

        // Write to $0123
        bus.write(0x0123, 0x42);

        // Should be readable from mirrors
        assert_eq!(bus.read(0x0123), 0x42); // Original
        assert_eq!(bus.read(0x0923), 0x42); // Mirror 1
        assert_eq!(bus.read(0x1123), 0x42); // Mirror 2
        assert_eq!(bus.read(0x1923), 0x42); // Mirror 3
    }

    #[test]
    fn test_ppu_register_mirroring() {
        let mut bus = create_test_bus();

        // PPU registers repeat every 8 bytes
        bus.write(0x2000, 0x80);
        bus.write(0x2008, 0x90);
        bus.write(0x3FF8, 0xA0);

        // All writes should affect $2000 (PPUCTRL)
        // Reading PPUCTRL returns write-only value (implementation dependent)
    }

    #[test]
    fn test_controller_strobe() {
        let mut bus = create_test_bus();

        // Set controller state
        bus.controller1.set_button(Button::A, true);
        bus.controller1.set_button(Button::Start, true);

        // Strobe high loads shift register
        bus.write(0x4016, 0x01);
        bus.write(0x4016, 0x00);

        // Read 8 bits
        assert_eq!(bus.read(0x4016) & 0x01, 0x01); // A
        assert_eq!(bus.read(0x4016) & 0x01, 0x00); // B
        assert_eq!(bus.read(0x4016) & 0x01, 0x00); // Select
        assert_eq!(bus.read(0x4016) & 0x01, 0x01); // Start
    }

    #[test]
    fn test_oam_dma() {
        let mut bus = create_test_bus();

        // Write test pattern to RAM page $02
        for i in 0..256 {
            bus.write(0x0200 + i, i as u8);
        }

        // Trigger OAM DMA
        bus.write(0x4014, 0x02);

        // Execute DMA
        let mut cycles = 0;
        while bus.tick_dma(cycles) {
            cycles += 1;
        }

        // Should take 513 or 514 cycles
        assert!(cycles == 513 || cycles == 514);

        // Verify OAM data transferred
        for i in 0..256 {
            assert_eq!(bus.ppu.read_oam(i), i as u8);
        }
    }
}
```

---

## Dependencies

**Required:**

- rustynes-cpu (CPU subsystem)
- rustynes-ppu (PPU subsystem)
- rustynes-apu (APU subsystem)
- rustynes-mappers (Mapper trait)
- log = "0.4" (logging)

**Blocks:**

- Sprint 5.3: Console Coordinator (needs Bus implementation)
- All subsequent integration work

---

## Related Documentation

- [Memory Map](../../../docs/bus/MEMORY_MAP.md)
- [Bus Architecture](../../../docs/bus/BUS_ARCHITECTURE.md)
- [Bus Conflicts](../../../docs/bus/BUS_CONFLICTS.md)
- [PPU Registers](../../../docs/ppu/PPU_REGISTERS.md)
- [APU Registers](../../../docs/apu/APU_REGISTERS.md)
- [Controller Input](../../../docs/input/CONTROLLER.md)

---

## Technical Notes

### Memory Mirroring

**Internal RAM**: 2KB physical RAM mirrored 4 times to fill 8KB space

- Implementation: `addr & 0x07FF`

**PPU Registers**: 8 registers mirrored throughout 8KB range

- Implementation: `0x2000 + (addr & 0x0007)`

### DMA Cycle Timing

**OAM DMA**:

- 513 cycles if started on odd CPU cycle
- 514 cycles if started on even CPU cycle
- CPU is halted during DMA

**DMC DMA**:

- 4 cycles per sample fetch
- CPU stalled but continues instruction execution
- Conflicts with OAM DMA (OAM takes priority)

### Open Bus Behavior

Reads from unmapped addresses return the last value on the data bus. For simplicity, emulators often return 0 or ignore this behavior unless accuracy is critical.

### Interrupt Priority

1. **RESET** (highest priority, cannot be masked)
2. **NMI** (non-maskable, from PPU VBlank)
3. **IRQ** (maskable with I flag, from APU/Mapper)

---

## Performance Targets

- **Bus read**: <10 ns per access
- **Bus write**: <10 ns per access
- **OAM DMA**: <1 μs for 256-byte transfer
- **Memory overhead**: <10 KB (2KB RAM + state)

---

## Success Criteria

- [ ] All CPU address space regions routed correctly
- [ ] RAM mirroring works ($0000-$1FFF)
- [ ] PPU register mirroring works ($2000-$3FFF)
- [ ] APU registers accessible
- [ ] Controller input functional
- [ ] OAM DMA transfers 256 bytes correctly
- [ ] OAM DMA cycle timing accurate (513/514 cycles)
- [ ] DMC DMA implemented
- [ ] Interrupts poll correctly (NMI, IRQ)
- [ ] All unit tests pass
- [ ] Zero unsafe code
- [ ] Documentation complete

---

**Next Sprint:** [Sprint 5.3: Console Coordinator](M5-S3-console-coordinator.md)
