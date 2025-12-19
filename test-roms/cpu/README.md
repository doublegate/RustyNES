# CPU Test ROMs

This directory contains test ROMs for validating CPU (6502/2A03) implementation.

## Test ROM Sources

The primary test ROM is the legendary nestest.nes:

- **Primary Source**: [christopherpow/nes-test-roms](https://github.com/christopherpow/nes-test-roms)
- **Original Author**: Kevin Horton (kevtris)

## Available Test ROMs

### nestest.nes

The gold standard for 6502 CPU validation. Tests all official opcodes and many unofficial opcodes.

- **Source**: <https://github.com/christopherpow/nes-test-roms/tree/master/other>
- **Author**: Kevin Horton (kevtris)
- **Documentation**: <http://www.qmtpro.com/~nes/misc/nestest.txt>

Features tested:

- All 151 official 6502 opcodes
- 105 unofficial/undocumented opcodes
- All 13 addressing modes
- Flag behavior (N, Z, C, V, I, D, B)
- Stack operations
- Interrupt handling (BRK)
- Page-crossing penalties

### nestest.log

Golden log file for automated validation. Contains expected CPU state after each instruction execution.

- **Format**: `PC    OPCODE OPERANDS    A:XX X:XX Y:XX P:XX SP:XX CYC:XXXXX`
- **Lines**: 5003+ instructions
- **Usage**: Compare emulator trace output against this log

## Test Modes

nestest.nes supports two execution modes:

### Interactive Mode (Normal Boot)

- CPU starts at reset vector (reads from $FFFC-$FFFD)
- Displays on-screen test results
- Requires PPU implementation

### Automated Mode (Direct Entry)

- CPU starts directly at $C000
- No PPU required
- Results written to memory addresses
- **This is the mode used for validation**

To run in automated mode, set PC = $C000 before execution.

## Test Results Format

Results are written to specific memory addresses:

| Address | Meaning |
|---------|---------|
| $0002 | Official opcode test result (0x00 = pass) |
| $0003 | Unofficial opcode test result (0x00 = pass) |

### Error Codes

Non-zero values indicate which test failed:

- **$0002 = 0x00**: All official opcodes passed
- **$0002 != 0x00**: Official opcode test failed at error code
- **$0003 = 0x00**: All unofficial opcodes passed
- **$0003 != 0x00**: Unofficial opcode test failed at error code

## Running Tests

```bash
# Run nestest validation
cargo test -p rustynes-cpu --test nestest_validation

# Run with verbose output
cargo test -p rustynes-cpu --test nestest_validation -- --nocapture

# Run all CPU tests
cargo test -p rustynes-cpu
```

## Current Status

| Test | Status | Notes |
|------|--------|-------|
| nestest.nes (official) | PASSED | 100% golden log match |
| nestest.nes (unofficial) | PASSED | All 105 unofficial opcodes |
| Golden log validation | PASSED | 5003+ instructions verified |

### Achievements

- 100% golden log match (cycle-accurate)
- All 256 opcodes implemented (151 official + 105 unofficial)
- All addressing modes validated
- Interrupt handling verified
- Flag behavior verified

## Additional Test ROMs to Download

For comprehensive CPU validation, download these additional test suites:

### Blargg CPU Tests

```bash
cd test-roms/cpu
# Individual instruction tests
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/instr_test-v3/official_only.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/instr_test-v3/all_instrs.nes

# ROM singles
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/instr_test-v3/rom_singles/01-implied.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/instr_test-v3/rom_singles/02-immediate.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/instr_test-v3/rom_singles/03-zero_page.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/instr_test-v3/rom_singles/04-zp_xy.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/instr_test-v3/rom_singles/05-absolute.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/instr_test-v3/rom_singles/06-abs_xy.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/instr_test-v3/rom_singles/07-ind_x.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/instr_test-v3/rom_singles/08-ind_y.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/instr_test-v3/rom_singles/09-branches.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/instr_test-v3/rom_singles/10-stack.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/instr_test-v3/rom_singles/11-special.nes
```

### CPU Timing Tests

```bash
cd test-roms/cpu
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/instr_timing/instr_timing.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/instr_timing/rom_singles/1-instr_timing.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/instr_timing/rom_singles/2-branch_timing.nes
```

### CPU Misc Tests

```bash
cd test-roms/cpu
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/cpu_interrupts_v2/cpu_interrupts.nes
curl -L -O https://github.com/christopherpow/nes-test-roms/raw/master/cpu_reset/registers.nes
```

## Trace Log Format

The nestest golden log uses this format:

```text
C000  4C F5 C5  JMP $C5F5                       A:00 X:00 Y:00 P:24 SP:FD CYC:7
```

Fields:

- `C000`: Program Counter
- `4C F5 C5`: Raw opcode bytes
- `JMP $C5F5`: Disassembled instruction
- `A:00`: Accumulator
- `X:00`: X register
- `Y:00`: Y register
- `P:24`: Status register (flags)
- `SP:FD`: Stack pointer
- `CYC:7`: Cycle count

## References

- [NESdev Wiki: CPU](https://www.nesdev.org/wiki/CPU)
- [NESdev Wiki: CPU Status Flags](https://www.nesdev.org/wiki/Status_flags)
- [NESdev Wiki: Emulator Tests](https://www.nesdev.org/wiki/Emulator_tests)
- [6502 Instruction Reference](https://www.nesdev.org/obelisk-6502-guide/)
- [Unofficial Opcodes](https://www.nesdev.org/undocumented_opcodes.txt)
- [nes-test-roms Repository](https://github.com/christopherpow/nes-test-roms)

## License

Test ROMs are created by their respective authors:

- nestest.nes: Created by Kevin Horton (kevtris)

All test ROMs are used for educational and emulator development purposes.
