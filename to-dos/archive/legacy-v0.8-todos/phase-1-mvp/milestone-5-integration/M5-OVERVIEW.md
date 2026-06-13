# Milestone 5: Core Integration

**Status:** ✅ COMPLETED
**Started:** December 19, 2025
**Completed:** December 19, 2025
**Duration:** 1 day (accelerated from 3-4 weeks estimate)
**Progress:** 100%

---

## Overview

Milestone 5 will **integrate all emulation components** (CPU, PPU, APU, Mappers) into a cohesive emulator core. This establishes the complete emulation engine ready for frontends.

### Goals

- ⏳ Bus system connecting all components
- ⏳ Console master coordinator
- ⏳ ROM loading (iNES, NES 2.0)
- ⏳ Save state system
- ⏳ Input handling (controllers, Zapper)
- ⏳ Master clock synchronization
- ⏳ DMA handling (OAM DMA, DMC DMA)
- ⏳ Zero unsafe code
- ⏳ First game playable end-to-end

---

## Sprint Breakdown

### Sprint 1: Bus & Memory Routing ⏳ PENDING

**Duration:** Week 1
**Target Files:** `crates/rustynes-core/src/bus.rs`, `memory.rs`

**Goals:**

- [ ] Bus trait implementation
- [ ] Memory map ($0000-$FFFF CPU, $0000-$3FFF PPU)
- [ ] RAM (2KB, mirrored to $2000)
- [ ] PPU register routing ($2000-$3FFF)
- [ ] APU register routing ($4000-$4017)
- [ ] Controller register routing ($4016-$4017)
- [ ] Mapper integration ($4020-$FFFF)

**Outcome:** Complete memory routing system.

### Sprint 2: Console Coordinator ⏳ PENDING

**Duration:** Week 1-2
**Target Files:** `crates/rustynes-core/src/console.rs`

**Goals:**

- [ ] Console struct integrating all components
- [ ] Master clock (21.477272 MHz NTSC)
- [ ] CPU/PPU/APU synchronization
- [ ] Frame execution (step_frame method)
- [ ] NMI/IRQ delivery
- [ ] OAM DMA handling (513/514 cycle stall)
- [ ] DMC DMA handling (conflicts with CPU)
- [ ] Power-on and reset

**Outcome:** Coordinated component execution.

### Sprint 3: ROM Loading ⏳ PENDING

**Duration:** Week 2
**Target Files:** `crates/rustynes-core/src/rom_loader.rs`

**Goals:**

- [ ] Load ROM from file path
- [ ] Load ROM from bytes
- [ ] iNES format validation
- [ ] NES 2.0 detection
- [ ] Mapper creation
- [ ] Battery-backed SRAM loading
- [ ] Error handling (invalid ROMs, unsupported mappers)

**Outcome:** Robust ROM loading system.

### Sprint 4: Save States ⏳ PENDING

**Duration:** Week 3
**Target Files:** `crates/rustynes-core/src/save_state.rs`

**Goals:**

- [ ] Serialize all emulator state
- [ ] Deserialize and restore state
- [ ] Version compatibility
- [ ] Save to file
- [ ] Load from file
- [ ] Slot management (multiple save states)
- [ ] Deterministic execution after load

**Outcome:** Full save state system.

### Sprint 5: Input Handling ⏳ PENDING

**Duration:** Week 3-4
**Target Files:** `crates/rustynes-core/src/input.rs`

**Goals:**

- [ ] Controller 1 & 2 registers ($4016, $4017)
- [ ] Button state tracking (A, B, Select, Start, Up, Down, Left, Right)
- [ ] Strobe signal
- [ ] Read sequence (8 buttons + open bus)
- [ ] Zapper (light gun) support
- [ ] Input API for frontends

**Outcome:** Complete input system.

---

## Technical Requirements

### Bus Implementation

```rust
pub struct Bus {
    ram: [u8; 2048],          // 2KB RAM
    ppu: Ppu,
    apu: Apu,
    cartridge: Box<dyn Mapper>,
    controller1: Controller,
    controller2: Controller,
}

impl Bus {
    pub fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],  // Mirrored RAM
            0x2000..=0x3FFF => self.ppu.read_register(addr),        // PPU (mirrored)
            0x4000..=0x4015 => self.apu.read_register(addr),        // APU
            0x4016 => self.controller1.read(),                       // Controller 1
            0x4017 => self.controller2.read(),                       // Controller 2
            0x4020..=0xFFFF => self.cartridge.read_prg(addr),       // Mapper
            _ => 0, // Open bus
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize] = value,
            0x2000..=0x3FFF => self.ppu.write_register(addr, value),
            0x4000..=0x4013 => self.apu.write_register(addr, value),
            0x4014 => self.oam_dma(value),  // OAM DMA
            0x4015 => self.apu.write_register(addr, value),
            0x4016 => self.controller1.write(value),  // Strobe
            0x4017 => self.apu.write_register(addr, value),  // APU frame counter
            0x4020..=0xFFFF => self.cartridge.write_prg(addr, value),
            _ => {}
        }
    }
}
```

### Console Structure

```rust
pub struct Console {
    cpu: Cpu,
    bus: Bus,
    master_clock: u64,
    frame_count: u64,
}

impl Console {
    pub fn new(rom: Rom) -> Result<Self, EmulatorError> {
        let mapper = create_mapper(rom)?;
        let mirroring = mapper.mirroring();

        Ok(Self {
            cpu: Cpu::new(),
            bus: Bus::new(Ppu::new(mirroring), Apu::new(), mapper),
            master_clock: 0,
            frame_count: 0,
        })
    }

    pub fn step_frame(&mut self) {
        // Run until next frame complete
        loop {
            // CPU runs at master / 12
            if self.master_clock % 12 == 0 {
                self.cpu.step(&mut self.bus);
            }

            // PPU runs at master / 4 (3 dots per CPU cycle)
            if self.master_clock % 4 == 0 {
                let nmi = self.bus.ppu.tick();
                if nmi {
                    self.cpu.trigger_nmi(&mut self.bus);
                }
            }

            // APU runs at master / 12 (same as CPU)
            if self.master_clock % 12 == 0 {
                self.bus.apu.tick();
            }

            self.master_clock += 1;

            // Check for frame completion (89,342 PPU dots)
            if self.bus.ppu.frame_complete() {
                self.frame_count += 1;
                break;
            }
        }
    }

    pub fn reset(&mut self) {
        self.cpu.reset(&mut self.bus);
        self.bus.ppu.reset();
        self.bus.apu.reset();
    }
}
```

### Save State Format

```rust
#[derive(Serialize, Deserialize)]
pub struct SaveState {
    version: u32,
    cpu: CpuState,
    ppu: PpuState,
    apu: ApuState,
    ram: [u8; 2048],
    mapper_state: Vec<u8>,  // Mapper-specific serialization
    frame_count: u64,
}

impl Console {
    pub fn save_state(&self) -> Vec<u8> {
        let state = SaveState {
            version: 1,
            cpu: self.cpu.save_state(),
            ppu: self.bus.ppu.save_state(),
            apu: self.bus.apu.save_state(),
            ram: self.bus.ram,
            mapper_state: self.bus.cartridge.save_state(),
            frame_count: self.frame_count,
        };

        bincode::serialize(&state).unwrap()
    }

    pub fn load_state(&mut self, data: &[u8]) -> Result<(), EmulatorError> {
        let state: SaveState = bincode::deserialize(data)?;

        self.cpu.load_state(&state.cpu);
        self.bus.ppu.load_state(&state.ppu);
        self.bus.apu.load_state(&state.apu);
        self.bus.ram = state.ram;
        self.bus.cartridge.load_state(&state.mapper_state)?;
        self.frame_count = state.frame_count;

        Ok(())
    }
}
```

---

## Acceptance Criteria

### Functionality

- [ ] Console initializes with ROM
- [ ] Frame execution completes in correct time
- [ ] NMI/IRQ delivered correctly
- [ ] OAM DMA stalls CPU
- [ ] DMC DMA conflicts with CPU
- [ ] Input responds to controller presses
- [ ] Save state captures all state
- [ ] Load state restores deterministically

### First Playable Game

- [ ] Super Mario Bros. loads
- [ ] Title screen displays correctly
- [ ] Controller input works (can navigate menus)
- [ ] Gameplay works (can play level 1-1)
- [ ] Audio plays correctly
- [ ] Can save and load state mid-game

### Integration Tests

- [ ] CPU + PPU timing synchronization
- [ ] PPU NMI triggers CPU interrupt
- [ ] APU generates audio samples
- [ ] Mapper banking works across components
- [ ] DMA cycles counted correctly

---

## Code Structure

```text
crates/rustynes-core/
├── src/
│   ├── lib.rs           # Public API
│   ├── console.rs       # Console coordinator
│   ├── bus.rs           # Bus and memory routing
│   ├── rom_loader.rs    # ROM loading
│   ├── save_state.rs    # Save state serialization
│   ├── input.rs         # Controller/Zapper input
│   ├── timing.rs        # Clock synchronization
│   └── error.rs         # Error types
├── tests/
│   └── integration_tests.rs
└── Cargo.toml
```

**Estimated Total:** ~1,500-2,000 lines of code

---

## Dependencies

### External Crates

- **bincode** - Save state serialization
- **serde** - Serialization framework
- **thiserror** - Error handling
- **log** - Logging

### Internal Dependencies

- rustynes-cpu
- rustynes-ppu
- rustynes-apu
- rustynes-mappers

---

## Testing Strategy

### Unit Tests

- [ ] Bus read/write routing
- [ ] Memory mirroring
- [ ] Controller read sequence
- [ ] Save state serialization/deserialization

### Integration Tests

- [ ] Load and run test ROM
- [ ] Execute 1000 frames
- [ ] Save and restore state
- [ ] Controller input injection
- [ ] Audio buffer collection

---

## Performance Targets

- **Frame Time:** <17ms (60 FPS)
- **Memory:** <50 MB
- **Save State:** <100 KB
- **ROM Loading:** <10ms

---

## Challenges & Risks

| Challenge | Risk | Mitigation |
|-----------|------|------------|
| Timing synchronization | High | Use master clock, test with timing ROMs |
| DMA conflicts | Medium | Study NesDev Wiki, test with DMC games |
| Save state completeness | Medium | Comprehensive testing, determinism checks |
| Component coupling | Low | Clean interfaces, trait abstractions |

---

## Related Documentation

- [Core API](../../../docs/api/CORE_API.md)
- [Bus Architecture](../../../docs/bus/BUS_OVERVIEW.md)
- [Memory Map](../../../docs/bus/MEMORY_MAP.md)
- [Save States](../../../docs/api/SAVE_STATES.md)
- [Input Handling](../../../docs/input/CONTROLLER.md)

---

## Next Steps

### Pre-Sprint Preparation

1. **Review Component APIs**
   - CPU, PPU, APU public interfaces
   - Mapper trait
   - Identify integration points

2. **Set Up Crate**
   - Create rustynes-core/Cargo.toml
   - Add all component dependencies
   - Set up integration test framework

3. **Timing Research**
   - Study master clock timing
   - Review DMA cycle stealing
   - Understand IRQ priority

### Sprint 1 Kickoff

- Implement Bus struct
- Create memory map
- Test component communication
- Begin Console struct

---

**Milestone Status:** ⏳ PENDING
**Blocked By:** M1 ✅, M2 ✅, M3 ⏳, M4 ⏳ (needs at least one mapper)
**Next Milestone:** [Milestone 6: Desktop GUI](../milestone-6-gui/M6-OVERVIEW.md)
