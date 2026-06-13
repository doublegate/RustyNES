# `m22/` — Konami VRC2 (Mapper 22) CHR-Banking Test

A single CHR-banking smoke test for iNES mapper 22 (Konami VRC2a, used
by TwinBee 3 and a handful of other Famicom releases).

## ROM

| File | Mapper | Author | License |
|------|--------|--------|---------|
| `0-127.nes` | VRC2a (22) | NewRisingSun | Public domain (via `christopherpow/nes-test-roms` aggregator) |

## Source

`christopherpow/nes-test-roms/m22chrbankingtest/0-127.nes`

The upstream aggregator README places all its corpora under
public-domain redistribution terms.

## What it tests

Verifies that all 128 4 KiB CHR banks are reachable. VRC2's CHR-bank
register address decoding has a quirky "nybble-swap" pattern (low 4 bits
of bank index at one register, high 4 bits at another). This ROM
writes through each combination and renders the resulting CHR data to
the screen so a visual diff against a reference image (`0-127.png` in
the upstream) shows immediately if any bank is silently dropped.

The ROM is **visual**, not the blargg `$6000` status protocol. The
matching integration tests in `crates/nes-test-harness/` use this ROM
as a boot-without-crash smoke gate. A pixel-decode regression check is
on the longer-term TODO list (see `to-dos/ROADMAP.md`).

## License

Public domain (per the `nes-test-roms` aggregator README). The
upstream source files at `0-127.c` / `0-127h.asm` / `0-127.chr` are
also in the aggregator; we vendor only the assembled `.nes` since the
test is consumed by integration tests, not rebuilt.
