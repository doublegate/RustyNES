# M7 Sprint 4: Timing & Synchronization

## Overview

Ensure precise bus timing and CPU/PPU/APU synchronization for integrated system accuracy.

## Objectives

- [x] Achieve exact OAM DMA timing (determine 513 vs 514 cycles) ✅
- [x] Verify CPU/PPU synchronization precision ✅
- [ ] Test bus timing and conflicts
- [ ] Validate memory access timing
- [ ] Integration testing across all subsystems

## Tasks

### Task 1: OAM DMA Precision ✅ COMPLETE
- [x] Study hardware OAM DMA timing (513 or 514 cycles depending on alignment) - **NESdev research**
- [x] Implement precise cycle counting (odd/even cycle start) - **CPU cycle parity tracking**
- [x] Implement 513 vs 514 cycle detection - **Even cycle: 1 dummy + 512 = 513, Odd cycle: 2 dummy + 512 = 514**
- [ ] Test with cpu_sprdma_and_dmc_dma.nes (test ROM needed)
- [ ] Verify DMA conflicts with DMC (deferred to M8)
- [ ] Test OAM DMA during various PPU states (deferred to M8)

### Task 2: CPU/PPU Synchronization ✅ VERIFIED
- [x] Verify PPU runs 3 dots per CPU cycle - **console.rs implements `for _ in 0..(cpu_cycles * 3)`**
- [x] Test CPU cycle stealing (OAM DMA, DMC DMA) - **OAM DMA complete, DMC documented**
- [x] Validate PPU register read/write timing ($2002, $2004, $2007) - **Existing implementation**
- [ ] Test PPU writes during rendering (deferred to M8)
- [ ] Handle edge cases (mid-scanline register access) (deferred to M8)

### Task 3: Bus Timing & Conflicts
- [ ] Test open bus behavior ($2000-$2007, $4000-$4017)
- [ ] Verify bus conflicts (older mappers)
- [ ] Test memory access timing (zero page, absolute, etc.)
- [ ] Validate stack operations timing
- [ ] Test PPU memory access timing

### Task 4: Integration Testing
- [ ] Run comprehensive test ROM suite
- [ ] Test with complex games (Super Mario Bros., Zelda, Mega Man)
- [ ] Verify audio/video sync in real gameplay
- [ ] Profile performance impact
- [ ] Benchmark all subsystems

## Test ROMs

| ROM | Status | Notes |
|-----|--------|-------|
| cpu_sprdma_and_dmc_dma.nes | [ ] Pending | OAM DMA + DMC DMA conflicts |
| cpu_sprdma_and_dmc_dma_512.nes | [ ] Pending | Variant test |
| ppu_open_bus.nes | [ ] Pending | Open bus behavior |
| cpu_exec_space_ppuio.nes | [ ] Pending | PPU I/O timing |
| cpu_dummy_writes_oam.nes | [ ] Pending | OAM write timing |

## Acceptance Criteria

- [ ] OAM DMA timing exact (513/514 cycles determined)
- [ ] CPU/PPU synchronization verified (3:1 ratio precise)
- [ ] Bus timing tests pass
- [ ] cpu_sprdma_and_dmc_dma.nes passes
- [ ] Integration tests show no regressions
- [ ] Performance impact <5%
- [ ] Ready for v0.6.0 release

## Version Target

v0.6.0
