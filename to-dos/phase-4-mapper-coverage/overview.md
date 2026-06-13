# Phase 4 — Mapper Coverage

## Goal

Implement MMC3 (with cycle-accurate IRQ counter), then sweep through the rest of the top-25 mapper list including the audio-extension mappers (VRC6, Sunsoft 5B, Namco 163) and MMC5. By the end of this phase the emulator can run > 95% of the licensed library.

## Exit criteria

- [ ] `mmc3_test_2/*` (5 sub-ROMs) pass.  **Status: 1/6 (5-MMC3 PASS; 1-clocking, 2-details, 3-A12_clocking, 4-scanline_timing fail; 6-MMC3_alt is the rev-B counterpart and is mutually exclusive with our Sharp default).**
- [ ] `mmc3_irq_tests/*` (6 sub-ROMs) pass.  **Status: untestable from the harness — these older ROMs use a visual-only protocol.**
- [ ] `vrc24test` passes.  **Status: ROM not vendored.**
- [ ] `holy_mapperel` passes (detects all implemented mappers, exercises bank reachability).  **Status: ROM not vendored; covered by per-mapper unit tests instead.**
- [ ] `AccuracyCoin` pass rate ≥ 80%.  **Status: ROM not vendored.**
- [x] Per-mapper boot test for each implemented mapper.  **Status: parse-success + unit tests cover MMC3, MMC2, MMC4, ColorDreams, CPROM, M34, Camerica, VRC1, VRC2, VRC4, VRC6, FME-7, Namco163, MMC5 (v0).**

## Scope

In-scope:
- MMC3 (defining mid-life mapper; PPU A12 IRQ).
- MMC2, MMC4, Color Dreams, CPROM, BNROM/NINA-001, Camerica, VRC1.
- VRC2/4/6/7 (Konami).
- Sunsoft FME-7 + 5B audio.
- Namco 163 + audio.
- MMC5 (without audio, gated behind `mmc5-audio` feature for later).

Out-of-scope:
- VRC7 FM audio (defer to v0.x).
- MMC5 audio (Phase 5 or later).
- FDS.
- Pirate / multicart mappers.

## Sprints

- [Sprint 1 — MMC3](sprint-1-mmc3.md)
- [Sprint 2 — Misc mappers (MMC2, MMC4, Color Dreams, CPROM, BNROM/NINA, Camerica, VRC1)](sprint-2-misc-mappers.md)
- [Sprint 3 — VRC family + Sunsoft FME-7 + Namco 163](sprint-3-vrc-extended.md)
- [Sprint 4 — MMC5](sprint-4-mmc5.md)

## Dependencies

Phase 3 complete (audio extension mappers need the APU mixing path).

## Risks

- **Risk: MMC3 IRQ A12 filter misbehavior.** Detection: `mmc3_irq_tests`. Mitigation: literal implementation of "3 falling edges of M2" per `docs/mappers.md`.
- **Risk: MMC3A vs MMC3B revision selection wrong.** Detection: Star Trek 25th Anniversary boot. Mitigation: NES 2.0 submapper field; sane default.
- **Risk: MMC5 ExRAM modes interact unexpectedly with PPU.** Detection: known ROMs (Castlevania III JP). Mitigation: implement modes incrementally; visual diff.
- **Risk: VRC submapper confusion.** Detection: ROM compatibility breaks across submapper variants. Mitigation: NES 2.0 submapper field is authoritative; documented in `docs/mappers.md`.

## Reference docs

- [docs/mappers.md](../../docs/mappers.md)
- [docs/apu-2a03.md](../../docs/apu-2a03.md) — `mix_audio` integration for extended mappers
