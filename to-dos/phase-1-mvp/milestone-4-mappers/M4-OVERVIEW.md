# Milestone 4: Mapper Implementation

**Status:** ⏳ PENDING
**Started:** TBD
**Completed:** TBD
**Duration:** ~4-6 weeks (estimated)
**Progress:** 0%

---

## Overview

Milestone 4 will deliver **mapper infrastructure and 5 essential mappers** covering 77.7% of licensed NES games. This establishes the foundation for cartridge emulation and game compatibility.

### Goals

- ⏳ Mapper trait and infrastructure
- ⏳ iNES and NES 2.0 ROM format parsing
- ⏳ Mapper 0 (NROM) - 9.5% of games
- ⏳ Mapper 1 (MMC1/SxROM) - 27.9% of games
- ⏳ Mapper 2 (UxROM) - 10.6% of games
- ⏳ Mapper 3 (CNROM) - 6.3% of games
- ⏳ Mapper 4 (MMC3/TxROM) - 23.4% of games
- ⏳ Battery-backed SRAM support
- ⏳ Zero unsafe code
- ⏳ Comprehensive tests for each mapper

---

## Sprint Breakdown

### Sprint 1: Mapper Infrastructure ⏳ PENDING

**Duration:** Week 1-2
**Target Files:** `crates/rustynes-mappers/src/mapper.rs`, `rom.rs`

**Goals:**

- [ ] Mapper trait definition
- [ ] iNES header parsing (16-byte header)
- [ ] NES 2.0 header parsing (extended format)
- [ ] ROM loading from bytes
- [ ] Mirroring mode detection
- [ ] Mapper factory (create mapper from ROM)
- [ ] Battery-backed SRAM interface

**Outcome:** Infrastructure for all mappers.

### Sprint 2: Mapper 0 (NROM) & Mapper 2 (UxROM) ⏳ PENDING

**Duration:** Week 2-3
**Target Files:** `crates/rustynes-mappers/src/mapper000.rs`, `mapper002.rs`

**Goals:**

- [ ] Mapper 0: No banking, simple passthrough
- [ ] Mapper 0: 16KB or 32KB PRG-ROM
- [ ] Mapper 0: 8KB CHR-ROM or CHR-RAM
- [ ] Mapper 2: 16KB switchable + 16KB fixed PRG
- [ ] Mapper 2: 8KB CHR-RAM (no banking)
- [ ] Test with Super Mario Bros., Mega Man

**Outcome:** Simplest mappers working.

### Sprint 3: Mapper 1 (MMC1) ⏳ PENDING

**Duration:** Week 3-4
**Target Files:** `crates/rustynes-mappers/src/mapper001.rs`

**Goals:**

- [ ] 5-bit shift register write mechanism
- [ ] 4 internal registers (control, CHR0, CHR1, PRG)
- [ ] Switchable 16KB or 32KB PRG banking
- [ ] Switchable 4KB or 8KB CHR banking
- [ ] Dynamic mirroring control
- [ ] Test with Legend of Zelda, Metroid

**Outcome:** MMC1 fully functional (27.9% game coverage).

### Sprint 4: Mapper 3 (CNROM) ⏳ PENDING

**Duration:** Week 4
**Target Files:** `crates/rustynes-mappers/src/mapper003.rs`

**Goals:**

- [ ] Simple CHR banking only
- [ ] 8KB CHR bank switching
- [ ] Fixed 16KB or 32KB PRG-ROM
- [ ] Test with Arkanoid, Solomon's Key

**Outcome:** CNROM working (6.3% additional coverage).

### Sprint 5: Mapper 4 (MMC3) ⏳ PENDING

**Duration:** Week 5-6
**Target Files:** `crates/rustynes-mappers/src/mapper004.rs`

**Goals:**

- [ ] 8 internal registers (bank select + 6 data registers)
- [ ] 8KB/8KB/8KB/8KB PRG banking (configurable)
- [ ] 2KB/2KB/1KB/1KB/1KB/1KB CHR banking
- [ ] Scanline counter IRQ
- [ ] A12 edge detection
- [ ] Mirroring control
- [ ] Battery-backed RAM
- [ ] Test with Super Mario Bros. 3, Mega Man 3

**Outcome:** MMC3 working (23.4% additional coverage, 77.7% total).

---

## Technical Requirements

### Mapper Trait

```rust
pub trait Mapper: Send {
    /// Read from PRG-ROM address space ($8000-$FFFF)
    fn read_prg(&self, addr: u16) -> u8;

    /// Write to PRG address space (for mapper registers)
    fn write_prg(&mut self, addr: u16, value: u8);

    /// Read from CHR address space ($0000-$1FFF)
    fn read_chr(&self, addr: u16) -> u8;

    /// Write to CHR address space (for CHR-RAM)
    fn write_chr(&mut self, addr: u16, value: u8);

    /// Get current mirroring mode
    fn mirroring(&self) -> Mirroring;

    /// Check if IRQ is pending
    fn irq_pending(&self) -> bool { false }

    /// Clear IRQ flag
    fn clear_irq(&mut self) {}

    /// Clock the mapper (for IRQ counters)
    fn clock(&mut self, _cycles: u8) {}

    /// Notify of PPU A12 edge (MMC3 scanline counter)
    fn ppu_a12_edge(&mut self) {}

    /// Get battery-backed SRAM
    fn sram(&self) -> Option<&[u8]> { None }

    /// Get mutable battery-backed SRAM
    fn sram_mut(&mut self) -> Option<&mut [u8]> { None }
}
```

### iNES Header

```rust
pub struct INesHeader {
    pub prg_rom_size: usize,  // In 16KB units
    pub chr_rom_size: usize,  // In 8KB units (0 = CHR-RAM)
    pub mapper_number: u16,
    pub mirroring: Mirroring,
    pub has_battery: bool,
    pub has_trainer: bool,
    pub four_screen: bool,
    pub nes2_format: bool,
}
```

### ROM Structure

```rust
pub struct Rom {
    pub header: INesHeader,
    pub trainer: Option<Vec<u8>>,  // 512 bytes if present
    pub prg_rom: Vec<u8>,
    pub chr_rom: Vec<u8>,           // Empty if CHR-RAM
}
```

---

## Mapper Details

### Mapper 0 (NROM)

**Games:** Super Mario Bros., Donkey Kong, Balloon Fight

**Features:**

- Simplest mapper (no banking)
- 16KB or 32KB PRG-ROM
- 8KB CHR-ROM or CHR-RAM
- Horizontal or vertical mirroring

**Variants:**

- NROM-128: 16KB PRG (mirrored to 32KB)
- NROM-256: 32KB PRG

### Mapper 1 (MMC1/SxROM)

**Games:** Legend of Zelda, Metroid, Final Fantasy, Mega Man 2

**Features:**

- 5-bit shift register (write 5 times to load)
- Switchable 16KB or 32KB PRG banking
- Switchable 4KB or 8KB CHR banking
- Dynamic mirroring control
- Battery-backed SRAM (8KB)

**Registers:**

- Control ($8000-$9FFF): Mirroring, PRG/CHR modes
- CHR bank 0 ($A000-$BFFF): CHR bank selection
- CHR bank 1 ($C000-$DFFF): CHR bank selection
- PRG bank ($E000-$FFFF): PRG bank selection

### Mapper 2 (UxROM)

**Games:** Mega Man, Castlevania, Duck Tales

**Features:**

- 16KB switchable + 16KB fixed PRG
- 8KB CHR-RAM (no banking)
- Simple register (any write to $8000-$FFFF)

**Banking:**

- $8000-$BFFF: Switchable 16KB bank
- $C000-$FFFF: Fixed to last 16KB bank

### Mapper 3 (CNROM)

**Games:** Arkanoid, Solomon's Key, Paperboy

**Features:**

- Fixed 16KB or 32KB PRG-ROM
- 8KB CHR-ROM bank switching
- Simple register (any write to $8000-$FFFF)

**Banking:**

- $0000-$1FFF: Switchable 8KB CHR bank

### Mapper 4 (MMC3/TxROM)

**Games:** Super Mario Bros. 3, Mega Man 3-6, Kirby's Adventure

**Features:**

- Complex banking with 8 registers
- Configurable PRG/CHR banking modes
- Scanline counter IRQ (for split-screen effects)
- Battery-backed SRAM (8KB)
- Mirroring control

**Registers:**

- $8000-$9FFF (even): Bank select
- $8001-$9FFF (odd): Bank data
- $A000-$BFFF (even): Mirroring
- $A001-$BFFF (odd): PRG-RAM protect
- $C000-$DFFF (even): IRQ latch
- $C001-$DFFF (odd): IRQ reload
- $E000-$FFFF (even): IRQ disable
- $E001-$FFFF (odd): IRQ enable

---

## Acceptance Criteria

### Infrastructure

- [ ] Mapper trait compiles and is usable
- [ ] iNES header parsing works for all test ROMs
- [ ] NES 2.0 detection works
- [ ] ROM loader handles malformed headers gracefully
- [ ] Mapper factory creates correct mapper type

### Mappers

- [ ] Mapper 0: Super Mario Bros. playable
- [ ] Mapper 1: Legend of Zelda, Metroid playable
- [ ] Mapper 2: Mega Man, Castlevania playable
- [ ] Mapper 3: Arkanoid playable
- [ ] Mapper 4: Super Mario Bros. 3 playable
- [ ] All mappers pass mapper-specific test ROMs

### Test ROMs

- [ ] holy_mapperel (multi-mapper test)
- [ ] mmc3_test (MMC3 IRQ timing)
- [ ] mapper_test (generic mapper tests)

---

## Code Structure

```text
crates/rustynes-mappers/
├── src/
│   ├── lib.rs           # Public API, Mapper trait
│   ├── mapper.rs        # Mapper trait definition
│   ├── rom.rs           # ROM and iNES header parsing
│   ├── factory.rs       # Mapper factory
│   ├── mapper000.rs     # NROM
│   ├── mapper001.rs     # MMC1
│   ├── mapper002.rs     # UxROM
│   ├── mapper003.rs     # CNROM
│   └── mapper004.rs     # MMC3
├── tests/
│   └── mapper_tests.rs  # Integration tests
└── Cargo.toml
```

**Estimated Total:** ~2,500-3,000 lines of code

---

## Testing Strategy

### Unit Tests

- [ ] iNES header parsing (valid/invalid headers)
- [ ] Bank calculation for each mapper
- [ ] Mirroring mode handling
- [ ] Register writes
- [ ] MMC3 scanline counter

### Integration Tests

- [ ] Load test ROMs
- [ ] Execute code from different banks
- [ ] CHR banking switches correctly
- [ ] IRQ generation (MMC3)

### Game Testing

- [ ] 5 test games per mapper (25 total)
- [ ] Visual inspection (no glitches)
- [ ] Gameplay testing (5 minutes each)
- [ ] Save/load SRAM (battery-backed games)

---

## Performance Targets

- **Bank Switching:** <50 ns per access
- **ROM Loading:** <10ms for 512KB ROM
- **Memory:** <1 MB per ROM loaded

---

## Challenges & Risks

| Challenge | Risk | Mitigation |
|-----------|------|------------|
| MMC1 shift register | Medium | Careful bit-shifting logic, test with Zelda |
| MMC3 scanline counter | High | Study Mesen2, mmc3_test ROM, community docs |
| A12 edge detection | High | PPU-mapper integration testing |
| Bank overflow | Low | Modulo arithmetic for bank calculation |

---

## Related Documentation

- [Mapper Overview](../../docs/mappers/MAPPER_OVERVIEW.md)
- [iNES Format](../../docs/formats/INES_FORMAT.md)
- [NES 2.0 Format](../../docs/formats/NES20_FORMAT.md)
- [Mapper 0 (NROM)](../../docs/mappers/MAPPER_000_NROM.md)
- [Mapper 1 (MMC1)](../../docs/mappers/MAPPER_001_MMC1.md)
- [Mapper 2 (UxROM)](../../docs/mappers/MAPPER_002_UXROM.md)
- [Mapper 3 (CNROM)](../../docs/mappers/MAPPER_003_CNROM.md)
- [Mapper 4 (MMC3)](../../docs/mappers/MAPPER_004_MMC3.md)

---

## Next Steps

### Pre-Sprint Preparation

1. **Review Mapper Documentation**
   - Study mapper specifications
   - Review banking diagrams
   - Understand IRQ timing (MMC3)

2. **Set Up Crate**
   - Create rustynes-mappers/Cargo.toml
   - Add test ROM loader
   - Set up integration test framework

3. **Acquire Test ROMs**
   - Download mapper test ROMs
   - Acquire 5 test games per mapper
   - Set up test game library

### Sprint 1 Kickoff

- Define Mapper trait
- Implement iNES parser
- Create mapper factory
- Begin Mapper 0 implementation

---

**Milestone Status:** ⏳ PENDING
**Blocked By:** None (can start in parallel with APU)
**Next Milestone:** [Milestone 5: Integration](../milestone-5-integration/M5-OVERVIEW.md)
