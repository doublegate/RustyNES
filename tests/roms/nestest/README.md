# `nestest/` — kevtris CPU validation ROM

The canonical NES CPU instruction-validation ROM by kevtris, plus the
matching Nintendulator-generated golden trace log.

## ROMs

| File | Mapper | Author | License |
|------|--------|--------|---------|
| `nestest.nes` | NROM (0) | kevtris | Public domain |
| `nestest.log` | n/a (trace) | kevtris + Nintendulator | Public domain |

Source: <https://www.qmtpro.com/~nes/misc/nestest.txt> (canonical) and
the `other/nestest.nes` entry in `christopherpow/nes-test-roms`.

## What it tests

Runs every 6502 opcode (official + unofficial) plus a small set of
common bug patterns. The matching `nestest.log` is a 26 character
line-per-instruction trace from Nintendulator. Our integration test
at `crates/nes-test-harness/tests/nestest.rs` runs the ROM in headless
mode and compares the CPU trace produced by `nes-cpu::Cpu::tick`
line-by-line against the golden log.

The current zero-diff target is **8,991 instructions** (the entire
official-instruction prefix of the test). The full ROM extends to
9,952 instructions when the unofficial-instruction tests are
executed, but the standard test target is the official-only path.

## How to run

```bash
cargo test -p nes-test-harness nestest_zero_diff_official
```

The full `--features test-roms` run includes nestest by default.

## License

Public domain. Both kevtris and the Nintendulator authors released
their respective contributions for this purpose.
