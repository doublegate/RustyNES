# Sprint 4 - Mappers, Expansion Audio, And Platform Variants

**Goal:** decide and execute the v1.x cartridge/platform scope beyond stock
mapper behavior already covered by v1.0.

## Tickets

- [x] **T-74-001 - NES 2.0 submapper audit.** Build a fixture matrix for
  revision-sensitive supported mappers: MMC3A/B/C, VRC2/VRC4 wiring,
  BNROM/NINA, and bus-conflict variants.
- [x] **T-74-002 - MMC5 deferred features.** Decide whether to implement MMC5
  audio and multi-bank PRG RAM in v1.x. If accepted, add register tests,
  save-state sections, and audio baselines.
- [x] **T-74-003 - VRC7 FM audio decision.** Re-evaluate the ADR-0004 deferral.
  Either port a permissive OPLL core or keep mapper 85 FM audio explicitly
  silent with compatibility notes.
- [x] **T-74-004 - FDS platform plan.** Scope BIOS loading, disk image format,
  motor/transfer timing, timer IRQ, writable disk persistence, and FDS audio.
- [x] **T-74-005 - Expanded input devices.** Implement or explicitly defer Four
  Score, Zapper, Famicom expansion controllers, microphone, and NES 2.0 default
  device handling.
- [x] **T-74-006 - Vs. System / PlayChoice-10 decision.** Decide whether arcade
  variants enter v1.x or remain unsupported with precise ROM-load diagnostics.
- [x] **T-74-007 - Long-tail mapper policy.** Define how pirate, multicart, and
  homebrew-only mappers are accepted: user demand, test availability, NES 2.0
  metadata, and maintenance cost.

## Exit Checklist

- [x] `docs/compatibility.md` reflects accepted and rejected platform scope
  (FDS plan, input devices, Vs/PC10, long-tail policy).
- [x] Mapper coverage matrix / submapper variants test-guarded (NINA-001 added;
  MMC3/VRC2-4/bus-conflict already covered; STATUS.md MMC5-audio row corrected).
- [x] Expansion-audio tested (VRC6/5B/N163/MMC5/VRC7 FM) or explicitly deferred
  (FDS audio -> v2.0).

**Sprint 4 outcome (v1.5.0):** T-74-001 NINA-001 submapper test added; T-74-002
(MMC5 feature-complete), T-74-003 (VRC7 done v1.1.0), T-74-004 (FDS plan/defer
v2.0), T-74-005 (standard pad only), T-74-006 (Vs/PC10 out of scope), T-74-007
(long-tail policy) decided + documented. See the Sprint 4 audit doc.
