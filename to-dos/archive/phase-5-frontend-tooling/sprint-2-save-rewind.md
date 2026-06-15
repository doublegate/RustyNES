# Sprint 5-2 — Save state + rewind + input bindings

**Phase:** Phase 5 — Frontend + Tooling
**Sprint goal:** Save state files (.rns) with versioned tagged-section format; rewind ring buffer with delta compression; user-rebindable input.
**Estimated duration:** 2 weeks

**Status:** delivered (T-52-001..007 complete; T-52-007 egui modal landed in Sprint 5-3 alongside T-53-001 egui-wgpu integration).

## Tickets

- [x] T-52-001 — Save state format spec + serialization for each chip.
  - 16-byte container header (`RUSTYNES` magic + format version + truncated ROM SHA-256 sanity tag).
  - Tagged-section body (`BUS`, `CPU`, `PPU`, `APU`, `MAP`); per-section schema version byte; little-endian binary throughout.
  - `rustynes_core::save_state` module with `BinReader` / `BinWriter` helpers + `Section` / `SectionIter` parser.
  - Per-chip `snapshot()` / `restore(&[u8])` methods on `Cpu`, `Ppu`, `Apu`; mappers reuse the existing `Mapper::save_state` / `load_state` infrastructure.
  - `LockstepBus::snapshot` / `restore` and `Nes::snapshot` / `restore` glue; deterministic by construction (verified by unit test).
- [x] T-52-002 — Save state versioning + load-error handling.
  - `SnapshotError` enum: `HeaderTruncated` / `BadMagic` / `UnsupportedFormat` / `SectionTruncated` / `VersionMismatch` / `SectionInvalid` / `MissingSection` / `Eof`.
  - Per-chip `*SnapshotError` variants (Truncated / UnsupportedVersion / domain-specific).
  - Forward compatibility: unknown section tags are skipped on load. Required sections (BUS / CPU / PPU / APU / MAP) hard-error if missing. Per-section version mismatches surface a `VersionMismatch` with both the file's and the chip's accepted version.
- [x] T-52-003 — Save state file I/O (per-ROM directory keyed by SHA-256).
  - `crates/rustynes-frontend/src/save_state.rs`: `save_to_slot` / `load_from_slot` / `slot_path` / `slot_exists` against `<data_dir>/saves/<rom_sha256_hex>/slot{0..9}.rns`.
  - 10 slots per ROM; slot 0 is the implicit "latest" slot driven by the bare F1 / F4 keys.
  - `directories::ProjectDirs::from("dev", "DoubleGate", "RustyNES")` resolves the data dir cross-platform.
- [x] T-52-004 — Rewind ring buffer (60 s @ 60 fps target ≤ 32 MB).
  - `crates/rustynes-core/src/rewind.rs`: `RewindRing` with LZ4 keyframe / XOR-delta compression. `keyframe_period = 60` by default → 1 keyframe/sec, 59 deltas in between.
  - Memory accounting: per-entry `approx_bytes` summed against a soft `max_bytes` cap; LRU eviction from the front; orphaned deltas (whose keyframe got evicted) are dropped too.
  - Steady-state size for synthetic NROM is ~1-9 MiB for 60 s of capture (well under the 32 MiB cap).
  - `Nes::run_frame` automatically calls `rewind_capture()` at end-of-frame when rewind is enabled; frontends don't need to plumb it.
- [x] T-52-005 — Hold-F5 rewind UX.
  - On F5 press, `InputState::rewind_held = true`; per redraw the run-loop calls `nes.rewind_step_back()` instead of `nes.run_frame()` until release. On release, forward play resumes from the current pointer; future captures append to the ring.
  - Audio is silenced during rewind playback (no `drain_audio_into` call from the rewind branch).
- [x] T-52-006 — Settings persistence (TOML at `directories::ProjectDirs::config_dir()`).
  - `crates/rustynes-frontend/src/config.rs`: `Config { input, rewind, graphics, audio }` with `serde::Serialize/Deserialize`; `Config::load_or_default()` at boot, `Config::save()` for the future settings modal.
  - Missing file → defaults; malformed file → log + defaults.
- [x] T-52-007 — Input rebinding UI (egui modal).
  - **Sprint 5-2 (TOML)**: rebinding via `[input.player1]` / `[input.player2]` / `[input.system]` blocks. `KeyBindings::from_config(&InputConfig)` resolves keycode strings against `parse_keycode`; unknown names are logged + dropped. Edit `config.toml` and restart to apply new bindings.
  - **Sprint 5-3 (egui modal)**: in-app rebinding modal in `crates/rustynes-frontend/src/debugger/input_rebind_panel.rs`. Clicking a row enters capture mode; the next non-Esc key press writes back into the in-memory `Config`. "Save to disk" persists via `Config::save`; "Reset to defaults" reverts to `Config::default`. Esc cancels an in-flight capture. TOML file format is unchanged from Sprint 5-2.

## Reference docs

- [docs/frontend.md](../../docs/frontend.md) §Save state files
- [docs/architecture.md](../../docs/architecture.md) §Public API surface

## Outputs

- Save-state container: `rustynes_core::save_state` (~537 LOC)
- Per-chip snapshot encoders: `rustynes_cpu::snapshot`, `rustynes_ppu::snapshot`, `rustynes_apu::snapshot` (~1,150 LOC combined)
- Bus snapshot wrapper: `rustynes_core::bus_snapshot` (~143 LOC)
- Rewind ring: `rustynes_core::rewind` (~402 LOC)
- Frontend save-state file I/O: `rustynes_frontend::save_state` (~199 LOC)
- Frontend config: `rustynes_frontend::config` (~346 LOC)
- Frontend input rebinding: `rustynes_frontend::input` (~430 LOC, full rewrite)
- E2E tests: `crates/rustynes-test-harness/tests/save_state.rs` (~106 LOC, 5 tests)
- Workspace test count: 332 → ~390 (+~50) with `--features test-roms`

## Followup (post-Sprint 5-3)

- Save-state thumbnails (Phase 5+).
- Cross-version save-state migration (best-effort per CLAUDE.md).
- Player-2 routing through `Nes::set_buttons(1, ...)` — bindings are accepted today but the bus only wires player 1 to the controller shift register from the keyboard.
