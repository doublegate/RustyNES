# `mmc1_a12/` — MMC1 + PPU A12 Transition Test

A control-case test for the PPU A12 transition axis that drives MMC3
IRQ counter clocking. MMC1 does **not** use A12 for IRQ — so this ROM
running cleanly on MMC1 confirms that A12 transitions are not
incorrectly affecting non-MMC3 mappers.

## ROM

| File | Mapper | Author | License |
|------|--------|--------|---------|
| `mmc1_a12.nes` | MMC1 (1) | tepples | Public domain (via `christopherpow/nes-test-roms` aggregator) |

## Source

`christopherpow/nes-test-roms/MMC1_A12/mmc1_a12.nes`

The aggregator's root README places its corpora under public-domain
redistribution terms.

## What it tests

The PPU's A12 line transitions on every background / sprite CHR fetch.
For MMC3, those transitions are filtered through a small counter and
ultimately fire IRQ — see `docs/adr/0002-irq-timing-coordination.md`
for the full chain.

This ROM exercises MMC1 (which has no IRQ counter) under conditions
where an emulator's PPU A12 dispatch would historically over-fire if
it routed A12 events to ALL mappers instead of only the MMC3 family.

The expected behaviour is "no crash, no visible glitch" — the ROM
renders a static pattern and idles. The integration test harness
treats it as a boot-without-crash smoke gate. A pixel-decode regression
check is a future addition (see `to-dos/ROADMAP.md`).

## Why this matters for RustyNES

RustyNES's `Mapper::notify_a12(level)` default impl is a no-op; only
MMC3-family mappers override it. This ROM is a regression guard
against accidentally moving any A12-filtering logic up to the
generic PPU code path.

## License

Public domain via the `nes-test-roms` aggregator README.
