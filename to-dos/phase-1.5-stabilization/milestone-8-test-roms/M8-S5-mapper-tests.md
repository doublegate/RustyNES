# M8 Sprint 5: Mapper Tests

## Overview

Systematically validate mapper implementations (NROM, MMC1, UxROM, CNROM, MMC3) using Holy Mapperel test suite and mapper-specific tests to achieve 95%+ mapper compatibility.

## Objectives

- [ ] Pass 54/57 mapper tests (95%)
- [ ] Validate NROM (0), MMC1 (1), UxROM (2), CNROM (3), MMC3 (4)
- [ ] Test bank switching edge cases
- [ ] Verify MMC3 IRQ timing
- [ ] Ensure mirroring modes correct
- [ ] Complete v0.7.0 release

## Tasks

### Task 1: Holy Mapperel Suite (40 tests)
- [ ] Download Holy Mapperel test suite (comprehensive mapper tests)
- [ ] Run all 40 Holy Mapperel tests
- [ ] Debug mapper-specific failures
- [ ] Verify bank switching behavior
- [ ] Test PRG/CHR RAM behavior
- [ ] Validate mirroring modes (horizontal, vertical, single-screen, four-screen)

### Task 2: NROM (Mapper 0) - 3 tests
- [ ] Test NROM-128 (16KB PRG-ROM)
- [ ] Test NROM-256 (32KB PRG-ROM)
- [ ] Verify mirroring (horizontal/vertical)
- [ ] Validate no bank switching
- [ ] Test with Super Mario Bros., Donkey Kong, Balloon Fight

### Task 3: MMC1 (Mapper 1) - 5 tests
- [ ] Test PRG bank switching (16KB/32KB modes)
- [ ] Test CHR bank switching (4KB/8KB modes)
- [ ] Verify shift register write behavior ($8000-$FFFF)
- [ ] Test mirroring control (horizontal, vertical, single-screen)
- [ ] Validate PRG RAM enable/disable
- [ ] Test with Zelda, Metroid, Mega Man 2

### Task 4: UxROM (Mapper 2) - 3 tests
- [ ] Test PRG bank switching (16KB switchable + 16KB fixed)
- [ ] Verify bus conflicts (write to PRG-ROM)
- [ ] Test fixed bank behavior (last bank)
- [ ] Validate with Mega Man, Castlevania, Contra

### Task 5: CNROM (Mapper 3) - 2 tests
- [ ] Test CHR bank switching (8KB banks)
- [ ] Verify bus conflicts (write to PRG-ROM)
- [ ] Test with Arkanoid, Solomon's Key, Gradius

### Task 6: MMC3 (Mapper 4) - 4 tests
- [ ] Test PRG bank switching (8KB/16KB modes)
- [ ] Test CHR bank switching (2KB/1KB banks)
- [ ] Verify MMC3 IRQ counter (scanline counter)
- [ ] Test IRQ timing precision
- [ ] Validate mirroring control (horizontal/vertical)
- [ ] Test with Super Mario Bros. 3, Mega Man 3-6, Kirby's Adventure

### Task 7: Integration Testing
- [ ] Run full mapper test suite (57 tests)
- [ ] Verify no regressions in CPU/PPU/APU tests
- [ ] Test complex games (Super Mario Bros. 3, Zelda, Mega Man)
- [ ] Validate save state compatibility
- [ ] Benchmark performance impact

## Test ROMs

| ROM | Status | Mapper | Notes |
|-----|--------|--------|-------|
| holy_mapperel_*.nes | [ ] Pending | Various | Comprehensive suite (40 tests) |
| mapper_000_nrom.nes | [ ] Pending | 0 | NROM basic test |
| mapper_001_mmc1.nes | [ ] Pending | 1 | MMC1 bank switching |
| mapper_001_mmc1_mirroring.nes | [ ] Pending | 1 | MMC1 mirroring |
| mapper_001_mmc1_shift_register.nes | [ ] Pending | 1 | MMC1 shift register |
| mapper_002_uxrom.nes | [ ] Pending | 2 | UxROM bank switching |
| mapper_002_uxrom_bus_conflicts.nes | [ ] Pending | 2 | UxROM bus conflicts |
| mapper_003_cnrom.nes | [ ] Pending | 3 | CNROM CHR switching |
| mapper_004_mmc3.nes | [ ] Pending | 4 | MMC3 comprehensive |
| mapper_004_mmc3_irq.nes | [ ] Pending | 4 | MMC3 IRQ timing |
| mapper_004_mmc3_prg_switching.nes | [ ] Pending | 4 | MMC3 PRG switching |
| mapper_004_mmc3_chr_switching.nes | [ ] Pending | 4 | MMC3 CHR switching |

**Additional Mapper Tests (12 ROMs):**
- Bus conflicts tests
- Mirroring mode tests
- Bank switching edge cases
- PRG/CHR RAM tests

## Acceptance Criteria

- [ ] 54/57 mapper tests passing (95%)
- [ ] NROM (0) validated (3/3 tests)
- [ ] MMC1 (1) validated (5/5 tests)
- [ ] UxROM (2) validated (3/3 tests)
- [ ] CNROM (3) validated (2/2 tests)
- [ ] MMC3 (4) validated (4/4 tests)
- [ ] Holy Mapperel suite passing (37/40 tests)
- [ ] Zero regressions in CPU/PPU/APU tests
- [ ] Complex games working (SMB3, Zelda, Mega Man)
- [ ] v0.7.0 release complete

## Expected Failures (3 tests)

**Rare Mapper Variants:**
- holy_mapperel_mapper_015.nes - Mapper 15 (100-in-1) - Not in Phase 1.5 scope
- holy_mapperel_mapper_019.nes - Mapper 19 (Namco 163) - Requires expansion audio
- holy_mapperel_mapper_024.nes - Mapper 24 (VRC6) - Requires expansion audio

**Rationale:** These mappers represent rare variants (<1% of NES library) and/or require expansion audio implementation deferred to Phase 2.

## Mapper Implementation Reference

### NROM (Mapper 0)
```rust
// No bank switching
// PRG-ROM: 16KB or 32KB (mirrored if 16KB)
// CHR-ROM: 8KB (or CHR-RAM)
// Mirroring: Fixed by iNES header
```

### MMC1 (Mapper 1)
```rust
// Shift register: 5 writes to $8000-$FFFF
// PRG: 16KB/32KB modes, switchable/fixed banks
// CHR: 4KB/8KB modes
// Mirroring: Controllable (H/V/single-screen)
// PRG RAM: $6000-$7FFF (8KB), enable/disable
```

### UxROM (Mapper 2)
```rust
// PRG: 16KB switchable ($8000-$BFFF) + 16KB fixed ($C000-$FFFF)
// CHR: 8KB CHR-RAM (not switchable)
// Mirroring: Fixed by iNES header
// Bus conflicts: Write value must match ROM value
```

### CNROM (Mapper 3)
```rust
// PRG: 16KB/32KB (no switching)
// CHR: 8KB switchable banks
// Mirroring: Fixed by iNES header
// Bus conflicts: Write value must match ROM value
```

### MMC3 (Mapper 4)
```rust
// PRG: 8KB/16KB banks, switchable/fixed
// CHR: 2KB/1KB banks (6 switchable banks)
// IRQ: Scanline counter (A12 rising edge detection)
// Mirroring: Controllable (H/V)
// PRG RAM: $6000-$7FFF, write protect
```

## Debugging Strategy

1. **Identify Mapper Failure:**
   - Run test ROM, note which mapper failing
   - Review mapper implementation

2. **Isolate Behavior:**
   - Determine bank switching issue vs mirroring vs IRQ
   - Check register writes and reads

3. **Trace Execution:**
   - Enable mapper trace logging
   - Log bank switches, register writes, IRQ triggers

4. **Fix & Verify:**
   - Implement fix
   - Test with known working games
   - Run full mapper test suite

## Game Compatibility Testing

| Game | Mapper | Test |
|------|--------|------|
| Super Mario Bros. | 0 | Basic gameplay, scrolling |
| Donkey Kong | 0 | Graphics, collision |
| Zelda | 1 | Save states, bank switching |
| Metroid | 1 | Scrolling, large ROM |
| Mega Man | 2 | Bank switching, boss fights |
| Castlevania | 2 | Scrolling, stages |
| Arkanoid | 3 | CHR switching, graphics |
| Super Mario Bros. 3 | 4 | IRQ timing, scrolling, status bar |
| Mega Man 3 | 4 | Bank switching, weapons |
| Kirby's Adventure | 4 | Large ROM, complex graphics |

## Version Target

v0.7.0 (Final Release for M8)

## Release Checklist

- [ ] All 5 mapper tests complete (54/57 passing)
- [ ] CPU/PPU/APU tests regression-free
- [ ] Overall test pass rate: 202/212 (95%+)
- [ ] Performance benchmarks acceptable (<5% regression)
- [ ] Documentation updated (CHANGELOG, README, ROADMAP)
- [ ] Version bumped to v0.7.0
- [ ] Git tag created
- [ ] GitHub release published
