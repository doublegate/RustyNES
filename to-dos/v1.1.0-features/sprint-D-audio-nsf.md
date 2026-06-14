# v1.1.0 · Sprint D — Audio & NSF player  → beta.2

## T-110-D1 — NSF / NSFe player

- Loader for `.nsf`/`.nsfe` + a synthetic NSF "mapper" that runs the tune's
  init/play routines driven by a frame timer, reusing the existing APU (incl.
  expansion audio: VRC6/VRC7/MMC5/N163/Sunsoft-5B/FDS already synthesize).
  Frontend track-select UI (next/prev, track count, time).
- **Refs:** `ref-proj/Mesen2/.../NsfLoader.h`, `NsfeLoader.h`, `NsfMapper.h`, `NsfPpu.h`.
- New parse branch in `crates/rustynes-mappers/src/lib.rs`; frontend NSF panel +
  load path (drag-drop `.nsf`).
- **Done when:** a CC0/public-domain NSF plays with track switching; never commit
  copyrighted music (use a CC0 test tune only).

## T-110-D2 — Per-channel audio polish (optional parametric EQ)

- Add an optional parametric EQ stage (per-channel mute already exists). Frontend
  audio settings.
- **Ref:** `ref-proj/Mesen2/Utilities/Audio/Equalizer.h`.
- **Done when:** EQ is opt-in and does not alter the determinism-critical core
  synthesis (it is a frontend output stage, like the existing DRC resampler).

## Verification
- NSF path is separate from ROM emulation → AccuracyCoin/oracle unaffected.
- EQ is a frontend stage (never the core synthesis rate) → determinism held.
- Smoke-play test on a committed CC0 NSF.
