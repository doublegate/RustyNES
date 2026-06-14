# v1.1.0 · Sprint D — Audio & NSF player  → beta.2

## T-110-D1 — NSF / NSFe player  ✅ DONE (2026-06-14)

- Loader for `.nsf`/`.nsfe` + a synthetic NSF "mapper" that runs the tune's
  init/play routines driven by a frame timer, reusing the existing APU (incl.
  expansion audio: VRC6/VRC7/MMC5/N163/Sunsoft-5B/FDS already synthesize).
  Frontend track-select UI (next/prev, track count, time).
- **Refs:** `ref-proj/Mesen2/.../NsfLoader.h`, `NsfeLoader.h`, `NsfMapper.h`, `NsfPpu.h`.
- New parse branch in `crates/rustynes-mappers/src/lib.rs`; frontend NSF panel +
  load path (drag-drop `.nsf`).
- **Done when:** a CC0/public-domain NSF plays with track switching; never commit
  copyrighted music (use a CC0 test tune only).
- **DONE:** `crates/rustynes-mappers/src/nsf.rs` — `parse_nsf` (classic `NESM\x1A`
  header: load/init/play, song count, `$5FF8-$5FFF` 4 KiB bank init, expansion
  bitfield, NUL-trimmed metadata) + `NsfMapper`. Rather than a bespoke run-loop
  mode, the mapper serves a synthetic 6502 **driver** at `$5000` and points the
  reset/NMI/IRQ vectors at it (Mesen2/FCEUX/rustico approach): reset → `JSR init`
  (song in A, region in X) → enable vblank NMI → spin; NMI → `JSR play` → RTI. So
  the unchanged `Nes::run_frame` lockstep loop drives playback and the APU fills
  audio exactly as for a cartridge — determinism untouched, AccuracyCoin/oracles
  byte-identical (the new `Nes::from_nsf` is a separate construction path; the
  `Mapper` trait gained three default-no-op `nsf_*` hooks). Frontend:
  `is_nsf_image` load branch → `Nes::from_nsf_with_sample_rate`, a
  `debugger/nsf_panel.rs` **NSF Player** chip panel (metadata + Prev/Next/Restart
  + song slider) auto-opened on load, and a Debug → NSF Player menu entry. Core
  tests `nsf_constructs_runs_and_selects_tracks` (asserts audio is produced) +
  `nsf_song_apis_are_inert_on_a_cartridge`; mapper-unit tests for parse, driver
  vectors, track patching, save-state. **Deferred (documented):** expansion-chip
  audio (VRC6/7, MMC5, N163, 5B, FDS), the FDS-style `$5FF6/$5FF7` RAM banking,
  exact non-60 Hz play rates, a wasm NSF loader, and NSFe (`NSFE` chunked) parsing.

## T-110-D2 — Per-channel audio polish (optional parametric EQ)  ✅ DONE (2026-06-14)

- Add an optional parametric EQ stage (per-channel mute already exists). Frontend
  audio settings.
- **Ref:** `ref-proj/Mesen2/Utilities/Audio/Equalizer.h`.
- **Done when:** EQ is opt-in and does not alter the determinism-critical core
  synthesis (it is a frontend output stage, like the existing DRC resampler).
- **DONE:** `crates/rustynes-frontend/src/eq.rs` — a 5-band graphic EQ (60 / 240 /
  1k / 3.8k / 12k Hz, ±12 dB) of cascaded RBJ-cookbook peaking biquads. It runs in
  the **producer** path *after* the DRC resampler and *before* the lock-free queue
  (an `EqStage` on both `AudioOutput` + `AudioProducer`), so it touches only the
  host-rate output — never the deterministic core synthesis. Params live in the
  shared queue (`set_eq` / gen counter); the Settings → Audio panel pushes changes
  (`SettingsApply::audio_eq` → `App::apply_audio_eq`) and the producer rebuilds its
  biquads on the next push (live, lock-free). Off by default + bypassed when flat
  → byte-identical output (the no-DRC path stays zero-copy when disabled). Tests:
  `flat_eq_is_bypassed_and_identity` (bit-identical bypass), `nonflat..stable`,
  `band_boost_amplifies_its_center_frequency`. **Deferred:** a wasm audio feed +
  configurable band freqs/Q (parametric beyond fixed bands).

## Verification
- NSF path is separate from ROM emulation → AccuracyCoin/oracle unaffected.
- EQ is a frontend stage (never the core synthesis rate) → determinism held.
- Smoke-play test on a committed CC0 NSF.
