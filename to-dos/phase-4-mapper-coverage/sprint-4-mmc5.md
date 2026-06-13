# Sprint 4-4 — MMC5

**Phase:** Phase 4 — Mapper Coverage
**Sprint goal:** Implement MMC5 (mapper 5) without the audio extension. Banking, ExRAM modes, dual sprite/BG CHR banks for 8x16 sprites, scanline IRQ at PPU cycle 4.
**Estimated duration:** 2 weeks

**Status:** v0 + v1 follow-up both landed — banking + scanline IRQ + ExRAM modes 10/11 + multiplier + dual sprite/BG CHR for 8x16 + 4-byte fill mode + ExGrafix + vertical split-screen. Only the audio extension and multi-bank PRG-RAM remain for v1.x.

## Planned tickets

- [x] T-44-001 — Register layout `$5000-$5FFF`; banking modes (PRG modes 0-3, CHR modes 0-3).
- [x] T-44-002 — ExRAM (1 KB internal RAM) with modes 10/11 fully working; mode 00 routes through `$5105` for nametable selection. Mode 01 ExGrafix (per-tile palette + per-tile CHR bank) landed in v1 follow-up via `Mapper::peek_ex_attribute`.
- [x] T-44-003 — Dual CHR bank selection for sprites vs BG (8x16 sprite mode). Landed in v1 follow-up: PPU sprite tile fetches route through new `PpuBus::ppu_read_sprite` / `Mapper::ppu_read_sprite` trait methods; MMC5 resolves these through `$5120-$5127` while BG fetches continue using `$5128-$512B`.
- [x] T-44-004 — Scanline IRQ at PPU cycle 4 — implemented via new `Mapper::notify_scanline_start` (PPU calls at dot 0 of every rendered scanline) + `Mapper::notify_vblank` (PPU calls at scanline 241 dot 1).
- [ ] T-44-005 — Smoke test: Castlevania III (J), Just Breed, Laser Invasion boot. DEFERRED: synthetic banking test landed in unit tests; 3 MMC5 test ROMs vendored from `christopherpow/nes-test-roms/mmc5test/` and run as smoke gates in `crates/rustynes-test-harness/tests/mmc5.rs`. No commercial ROMs vendored.

## Landed in v1 follow-up

- [x] **Vertical split-screen** (`$5200`-`$5202`): new `Mapper::bg_split_state(scanline_y, coarse_x) -> Option<BgSplitState>` trait method (default `None`); PPU queries it at the NT-byte boundary of every 8-dot BG fetch group. Castlevania III (J) status-bar HUD is the canary. Save-state version bumped 2 → 3.
- [x] **4-byte fill mode** (`$5106`/`$5107`): new `Mapper::nametable_fetch` / `nametable_write` trait methods let MMC5 synthesize fill-tile/fill-attribute bytes for `$5105`-selected fill-mode nametables.
- [x] **Dual sprite/BG CHR banks for 8x16** (T-44-003 above): `ppu_read_sprite` Mapper trait method routes sprite tile fetches separately from BG.
- [x] **ExRAM extended-attribute mode** (`$5104` mode 01, "ExGrafix"): per-tile palette + per-tile CHR bank from ExRAM byte. New `Mapper::peek_ex_attribute(v) → Option<ExAttribute>` queried at NT-byte fetch boundary; `Ppu::ex_attr_latch` field caches per-tile palette through the AT shifters.

## Deferred for v1.x

- **MMC5 audio extension** (`$5000-$5015`): 2 extra pulse + raw PCM. Behind future `mmc5-audio` cargo feature; tracked as Track C2 in the gap-analysis plan.
- **Multi-bank PRG-RAM** via `$5113`: stored but only one 8 KiB bank is wired up.

## Open questions

- **MMC5 audio extension** (2 extra pulse + raw PCM): deferred behind `mmc5-audio` cargo feature; not in v0.
- **`nametable_address` and ExRAM nametable mode**: the current bus's `nametable_address` -> CIRAM-offset path can't fully express "fetch this nametable byte from ExRAM/Fill instead of CIRAM." For v0 we route ExRAM/Fill nametable fetches *only* when the CPU reaches them through `ppu_read` ($2000-$3EFF) calls — which the lockstep bus does NOT do for the standard PPU rendering path (PPU reads CIRAM directly). Genuine ExRAM-as-nametable rendering will need a deeper PPU hook in a follow-up.

## Reference docs

- [docs/mappers.md](../../docs/mappers.md) §MMC5 — the most complex official mapper
- [crates/rustynes-mappers/src/mmc5.rs](../../crates/rustynes-mappers/src/mmc5.rs) — implementation; module-level docs enumerate v0 scope vs. deferrals.
