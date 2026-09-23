# `mmc1_a12/` — MMC1 + PPU A12 Transition Test

A control-case test for the PPU A12 transition axis that drives MMC3
IRQ counter clocking. MMC1 does **not** use A12 for IRQ — so this ROM
running cleanly on MMC1 confirms that A12 transitions are not
incorrectly affecting non-MMC3 mappers.

## ROM

| File | Mapper | Author | License |
|------|--------|--------|---------|
| `mmc1_a12.nes` | MMC1 (1) | Bregalad (per its title screen, "(C) 2010 BREGALAD"; earlier revisions of this file said tepples) | Public domain (via `christopherpow/nes-test-roms` aggregator) |

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

**Correction (v2.7.2).** The description above is how this ROM was
first read, and it is incomplete. Its title screen reads "MMC1 WRAM
DISABLE SCANLINE COUNTER TEST". The ROM turns the one A12 dependency
MMC1 *does* have into a raster timer. In 4 KiB CHR mode, the CHR
register that drives SNROM's PRG-RAM enable (`E`, bit 4) is whichever
one the PPU's A12 last selected. `nesdev_wiki/MMC1.xhtml` warns that
mismatched registers leave the RAM "enabled as the PPU renders". From
its title and on-screen text, the program sets the two registers
differently, detects the enable changing during rendering, and draws a
grey bar there. The controller adjusts a delay, which starts at 16.
(That is read from the screen; the program itself was not
disassembled.)

Up to v2.7.1 RustyNES ignored A12 for the enable bit, so the bar never
appeared, and the pinned snapshot recorded a screen without it. v2.7.2
models the A12-selected register (the MMC1 SUROM / SXROM work, core
audit §5.4). The bar now renders below the text at frame 240, and the
snapshot was re-blessed with it. Reverting only the A12 selection in
`m001_mmc1.rs` (`outer_reg`) restores the old hash exactly, so the new
frame comes from that one behaviour.

The test still guards what it always did: MMC1 has no IRQ counter, so
A12 transitions must never reach an IRQ path.

## Why this matters for RustyNES

RustyNES's `Mapper::notify_a12(level)` default impl is a no-op; only
MMC3-family mappers override it. This ROM is a regression guard
against accidentally moving any A12-filtering logic up to the
generic PPU code path.

## License

Public domain via the `nes-test-roms` aggregator README.
