# Changelog

All notable changes to RustyNES will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

> **v1.0.0 integrates a cycle-accurate emulation engine.** The original RustyNES
> emulation core (v0.1.0 -> v0.8.6, preserved below) was replaced wholesale with a
> new master-clock-precise, lockstep-scheduled core that reaches 100% on the
> AccuracyCoin hardware-accuracy suite. The `v0.9.0` -> `v0.9.7` entries are the
> **documentary lineage** of how that core was built and hardened (each notes the
> engine-internal milestone it came from); `v1.0.0` is the production cut that ports
> the original RustyNES desktop experience onto the new engine.

## [Unreleased]

## [1.7.1] - 2026-06-19

### Fixed

- **ROM-close GPU abort fixed (#1).** In a release build `Gfx::render_with_overlay`'s
  `debug_assert_eq!` size check is compiled out, so when `close_rom` set
  `emu.nes = None` and the render path handed an empty `present_staging` slice to
  `queue.write_texture`, wgpu validation aborted the process ("Copy ... would end
  up overrunning the bounds of the Source buffer of size 0"). `render_with_overlay`
  (the framebuffer + `index` upload branches) and `render_hd_with_overlay` now skip
  the `write_texture` upload when the supplied pixel slice length does not match
  `width * height * bytes_per_texel`, keeping the previously-uploaded texture; the
  `debug_assert` stays for dev builds. `close_rom` now seeds `present_staging` with
  a valid zeroed `NES_W * NES_H * 4` frame, so closing a ROM presents a clean blank
  (black) window rather than a frozen last frame or a crash. Frontend-only;
  byte-identical when never closing a ROM.
- **Clean pause / unpause — no spurious pacing spike, zero audio underruns (#6).**
  Two paused-state glitches fixed. (1) Pacing: on resume `set_paused` rebased
  `next_frame_time` to "now" so the producer does not burst-catch-up, but the
  produced/presented interval rings still held the pre-pause `last` timestamp, so
  the next `record_produced` logged the whole pause duration as a single giant
  `produced_max_ms` spike (675 ms / 1395 ms in captured logs). `set_paused` now also
  calls `perf.break_phase()` on resume so the paused wall-clock gap is dropped
  instead of counted as a produced/presented frame interval. (2) Audio: while paused
  the producer stops pushing and the cpal callback drained the ring, then logged an
  underrun and re-gated on resume. A new sticky pause flag
  (`SampleQueue::gate_for_pause()` / `resume_from_pause()`) outputs silence WITHOUT
  consuming the buffered tail and takes precedence over the `start_threshold`
  auto-reopen (which previously re-opened the gate on the very next callback because
  the full ring satisfied `avail >= start_threshold`), so a clean pause/unpause now
  yields zero underruns. Frontend-only; byte-identical when never pausing.
- **Help -> Documentation pane overhaul (#4).** Three fixes to `doc_panel.rs`
  (emulation core untouched). (1) Word-wrap at any UI scale: the content pane was a
  `ScrollArea::both`, which gives the inner UI unbounded horizontal width so
  `Label::wrap` had nothing to wrap against and text only reflowed after a manual
  resize at x4 scale; it is now a vertical-only `ScrollArea` with the content column
  pinned to `available_width`, so bodies wrap to the pane's real width at x1-x4 and
  any pane size. (2) A collapsible multi-level sidebar tree (default-collapsed below
  the top level) with intra-document hyperlinks. (3) The changelog dropdown now
  lists released versions newest-first but orders the `[Unreleased]` section last.
- **HD-pack tile substitution now applies in the debugger / tool branch (#3).**
  A loaded HD pack (e.g. *Zelda Remastered*, ~15,849 rules) parsed and reported
  success, but nothing changed on screen whenever the debugger overlay or a
  Cheats / ROM-Database panel was open: that render branch (`needs_nes`) always
  presented the stock NES framebuffer and never invoked the HD compositor. The
  branch now captures the same per-frame snapshots (tile-source telemetry, the
  8 KiB CHR pattern space, watched-memory) under the emu lock, drops the lock,
  runs `HdCompositor::composite`, and presents the upscaled buffer via
  `render_hd_with_overlay` (the deep-overlay panels still draw on top). The
  runtime tile-key derivation was verified against a real `<ver>106` pack +
  live ROM (every rendered tile's CRC-32-of-16-CHR-bytes key hit the loaded
  rule set). All `hd-pack`-gated frontend code; with the feature off the
  shipped / native / `no_std` / wasm builds stay byte-identical and AccuracyCoin
  holds 100% (139/139).
- **Tools-menu icons for "NSF Player" and "Pixel Inspector" now render
  correctly.** They used Font Awesome U+F001 / U+F002, which egui's default
  proportional font (`Ubuntu-Light`) maps to its `fi` / `fl` ligatures in the
  Private Use Area; since the icon font is only the trailing fallback, those two
  codepoints resolved to the ligatures and showed a literal "fi" / "fl". Switched
  to `headphones` (U+F025) and `magnifying-glass-plus` (U+F00E), which sit above
  the collision range and render from the icon font as intended.

### Removed

- **The vestigial "Show Debugger" checkbox in the Debug menu.** Since v1.7.0
  "Forge" beta.5 (#55) retired the debugger toolbar/overlay, every chip inspector
  opens its own window directly from the Debug menu, so the bare visibility toggle
  did nothing. Removed the checkbox and its now-dead plumbing
  (`MenuAction::ToggleDebugger`, the `ShellFrame::debugger_visible` field, and the
  `DebuggerOverlay::toggle` method).

### Documentation

- **Exhaustive README rewrite (#7).** README.md rewritten for v1.7.0 "Forge":
  the full feature catalogue, a complete WebAssembly / browser section, the
  previously-missing workspace crate, and the full peripheral set, with version
  tags and links corrected (#151 review nits applied).

## [1.7.0] - 2026-06-19 - "Forge" (Feature Release)

### Added

- **v1.7.0 "Forge" H5 — i18n framework: a compile-time string catalog + a
  Settings language picker.** RustyNES's one systemic gap — no localization
  anywhere — closed with a lightweight, frontend-only, English-by-default i18n
  layer (`crate::i18n`, ADR 0023). A `Key` enum + one `const fn` catalog per
  locale resolve through `tr(key)` / the `t!(Key)` macro; there is **no runtime
  file I/O, no Fluent/ICU/`rust-i18n` dependency** (rejected on the wasm size
  budget — the bundle stays inside the `scripts/wasm_size_budget.sh` 5 MiB gate),
  just a few KiB of `&'static str` data. **English (`Locale::English`) is the
  `Default` and the universal fallback**: the `[ui] locale` config field is
  `#[serde(default)]`, the English catalog holds the *verbatim* current strings,
  and any key a non-English catalog omits falls back to English — so with the
  default locale every label is **byte-identical to v1.6.0** and AccuracyCoin
  holds 100% (139/139). Ships a second real locale (Spanish) to prove the
  mechanism, a **Language** combo in Settings -> Video -> Display (persisted to
  `[ui] locale`, re-renders on the next egui frame), and conversion of the
  high-visibility surfaces — the menu bar (top-level menus + common File/View
  items), the Settings title/tab strip/Display labels, and the status-bar state
  words. Conversion is **incremental**: deeper panels keep their literals and
  follow the documented `t!(Key)` / `tr(..)` pattern (`docs/frontend.md`) over time. New ADR
  **0023** (i18n string-catalog approach). Frontend-only; the chip stack /
  `rustynes-core` / test-harness are untouched.
- **v1.7.0 "Forge" #54 — `full` maximal-native-feature build + `cargo full-run`
  alias (the "cargo --full equivalent").** A new opt-in `full` feature on
  `rustynes-frontend` aggregates the maximal NATIVE feature set —
  `retroachievements` + `scripting` + `script-ipc` + `hd-pack` + `debug-hooks` +
  `av-record` — additively on top of the default set (`wasm-winit` / `emu-thread`
  / `help-tui` / `gpu-timing`). Two cargo aliases in `.cargo/config.toml` make it
  a one-liner: `cargo full-run path/to/rom.nes` and `cargo full-build`. The
  WASM-only features (`script-wasm` — wasm-only and mutually exclusive with
  `scripting`; `browser-cheevos`; `wasm-canvas`) are deliberately excluded, since
  `full` targets a native binary. Purely opt-in: the shipped/default build and the
  emulation core are unchanged (`hd-pack`/`debug-hooks` only forward to the
  existing off-by-default `rustynes-core` telemetry, proven byte-identical), so
  AccuracyCoin holds 100% (139/139). The value of this entry point is the
  full-feature *combination* compiling clean — a gap single-feature CI gates miss.
- **v1.7.0 "Forge" H6 — web/wasm parity: browser Lua, File System Access API,
  Gamepad API, PWA/offline, and `?settings=` share-links.** Five additive,
  web-only browser-platform features. All are wasm-only or behind the existing
  off-by-default `script-wasm` feature, so the **native build is byte-identical**
  (the chip stack / `rustynes-core` / test-harness are untouched) and
  AccuracyCoin holds 100% (139/139). (1) **Lua in the browser** — the unified
  winit path now runs the experimental piccolo Lua engine end-to-end: a `.lua`
  picker / paste box in `web/index.html` drives the `rustynes_load_script` /
  `rustynes_stop_script` bridge, the `App` drains + pumps it each produced frame
  under the live `Nes`, and overlay draws render through the egui pass; writes
  are gated during browser netplay. piccolo is observational + NOT byte-parity
  with native mlua (ADR 0012) and never in the determinism oracle; off by
  default. (2) **File System Access API** (`wasm_io::save_file_with_fallback`,
  ADR 0021) — TAS `.rnm` exports save through a real `showSaveFilePicker` "Save
  As" dialog on Chromium-family browsers, with a graceful fallback to the
  synthetic-anchor download on Firefox/Safari; reached dynamically via
  `js_sys::Reflect` so no `web_sys_unstable_apis` flag is needed. (3) **Gamepad
  API** (`wasm_gamepad.rs`) — the page polls `navigator.getGamepads()` each
  `requestAnimationFrame`, maps the standard (Xbox) layout + left stick to a
  `Buttons` mask, and routes it to player 1 at the SAME late-latch as
  touch/keyboard, so it records/replays identically in TAS movies + netplay
  (empty when no pad is connected = byte-identical default). (4) **PWA /
  offline** — a `manifest.webmanifest` + a service worker (`sw.js`,
  cache-first-then-network over same-origin GETs) make the demo installable +
  offline-capable; manifest, icons, and `sw.js` are copied into `dist/` by the
  Trunk asset pipeline and the bundle stays within the 5 MiB gzip budget. (5)
  **`?settings=` share-links** (`wasm_share.rs`, ADR 0022) — a curated subset of
  `Config` (NTSC/CRT filter + knobs, overscan, theme, 8:7, zoom, FPS, volume)
  serializes to a compact URL-safe base64 blob applied on load; a "Copy share
  link" button mints + copies the URL for the live settings. Decode is
  length-capped (8 KiB) + tolerant of malformed/old/new blobs (all
  `#[serde(default)]`). New web-sys feature `Location`; new ADRs **0021** (FS
  Access fallback) + **0022** (share-link format/versioning).

- **v1.7.0 "Forge" H3 — audio depth: stereo panning, reverb/crossfeed, output
  device picker, 20-band graphic EQ, per-context volume.** Five additive,
  **bypass-by-default** frontend mixer/output stages. The deterministic core
  sample stream is untouched (no chip-stack / `rustynes-core` / test-harness
  change), so with the new controls at their defaults the native / `no_std` /
  wasm output is **byte-identical** to v1.6.0 and AccuracyCoin holds 100%
  (139/139). (1) **Stereo panning** widens the mono master to L/R in the cpal
  callback (`audio_dsp.rs::StereoStage`) with a per-APU-channel pan array
  (default all-center, which duplicates the mono value bit-for-bit). (2)
  **Reverb / crossfeed** — a small Schroeder reverb (4-comb + 2-allpass) and a
  headphone crossfeed, both default off (0% wet / 0 crossfeed = dry passthrough).
  (3) **Output device picker** (`[audio] output_device`) enumerates the cpal
  output devices; default = the system default device, and a now-absent device
  falls back to it gracefully (native-only; the wasm path is unaffected). (4) a
  **20-band graphic EQ** at the ISO third-octave centers (25 Hz–20 kHz) extends
  the existing `eq.rs` (the `Equalizer` is now band-count-generic); flat in
  either the 5- or 20-band mode is a true unity bypass, and a pre-v1.7.0 config
  migrates (5-band kept, flat 20-band default). (5) **Per-context volume**
  (master × game / menu legs) folds into the single cpal consume gain
  (`AudioConfig::effective_gain_for`); all legs default to 1.0 (no-op). Wired
  into Settings → Audio with `#[serde(default)]` config fields. See ADR 0020 for
  the mono→stereo widening contract. (`docs/frontend.md`, `docs/adr/0020`.)
- **v1.7.0 "Forge" H4 — per-game `<rom>.json` config overrides + DIP editor +
  lag-frame counter.** Additive / off-by-default; the core stays byte-identical
  and AccuracyCoin holds 100% (139/139). A frontend-only per-game config overlay
  (`per_game::PerGameConfig`) layered on the v1.2.0 game-DB: at ROM load the
  frontend resolves a `<rom>.json` — a config-dir overlay
  (`<data-dir>/per-game/<CRC8>.json`) that **wins** over a sibling
  `<rom-stem>.json`, keyed on the same header-excluded CRC32 the game-DB uses —
  and applies its `overrides` (region / mapper / submapper / mirroring) through
  the SAME `apply_header_overrides` + `set_mirroring_override` paths and its Vs.
  `dip_switches` via `Nes::set_vs_dip`; an absent or inert file applies nothing,
  so the load path is byte-identical and the deterministic core / test harness
  never read it (the firewall). A **DIP-switch editor** in the Tools → ROM
  Database panel (shown for Vs. System carts) exposes the 8 DIP bits, applies
  edits live, and persists them into the config-dir overlay (atomic write; an
  inert overlay deletes the file, never a sibling ROM). A **lag-frame counter**
  (View → Show Lag Frames, off by default) tallies forward frames since ROM load
  in which the program polled no controller — sampled via the core's output-only
  `was_input_polled_this_frame()` `debug-hooks` telemetry, a pure observation
  that never perturbs emulation — and shows it next to the FPS readout. New
  `[ui] show_lag_frames` config toggle (`#[serde(default)] = false`). See ADR
  0019 for the precedence/firewall decision.
- **v1.7.0 "Forge" H1/H2 — browser RetroAchievements completion + RA HUD depth.**
  Additive / off-by-default; the core stays byte-identical and AccuracyCoin holds
  100% (139/139). **H1 (browser RA, `browser-cheevos`):** `web/cheevos/ra_glue.js`
  gained the real rc_client **wasm trampoline marshalling** — `addFunction`-bound
  read-memory / server-call / event-handler callbacks, the `rc_api_request_t` →
  auth-proxy `fetch` → `rc_api_server_response_t` bridge (verbatim path/body
  forwarding so the proxy can inject the browser-forbidden RA `User-Agent`
  server-side), client create + event-handler install + casual-only enforcement
  (no hardcore export), and a per-frame `ra_do_frame(readByte)` driver returning a
  JSON event array. The Rust bridge (`wasm_cheevos.rs`) added `begin_login` /
  `load_game` / `do_frame` over those imports; the side-module build script now
  exports `set_event_handler` + the `getValue` / `setValue` / `HEAPU8` runtime
  methods the marshalling needs. The auth-proxy **deploy** and a **live-browser
  unlock with a real RA account** remain maintainer-manual (no headless path —
  ADR 0015; native RA unaffected). **H2 (RA HUD completion):** surfaces RA data
  the session already decoded then dropped — a **leaderboard-scoreboard popup**
  (new `RaEvent::LeaderboardScoreboard` from the previously-unmodeled event 13,
  shown as "#N of M" + the top entries), **challenge** + **progress** indicators
  (now stored + drawn instead of ignored), per-achievement **rarity** ("% earn")
  in the panel, and hardcore **pause-gating** (`rc_client_can_pause`) wired into
  `set_paused` so a hardcore pause is deferred with the seconds remaining.
- **v1.7.0 "Forge" Workstream H8 — spectator netplay (read-only).** A
  determinism-safe, receive-only extension of the rollback stack:
  `rustynes_netplay::SpectatorSession` replays a match's confirmed input stream
  into a local emulator, one frame at a time, the moment every player's real
  input for that frame arrives. It **predicts nothing, rolls back never, and
  sends nothing** (poll-only transport), so it is byte-identical to the players'
  confirmed timeline and invisible to the match it watches — the existing 2-4
  player rollback path is untouched (AccuracyCoin 100%, 139/139). The native
  Netplay panel gains a **Spectate** control (`netplay_ui::start_spectate`); the
  status bar shows `spectate fN +pending` (how far behind the live match). The
  byte-identical replay is unit-tested against a reference run; the host-side
  broadcast/relay + `deploy/` relay config remain a documented maintainer-manual
  carryover (see `docs/netplay-webrtc.md` §4). New module
  `crates/rustynes-netplay/src/spectator.rs`.
- **v1.7.0 "Forge" Workstream H9 — power-user niceties.** All additive,
  frontend-only, determinism-neutral. **Game Genie encoder** — a new pure
  `genie_encode` module produces a canonical 6-/8-character code from a known
  `(address, data[, compare])` substitution (the exact inverse of the core
  decoder; every code round-trips back through `rustynes_core::GenieCode::new`),
  surfaced as a "Game Genie encoder" section in the Cheats panel ("Encode" →
  "Add to list"). **`.tbl` text tables** — the same module parses the
  community `XX=glyph` table format and renders a byte stream into readable text
  (for non-ASCII game encodings in the hex editor / RAM search). **Movie
  subtitles → `.srt` export** — a new pure `movie_srt` module converts TAStudio
  markers into a frame-exact SubRip subtitle track at the region's frame rate
  (NTSC 60.0988 fps drift-free), via File → Movies → "Export subtitles (.srt)".
  New modules `crates/rustynes-frontend/src/{genie_encode,movie_srt}.rs`.
- **v1.7.0 "Forge" beta.5 — UI overhaul (#51/#52/#53/#55).** Frontend-only and
  determinism-neutral (the core stays byte-identical; AccuracyCoin 100%,
  139/139). (#51) The two controller HUDs were **consolidated into one "Input
  Display" panel** — the superset panel (standard pads + Zapper / Arkanoid Vaus /
  SNES mouse / Power Pad / Family Trainer mat / Family BASIC / Subor keyboard /
  Konami + Bandai Hyper Shot / Four Score multitap) kept its full capability and
  took the "Input Display" name. (#52) The menu bar was audited for the complete
  v1.7.0 feature set and **every entry now carries a Font-Awesome glyph**; the
  bottom status bar gained live recording markers (TAS movie REC/PLAY, A/V REC,
  HD-Pack REC) and a rich netplay read-out (peer role / ping / frame /
  rollback / stall). (#53) The **Help → Documentation** pane was overhauled —
  word-wrap, heading/code/bullet colorization, navigable sub-pages (a tree with
  per-chip-inspector and TAStudio child pages), and clickable intra-doc links
  (`[[id]]` / `[[label|id]]`), plus expanded content covering all v1.7.0
  features.
- **v1.7.0 "Forge" Workstream G2/G3 — NSF + MMC5 expansion-audio synthesis.**
  Classic `.nsf` files that declare expansion audio in the `$07B` bitfield now
  actually play it. A new thin router (`crates/rustynes-mappers/src/nsf_expansion.rs`,
  `NsfExpansion`) owns instances of the **existing** cartridge synth cores —
  VRC6 (`Vrc6Pulse`/`Vrc6Saw`), VRC7 (`rustynes_apu::Opll`, MIT emu2413 lineage —
  **not** Nuked-OPLL), FDS (`FdsAudio`), MMC5 (`Mmc5Audio`), Namco 163
  (`Namco163Audio`), Sunsoft 5B (`Sunsoft5BAudio`) — and routes the NSF
  register windows (`$9000-$B002`, `$9010`/`$9030`, `$4040-$408A`, `$5000-$5015`,
  `$4800`/`$F800`, `$C000`/`$E000`) into them, clocking on `notify_cpu_cycle`
  and fanning APU frame events on `notify_frame_event`. No synthesis is
  reimplemented — the bit-for-bit math is shared with the cartridge path, so an
  NSF VRC6/MMC5/etc. tune is identical to the cartridge. **G3** drives the
  cartridge `Mmc5Audio` core (the one expansion chip whose NSF synthesis had
  been started-but-deferred) for both cartridge-MMC5 and NSF-MMC5 playback.
  `NsfExpansion` is constructed only for NSF files and is unreachable from any
  oracle cartridge ROM, so existing audio is byte-identical: AccuracyCoin holds
  **100% (139/139)**, the chip stack stays `no_std` (feature-off `clock`/`mix`
  shims return silence), and base-2A03 NSFs carry no extra state (`exp_audio:
  None`, `caps() == NONE`). NSF save-states gain an additive v2 presence-byte
  tail (ADR-0003 style; v1 blobs still load). `docs/apu-2a03.md` gains an
  "Expansion-chip audio" section. The synth cores were exposed `pub(crate)` from
  their owning mapper modules for the reuse (cartridge code byte-identical).
- **v1.7.0 "Forge" Workstream G (G4 + G5) — broad movie-format import +
  HD-Pack Builder.** Both additive / output-only; the shipped / native / `no_std`
  / wasm builds stay byte-identical and AccuracyCoin holds **100% (139/139)**.
  Imported movies replay deterministically via the same canonical power-on
  alignment as the v1.6.0 `.fm2` importer. New ADR 0017 records the HD-Pack
  Builder.
  - **G4 — broad movie-format import** (`crates/rustynes-core/src/legacy_movie.rs`,
    `no_std`-clean; frontend dispatch in `app.rs`). Importers for the historical
    pre-`.fm2` TASVideos corpus so RustyNES can "play any NES TAS": **`.fcm`**
    (FCEUX / FCE Ultra — a sparse toggle/delta stream, decoded to a dense input
    log), **`.fmv`** (Famtasia — fixed 144-byte header, full per-frame dump with
    the Famtasia bit permutation), and **`.vmv`** (VirtuaNES —
    documentation-derived, since BizHawk never shipped a `.vmv` importer). Each
    rejects save-state-anchored starts (only `StartPoint::PowerOn` is portable).
    `.mc2` is handled with a clean, documented rejection (it is a PC Engine
    PCEjin/Mednafen format, not NES). Plus **`.fm2`/`.bk2` export hardening**: the
    matching ROM checksum is recomputed from the loaded ROM and stamped on
    (`base64:`-MD5 `romChecksum` for `.fm2`, hex SHA-1 `SHA1` for `.bk2`) so an
    exported movie is verifiable on TASVideos. The import file dialog now offers
    `fm2`/`bk2`/`fcm`/`fmv`/`vmv`.
  - **G5 — HD-Pack Builder** (`crates/rustynes-frontend/src/hdpack_builder.rs`,
    `hd-pack` feature + native; ADR 0017). The authoring counterpart to the
    existing pack *loader* (v1.2.0): an in-emulator recorder (Tools → HD Pack →
    Build HD Pack (Record) / Stop & Save) that observes the same per-frame PPU
    tile-source telemetry + CHR snapshot the compositor already captures, dedups
    the distinct tiles a game draws by their Mesen CRC-32 key, captures each
    tile's native 8x8 pixels, and emits a Mesen-compatible `hires.txt` +
    `tiles.png` starter pack that loads straight back through the loader. Reads
    only already-deterministic snapshots under the existing lock discipline;
    mutates no emulation state.
- **v1.7.0 "Forge" Workstream D — timeline + scaling rewind engine.** A
  scrubbable full-session timeline with clip export, plus a compressed,
  density-tiered greenzone that scales the TAStudio editor to feature-length
  TASes. Both ride the current PPU-dot scheduler (**no timebase change**), are
  additive / output-only / determinism-neutral, so AccuracyCoin holds **100%
  (139/139)** and the shipped / native / `no_std` / wasm builds stay
  byte-identical.
  - **D1 — HistoryViewer** (`crates/rustynes-frontend/src/history_viewer.rs`,
    native + `wasm-winit`). A bookkeeping layer over the per-frame rewind ring
    that records the live session's input stream (`FrameInput` per port) in
    lock-step with the emulator + periodically stashes a lightweight
    start-anchor save-state. Driven from `EmuCore::produce_one_frame` on
    persistent forward frames only (never a rewind step), after the `nes` borrow
    is released — it observes already-latched inputs + copies an already-produced
    save-state, so it cannot perturb emulation. New **Tools → Export Last 30s
    (.rnm)** assembles a `Movie` covering the trailing N seconds (start = the
    nearest anchor at-or-before the window) and writes a `.rnm` via the save
    dialog; the clip **replays bit-identically** (proven by a `MoviePlayer`
    round-trip test). Cleared on ROM load + power-cycle.
  - **D2 — Zwinder-class compressed tiered greenzone**
    (`rustynes_core::zwinder::ZwinderStateManager`, `#![no_std]` + `alloc`). The
    compressed successor to the v1.6.0 uncompressed greenzone: frame-keyed
    snapshots as **XOR-deltas + LZ4** against periodic keyframes, with reserved
    anchors (frame 0 / markers / branch points — always self-contained
    keyframes, never evicted) and density-tiered eviction over the *compressed*
    sizes. Source: BizHawk `ZwinderBuffer` / `ZwinderStateManager`. The TAStudio
    greenzone (`crates/rustynes-frontend/src/tastudio/greenzone.rs`) now stores
    its save-states through it (a thin `usize`-frame adapter), so the same RAM
    holds far more history while the deterministic seek/replay contract is
    unchanged. **Determinism gate:** compression is lossless —
    `restore(compress(store(s))) == s` byte-for-byte — proven by an in-module
    round-trip-equality test (keyframes + deltas + post-eviction) **and**
    `rustynes-test-harness` integration tests that drive real `Nes` snapshots
    through save → compress → decompress → restore and assert byte-equality.

- **v1.7.0 "Forge" Workstream E — host IPC / automation (RustyNES as a
  platform).** The power-user tier (modelled on BizHawk's `comm` / `client` /
  `userdata` libraries) that turns RustyNES into a host for external bots / RL
  agents / randomizers / stream tools — its determinism is a selling point for
  reproducible RL episodes. All additive; AccuracyCoin holds **100% (139/139)**
  and the core synthesis never sees any of it. New ADR 0016 records the
  host-mediated IPC security posture; `docs/scripting.md` documents the three
  tables.
  - **E1 — host-mediated `comm.*` IPC** (`crates/rustynes-frontend/src/script_host.rs`,
    behind a NEW off-by-default `script-ipc` feature; requires `scripting`). TCP
    (`comm.socketServerSend`), HTTP (`comm.httpGet` / `httpPost`), WebSocket
    (`comm.ws_open` / `ws_send` / `ws_close`), and a memory-mapped-file bridge
    (`comm.mmfWrite` / `mmfRead`), with `comm.receive()` polling the host-fulfilled
    results. The defining contract: **the Lua sandbox never gets a raw socket** —
    the script only queues a marshalled `CommCmd`, and the host (`ScriptHost`)
    owns every connection and does the I/O **off the emulator lock** on a worker
    thread, feeding plain `CommResult` values back. The no-`io`/`os`/`package`/net
    sandbox guarantee is preserved even with IPC on. IPC is a new
    non-deterministic source, so every `comm.*` verb is **gated like `emu.write`**
    (`set_writes_locked` + RA-hardcore): dropped at the source under netplay / TAS
    replay or record / RA-hardcore, so no `CommCmd` is queued and the host opens
    no connection. Off by default → the shipped / native-default / `no_std` / wasm
    builds are byte-identical. Pulls no new dependency into `rustynes-script`
    (reuses the frontend's existing native-only `ureq` for HTTP; TCP + the
    in-process MMF bridge use `std`). The full WebSocket client + an OS
    shared-memory MMF backing are documented maintainer follow-ups.
  - **E2 — `client.*` host-automation verbs** (`crates/rustynes-script` `client`
    table + `App::apply_script_client`; ships with the base `scripting` surface,
    no feature gate). `opentool`, `screenshot` / `screenshottoclipboard`,
    `setwindowsize`, `speedmode` / `frameskip`, `reboot_core`, `pause_av` /
    `unpause_av`, `addcheat` / `removecheat`. Collected (never applied inline)
    and drained by the host; the state-changing verbs (`reboot_core`, cheats) are
    gated like `emu.write` (dropped under a locked session), the observational
    verbs are presentation-only and never perturb the deterministic core.
  - **E3 — `userdata.*` persisted KV store** (`crates/rustynes-script` `userdata`
    table). A per-script string→string store (`set` / `get` / `containskey` /
    `remove` / `keys`) the host can snapshot/restore across runs (and into
    save-states). Script-local host memory, never emulator state, so it is not
    write-gated. (A SQLite-backed store is a documented later option, not in this
    pass.)

- **v1.7.0 "Forge" Workstream A — editing-capable debugger tools (the
  read-only → writable leap).** The inspect-only PPU/OAM panels become a
  creator/RE workbench; all writeback is `debug-hooks`-gated and routes through
  the SAME gated post-frame poke path the raw RAM cheats use, so it is a no-op
  under netplay / TAS replay or record / RA-hardcore and **byte-identical with
  the feature off**. AccuracyCoin holds **100% (139/139)**; the chip stack stays
  `#![no_std]`.
  - **A1 — tile/CHR + palette + nametable + OAM editors (writeback).** An
    "Edit (writeback)" toggle on the PPU panel exposes a palette-entry editor
    (click a swatch → edit the 6-bit value), a nametable tile/attribute editor
    (click a cell → edit the tile byte + the 2-bit attribute quadrant via a
    read-modify-write), and a CHR byte poker; the OAM panel gains a per-sprite
    Y/tile/attr/X editor. Writes queue as one-shot `DebugPoke`s drained after the
    next frame. New gated core hooks `Nes::debug_poke_ppu` / `Nes::poke_oam_byte`
    (`crates/rustynes-core/src/{nes,bus}.rs` + `crates/rustynes-ppu/src/ppu.rs`),
    plus a "locked = no-op = byte-identical" gate in the frontend produce path.
  - **A2 — iNES / NES 2.0 header editor + read-only "Cartridge Info" pane**
    (native-only; `crates/rustynes-frontend/src/debugger/header_editor.rs`,
    opened from **Debug → Cartridge Info / Header Editor...**). Inspects (read-only
    by default) and optionally edits the 16-byte header of a ROM file *on disk*
    (format, mapper, submapper, mirroring, PRG/CHR sizes, battery, trainer,
    region, console type, RAM sizes, Vs. DualSystem). Decode + re-encode reuse the
    core's canonical `parse_header` / `serialize_header` round-trip. Edits a file,
    never the running core.
  - **A3 — inline 6502 assembler** (`crates/rustynes-frontend/src/debugger/{cpu_panel,assembler.rs}`).
    Assembles one or more source lines (e.g. `LDA #$42`, `STA $0200,X`,
    `BNE $C010`) at a target address into the gated work-RAM poke path. The
    opcode-encoding table is **derived at runtime from the canonical
    disassembler**, so it can never drift from the CPU core's decode.

- **v1.7.0 "Forge" Workstream B — scriptable TAStudio + full Lua API parity.**
  The v1.6.0 piano-roll editor becomes *programmable* and the Mesen2 Lua surface
  is rounded out. All native-only, behind `scripting`; **byte-identical with
  `scripting` off** (the shipped / wasm / `no_std` builds don't pull
  `rustynes-script`), AccuracyCoin **100% (139/139)** held. Every mutator gates
  IDENTICALLY to `emu.write` — a silent no-op under netplay / TAS replay /
  RA-hardcore (`set_writes_locked`), proven by per-item gating tests.
  - **B1 — the `tastudio.*` Lua control API** (BizHawk `TAStudioLuaLibrary`
    model). Queries (`engaged` / `getrecording` / `getseekframe` /
    `getselection` / `islag` / `hasstate` / `getmarker` / `getbranches` /
    `getbranchtext` / `getbranchinput`) read a per-frame read-only editor
    snapshot the host pushes (the `set_symbols` pattern); mutators
    (`setrecording` / `togglerecording` / `setplayback(frame|marker)` /
    `setlag` / `setmarker` / `removemarker` / `submitinputchange` +
    `applyinputchanges` atomic-edit batch / `loadbranch` / `setbranchtext`)
    queue a gated `TasCmd` the host drains and applies to the live `TasEditor`.
    Self-contained in `crates/rustynes-script/src/tastudio.rs`.
  - **B2 — `tastudio` analysis-canvas callbacks.** `onqueryitembg|text|icon`
    (per-cell colour / text / icon — pure overlay) + `clearIconCache`, and the
    observational `ongreenzoneinvalidated(fn)` / `onbranchload(fn)`. The host
    queries via `ScriptEngine::query_tas_cell` and fires the events through
    `fire_greenzone_invalidated` / `fire_branch_load`.
  - **B3 — full Lua API parity** (Mesen2). `emu.getScreenBuffer()` /
    `getPixel(x,y)` (read the RGBA frame) + the gated, output-only
    `emu:setScreenBuffer(t)`; the full `emu.addEventCallback(fn, type)` enum
    (`nmi`/`irq`/`startFrame`/`endFrame`/`inputPolled`/`stateLoaded`/`stateSaved`);
    the **value-modifying** `emu.addMemoryCallback(fn, "write", start[, end])`
    (returns a replacement byte, poked back through the gated path — a scriptable
    cheat/watchpoint); the structured `emu:getState()` / gated `emu:setState(t)`
    CPU-register map; `emu.takeScreenshot()` (host PNG write, read-only); and the
    sandboxed `emu.getScriptDataFolder()`. New gated core hooks
    `Nes::debug_set_framebuffer` (+ `Bus`/`Ppu`) and `Nes::debug_set_cpu_state`
    (`debug-hooks`), reached only through the gated post-frame script path.

- **Mapper breadth 150 → 168 families** (v1.7.0 "Forge" Workstream G1 — next
  reusable-ASIC BMC/pirate cores, `crates/rustynes-mappers/src/sprint12.rs`). 18
  new **BestEffort** (Tier-2, honesty-gated, off the AccuracyCoin / commercial
  oracle) families ported from `Mesen2` + the nesdev wiki: the Waixing **FK23C**
  8/16 Mbit BMC (mapper 176 — a `$5000-$5003` config bank over a full MMC3
  surface with an A12 scanline IRQ + outer-bank / extended-MMC3 / CNROM-CHR
  modes), **COOLBOY / MINDKIDS** (268 — MMC3 + four `$6000-$7FFF` outer-bank
  registers), Sachen **9602** (513 — MMC3 + PRG-A19/A20 outer override) and
  **3011** (136 — the TXC protection accumulator driving an 8 KiB CHR select),
  Waixing **164** (split `$5000`/`$5100` PRG), **253** (*Dragon Ball Z*
  VRC4-clone — per-1 KiB CHR low/high regs, a CHR-RAM escape, a /114-scaled
  CPU-cycle IRQ) and **286** (BS-5 DIP-gated multicart), the **Kaiser**
  FDS-conversion family (56/142 KS202/KS7032 with an up-counting M2 IRQ, 303
  KS7017 with a down-counting M2 IRQ + `$4030` read-ack, and the PRG-window
  boards 305 KS7031 / 306 KS7016 / 312 KS7013B), and the BMC multicarts **261**
  (810544-C-A1) / **289** (60311C) / **320** (830425C-4391T) / **336** (K-3046)
  / **349** (G-146). Each is register-decode + save-state-round-trip unit-tested,
  pure / deterministic / `#![no_std]`. The `mapper_tier` honesty gate
  (`crates/rustynes-test-harness/tests/mapper_tier_honesty.rs`) stays green and
  **AccuracyCoin holds 100% (139/139)** — these additions are off the oracle, so
  the shipped / native / `no_std` / wasm builds are byte-identical. ROM staging
  and boot-smoke screenshots are a separate later coverage pass.

- **v1.7.0 "Forge" Workstream F — accuracy hardening (dot/CPU-cycle-granular,
  NOT the v2.0 timebase rewrite).** All additive / off-by-default; AccuracyCoin
  holds **100% (139/139)**, nestest 0-diff, the commercial oracle byte-identical.
  - **F1 — test-ROM hardening + a battery-save oracle.** A new
    `battery_save.rs` oracle (none existed): a synthetic battery-backed NROM
    fills `$6000` PRG-RAM with a known pattern and the test proves it survives a
    `snapshot`->`restore` round-trip (the battery-save persistence mechanism) and
    resumes bit-identically. Audited the existing F1 bundle wiring
    (`ppu_read_buffer`, `vbl_nmi_timing` x7, `sprdma_and_dmc_dma`, `dmc_tests`,
    `cpu_exec_space`, `read_joy3`, `volume_tests`, `scanline`) — all wired and
    green. `vbl_nmi_timing/5.nmi_suppression` already passes on this core, so it
    is kept as a live pin (not ignored) per the never-reduce-coverage contract.
    Holy-mapperel **M28/M118/M180** are now supported mappers but their ROMs are
    not in the committed corpus — recorded as a documented carryover.
  - **F2 — sub-v2.0 behavior audit.** The APU **length-counter halt/reload
    race** and the **DMC load-DMA even/odd-cycle delay** are both already
    implemented and verified on the current dot-lockstep scheduler; added named
    regression pins (`f2_accuracy_audit.rs`) gating the halt/reload race
    (`blargg_apu_2005/10.len_halt_timing` + `11.len_reload_timing`) and the DMC
    load even/odd defer (`dmc_dma_defer_load_entry`, exercised by
    `dmc_tests/latency` + `sprdma_and_dmc_dma`).
  - **F3 — PPU extra-scanlines overclock**, determinism-gated. New
    `Nes::set_extra_scanlines(n)` / `extra_scanlines()` insert `n` extra idle
    vblank scanlines per frame at the existing dot resolution (more CPU
    run-time, no visible change). **Off by default (`0`)** and **byte-identical
    at zero** (proved by `extra_scanlines.rs`), and distinct from the
    CPU-multiplier overclock (a v2.0 item). The configured count is a frontend
    knob re-applied on restore; the in-flight per-frame insertion countdown is
    snapshotted (PPU snapshot v4) so a save-state taken mid-insertion resumes
    exactly.

- **v1.7.0 "Forge" Workstream C — debugger depth (source-level / step /
  callstack).** All additive, output-only telemetry, gated behind the always-on
  frontend `debug-hooks` feature and **byte-identical with the core's
  `debug-hooks` feature OFF** (the headless test / bench builds), so the
  shipped / native / `no_std` / wasm builds are unchanged and **AccuracyCoin
  holds 100% (139/139)**. None of these are v2.0 items — they ride the current
  PPU-dot scheduler.
  - **C1 — call stack + step verbs.** A Mesen2-`CallstackManager`-class live
    6502 call stack (`crates/rustynes-frontend/src/debugger/callstack.rs`),
    rebuilt each frame by replaying the observational per-frame exec log +
    interrupt-service log: `JSR` pushes, `RTS`/`RTI` pops, and an unexplained
    non-sequential PC is correlated against the interrupt log to label an
    NMI/IRQ frame. Adds the stepping verbs **step-over / step-out / run-to-NMI /
    run-to-IRQ / step-scanline / step-frame** (the exec/interrupt-driven verbs
    ride the per-frame logs; scanline/frame ride frame-advance), surfaced in a
    new "Call stack" section of the CPU panel. Step completion pauses the
    emulator and opens the CPU panel, like a breakpoint hit. Output-only.
  - **C2 — memory access counter + uninitialized-read detection.** A
    Mesen2-`MemoryAccessCounter`-class per-address read/write/exec counter with
    last-access stamps and a sticky **uninitialized-read** flag (a read of
    volatile RAM — `$0000-$1FFF` / `$6000-$7FFF` — before it was ever written),
    folded from the per-frame access + exec logs
    (`crates/rustynes-frontend/src/debugger/access_counter.rs`). Surfaced as an
    "Access counters" section in the Memory panel. Output-only side-array.
  - **C3 — ca65/cc65 `.dbg` source-line mapping.** A frontend parser for the
    ld65 `--dbgfile` `.dbg` format
    (`crates/rustynes-frontend/src/debugger/source_map.rs`): it resolves each
    `line` record's spans through the `span`/`seg` tables to CPU addresses,
    building an `address -> (source file, line)` map that annotates the
    disassembly with the original source line (`; file:line`). Loaded via the
    existing **Debug -> Load Symbols** picker (now also accepts `.dbg`), pairing
    with the v1.4.0 `.sym`/`.mlb`/`.nl` symbol-name loader. Display-only.

### Testing

- **v1.7.0 "Forge" T-PS-059 — coverage harness archive/`.fds` ROM discovery.**
  The external-coverage boot-smoke harness now discovers ROMs inside `.zip` /
  `.7z` archives and `.fds` disk images (in addition to `.nes` / `.unf`),
  unwrapping an archive to its first NES/FDS/UNIF entry and routing an FDS disk
  through the BIOS path — mirroring the frontend's load dispatch so a dump left
  zipped or as a raw disk gets a boot screenshot just like a loose `.nes`. The
  archive reader is gated behind the existing `commercial-roms` feature and the
  new discovery only fires on the maintainer's local gitignored
  `tests/roms/external/` dumps (CI's committed corpus is `.nes`-only), so CI
  behaviour is unchanged. Commercial-ROM screenshot baselines for the
  newly-discoverable dumps remain a local maintainer follow-up.

### Performance

- **v1.7.0 "Forge" Workstream H7 (tier-1 perf, measure-first): no change adopted.**
  Measured both researched candidates against the **>3% Criterion-stable +
  byte-identical** bar; neither cleared it, so the emulation core stays
  byte-identical. T1.2 (unified-DMA cycle fast-path) is a no-op — the per-cycle DMA
  dispatch already sits behind a short-circuiting `unified_dma_pending()` floor and is
  inlined by the existing `lto = "fat"` profile (an explicit `#[inline]` measured "no
  change"). T1.3 (BLEP phase-row cache) is the same optimization the v1.4.0 F2 pass
  dropped: `Kernel::row()` runs only on signal edges (not per sample) and the `PHASES =
  256` kernel advances ~6.3 phase buckets per sample, so a phase-row cache has a
  near-zero hit rate. Both rejections are documented in `docs/performance.md`.

- **v1.7.0 beta.2 review — debugger-tooling allocation cleanups** (frontend-only;
  no core or default-path effect, so AccuracyCoin holds **100% (139/139)** and the
  builds stay byte-identical):
  - The access counter (`debugger/access_counter.rs`) folds the per-frame access
    and exec logs by iterating the borrowed slices directly instead of
    `collect`ing / `to_vec`-cloning them into fresh `Vec`s every frame.
  - The inline assembler (`debugger/assembler.rs`) builds its 256-entry
    `(mnemonic,mode) → opcode` table once via a `OnceLock` and reuses it, instead
    of re-deriving it (~256 disassemblies) on every assembled line.
  - The header editor (`debugger/header_editor.rs`) reads only the 16-byte header
    (and overwrites it in place via seek + partial write) instead of reading /
    rewriting the entire ROM file just to inspect or edit the header.

### Fixed

- **v1.7.0 beta.4 review — untrusted-movie-parsing hardening + rewind/NSF fixes**
  (all additive / determinism-neutral; imported movies still replay
  deterministically, the Zwinder round-trip stays lossless, the chip stack stays
  `no_std`, and AccuracyCoin holds **100% (139/139)**):
  - **`.fcm` import can no longer be forced into an unbounded allocation
    (security).** `decode_fcm_stream` now enforces a hard `1 << 24`-frame output
    cap *unconditionally* — previously the cap was skipped when the header frame
    count was `0`, so a crafted `.fcm` with a large delta-advance could emit
    millions of frames (DoS / OOM). The cap is the tighter of the header hint and
    the hard limit.
  - **`.fcm` rejects a header-overlapping input offset.** A `firstFrameOffset`
    below the `0x34` fixed header (which would parse header bytes as input) is now
    rejected as malformed (previously only an offset past EOF was caught).
  - **`.vmv` rejects a header-overlapping data offset.** A non-zero movie-data
    offset below the `0x40` header is rejected as malformed.
  - **`.vmv` rejects four-controller movies instead of silently dropping P3/P4.**
    `FrameInput` models only the two standard NES ports, so a `.vmv` enabling
    controllers 3/4 is now rejected up front (matching the `.fcm` four-score
    stance) rather than producing a "successful" import with missing inputs that
    would desync replay.
  - **Zwinder greenzone store decodes the preceding keyframe at most once.**
    `ZwinderStateManager::store` no longer calls `preceding_keyframe_raw` twice on
    the common delta path (once to test presence, once to fetch), halving the LZ4
    decompress + allocation in the greenzone hot loop. Lossless round-trip
    equality is unchanged.
  - **NSF v2 save-state tail is now consumed + validated, and its doc matches the
    implementation.** `NsfMapper::load_state` reads the v2 expansion presence byte
    and rejects a tail that disagrees with the `$07B` bitfield, so the blob fully
    round-trips. `NsfExpansion::save_state`'s doc-comment is corrected to state
    plainly that volatile oscillator/register state is *intentionally not*
    persisted — the chips are rebuilt from the immutable `$07B` bitfield and live
    phase re-converges from the next register write (correct for a paused/restored
    NSF); the presence byte only records which chips a v2 tail described.
- **HD-pack loader now parses the REAL Mesen `hires.txt` format** (task #56;
  `crates/rustynes-frontend/src/hdpack.rs`, `hd-pack` feature; ADR 0018). Every
  real third-party HD-pack previously failed to load with a red "hires.txt"
  status-bar error: the parser only accepted an invented `hash,image,x,y` /
  `image,x,y,hash` `<tile>` grammar and `.parse::<u32>()`'d x/y from the wrong
  positions, so a real `<ver>106` tile line
  (`<tile>bitmapIndex,tileData,palette,x,y,brightness,defaultTile[,chrBankPage,tileIndex]`)
  failed for **every** tile → zero rules → `load()` returned `None`. The parser now reads the
  real Mesen layout (verified against `Mesen2/Core/NES/HdPacks/HdPackLoader.cpp`):
  the real `<tile>` field order (`bitmapIndex,tileData,palette,x,y,brightness,
  defaultTile[,chrBankPage,tileIndex]`) with the 32-hex `tileData` (the tile's 16
  CHR bytes) as the match key (CRC-32, the exact key the compositor computes from
  the live CHR snapshot); `[Cond1&Cond2]` per-line **condition prefixes** (AND-
  joined, with `!name` inversion); hex condition memory addresses/operands/masks;
  `<img>` referenced by declaration index; the real `<background>`
  `name,brightness[,hScroll,vScroll][,priority][,left,top]` layout; and lenient
  `<ver>`/`<options>`/`<supportedRom>`/`#`-comment handling. A real *Zelda
  Remastered* `<ver>106` pack now parses from **0 → 15,849 tile rules**. The G5
  **HD-Pack Builder** was reconciled to **emit** the same real `<ver>106`
  `<tile>bitmapIndex,tileData,palette,x,y,brightness,defaultTile` form (the
  captured 16 CHR bytes as `tileData`), so author→load round-trips and its output
  is consumable by real Mesen tooling. A committed synthetic `<ver>106` fixture
  regression-guards the parser (no copyrighted assets committed). Output-only +
  `hd-pack`-gated: the core is untouched, AccuracyCoin holds **100% (139/139)**,
  and with the feature off the shipped / native / `no_std` / wasm builds stay
  byte-identical.
- **v1.7.0 beta.3 review — scriptable-TAStudio + host-IPC robustness** (all
  additive / off the default path; `scripting` / `script-ipc` stay byte-identical
  off; AccuracyCoin holds **100% (139/139)**):
  - **Host IPC never hangs the worker.** The outbound `comm.socketServerSend` TCP
    socket now connects with a bounded `TcpStream::connect_timeout` (2 s) and
    backs off reconnects to a dead/unreachable endpoint (5 s), so a script
    spamming sends can no longer stall the IPC worker thread on a blocking
    `connect`.
  - **Batched TAStudio input edits re-seek once.** `App::apply_tas_commands` now
    accumulates `SetInput` edits and performs the expensive deterministic re-seek
    a single time at the end of the batch (the `applyinputchanges` case), instead
    of once per edit.
  - **Idle TAStudio costs no per-frame clone.** The host now pushes a rebuilt
    `TasSnapshot` only when the editor's new `TasEditor::revision()` edit-counter
    moves (or it opens/closes); an idle editor no longer clones its input log,
    lag vector, markers, and branches every frame.
  - **Port validation at the script boundary.** `tastudio.submitinputchange` and
    `emu.setInput` now reject a controller port outside `{0, 1}` with a clear Lua
    error (the host treated any `port != 0` as P2), and the host-side
    `TasCmd::SetInput` mapping mirrors the rule defensively.
  - **Bounded `emu.addMemoryCallback` range.** The value-modifying write callback
    now rejects a range wider than 4096 addresses (it registered one Lua registry
    key per address, so a 64K span allocated 64K entries); a whole-RAM watch
    belongs on the observational `onWrite` hook.
  - **No double-borrow in `tastudio.query_cell`.** The cell-query callbacks are
    resolved into owned handles before any Lua runs, so a callback that registers
    another no longer double-borrow-panics (matching `fire_event`).
  - **Gate-first short-circuit + minor cleanups.** The value-modifying-write
    replay now checks `!writes_locked` first (skipping the whole loop and its
    callback work under a locked session); `Nes::debug_set_cpu_state` drops a
    pointless `const`; and the `TasSnapshot::lag` doc clarifies the dense-`bool`
    field vs. the tri-state `tastudio.islag` Lua return.

- **v1.7.0 beta.2 review — debugger-tooling robustness** (frontend-only;
  AccuracyCoin holds **100% (139/139)**):
  - The inline assembler now parses indexed-indirect `(zp,x)` case-insensitively
    (matching the other addressing modes), and the CPU panel validates the
    assemble target stays within work RAM (`$0000-$1FFF`) before queueing — the
    only region `DebugPoke::CpuRam` applies to — rejecting out-of-range targets
    and assembled bytes that would run past it.
  - The PPU/OAM/CPU panel hex parsers now strip an upper-case `0X` prefix as well
    as `0x`, so `0X..` inputs parse.

- **v1.7.0 beta.1 review fixes** (all additive / off the default path;
  AccuracyCoin holds **100% (139/139)** and the default `extra_scanlines == 0`
  path stays byte-identical):
  - **F3 — snapshot the in-flight extra-scanlines countdown.** A save-state
    taken mid-insertion (`extra_lines_remaining > 0`) previously restored as
    `0` and desynced. The PPU snapshot is bumped to **v4** to carry the
    countdown (v1-v3 blobs upconvert to `0`); at the default it is a zero `u16`
    so restore is behaviourally identical.
  - **F3 — reset the per-frame countdown when `set_extra_scanlines()`
    changes.** Reconfiguring the count (e.g. 8 → 2, or N → 0 disable) now
    cancels any in-flight insertion so the countdown cannot be left stale or
    out-of-bounds relative to the new value.
  - **F3 — avoid a `prerender_line() - 1` underflow.** The "line immediately
    before pre-render" check is rewritten as `scanline + 1 == prerender_line()`,
    removing a debug-mode panic risk should `prerender_line()` ever be `0`.
  - **Mapper 253 (Waixing VRC4-clone) CHR-RAM variant.** A CHR-RAM cart now
    allocates the conventional 8 KiB (not a 1 KiB stub) and `ppu_write` writes
    through the banked CHR store, so CHR-RAM is no longer effectively
    read-only; the CHR-RAM is serialized in the save-state.
  - **Mapper 176 (FK23C) CHR-ROM mutability.** `ppu_write` no longer writes
    through `self.chr` when the cart provided CHR-ROM (`select_chr_ram` set but
    `chr_is_ram == false`), so CHR-ROM is not mutated (which also was not
    serialized). CHR writes are gated on `chr_is_ram` for behaviour/serialization
    consistency.
  - **v1.7.0 "Forge" beta.5 (#53) — Documentation pane.** The Help →
    Documentation body text now word-wraps to the pane width (it previously
    overflowed in a non-wrapping monospace block), and the left-sidebar
    sub-level navigation entries resolve to real content instead of returning
    nothing.

### Changed

- **v1.7.0 "Forge" beta.5 (#55) — backtick key repurposed.** The backtick
  (`` ` ``) key no longer toggles the debugger overlay (which now opens from
  Debug → Show Debugger, every panel being menu-driven). It now toggles the
  status-bar RetroAchievements read-out between its compact and long-form
  variants — the only distinct content the removed debugger toolbar HUD carried.
  The Keyboard Shortcuts window reflects the new binding.

### Removed

- **v1.7.0 "Forge" beta.5 (#55) — debugger toolbar HUD.** The `debugger_top`
  toolbar panel was removed; its read-outs (frame/cycle, FPS, movie/disk/netplay
  status, RetroAchievements) are now surfaced in the bottom status bar.
- **v1.7.0 "Forge" beta.5 (#51) — standalone Input Display panel.** The old
  `debugger/input_display_panel.rs` + `ToolPanel::InputDisplay` (the simple
  pads-only HUD) were removed; the superset "Input Display" panel (formerly
  "Input Miniatures") absorbed it.

## [1.6.0] - 2026-06-18 - "Studio" (Feature Release)

### Added

- **Shader / filter ecosystem — LMP88959 NTSC/PAL, hqNx/xBRZ upscalers, +
  constrained RetroArch preset import** (v1.6.0 Workstream I, extends the v1.2.0
  composable `ShaderStack` / ADR 0013). Three new RGBA built-in passes join the
  Settings → Shaders "Shader stack" picker (each declaring its knobs via
  `#pragma parameter` headers that drive generic sliders, like the existing CRT
  pass): **`lmp88959`** (`crates/rustynes-frontend/src/ntsc_lmp88959.rs`) — an
  LMP88959-style composite NTSC/PAL look (per-texel encode-then-demodulate giving
  chroma bleed, dot crawl, and edge fringing; knobs `saturation` / `sharpness` /
  `tint` / `phase` / `pal`), which — unlike the index-only Bisqwit `composite-rt`
  pass — samples the RGBA framebuffer so it composes anywhere in the stack; and
  **`hqx`** / **`xbrz`** (`crates/rustynes-frontend/src/upscale.rs`) —
  hqNx- and xBRZ-style edge-directed pixel-art smoothers (single-pass GPU
  adaptations of the published edge-blend kernels; independent WGSL, each with a
  `strength` knob). Plus a **constrained RetroArch `.slangp` / `.cgp` preset
  importer** (`crates/rustynes-frontend/src/slang_preset.rs`, Settings → Shaders →
  "Import .slangp / .cgp…"): it parses a preset and maps well-known shader
  filename stems (`crt-*`, `*ntsc*`/`*composite*`, `*hqx*`/`*hq2x*`,
  `*xbr*`/`*xbrz*`) onto the built-in passes, carrying over matching parameter
  overrides — it is **not** a GLSL/Slang → WGSL transpiler (source translation
  stays out of scope per ADR 0013), and passes with no built-in equivalent are
  reported as **unsupported** (surfaced with a mapped/unsupported count, never
  silently dropped). Everything is **output-only** (post-framebuffer, never
  touching the core, the index framebuffer, or determinism) and **off by default**
  (an empty / all-disabled stack is the byte-identical direct blit), so
  **AccuracyCoin holds 100% (139/139)** and the shipped / `no_std` / wasm builds
  stay byte-identical. The shaders matter for wasm, and both wasm clippy combos +
  `trunk build` pass. Unit-tested: the `.slangp`/`.cgp` parser + honest-reject
  path, the stack wiring + param forwarding, and a WGSL parse+validate gate for
  every new pass; visual shader output is a maintainer manual-verify (it can't be
  headless-checked). (See `docs/frontend.md` §"CRT / NTSC shaders" and ADR 0013's
  v1.6.0 supplement.)
- **HD-pack HD audio — `<bgm>` / `<sfx>` OGG tracks via the `$4100` control
  register** (v1.6.0 Workstream H, native + default-OFF `hd-pack` feature; the
  biggest Mesen2 gap vs ADR 0014). The HD-pack `hires.txt` parser
  (`crates/rustynes-frontend/src/hdpack.rs`) now reads `<bgm>album,track,file`
  (looping background music) and `<sfx>album,track,file` (one-shot sound effects)
  declarations, and a new module
  (`crates/rustynes-frontend/src/hd_audio.rs`) decodes their OGG Vorbis files to
  mono PCM (pure-Rust `lewton` decoder, pulled in only by `hd-pack` — no C, no
  extra system deps; default / wasm builds never see it) and runs an
  `HdAudioMixer`. The mixer is an **output-only** tap, the audio analogue of the
  HD tile-substitution on the framebuffer and Workstream G's recording tap: it
  sits in the FRONTEND audio path on top of the buffer the core already produced
  (`Nes::drain_audio_into`), and each produced frame `EmuCore::produce_one_frame`
  *peeks* the `$4100` HD-pack audio-control register (a side-effect-free read of
  the already-produced bus state) and, on the selector's change edge, sums the
  decoded track into the drained APU buffer **in place** before the DRC stage. It
  touches no emulation state and the core's deterministic per-frame audio
  (save-state / TAS / netplay) is unaffected; the mixer is `Option`-gated on
  `EmuCore`, so with no audio pack loaded — or the feature off — the audio is
  byte-identical and **AccuracyCoin holds 100% (139/139)**. The `$4100` selection
  is best-effort (RustyNES does not intercept the register write — it reads the
  value back and edge-detects, faithful on carts that map `$4100` into readable
  expansion space, inert on open-bus carts, a documented honesty caveat like the
  BestEffort mapper tier); folder packs are supported, `.zip`-pack audio + the
  full `$4100`..`$4106` state machine are noted future extensions. Audible
  playback is a **maintainer manual-check** (no audio device in CI); the
  `<bgm>`/`<sfx>` parse, the `$4100` trigger-edge logic, and the mixer buffering
  are unit-tested. (See `docs/frontend.md` §"HD-pack HD audio" + ADR 0014.)
- **A/V recording — synchronized video + audio capture** (v1.6.0 Workstream G,
  native + default-OFF `av-record` feature). A new frontend module
  (`crates/rustynes-frontend/src/av_record.rs`) records the running game to an
  `.mp4` / `.mkv`. Capture is a **read-only tap on the already-produced output**:
  inside `EmuCore::produce_one_frame`, *after* the emulator produces the frame,
  the recorder copies the visible framebuffer (RGBA8 256x240 — the same source
  the screenshot path reads) and the same audio samples the audio sink received
  that frame (mono `f32`). It never advances the emulator or alters the per-frame
  framebuffer / audio, so the **determinism contract is untouched** and
  **AccuracyCoin holds 100% (139/139)**; with the feature off the produce path is
  byte-identical and the module is not compiled (the shipped / wasm / `no_std`
  builds are unchanged). The encoder is an **external `ffmpeg` pipe** — rawvideo
  (`rgba`, exact rational region frame rate) over stdin + a mono `f32le` audio
  temp sidecar as a second input (avoiding a two-pipe deadlock), muxed to H.264 +
  AAC — so the default build pulls **no extra Rust deps** (only the system
  `ffmpeg` at run time, with a graceful "ffmpeg not found" fallback). Wired into
  **Tools → Record A/V** via `MenuAction::AvRecordToggle` dispatched after the
  egui pass; start opens an rfd save dialog (default
  `<data_dir>/recordings/<rom>-<utc>.mp4`), a second click stops + finalizes;
  wasm = no-op (gated out). Unit-tested parts: ffmpeg arg construction, container
  inference, framebuffer-stride guard, sidecar path, start/stop state. **Deferred:**
  the FCEUX-style Code/Data Logger; on-device encoded-file playback is a
  maintainer manual-verify (CI cannot exercise the codec). Enable with
  `cargo build -p rustynes-frontend --features av-record`. (See `docs/frontend.md`
  §"Workstream G — A/V recording".)
- **Off-axis accuracy verification + documentation** (v1.6.0 Workstream D). A
  pin-test-first audit of the dot/CPU-cycle-granular off-axis accuracy cluster
  confirmed the cycle-accurate engine (plus the v1.4.0 DMC-DMA pass) already
  models every Workstream D target, with all committed oracles passing and
  AccuracyCoin holding 100% (139/139). No engine change was made — a speculative
  edit here could only risk the oracle for zero oracle benefit. The as-built
  models are now documented in lockstep: **D1** the complete DMC-DMA and
  OAM-DMA ↔ `$4016`/`$4017` controller-read double-clock / dropped-bit conflict
  model (verified by `dmc_dma_during_read4/dma_4016_read.nes`,
  `double_2007_read.nes`, `sprdma_and_dmc_dma{,_512}.nes`, and the `AccuracyCoin`
  `APU Register Activation` Tests 5-7; see `docs/cpu-6502.md` "DMA ↔
  controller-read conflicts"); **D2** the `$2007` (PPUDATA) read-during-active-
  rendering render-buffer window with the deferred state-machine reload and
  `v`-increment glitch (`ppudata_sm_countdown` / `ppudata_v_inc_pending`, the
  `AccuracyCoin` `$2007 Stress` bracket; see `docs/ppu-2c02.md`); **D3** the
  buggy sprite-overflow `n+m` OAM-index evaluation and the three-group MDR /
  open-bus decay timer (verified by `sprite_overflow_tests` 4/5 and
  `ppu_open_bus` tests 7/9; already documented in `docs/ppu-2c02.md`). The Test
  5/6 active-window-mirror refinement and the `$2002` NMI-suppression race
  remain deferred to the future v2.0 fractional-master-clock refactor (ADR 0002).
- **FDS-proper — timed disk-head position + `$4032` auto-insert + per-game CRC
  quirk table** (v1.6.0 Workstream F, modelled on puNES `fds.c`). The FDS RAM
  adapter (`rustynes-mappers::Fds`) now models the belt-driven drive's physical
  rewind/re-seek: a motor restart after the cold spin-up rewinds the disk to the
  disk-start gap and opens a short deterministic head re-seek not-ready window
  (`HEAD_RESEEK_CYCLES`) before bytes stream again, rather than the head
  teleporting to track 0. `$4032` (drive status) now presents the not-ready ->
  ready transition the BIOS waits for on **every** re-read (the auto-insert
  behaviour), and a per-game CRC quirk table (`quirk_for_crc` / `FdsQuirk`,
  keyed off the headerless disk-image CRC-32 via the new `no_std` `fds_crc32`)
  lets individual titles request extra re-seek slack. The general timed
  head-position model closes the **Kid Icarus side-B post-registration** replay
  (deferred since v1.0.0): the BIOS re-read loop now sees the drive report
  not-ready while the head returns, so the post-registration screen streams its
  blocks. Cycle-count-based — NOT the v2.0 master-clock axis. Additive +
  determinism-preserving: AccuracyCoin holds 100% (139/139), FDS save-state
  round-trip + boot suite stay green; the quirk is derived from immutable
  construction inputs (not serialized). (See `crates/rustynes-mappers/src/fds.rs`
  and `docs/STATUS.md`.)
- **Lua driving primitives — `emu.run` + `emu.frameadvance`** (v1.6.0 Workstream
  B2). A script can now *drive* the emulator a frame at a time (the FCEUX /
  BizHawk model) instead of only reacting to per-frame callbacks: `emu.run(fn)`
  registers a driving coroutine and `emu.frameadvance()` (a thin alias of Lua's
  `coroutine.yield`) hands exactly one frame to the emulator before resuming the
  coroutine. This unblocks the bot / TAS-script ecosystem. Native-only (the mlua
  backend), the same carve-out as the dev/TAS API; a driver's `emu.setInput` /
  `emu.write` / `load_state` effects are **gated identically to `emu.write`**
  (silent no-op under netplay / TAS replay / RA-hardcore), so driving is
  determinism-safe. Bundled example `examples/scripts/driving_loop.lua`. (See
  `docs/scripting.md`.)
- **External TAS movie interop — `.bk2` (BizHawk) import/export ↔ `.rnm`**
  (v1.6.0 Workstream B1). A new `no_std` core module (`bk2_interop`) parses /
  emits a `.bk2`'s `Header.txt` + `Input Log.txt` text members (the NES
  `U D L R S s B A` mnemonic layout, console-buttons group dropped, P1/P2
  mapped), mirroring the existing FCEUX `.fm2` interop; the `.bk2` ZIP container
  is read / written frontend-side (keeping the core `no_std`). A new **Movies
  (TAS) → Import / Export (.fm2 / .bk2)** menu pair wires both formats: import
  begins playback against the running ROM, export writes the in-progress
  recording (or the loaded movie). Imported movies use the **canonical
  movie-import power-on alignment** (a deterministic zeroed-RAM cold boot via
  `Movie::seek_to_start`) and inherit the running ROM's SHA-256 identity, so they
  replay without desync. Save-anchored and non-NES `.bk2` movies are rejected
  cleanly. (See `docs/scripting.md` / `docs/adr/0008-tas-movie-format.md`.)
- **Debugger depth — expression/conditional breakpoints + R/W/X watchpoints +
  watch window + conditional trace** (v1.6.0 Workstream C, the Mesen2-class C1
  keystone + C4 free riders). A new frontend expression evaluator
  (`debugger::expr`) compiles a Mesen-`ExpressionEvaluator`-style string —
  CPU regs (`a x y s p pc`), PPU `scanline`/`cycle`/`frame`, memory `[addr]`
  (byte) / `{addr}` (LE word), access-context tokens (`value`, `address`,
  `isRead`/`isWrite`/`isExec`), and the full C operator set (`+ - * / % & | ^ ~
  << >> && || ! == != < > <= >=`, ternary, parens) — and drives a new **Watch /
  Breakpoints** panel (Debug → Watch / Breakpoints): conditional exec
  breakpoints (PC or range + optional condition), read/write/exec watchpoints
  (address range + access class + optional condition), a watch window (a list of
  expressions shown each frame), and a conditional trace logger (format-string
  rows filtered by a condition). All **observational** — `App::pump_watchpoints`
  replays the just-finished frame's exec/access logs after the frame, exactly
  like the Lua `onExec`/`onRead`/`onWrite` hooks (ADR 0010); it never intercepts
  mid-instruction or mutates deterministic state, so AccuracyCoin (139/139) and
  byte-identical builds hold. Behind the always-on-in-frontend `debug-hooks`
  feature. (See `docs/frontend.md`.)
- **Hex editor — in-place poke + freeze + access heatmap + find** (v1.6.0
  Workstream C, C2). The **Memory** panel is now a full hex editor: CPU bus /
  PPU bus / OAM domain tabs; click-to-poke a CPU work-RAM byte (`Nes::poke_ram`,
  `$0000-$1FFF` writable, other domains read-only); right-click to **freeze** a
  byte (emitted as a `RawCheat` re-applied after every frame, routed through the
  existing raw-cheat overlay like Mesen/FCEUX); an **access-type heatmap** that
  tints bytes read (blue) / written (red) in the last frame off the `debug-hooks`
  access log; and a **find** box for a hex byte sequence. The no-edit path is
  byte-identical and determinism holds (reads are side-effect-free peeks; the
  only write is the work-RAM poke/freeze applied like a cheat). (See
  `docs/frontend.md`.)
- **RAM Search + RAM Watch upgrade** (v1.6.0 Workstream C, C3). The **Memory
  Compare** panel is upgraded to the BizHawk/FCEUX-class tool: RAM Search gains
  an **operator × compare-to matrix** (`== != < > <= >=` against the previous
  snapshot OR a typed constant) and **1/2/4-byte little-endian sizes**, with
  per-candidate **watch** / **freeze**; a new **RAM Watch** list holds named
  `(address, size, label)` entries with live values, per-entry freeze (routed
  through the raw-cheat overlay; multi-byte freezes expand per LE byte), and
  native **`.wch` save/load**. Read-only against the core (freeze cheats are the
  only writes, applied post-frame), so the no-freeze path is byte-identical.
  (See `docs/frontend.md`.)
- **Lua data breadth — memory domains, sized reads, `joypad`** (v1.6.0
  Workstream B3). The `memory` table gains `memory:read_u16_le(addr)` /
  `memory:read_u16_be(addr)` (16-bit word reads, two side-effect-free CPU
  `peek`s) and `memory:read_oam(index)` (the sprite-RAM domain, alongside the
  existing CPU `peek` and PPU `peek_ppu`). A new `joypad` table adds
  `joypad:get(port)` (the latched standard-controller bitmask, read-only) and
  `joypad:set(port, buttons)` (gated identically to `emu.setInput`). All reads
  are observational — they never perturb the deterministic run. (`mlua` native
  backend; see `docs/scripting.md`.)
- **TAStudio piano-roll TAS editor** (v1.6.0 Workstream A2). A new Tools →
  TAStudio window: a vertically-scrolling piano-roll over the editor model —
  one row per frame, one column per controller button, with click-to-toggle
  and column drag-paint, cursor / lag / marker row tinting, a forkable-branch
  list, seek-on-frame-click, insert/delete-frame, and `.rnmproj` project
  save/load (native). The grid virtualizes its rows, so very long movies stay
  cheap. Like the other tool windows it queues edits/seeks as requests applied
  under the emu lock after the egui pass; seeking re-derives state by replaying
  inputs, so determinism is preserved. Native-only file I/O for now (browser
  `.rnmproj` is a parity follow-up). Additive + off the default path.
- **FCEUX `.fm2` movie interop** (v1.6.0 Workstream B1). RustyNES `.rnm`
  movies now round-trip with the **FCEUX `.fm2`** text movie format — import a
  `.fm2` as a playable movie and export the current movie to `.fm2` — over the
  same deterministic input model that backs the TAStudio editor (Workstream A).
  Pure core / movie plumbing (`no_std`); the deterministic per-frame output is
  untouched, so AccuracyCoin holds 100% (139/139).
- **J.Y. Company ASIC mappers (iNES 90 / 209 / 211)** (v1.6.0 Workstream E,
  BestEffort tier). One silicon implementation behind three iNES mapper
  numbers — 90 inhibits the ROM-nametable / extended-mirroring feature via a
  board jumper, 209 register-enables it, and 211 forces it on (a 209 duplicate
  defined before `$D001` bit 3 was understood). `crates/rustynes-mappers/src/jy_asic.rs`
  covers the four PRG modes (32/16/8 KiB + 8 KiB with the low-7 bank bits
  reversed), the four CHR modes (8/4/2/1 KiB), the CHR-block outer-bank mode,
  the MMC4-like CHR auto-latch (mapper 209), ROM nametables / extended
  per-1 KiB CIRAM mirroring, the hardware multiplier, and the configurable
  prescaler+counter IRQ with all four clock sources (CPU M2 / PPU A12 rise /
  PPU reads / CPU writes). Ported from the NESdev "J.Y. Company ASIC" page and
  the Mesen2 `JyCompany` implementation. Register-decode + save-state
  unit-tested only and not in the AccuracyCoin oracle, so AccuracyCoin holds
  100% (139/139) and the `mapper_tier_honesty` gate stays green (ADR 0011).
- **Mapper breadth 126 → 150 families** (v1.6.0 Workstream E continuation,
  BestEffort tier). +24 honesty-gated families ported from the NESdev wiki and
  the Mesen2 reference cores. The J.Y. Company ASIC gains its single-game
  "extended" sibling **mapper 35** (`jy_asic.rs`, same silicon as 209). A new
  `crates/rustynes-mappers/src/sprint11.rs` adds: a shared MMC3-style core
  (eight bank registers, the `$8000`/`$A000`/`$C000`/`$E000` protocol, an A12
  falling-edge IRQ) wrapped by the **MMC3-clone** variants
  **44 / 49 / 52 / 115 / 134 / 189 / 205 / 238 / 245 / 348 / 366** (each adds a
  board-specific outer-bank register + PRG/CHR transform); the **Sachen 8259
  A/B/C** 2 KiB-CHR variants **141 / 138 / 139** (siblings of the existing 8259D
  mapper 137, differing only by a CHR shift + per-slot OR constants); and the
  discrete unlicensed / FDS-conversion / multicart boards **42** + **50** (each
  with a CPU-cycle M2 IRQ) and **46 / 51 / 57 / 104 / 120 / 290 / 301**
  (hook-free). Every new family is register-decode + save-state-round-trip
  unit-tested and classified `BestEffort` in `mapper_tier` — outside the
  AccuracyCoin / commercial-ROM oracle by construction — so AccuracyCoin holds
  100% (139/139), the `mapper_tier_honesty` gate stays green (ADR 0011), and the
  shipped / native / `no_std` / wasm builds stay byte-identical
  (additive, off the deterministic-core path). (See `docs/mappers.md`.)
- **UNIF (`.unf`) cartridge loader** (v1.6.0 Workstream E2). UNIF carries no
  mapper number — it identifies the cartridge by a board-name string in its
  `MAPR` chunk. `rustynes_mappers::unif` parses the header + chunks
  (`MAPR`/`PRG?`/`CHR?`/`MIRR`/`BATR`/`TVCI`), resolves the board to an iNES
  mapper (puNES/Mesen2 table, vendor-prefix-tolerant, incl. the Sachen 8259
  A/B/C/D split), and synthesizes an equivalent NES 2.0 image so the standard
  `parse()` path builds the cartridge + mapper. Unlocks UNIF-only dumps that
  have no iNES equivalent. Determinism/oracle unaffected (additive format path).

### Fixed

- **A/V recording produced broken / empty audio (Workstream G).** The recorder
  spawned `ffmpeg` at arm time with the still-empty audio sidecar passed as a
  regular-file `-i` input; `ffmpeg` reads a regular-file input to EOF eagerly at
  startup, so it saw an empty audio file before any samples were written. The
  recorder now buffers **both** rawvideo and mono-`f32le` audio to temp files
  while recording (no child process is alive, so there is no read-before-write
  race and no two-pipe deadlock) and muxes the two COMPLETE files with a single
  `ffmpeg` invocation at `stop()`; `ffmpeg` availability is still probed at
  arm time so arming fails gracefully when it is absent. Output-only and
  determinism-safe; still default-OFF behind `av-record`.
  (`crates/rustynes-frontend/src/av_record.rs`.)
- **Mapper 301 (BMC-8157) ignored the A7 outer-bank select.** The PRG bank decode
  dropped address line A7 (the 256 KiB outer-bank bit), so any PRG image larger
  than 256 KiB could only reach its low half. A7 is now slotted between the
  128 KiB (A5-A6) and 512 KiB (A8) selects. BestEffort, honesty-gated.
  (`crates/rustynes-mappers/src/sprint11.rs`.)
- **Mapper 50 (Alibaba / SMB2J conversion) latched a stale IRQ on enable.** On an
  IRQ-enable write (`$4120` bit 0), the counter was not reset and any pending
  line was not cleared, so a fresh enable after a prior fire could trip
  immediately on the stale counter. Enable now resets the counter to 0 and
  clears the pending flag, per the hardware. (`crates/rustynes-mappers/src/sprint11.rs`.)
- **FDS per-game CRC quirk table shipped a fabricated placeholder key.** The
  `quirk_for_crc` timing-quirk table carried a hard-coded "Kid Icarus"
  placeholder CRC-32 that would have applied unverified head-reseek slack to any
  real disk that happened to hash to it. The table now ships **empty** (entries
  are added only from real, maintainer-measured dumps); the Kid Icarus side-B
  fix is title-independent (the general timed disk-head model) and never relied
  on the table. (`crates/rustynes-mappers/src/fds.rs`.)
- **Shader preset import counted stock / pass-through passes as unsupported.**
  `import_preset` reported `stock` / `passthrough` / `pixellate` stages as
  *unsupported* even though `map_stem_to_builtin` classified them as skip-able —
  inflating the unsupported count for presets that are perfectly importable.
  Pass-through stages are now skipped silently (not counted as unsupported).
  (`crates/rustynes-frontend/src/slang_preset.rs`.)
- **Memory hex editor allowed misleading no-op pokes outside work RAM.** The
  editor let you click-to-poke / freeze any CPU address, but `Nes::poke_ram` is
  a no-op outside `$0000-$1FFF`, so edits to ROM / register / mapper space
  silently did nothing. Poke + freeze are now restricted to `$0000-$1FFF` work
  RAM (matching the module's documented contract), and the help text says so.
  (`crates/rustynes-frontend/src/debugger/memory_panel.rs`.)
- **Mapper 30 (UNROM-512) self-flashing carts blank boot.** *Wampus* and the
  *PROTO DERE .NES* beta booted to a solid backdrop because the board always
  applied bus conflicts. Per NESdev "UNROM 512" (and Mesen2's `UnRom512`), a
  submapper-0 cart with the iNES **battery bit set** has *no* bus conflicts and
  its banking latch responds only to `$C000-$FFFF` (with `$8000-$BFFF` the
  flash-write window) — applying bus conflicts ANDed the boot-time bank-switch
  value with ROM and jumped the CPU into garbage. The mapper now keys the
  bus-conflict / flash wiring off submapper + battery, reads CHR-ROM when a dump
  carries it, and re-derives H/V/1-screen/4-screen nametable wiring from the raw
  iNES byte-6 bits. Both homebrews now render gameplay. (`sprint9.rs`.)
- **Mapper 80 (Taito X1-005) blank boot.** *Kyonshiizu 2* booted to a solid blue
  frame because only two of the chip's **three** switchable 8 KiB PRG banks were
  modelled — the `$7EFE`/`$7EFF` register for `$C000` was missing (treated as a
  fixed bank), stranding the reset bank. Added the third PRG register (with its
  odd-address alias) so only `$E000` is fixed, and corrected the `$7EF6`
  mirroring polarity (0 = Horizontal, 1 = Vertical, per nesdev mapper 080 /
  Mesen2). Kyonshiizu 2 now renders its title screen. (`taito_x1_005.rs`.)
- **Mapper 185 (CNROM CHR-disable protection) — Seicross.** *Seicross* hung in
  its copy-protection loop (a solid grey frame, rendering never enabled): it
  reads CHR back after a protection write and proceeds only when CHR reads as
  *disabled*, but the generic submapper-0 heuristic enabled CHR for the `$21`
  latch. Seicross is really submapper 4 (enabled iff the latch low bits are `0`,
  matching FCEUX `Sync181` / BizHawk's Seicross special-case), which the mapper
  already models correctly. Fixed via a per-game DB submapper correction
  (`game_database.txt` CRC `0F05FF0A` → submapper 4); `apply_header_overrides`
  now promotes an iNES-1.0 header to NES 2.0 when a non-zero submapper override
  is set, so the correction reaches the core. The cycle-accurate core is
  untouched. (`game_db.rs`, `game_database.txt`.)
- **Mapper 159 (Bandai LZ93D50 + X24C01 EEPROM) blank boot.** All mapper-159
  games (Dragon Ball Z - Kyoushuu! Saiya Jin, both Magical Taruruuto-kun
  titles, SD Gundam Gaiden) booted to a 1-colour blank screen because the
  Bandai FCG serial-EEPROM state machine mismodeled the X24C01: it clocked
  bits only on the SCL rising edge and shifted the address/data MSB-first,
  whereas the X24C01 advances its mode/ACK handshake on the falling edge and
  shifts **LSB-first** (the 24C02 on mapper 16 is MSB-first). The games
  busy-waited on the EEPROM probe and never proceeded. The `Eeprom` machine in
  `crates/rustynes-mappers/src/bandai_fcg.rs` is now a faithful port of the
  Mesen2 `Eeprom24C01` / `Eeprom24C02` models (rise/fall-split protocol,
  per-chip bit order, and X24C01 combined-byte addressing vs. the 24C02
  device-select-then-word-address sequence). Mapper 159 is not in the
  AccuracyCoin oracle, so the fix is AccuracyCoin- and determinism-neutral
  (100% (139/139) held).

- **BestEffort mapper decode fixes from the per-mapper screenshot-coverage
  pass** (all off the AccuracyCoin oracle; AccuracyCoin holds 100% (139/139)
  and the `mapper_tier_honesty` gate stays green; each was verified to render
  a previously-blank commercial boot). Checked against puNES / Mesen2 / the
  NESdev wiki:
  - **m250 Nitra:** the MMC3 even/odd register line is A10 (`0x0400`), not A8.
  - **m177 Hengedianzi:** `$8000` PRG bank is bits 0-4 only (bit 5 = mirroring).
  - **m178 Waixing FS305:** `$4800` bit 0 is mirroring and bits 1-2 are the PRG
    mode (they were swapped); the 4 documented PRG modes are now implemented.
  - **m162 Waixing FS304:** rewrite the PRG-bank decode per the NESdev
    A15-A20/`$5300`-mode table (reset boots 32 KiB bank #2) and add the
    `$6000-$7FFF` battery PRG-RAM the RPGs read at boot.
  - **m156 DAOU/DIS23C01:** CHR-nibble registers decode the slot as
    `(addr&0x03)+(addr>=0xC008?4:0)` with bit 2 selecting the high/low array;
    `$C014` selects H/V mirroring from a single-screen-A power-on.
  - **m244 Decathlon:** decode the written DATA byte through the two scramble
    LUTs with bit 3 selecting CHR vs PRG.
  - **m227 BMC 1200-in-1:** decode `s_flag`/`prg_mode`/`l_flag` per Mesen2 and
    compose the `$8000`/`$C000` 16 KiB pair from the documented mode table.
  - **m233 BMC 42-in-1:** the PRG-mode bit was inverted — reg bit 5 set selects
    16 KiB mode (puNES), clear selects the 32 KiB pair.
  - **m185 CNROM-protect:** CHR powers on ENABLED (was deriving disabled from a
    zero latch, blanking the title), and the submapper-0 heuristic is
    `(value & 0x0F) != 0 && value != 0x13`.
  - **m147 Sachen 3018 / TXC JV001 & m150 Sachen 74LS374N:** port the JV001
    handshake + bank decode (and the m150 PRG/CHR bank composition) bit-for-bit
    from puNES.
- **Blank-boot fix code-review hardening (m30 / m80 / m185).** Follow-ups from
  the PR #127 review, all behaviour-neutral for the affected commercial ROMs
  (the m30/m80 renders stay byte-identical):
  - **m30 bus-conflict source byte.** A `$C000-$FFFF` write on a bus-conflict
    cart now ANDs against the *fixed last 16 KiB bank* (the bank actually mapped
    at the write address), not the currently-selected low bank — matching
    Mesen2's address-based conflict resolution.
  - **m30 submapper 3 runtime mirroring.** Submapper 3 now flips H/V at runtime
    from latch bit 7 (`set` → Vertical, `clear` → Horizontal; power-on Vertical),
    instead of being stuck in one mode. A new `M30Nametable::SwitchableHv` wiring
    carries it. The header-mirroring comment was corrected (UNROM-512 uses the
    *standard* iNES byte-6 convention — no inversion — matching Mesen2), and the
    4-screen variant is documented as an honest single-screen approximation
    (true 4-screen would need CHR-RAM-backed nametables; no corpus ROM uses it).
  - **m30 save-state index masking.** `load_state` masks the PRG/CHR bank
    indices to their live widths so a corrupted/hand-edited state can't seed an
    out-of-range value (mirrors the JY-ASIC clamp).
  - **m80 save-state version bump.** The switchable-PRG array grew 2 → 3 entries
    (the `$C000` register), so `SAVE_STATE_VERSION` is bumped 1 → 2 and the
    version byte is now checked before the length, rejecting a legacy version-1
    state cleanly with `UnsupportedVersion` instead of a confusing `Truncated`.
  - **m185 header-promotion sanitization.** When `apply_header_overrides`
    promotes an iNES-1.0 header to NES 2.0 (the Seicross sub-0 → sub-4 path), it
    now zeroes byte-8's low nibble (the new mapper bits 8-11) and bytes 9-15 (the
    newly-meaningful NES-2.0 fields), so legacy garbage can no longer change the
    mapper number or fabricate RAM/timing fields.

### Testing

- **Vs.-aware + per-ROM-override boot coverage.** The `external_coverage`
  harness now detects Vs. System carts (iNES mapper 99/151 or the header Vs.
  flag) and applies the per-game DB's RGB-PPU type + DSW0 default and pulses a
  coin so they leave the attract loop (mirrors the frontend's `apply_vs_db`);
  and a per-ROM `capture_override` gives title-screen titles (Dr. Mario,
  Dragon Warrior, Metroid, Gyromite, Lagrange Point, both Harikiri baseball
  games, etc.) a passive `IdleOnly` capture instead of the START-tapping
  default that advanced them into a blank transition. Both are accuracy-
  neutral (the coverage harness is a screenshot/boot-smoke net, not the
  oracle) and no-ops on non-Vs./non-listed ROMs.

- Add `external_coverage` — a data-driven, auto-discovering commercial-ROM
  boot-coverage harness (`crates/rustynes-test-harness/tests/external_coverage.rs`,
  gated `commercial-roms`). One test discovers every ROM staged under
  `tests/roms/external/mapper-*/`, runs a default boot capture, and asserts
  each (a) is not a blank/few-colour boot via a shared distinct-colour health
  heuristic and (b) matches its committed `insta` baseline + PNG dump. New
  ROMs need no code change; a fresh checkout (no staged ROMs) skips cleanly.
  Reference-only — never feeds the AccuracyCoin/oracle gate.
- Factor the ROM walk + the blank-frame distinct-colour/dominant-fraction
  health heuristic out of the `coverage_smoke` / `render_smoke` diagnostic
  bins into a shared `rustynes_test_harness::coverage` module so the bins and
  the new harness apply the same detector. Both bins are kept (free-form CLI
  triage tools); only the duplicated logic was removed.

## [1.5.0] - 2026-06-17 - "Lens" (Feature Release)

### Added

- **v1.5.0 "Lens" Workstream A — debugger visualization (beta.1).** Four
  GeraNES-class *visualization* devtools, all output-only and determinism-neutral
  (the new core telemetry is `debug-hooks`-gated and off in the headless / shipped
  builds, so AccuracyCoin holds 100% (139/139) and the shipped / native / `no_std`
  / wasm builds stay byte-identical to v1.4.1):
  - **Input Miniatures overlay (A1).** A live **Tools -> Input Miniatures** panel
    drawing every connected input device with real-time button / axis feedback —
    the standard pads (P1..P4, multitap with the Four Score) plus whatever
    non-standard device occupies the port-2 / expansion slot: Zapper (trigger +
    light-sensor strip), Arkanoid Vaus (paddle knob + button), SNES mouse
    (buttons + motion delta), Power Pad / Family Trainer mat, Family BASIC / Subor
    keyboard (pressed-key count), Konami / Bandai Hyper Shot. Reads the same
    host-side input snapshot the emulator is fed
    (`debugger/input_miniatures_panel.rs`); no new core surface.
  - **Graphical PPU Event Viewer (A2).** The **Debug -> Event Viewer** panel is
    now a full 341 x 312 per-dot **read/write heatmap** (blue = PPU-register read,
    red = write) with hover/click cycle metadata (register name, value, scanline,
    dot) and a synchronized register-access table. Backed by a new
    `debug-hooks`-gated PPU-register **read** capture in the event log
    (a new `EventKind::PpuRead` plus a `value` byte on `EventRec`); bounded by the
    existing per-frame cap, output-only.
  - **PPU scanline-trace viewer + CHR->PNG export (A3).** The **Debug -> PPU**
    panel gains a **Scanline trace** tab (per-scanline scroll/render register-write
    trace — $2000/$2001/$2005/$2006 — derived from the event log, surfacing
    mid-frame raster splits) and a **CHR -> PNG export** of the combined 256x128
    pattern dump (native).
  - **HD-pack per-pixel inspector (A4; native + `hd-pack`).** A new
    **Tools -> HD Pack -> Pixel Inspector** window: per-pixel HD-pack composition
    trace via a new `HdCompositor::inspect_pixel` query — the dominant tile's CHR
    identity + Mesen hash, the matched replacement rule + image, the gating
    `<condition>` names with their per-frame outcomes (ADR 0014), base vs final
    RGBA, and an original/mod blend slider. The v1.4.0 D3 carryover; builds on the
    HD-pack tile-source telemetry. Display-only — the compositor reads the same
    deterministic per-frame snapshots `composite` consumed and mutates nothing.
- **v1.5.0 "Lens" Workstream F — mapper-breadth continuation (113 → 123).** Ten
  more BestEffort (Tier-2) mapper families ported into `sprint10.rs` from the
  nesdev decode tables (and the `Mesen2` / `GeraNES` / `puNES` references):
  **40** (NTDEC 2722, *SMB2J* pirate — fixed PRG + switchable `$C000` window +
  a 12-bit M2 IRQ), **81** (NTDEC Super Gun, CNROM-like), **95** (NAMCOT-3425,
  *Dragon Buster* — MMC3-subset + CHR-bit one-screen select), **112** (NTDEC
  ASDER / Huang-1, indexed register port, no A12 IRQ), **137** (Sachen 8259D,
  `$4100`/`$4101` command/data), **156** (DIS23C01 DAOU, split low/high CHR
  registers + one-screen select), **162** (Waixing FS304, *San Guo Zhi II* —
  nibble-composed 32K PRG), **178** (Waixing educational series, a
  `$4800-$4803` register block with work-RAM), **244** (Decathlon,
  address-decoded multicart), and **250**
  (Nitra, *Time Diver Avenger* — MMC3-register-compatible with the data carried
  in the address bits + an M2 IRQ counter). Each has a register-decode and a
  save-state round-trip unit test. All BestEffort: register-decode tested only
  and structurally excluded from the AccuracyCoin / commercial-ROM oracle gate
  (ADR 0011 honesty gate held), so AccuracyCoin stays 100% (139/139), the
  determinism contract is intact, and the chip stack still cross-compiles to
  `thumbv7em-none-eabihf` (`#![no_std]` + alloc).
- **v1.5.0 "Lens" Workstream B — Lua dev/TAS API depth (beta.2).** The native
  (mlua) Lua engine grows from an overlay/observe surface into a real dev/TAS
  automation surface. All behind the existing off-by-default `scripting` feature;
  every state-mutating call is gated **identically to `emu.write`/`setInput`** (a
  silent no-op under netplay / TAS-replay / RetroAchievements-hardcore via
  `set_writes_locked`), so the determinism contract holds and AccuracyCoin stays
  100% (139/139). Native-only (the same carve-out as `onExec`/`onNmi`; the
  experimental piccolo/wasm backend keeps the v1.2.0 subset). Builds with the
  feature off are byte-identical to v1.5.0 beta.1.
  - **Memory API (B1).** A `memory` table for explicit CPU + PPU-space access:
    `memory:peek(addr)` / `memory:read_range(addr, len)` (CPU, side-effect-free
    debug-peek — `$2002` does not clear VBL, `$2007` does not advance the read
    buffer) plus `memory:peek_ppu` / `memory:read_range_ppu` (the `$0000-$3FFF`
    PPU bus), and `memory:poke(addr, val)` / `memory:write_range(addr, bytes)`
    (system-RAM writes, gated like `emu.write`).
  - **Cart / system queries (B2).** A read-only `cart` table:
    `cart:mapper_id()`, `cart:prg_size()`, `cart:chr_size()`, `cart:sha256()`
    (lowercase hex of the ROM SHA-256), `cart:region()` (`"NTSC"`/`"PAL"`/
    `"Dendy"`), and `cart.frame`. Backed by new read-only core accessors
    (`Nes::prg_rom_len`/`chr_rom_len`/`mapper_id`).
  - **Save-state scripting (B3).** `emu:save_state(slot)` / `emu:load_state(slot)`
    to in-memory script slots (reusing `Nes::snapshot`/`restore`), distinct from
    the host's on-disk numbered slots — a TAS/analysis script can checkpoint and
    roll back without touching the user's save files. Save is read-only (always
    allowed); load is gated like a write (no-op + returns `false` under lock).
  - **Debug hooks for scripts (B4).** `emu:on_breakpoint(addr, fn)` (observational,
    replayed from the per-frame exec-PC log like `onExec`), `emu:pause_at_frame(n)`
    (queues a one-shot pause when the frame count reaches `n`), and a `sym` table —
    `sym:addr("name")` / `sym:name(addr)` — that resolves against the v1.4.0
    debugger symbol-file labels the host now pushes into the engine on script load
    and on every symbol load/clear.
  - **Examples + docs + tests (B5).** Three bundled example scripts
    (`examples/scripts/memory_scanner.lua`, `tas_frame_analysis.lua`,
    `game_state_tracker.lua`), an expanded `docs/scripting.md` API reference, and
    new `rustynes-script` tests covering the new API, the side-effect-free peek
    contract, the write-gating of `poke`/`write_range`/`load_state` under a locked
    session, and a guard that every bundled example loads + runs.
- **v1.5.0 "Lens" Workstream C — creator / TAS / speedrun tooling (beta.2).**
  All additive / off-by-default; replay stays bit-identical and AccuracyCoin holds
  100% (139/139):
  - **TASVideos / extended emulator-test pass (C1).** Audited RustyNES against the
    Nesdev "Emulator tests" + "Tricky-to-emulate games" indices and the
    `christopherpow/nes-test-roms` aggregator for committable tests beyond the 139
    AccuracyCoin battery. Wired the older **`mmc3_test` v1** suite (6 sub-ROMs,
    blargg/kevtris PD, distinct from the existing `mmc3_test_2`): 1/2/3
    strict-PASS, and 4/5/6 pinned as documented expected-fail probes that converge
    on the *same* ADR-0002 fractional-master-clock scanline-IRQ-cadence residual as
    `mmc3_test_2/4` #3 (no new bug — sub-scanline IRQ cadence deferred to v2.0).
    Added **`dpcmletterbox`** (Damian Yerrick, royalty-free) as a deterministic
    framebuffer-hash visual smoke — it uses the DMC "sample finished" IRQ as a
    scanline timer (no mapper IRQ), so it is a sensitive DMC-IRQ + sprite-0 +
    NMI/DMC-phase sentinel. (`tests/mmc3.rs`, `tests/tasvideos_extended.rs`,
    `tests/roms/LICENSES.md`, `docs/testing-strategy.md`.)
  - **Replay / TAS window polish (C2).** The TAS movie status surface now reports a
    **device topology** (the controller / peripheral occupying each port) and a
    **timebase / frame readout** (current frame, total, region Hz, elapsed time),
    with **seek-to-frame** and frame step controls in the playback UI. Frontend-only
    over the existing `MovieUi` / `rustynes_core::Movie` machinery — replay re-drives
    the same `set_buttons` + `run_frame`, so it stays bit-identical (no new
    determinism surface).
  - **NSF waveform visualizer (C3).** The NSF Player window gains a per-channel
    oscilloscope (pulse 1/2, triangle, noise, DMC) sampled from the read-only
    `apu_snapshot()` DAC levels, plus the expansion-audio chip name + a master-mix
    trace when an expansion chip (VRC6/VRC7/FME-7/N163/MMC5/FDS) is present.
    Output-only eye-candy over the existing NSF/EQ path — samples a copy for
    display, no synthesis change.
- **v1.5.0 "Lens" Workstream I — native-UI fixes + menu overhaul + Documentation
  (beta.3).** Frontend-only and determinism-neutral (AccuracyCoin 100% (139/139)
  held; the feature-off / `no_std` / wasm builds stay byte-identical). The
  additive / changed UI:
  - **In-app Documentation pane (I10).** A new **Help -> Documentation** window
    (`debugger/doc_panel.rs`, native) reusing the SAME `cli::HELP_TOPICS` registry
    as `rustynes help` so the CLI and GUI manual share one source — plus
    GUI-specific topics (menu map / devtools / settings), an **About** card, and a
    **per-release CHANGELOG** selector. `/`-style search filters the topic list.
  - **Mapper panel depth (I8).** The Mapper debugger panel now shows mapper
    id + submapper, accuracy **tier** (Core/Curated/BestEffort), PRG/CHR ROM + RAM
    sizes with bank counts, battery/NVRAM, the **IRQ mechanism** (PPU A12 /
    scanline / CPU-cycle), and the expansion-audio chip — alongside the existing
    live bank windows + IRQ + register state. Driven by new output-only
    cartridge-metadata fields the bus fills on the existing `MapperDebugInfo` view
    (byte-identical; no per-mapper changes, no determinism surface).
  - **Keyboard Shortcuts (I9).** The Help -> Keyboard Shortcuts window now reads
    the **live** `[input]` / `[input.system]` bindings (not hardcoded defaults),
    separates emulator hotkeys from the controller mapping, and adds a **device
    selector** (Player 1-4 / Power Pad / Family BASIC keyboard).
  - **Input Display colours (I5).** Per-button-group palette — D-pad green,
    Select/Start yellow, B/A Nintendo red (`#E60012`) — in a shared `input_colors`
    module mirrored by the A1 Input Miniatures overlay.
  - **RetroAchievements status in the status bar (I7).** The RA readout
    (`RA n/total (pts) [HARDCORE]`) is now shown in the bottom status bar between
    the emulator-state label and the FPS counter.
  - **Settings polish (I3).** The Shaders tab's "Shader stack (composable)" header
    defaults open; the Input tab now auto-saves every control in both the Settings
    window and the standalone Tools -> Input window, and the redundant "Save to
    disk" button is relabelled **"Export config..."** (a real config-export to a
    chosen file).
  - **Game-Genie DB picklist (I4).** Confirmed shipped: the Cheats panel already
    surfaces the loaded ROM's known Game Genie codes from `genie_database.tsv`
    feeding the existing `genie.rs` decode + `cheats.rs` persistence.
- **v1.5.0 "Lens" Workstream H — frontend pacing & audio-sync performance + perf-log
  parity (beta.3).** Measure-first corrections to the frontend pacing/present/audio
  layer a real high-refresh perf capture flagged
  (`perf-logs/perf-Super_Mario_Bros_nes-20260616-231215.csv`: flat ~8.5 ms frame
  cost but recurring 50-128 ms produce stalls, climbing catch-up bursts /
  snap-forwards, audio queue oscillating 68-91 ms around a 60 ms target with
  underruns). All `rustynes-frontend` except an allocation-only core tweak; the
  determinism contract holds (AccuracyCoin 100% (139/139) + visual golden + APU
  oracle byte-identical, no `.snap` churn):
  - **H1 — decoupled triple-buffer framebuffer handoff** (`present_buffer.rs`).
    The present path's 240 KiB framebuffer copy moved OFF the emu mutex (which it
    formerly held, serializing the present against the full ~8.5 ms
    `produce_one_frame` on the dedicated emu thread) onto a triple buffer guarded
    by a small dedicated mutex held only for the brief copy. The emu thread
    publishes each produced frame; the common present path (no NTSC composite-rt /
    HD-pack) takes the freshest frame without ever blocking on produce. Native +
    `emu-thread` only; the synchronous + wasm paths keep the prior locked copy.
  - **H2 — pacer stall phase-break.** When the gap since the last scheduled frame
    already exceeds the catch-up window (`MAX_CATCHUP_FRAMES`*period) — an OS
    deschedule / UI stall, not a cadence — the produced/presented interval phase
    is broken before the gap is recorded, so a transient stall no longer dominates
    `produced_max` and reads as sustained judder. The existing cap + emu-thread
    priority elevation already bound the snowball + address the descheduling root
    cause. Perf-ring bookkeeping only.
  - **H4 — audio DRC + buffer tuning.** Widened the DRC band from +/-0.5% to +/-1%
    (~17 cents, far below audibility) so the servo can drain a catch-up-burst
    over-fill in ~5 s instead of ~10 s and the queue tracks the target instead of
    oscillating; plus a one-time +20 ms latency-target bump on high-refresh panels
    (>75 Hz) for ring headroom against the larger bursts. Audio *timing* only — the
    core's emitted samples (determinism + audio oracle) are untouched.
  - **H5 — GPU pass timing on by default.** The `gpu-timing` feature is now in the
    default native set, so the Performance panel + perf log report a real `gpu_ms`
    instead of a blank `-`. Timestamp queries are a side channel (requested only
    when the adapter offers `TIMESTAMP_QUERY`) — the presented image is
    byte-identical with it on/off, and the wasm builds are unchanged. The panel's
    pacer-anomaly readout also surfaces the worst recent present gap.
  - **H8 — perf-log <-> panel parity.** The CSV exporter (`perf_log.rs`) is rebuilt
    from a single ordered `columns()` list shared by the header + every data row,
    and a `csv_columns_cover_panel_metrics` test asserts every Performance-panel
    metric has a column (so the two can't silently drift again). Newly logged +
    panel-surfaced: `present_mode_fell_back`, `target_ms`, the audio DRC servo ratio
    and latency setpoint, and the run-ahead depth/throttle plus rewind
    enabled/buffered state.
  - **H7 — perf-log regression gate.** `scripts/perf/perf_capture.sh` drives a
    bounded windowed capture (perf logging auto-enabled via the new
    `RUSTYNES_PERF_LOG` env hook) and `scripts/perf/perf_log_check.py` parses the
    CSV and asserts `underruns` / `produced_max` / `catchup_bursts` / `snap_forwards`
    stay within bounds, turning them into a tracked signal. (Pacing/audio behavior
    needs a real display + audio device, so the capture is a maintainer-local /
    on-display gate, skipping cleanly when headless — like the bench ceiling's
    non-flaky philosophy.)
- **v1.5.0 "Lens" Workstream D — UX polish.** Frontend-only and
  determinism-neutral: every new field is `#[serde(default)]` returning today's
  value, every mode is off / neutral by default, and none touch core synthesis —
  so the feature-off / `no_std` / wasm builds stay byte-identical and AccuracyCoin
  holds 100% (139/139). The additive UI:
  - **Full palette editor (D1).** A **Settings -> Video -> Palette** section
    extends the v1.1.0 `.pal` loader + viewer into a named-palette bank
    (`[graphics.palettes]` + `[graphics] active_palette`): a live active-palette
    picker (built-in / any saved entry), an 8x8 per-index colour-picker editor,
    Save-As, import-a-`.pal`-into-the-bank, and delete. The selected palette is
    applied to the core via the existing `set_custom_palette` (presentation-only;
    built-in / unselected is byte-identical) and survives ROM loads. The legacy
    single `.pal` file path is preserved underneath.
  - **Overscan WYSIWYG editor (D2).** A **Settings -> Video -> Overscan
    (per-side)** group with live Top / Right / Bottom / Left pixel sliders + reset,
    alongside (and combined with) the legacy "Hide overscan" toggle. The blit
    uniform's overscan crop is generalized from the binary top/bottom-8 form to a
    per-side crop on both axes (the U/V remap is now in the `gfx`, CRT, NTSC,
    Bisqwit, and shader-stack final-pass shaders); all-zero + toggle off is
    byte-identical.
  - **"Enhancements" grouped settings (D3).** A new
    **Settings -> Emulation -> Enhancements (non-accuracy)** group + `[enhancements]`
    config section consolidating the non-accuracy enhancement modes (disable
    sprite limit / optional overclock) and cross-linking the max-rewind window.
    Each is off by default, clearly labelled, and **never applied while the
    determinism oracle / `AccuracyCoin` / TAS / netplay run**. NOTE: the
    sprite-limit-disable + overclock toggles persist the user's intent and are
    surfaced as *experimental / staged* — the cycle-accurate core has no hook for
    them yet (deferred to the v2.0 fractional-master-clock refactor, ADR 0002), so
    they are inert today and do not affect the deterministic core output.
  - **Device-config controls (D4).** The Input rebind panel now shows
    contextual device config for the selected port-2 device: SNES-mouse reported
    sensitivity (low/medium/high, the 2-bit serial field — was hardcoded `0`) + a
    pointer-speed (DPI) multiplier, Arkanoid Vaus pointer-speed, and a Power Pad /
    Family Trainer mat layout side (A / mirrored B). New `[input]` fields
    (`mouse_sensitivity`, `pointer_scale`, `power_pad_layout`); all defaults match
    the prior behaviour so the device report / input is byte-identical.
- **v1.5.0 "Lens" Workstream E — accessibility.** Frontend-only and
  determinism-neutral; all additive and off-by-default so the shipped / native /
  `no_std` / wasm builds stay byte-identical and AccuracyCoin holds 100%
  (139/139). Broadens reach for low-vision, colorblind, and keyboard-only users:
  - **Configurable UI scaling (E1).** A new `[ui] zoom_factor` setting (default
    `1.0`) scales the entire egui shell — menu bar, Settings, debugger panels,
    fonts — via `ctx.set_zoom_factor`. The emulated NES image is a raw
    framebuffer blit (not egui content) and is unaffected, so gameplay and
    determinism are untouched. Exposed as a **UI scale** slider (50%-300%, in 5%
    steps) with a Reset button in **View -> Settings -> Video -> Accessibility**;
    clamped on apply and persisted.
  - **Colorblind-safe + high-contrast themes (E2).** The light/dark/system theme
    selector gains two accessibility variants, wired into BOTH the **View ->
    Theme** menu and the Settings combo (single-sourced via `AppTheme::all()`):
    **High Contrast** (near-black/near-white WCAG 2.1 AA/AAA foreground pairs +
    bold focus strokes for low vision) and **Colorblind-Safe** (a dark theme
    whose interactive accents use the deuteranopia/protanopia-friendly Okabe-Ito
    palette). Both serialize with stable keys (`high-contrast`/`colorblind`), so
    existing configs are unchanged and the default stays Dark.
  - **Keyboard-only menu navigation (E3).** Audited the egui shell for
    mouse-free operation: the menu bar and Settings are already Tab/arrow/Enter
    navigable, and the **Settings / About / Keyboard Shortcuts** modal windows
    now close on **Esc** (egui's `Window` X-button has no key equivalent, and the
    app's Esc/Quit binding is suppressed while a shell window is open), giving
    every modal a consistent keyboard escape hatch.
- **v1.5.0 "Lens" Workstream G — casual-mode browser RetroAchievements
  (EXPERIMENTAL, the ADR 0015 carryover).** The buildable parts of browser RA,
  all behind the default-OFF, wasm-only `browser-cheevos` feature, so the shipped
  native + both default wasm builds are byte-identical and AccuracyCoin holds 100%
  (139/139); native RetroAchievements is unaffected:
  - **Emscripten rcheevos→wasm build track (proven).** `scripts/cheevos/build_rcheevos_wasm.sh`
    compiles the SAME vendored rcheevos sources + defines the native `build.rs`
    uses (26 translation units) with `emcc` to a loadable side module
    (`web/cheevos/rcheevos.wasm` + `.js`, gitignored build artifacts). It is a
    **separate artifact, not linked into the Rust `.wasm`**: trunk builds
    `wasm32-unknown-unknown`, whose ABI is incompatible with an emscripten `.a`.
    The Rust side reaches it through the committed `web/cheevos/ra_glue.js` host
    surface, bound by `crates/rustynes-frontend/src/wasm_cheevos.rs`'s
    `#[wasm_bindgen]` bridge.
  - **Casual-only is structural, not a toggle.** Hardcore is impossible in the
    browser at three independent layers: the Emscripten module never exports
    `rc_client_set_hardcore_enabled`, `ra_glue.js` exposes no hardcore method, and
    `BrowserRaSession` has no hardcore field/API (`hardcore_blocks()` is
    `const false`). The auth-proxy stub also refuses to forward a hardcore award.
  - **Auth-proxy contract + deployable stub.** RA's identifying HTTP `User-Agent`
    is browser-forbidden, so server calls route through a proxy that injects it
    server-side. Contract: `scripts/cheevos/auth-proxy.example.toml`; reference
    stub (stdlib-only Python): `scripts/cheevos/auth_proxy_stub.py`; full spec:
    `docs/cheevos-browser.md`.
  - **Loud, persistent in-UI caveat.** The wasm frontend renders a top-anchored
    banner: always casual-only + experimental, and (when the proxy is unset) that
    login + unlocks are unavailable — nothing silently pretends to work.
  - **Still maintainer-manual (no headless path, per ADR 0015):** deploy the auth
    proxy, finish the `ra_glue.js` rc_client trampoline marshalling, and verify a
    casual unlock live in a browser with a real RA account.

### Performance

- **H3 — reuse the rewind keyframe-cache allocation** (`rustynes-core`). The
  run-ahead snapshot buffer + the per-frame rewind snapshot buffer + the XOR delta
  scratch already reused their allocations (v2.8.0 Phase 3); the remaining
  steady-state heap churn in the rewind hot path was the keyframe-cache update
  doing a fresh ~9 KiB `to_vec()` (~1/s). It now overwrites the cache buffer in
  place. Bit-identical bytes — allocation strategy only; determinism + AccuracyCoin
  re-verified byte-identical, `no_std` clean.

### Fixed

- **GPU pass timing (H5) crashed at startup on adapters without
  `TIMESTAMP_QUERY_INSIDE_ENCODERS`.** The default-on `gpu-timing` feature's
  `GpuTimer` brackets the render encoder with `CommandEncoder::write_timestamp`,
  which requires the `TIMESTAMP_QUERY_INSIDE_ENCODERS` wgpu feature — but the
  device only requested/gated on `TIMESTAMP_QUERY`, so wgpu validation aborted
  (SIGABRT) at the first frame on any adapter that exposed `TIMESTAMP_QUERY`
  without the inside-encoders capability. The device now requests both
  timestamp features (whichever the adapter offers) and arms the timer only when
  both were granted; otherwise GPU timing stays disabled (`gpu_ms` reads `-`)
  instead of crashing. Presented image is byte-identical either way.
- **Closed-PR review-comment triage.** Adjudicated the backlog of open bot-review
  threads against current `main` and adopted the pertinent, still-actionable ones
  (all additive / off-by-default, byte-identical for valid states; AccuracyCoin
  holds 100%):
  - NSF player: clamp the starting/restored song index to the song count
    (`NsfMapper::new` + `load_state`) so a malformed header or corrupt save-state
    can't seed an out-of-range song; doc/wording corrected to classic `NESM` only.
  - Audio EQ: guard `f32::clamp` against a `NaN` gain deserialized from
    `config.toml`; treat a flat (all-zero) EQ as disengaged to skip the stage; add
    a Nyquist guard to the peaking biquad so a low host sample rate can't produce
    an unstable filter; apply EQ-slider gain live but persist on release; gate the
    EQ Settings block out of the wasm build (no cpal handle there).
  - Mapper 76 (Namcot-3446): use `saturating_sub` for the `$C000` fixed window so a
    single-8 KiB-bank ROM can't underflow in debug builds (output-identical).
  - HD-pack loader (off by default): accept the `$` hex prefix in condition rules
    and round (not truncate) the source-over alpha blend.
  - Frontend hygiene: clear the sibling presentation buffers on ROM close; reuse a
    single work-RAM snapshot per Memory-Compare filter step; preserve the Events
    panel aspect ratio on wide windows; drop a leaked IndexedDB upgrade closure on
    wasm; map Mesen `.mlb` `W`/`S` symbol types; minor allocation cleanups.
  - Mappers: drop a redundant `& 0x0F` mask in the Vs. DualSystem header check.
- **v1.5.0 "Lens" Workstream I — three native-UI bugs from hands-on testing**
  (frontend-only, determinism-neutral; AccuracyCoin holds 100%):
  - **Copy Screenshot to Clipboard no-op on Linux (I1).** The throwaway
    `arboard::Clipboard` was dropped immediately after `set_image`, so on X11 /
    Wayland (where the clipboard is owned by the live process) the image vanished
    while the status toast still claimed success. Hold a persistent session-long
    clipboard handle on `App`; report a real failure instead of a false success.
  - **Frame Advance (`\`) + Fast Forward (`Tab`) dead (I2).** egui consumed
    `Tab`/`\` for menu/widget focus navigation before the hotkey handler ran. The
    keyboard gate is split into text-input (block everything — a field is focused)
    vs egui-busy (route **system hotkeys only** via the new
    `InputState::handle_system_key`, never the NES controller), so the global
    hotkeys fire even when egui claims the key; the Emulation-menu predicates are
    fixed too (Frame Advance enabled only while paused; Fast Forward shows a live
    ON state instead of a permanently greyed hint). Adds hotkey-mapping tests.
  - **Tools -> ROM Database wouldn't open standalone (I6).** The locked render
    branch (which passes a live `&mut Nes` to the egui pass) keyed only on the
    Cheats panel, so the `nes`-reading ROM Database panel rendered only while
    Cheats was also open. `any_nes_tool_open` now also checks the ROM Database
    flag (and documents that EVERY `nes`-reading tool panel must be listed there).

## [1.4.1] - 2026-06-16

**Patch** — four more BestEffort mapper boot/decode fixes surfaced by the
boot-smoke-against-real-dumps pass, plus the boot-smoke screenshot corpus
reorganized to mirror the per-mapper `tests/roms/` tier layout. All fixes are to
Tier-2 BestEffort boards (excluded from the AccuracyCoin / commercial-ROM oracle
by the honesty gate), so AccuracyCoin holds 100% (139/139) and the shipped /
native / `no_std` / wasm builds remain byte-identical to v1.4.0.

### Fixed

- **Mapper 92 (Jaleco JF-19) PRG window layout.** The JF-17/JF-19 family shared
  one register model that always put the switchable bank at `$8000-$BFFF` and the
  fixed bank at `$C000-$FFFF`. JF-19 (mapper 92) is the mirror image — fixed FIRST
  bank at `$8000-$BFFF`, switchable bank at `$C000-$FFFF` — and the reset vector
  lives in the fixed half, so the wrong layout meant the board never booted. Added
  a `switchable_high` layout flag (`crates/rustynes-mappers/src/sprint6.rs`).
- **Mapper 94 (UN1ROM, *Senjou no Ookami*) bank decode + bus conflict.** The
  bus-conflict AND used a different window mapping than `cpu_read` (so a register
  write in the `$C000-$FFFF` fixed half ANDed against the wrong byte), and the
  16 KiB bank was decoded as a 4-bit field instead of the correct 3-bit (8-bank)
  `(data >> 2) & 0x07`. Both fixed in `crates/rustynes-mappers/src/sprint8.rs`.
- **Mapper 145 (Sachen SA-72007) 16 KiB PRG.** Required a 32 KiB-multiple PRG and
  rejected the real 16 KiB NROM-128-style dumps (e.g. *Sidewinder*); now accepts
  any non-zero 16 KiB multiple and mirrors a sub-32 KiB bank across the CPU window
  (`crates/rustynes-mappers/src/sprint6.rs`).
- **Mapper 147 (Sachen 3018 / TXC JV001) protection handshake.** Replaced the
  simple data-latch stand-in with a faithful model of the JV001 scrambling-
  accumulator ASIC: the boot code writes a value, reads the chip back at `$4100`,
  and compares — so the read must return the scrambled accumulator value, not open
  bus, or boot validation loops forever (`crates/rustynes-mappers/src/sprint7.rs`).

### Changed

- **Boot-smoke screenshot corpus reorganized by tier.** `screenshots/external/`
  (commercial titles) and `screenshots/besteffort/` (unlicensed / pirate /
  homebrew) now mirror the per-mapper `mapper-NNN-Name/<rom>.png` layout of the
  `tests/roms/` fixtures, and a new `scripts/screenshots/categorize_screenshots.py`
  automates that layout going forward. Screenshots only — no ROMs are committed.

## [1.4.0] - 2026-06-16

**"Fidelity"** — the compatibility-and-finish release on the cycle-accurate
v1.0.0 core: accuracy polish, per-channel audio mixing, the devtools finish
(symbol loading + event breakpoints), browser QoL (wasm `.rnm` movies +
IndexedDB save-states), a measure-first performance pass, a colorful
`rustynes help` TUI + styled `--help`, and mapper coverage 101 -> **113
families** (boot-smoke verified). All additive and off-by-default where it
matters, so the shipped / native / `no_std` / wasm builds stay byte-identical to
v1.3.0. AccuracyCoin holds 100% (139/139) and the determinism contract is intact
(the audio oracle is unchanged: no committed ROM exercises the affected paths).

### Added

- **Per-channel audio mixing UI (Workstream C).** The Audio Settings tab now has
  per-channel **volume** sliders (`0.0`–`2.0`) for the five APU channels (pulse 1,
  pulse 2, triangle, noise, DMC) — generalizing the existing per-channel mute
  mask — plus an expansion-audio slider that appears only when the loaded mapper
  has on-cart audio, labelled with the chip (VRC6 / VRC7 (OPLL) / MMC5 / Namco
  163 / Sunsoft 5B / FDS). The gains live in `[audio] channel_gain` and apply live
  through the same path as the mute mask (`Nes::set_apu_channel_gain`). A "Reset
  volumes (1.0)" button restores unity. Default (all `1.0`) is byte-identical to
  the un-scaled mixer output — the determinism contract holds and the AccuracyCoin
  / audio oracle are unaffected. See `docs/frontend.md` §Audio settings.
- **Debugger symbol / label files (Workstream D, D1).** Debug → Load Symbols
  loads `.sym` (ca65 / WLA-DX), Mesen `.mlb`, and FCEUX `.nl` label files and
  annotates the CPU disassembler (a `label:` line above the matching address),
  the breakpoint rows, and the Trace Logger tail / export with function/label
  names. Debug → Clear Symbols drops them. Hand-rolled line-based parsers (no
  new dependency; native-only — it reads a picked file); the parsed
  `address → label` map is a frontend display aid that never touches the
  deterministic core. New `crates/rustynes-frontend/src/symbols.rs` with
  per-format unit tests.
- **Event-driven breakpoints (Workstream D, D2).** The CPU panel's Event
  breakpoints section breaks on hardware events — NMI entry, IRQ entry,
  sprite-0 hit, OAM DMA, DMC DMA, and PPU/APU/mapper register read/write — each
  reporting frame / CPU cycle / scanline / dot when it fires. Behind
  `debug-hooks`; the core taps sit at the existing observational commit points
  (`Bus::cpu_read` / `cpu_write` / `notify_irq_service` / the DMC-DMA GET),
  record-only, so emulator-visible state is never perturbed and the
  feature-off build stays byte-identical (the unarmed path is a single
  `mask == 0` early-out). New `EventBpKind` / `EventBreakHit` types +
  `Nes::set_event_breakpoints` / `take_event_break_hit`, with core unit tests.
  (D3, the HD-pack per-pixel inspector, is deferred to a follow-up — see
  `docs/frontend.md`.)
- **240p test suite (`240pee`) render gates.** Wired the in-tree, free
  `240pee/240pee.nes` (mapper 2 / UxROM) and `240pee-bnrom.nes` (mapper 34 /
  BNROM) as deterministic framebuffer-FNV-1a smoke tests
  (`crates/rustynes-test-harness/tests/p240_test_suite.rs`), gating each
  mapper's boot + PRG/CHR bank-switch + render pipeline against the suite's
  title screen. No ROMs were downloaded — these were already committed.
- **Modern terminal CLI (clap 4).** Replaced the hand-rolled argv parser with a
  clap 4 derive `Command` (`crates/rustynes-frontend/src/cli.rs`): the
  `rustynes <ROM>` positional and all prior behavior are preserved (bad argument
  still exits 2), with auto `-h`/`--help`/`-V`/`--version`, an ANSI-styled
  `--help` (`Command::styles` + a `color-print` "Examples"/"Keyboard" footer),
  and `NO_COLOR` / `--color <auto|always|never>` support.
- **`help` subcommand + topic registry.** `rustynes help` and
  `rustynes help <topic>` (controls, hotkeys, gamepad, features, mappers, config,
  scripting, netplay, about) render from a single structured registry kept in
  sync with the docs and the in-app keybinding window. `rustynes completions
  <bash|zsh|fish|powershell>` emits a shell-completion script (`clap_complete`).
- **Interactive terminal help browser.** `rustynes help` on a TTY (or
  `rustynes help --interactive`) launches a ratatui + crossterm full-screen
  browser (`crates/rustynes-frontend/src/help_tui.rs`): topic list, scrollable
  colored content pane, `/` search, and arrow/Tab/PgUp-Dn/Home-End nav. Behind
  the default-on `help-tui` cargo feature; non-terminal output falls back to the
  static page so piped use and CI never block. All native-only — the clap /
  clap_complete / color-print / anstyle / ratatui deps are gated out of the wasm
  target, leaving the wasm build and size budget unchanged.
- **Browser TAS movie `.rnm` I/O (Workstream E, E1).** The lightweight
  canvas-2D embed (`wasm-canvas`) gains the F6/F7/F8 movie hotkeys: F6 toggles
  recording (a power-on `MovieRecorder`; stopping triggers a `.rnm` Blob
  download), F7 toggles playback (opening a hidden `.rnm` file picker, then
  deserializing + replaying), and F8 branches the current state into a new
  recording. It reuses the target-agnostic `MovieUi` state machine and the
  `wasm_io` Blob-download / file-picker helpers the unified winit/wasm frontend
  already used, so the canvas embed now records/replays the SAME deterministic
  `.rnm` format byte-for-byte. wasm-only; native is byte-identical.
- **Browser IndexedDB save-states + thumbnail grid (Workstream E, E2).** The
  browser save-state path moves off `localStorage` (string-only, ~5 MiB, base64
  bloat) and onto **IndexedDB** (binary `Uint8Array`, larger quota, multi-slot)
  in the new `wasm_idb.rs` — keyed by the ROM SHA-256 + slot, storing the exact
  `Nes::snapshot()` blob native filesystem slots hold. F1/F4 and the per-slot
  menu save/load now target the active/explicit slot; a new browser Save-States
  manager (`wasm_save_states.rs`, File → Manage States…) surfaces the same
  thumbnail grid the native manager has, populated by an async slot scan. The
  old `localStorage` slots are read as a fallback (private-mode / IDB-blocked
  browsers) and migrated into IndexedDB on first load. New web-sys features:
  `IdbFactory` / `IdbOpenDbRequest` / `IdbRequest` / `IdbDatabase` /
  `IdbTransaction` / `IdbTransactionMode` / `IdbObjectStore` / `DomStringList`
  (web-sys IDB is light — no new crate). wasm-only; native + the desktop
  save-state format are byte-identical. On-device browser manual-verify pending
  (like the v1.2.0 F1/F3 carryovers) — record/replay + the IDB grid can't be
  headlessly CI-certified.
- **12 new mapper families (101 → 113 coverage)** (Workstream G mapper-breadth
  continuation, `crates/rustynes-mappers/src/sprint9.rs`): mappers 28 (Action 53
  homebrew multicart), 30 (UNROM-512), 63 (NTDEC 0324 / Powerful 250-in-1), 76
  (NAMCOT-3446 / Namco 109), 174 (NTDEC 5-in-1), 225 (ColorDreams 72-in-1), 226
  (76-in-1 BMC), 227 (1200-in-1 BMC), 229 (31-in-1 BMC), 233 (42-in-1 reset-based
  BMC), 242 (Waixing 43-in-1 / Wai Xing Zhan Shi), and 246 (Fong Shen Bang /
  G0151-1). All are hook-free discrete / homebrew / multicart boards classified
  **BestEffort** (Tier-2, ADR 0011) — register-decode + save-state-round-trip
  unit-tested, deliberately **excluded** from the AccuracyCoin / commercial-ROM
  oracle gate, with the honesty gate (`mapper_tier_honesty.rs`) confirming no
  accuracy-corpus ROM resolves to a BestEffort mapper. Purely additive: the
  shipped / native / `no_std` / wasm builds stay byte-identical and AccuracyCoin
  holds 100% (139/139). See `docs/mappers.md` §Eighth long-tail batch.

### Fixed

- **BestEffort mapper boot regressions (`cpu_read_unmapped` inversion).** Mappers
  132 (TXC 22211) and 143 (Sachen TCA01) used a `!(register-range).contains(addr)`
  open-bus override that wrongly marked the entire `$8000-$FFFF` PRG-ROM window as
  open bus — so the reset vector and program code read back `$00` and the board
  never booted. Fixed to flag only the genuine open-bus holes, keeping PRG-ROM
  mapped. Surfaced by boot-smoking the new Workstream G mappers (225/246 had the
  same bug) against real unlicensed dumps; see `screenshots/besteffort/README.md`
  for the full sweep + per-mapper decode corrections (m225/m226/m233/m242/m246).
- **Triangle ultrasonic silence.** When the triangle timer period drops below 2
  (frequency above ~55.9 kHz) the sequencer now freezes instead of clocking at
  ultrasonic speed, matching hardware (and the common-emulator convention); the
  output holds its current step rather than emitting an aliasing tone. Mega Man
  2's "Crash Man" stage relies on this. See `crates/rustynes-apu/src/triangle.rs`.

### Changed

- **Verified + documented the DMC-DMA ↔ controller-read conflict as resolved.**
  The `$4016`/`$4017`-read-during-DMC-DMA shift-register conflict is modelled in
  `Bus::dmc_dma_read` and gated by the strict `dmc_dma_during_read4/dma_4016_read`,
  `sprdma_and_dmc_dma` (+`_512`), and `read_joy3/count_errors` smokes (all green).
  Moved it out of the "open/known-untested" lists in
  `docs/nesdev-hardware-emulation-checklist.md` and `docs/compatibility.md`. No
  code change was needed.
- **Confirmed pulse duty-sequencer phase reset on `$4003`/`$4007`** is already
  correct (`Pulse::write_timer_hi` resets the duty step to 0 without touching the
  timer divider); added a regression unit test and documented it in
  `docs/apu-2a03.md`.
- **Performance (v1.4.0 Workstream F — measure-first core micro-opts):** a
  rendering-path speedup of **−7.6% to −8.7% full-frame time** on the
  `flowing_palette` bench (2.354 ms → ~2.16 ms), `nestest` within criterion's
  noise threshold. The PPU `tick` now caches the scanline-stable `visible` /
  `pre_render` / `render_line` classifications once per scanline (sentinel
  `flags_cached_scanline`) instead of recomputing ~7 branches on every one of
  the 89,342 dots/frame, and the hot pixel-fetch / shift-register helpers
  (`fetch_nt` / `fetch_at` / `fetch_bg_lo` / `fetch_bg_hi` /
  `reload_bg_shift_regs` / `prefetch_shift_bg_regs`) are `#[inline]`; MMC5
  `cpu_read` short-circuits the dominant `$8000-$FFFF` PRG fetch before the
  `$5xxx` register-range match. All zero-behavior: bit-identical framebuffer +
  audio, AccuracyCoin 100% (139/139), the `visual_regression` golden + the APU
  oracle (`apu_mixer` / `apu_test`) unchanged with no snapshot re-baseline. See
  `docs/performance.md` for the per-opt deltas and the dropped candidates.

## [1.3.0] - 2026-06-16 - "Bedrock" (Feature Release)

The foundation + breadth release on the cycle-accurate v1.0.0 core, built atop
v1.2.0 "Curator". **AccuracyCoin 100% (139/139)** held throughout; the
determinism contract is intact (every new feature is additive / default-off, so
the shipped, wasm, and `no_std` builds stay byte-identical where the feature is
off). Headline:

- **Toolchain modernization** — Rust **edition 2024**, MSRV → **1.96**, and the
  coordinated **egui 0.34.3 / wgpu 29.0.3 / rfd 0.17.2 / naga 25** dependency tier
  (+ GitHub Actions runners to latest).
- **Frame-pacing fix** — the "Presented" judder was a measurement artifact
  (timestamped after `surface.present()`); now timestamped at RedrawRequested entry
  with a display-sync-aware pacer and B-diagnostic counters.
- **Developer tooling + UX** — the Memory Compare (cheat-hunt) panel, a reorganized
  menu bar (File / Emulation / View / Tools / Debug / Help) with a Close-ROM item +
  grouped Save-States submenu, Settings Shaders/Emulation tabs, and **every Settings
  control now auto-saves on change**.
- **Mapper breadth → 101 families** — a 14-board Tier-2 **BestEffort** sweep
  (honesty-gated; register-decode + save-state tested) + **Vs. DualSystem header
  detection** (the dual-console emulation stays a documented v2.0 item).
- **HD-pack `<condition>` gating + `<background>` regions** (behind `hd-pack`; ADR
  0014) — Mesen-style memory/frame/sprite conditions evaluated against a per-frame
  watched-address snapshot taken under the lock at produce time.
- **Netplay desync diagnostics** (CRC-mismatch history + first-desync-frame +
  topology, read-only) + **niche peripheral aliases** (Family Trainer, Subor
  keyboard, Konami / Bandai Hyper Shot).
- **Performance** — the manual/release-only **PGO/BOLT CI gate** exercised
  (>3%-Criterion + byte-identical promotion); no speculative hot-path changes.
- **Hard-tier accuracy residuals re-baselined** — `cpu_interrupts_v2` is now
  strict-pass (closed by the master clock); the remaining three (`mmc3_test_2/4`
  #3, two `apu_reset` cases) share one fractional-master-clock root cause and stay
  deferred as a future v2.0-scale item (see `docs/STATUS.md`, ADR 0002).

**Deferred / carryover:** **casual-mode browser RetroAchievements** (Workstream I)
is a **documented carryover** (ADR 0015) — it needs an Emscripten/pure-Rust
rcheevos→wasm build track + live-browser verification (no headless path); native RA
is unaffected. Full Vs. DualSystem dual-core, FDS-proper, and the fractional
master-clock refactor remain v2.0-scale.

### Added

- **HD-pack `<condition>` gating + `<background>` region replacement** (v1.3.0
  Workstream E1, behind the default-off `hd-pack` feature; ADR 0014). Extends the
  v1.2.0 unconditional tile loader with Mesen-style conditional rules — memory-address
  checks (`memoryCheck` / `memoryCheckConstant`, CPU- or PPU-space via the
  `0x8000_0000` marker, `(mem & mask) <op> value`), `frameRange`, and sprite
  `hmirror`/`vmirror`/`sppalette` — plus full-image/region `<background>` substitution.
  Conditions evaluate against a per-frame snapshot of only the watched addresses
  (read-only peeks taken under the emu lock at produce time, then evaluated during
  the lock-free composite), so it is determinism-safe and byte-identical with the
  feature off. Neighbor predicates (`TileNearby`/`SpriteAtPos`) + HD audio remain
  deferred.
- **14 new mapper families (87 → 101 coverage)** (v1.3.0 Workstream D1 mapper sweep,
  `sprint8.rs`): mappers 29 (Sealie RET-CUFROM), 31 (INL NSF-style / "2A03 Puritans"),
  58 (multicart), 60 (reset-based 4-in-1 multicart), 94 (UN1ROM), 101 (Jaleco JF-10 CHR
  latch), 107 (Magic Dragon), 111 (GTROM / Cheapocabra), 143 (Sachen TCA01), 177 / 179
  (Hengedianzi + variant), 218 (Magic Floor), 231 (20-in-1 multicart), and 234 (Maxi 15 /
  BNROM-like multicart). All are Tier-2 **BestEffort** discrete / homebrew / multicart
  boards ported from the GeraNES reference — no IRQ, no expansion audio, no per-cycle / A12
  hook (`MapperCaps::NONE`) — register-decode + save-state unit-tested only and explicitly
  **not** accuracy-gated (the AccuracyCoin / commercial-ROM oracle never references them).
- **Vs. DualSystem header detection** (v1.3.0 Workstream D2): the NES 2.0 byte-13 high
  nibble (Vs. hardware type 5/6) is now parsed into `Header`/`Cartridge.vs_dual_system`
  and exposed as `Nes::is_vs_dual_system()`, so the frontend's "DualSystem not yet
  emulated" note fires for properly-headered DualSystem ROMs, not only the four
  SHA-256-DB-known dumps. The two-CPU/two-PPU *emulation* remains a documented v2.0
  deferral (no committable test-ROM oracle; see `docs/STATUS.md` + the design audit).
- **Memory Compare (cheat-hunt memory search)** debugger panel (v1.3.0 Workstream C, C3).
  A classic emulator memory search over the 2 KB CPU work RAM (`$0000-$07FF`): snapshot a
  baseline, then iteratively narrow a candidate set by how each byte moved since the last
  snapshot — changed / unchanged / increased / decreased / equals-value — until one
  address remains (feed it to the raw-RAM cheat panel). Read-only (samples via the
  side-effect-free `cpu_bus_peek`; never writes the core, determinism unaffected), opened
  from **Debug → Memory Compare**, and disabled under RetroAchievements hardcore mode like
  the Memory viewer + cheat panel.
- **Netplay desync diagnostics** (v1.3.0 Workstream G1). The Netplay panel gains a
  read-only **Diagnostics** section mirroring GeraNES's `DesyncMonitor`: the room / input
  topology (player count + which controller port this peer drives), the in-sync /
  desynced-at-frame-N status, lifetime checksum-compare + mismatch counts, the consecutive-
  mismatch counter, the most recent local-vs-remote CRC (classified timing-vs-state), and a
  rolling CRC-match history table. Backed by a new observational `DesyncDiagnostics` ring in
  `rustynes-netplay` that records every confirmed-frame checksum comparison the rollback
  session already performs — purely telemetry, it never feeds back into the rollback
  algorithm or the checksum exchange, so the determinism contract is untouched.
- **Niche input-device peripherals** (v1.3.0 Workstream F1), all additive, default-off
  `InputDevice` overlays selectable as the player-2 expansion device — `ExpansionDevice::None`
  stays the default, so every standard / Four Score read is byte-identical when unused: the
  **Family Trainer** mat (`Nes::set_family_trainer`, layout-equivalent to the Power Pad,
  reusing its 12-button scan), the **Subor keyboard** (`Nes::set_subor_keyboard`, a Family
  BASIC keyboard work-alike reusing the 9×8 matrix scan), the **Konami Hyper Shot**
  (`Nes::set_konami_hyper_shot`, the 4-button 2-player Run/Jump parallel read with `$4016`
  per-player enable), and the **Bandai Hyper Shot** / Exciting Boxing punching bag
  (`Nes::set_bandai_hyper_shot`, the 8-sensor `$4016`-bit-1-multiplexed read). The reads are
  cross-checked against the `NESdev` "Konami Hyper Shot" / "Exciting Boxing Punching Bag"
  pages and round-trip through the save-state; new config / host-key wiring carries
  `#[serde(default)]` so older configs load unchanged.

### Changed

- **Menu-bar reorganization + Settings auto-save** (v1.3.0 Workstream C, UI). The
  top-level menu order is now **File / Emulation / View / Tools / Debug / Help**.
  Items were regrouped for discoverability: a **Close ROM** entry and a grouped
  **Save States** submenu (Save/Load State, Active Slot, Save-/Load-to-Slot, Manage
  States) in File; **Swap Disk Side** moved File → Emulation; **NSF Player** moved
  Debug → Tools; **Performance Monitor** moved Tools → Debug; and the standalone
  "Mod" menu folded into a Tools **HD Pack** submenu (still `hd-pack`-feature +
  native-gated). The Settings window gains a dedicated **Shaders** tab (the
  composable shader stack, split out of Video) and renames "Advanced" → **Emulation**
  (run-ahead + rewind); it now snapshots the config each frame and persists on any
  change, so **every setting in every tab auto-saves**. The redundant debugger
  checkbox toolbar was removed (the menu bar already surfaces every panel). Pure UI;
  no determinism surface. (The egui-0.34 "menu lingers until several clicks" report
  is documented in `ui_shell::menu_bar` pending an on-device repro to pin the exact
  `MenuState` trigger — not hacked blind.)
- **Rust edition 2021 → 2024** (v1.3.0 Workstream A, toolchain modernization). The
  whole workspace now compiles on the 2024 edition. The migration was mechanical and
  determinism-neutral: `extern "C"` FFI blocks became `unsafe extern "C"`
  (`rustynes-cheevos`), the `disasm` opcode-table macro pins its fragment specifiers
  to `expr_2021` (preserving 2021 macro-matching), one `gen` local became the raw
  identifier `r#gen` (`gen` is a reserved keyword in 2024), and a redundant
  block-return brace was removed. Imports were reformatted to the 2024 rustfmt style.
  No `tail_expr_drop_order` restructuring was needed. **Verified byte-identical:**
  AccuracyCoin 100% (139/139), `visual_regression` golden framebuffers, `nestest`
  0-diff, and `cpu_interrupts_v2` 5/5 strict all pass unchanged; the chip stack still
  cross-compiles `no_std` (`thumbv7em-none-eabihf`).
- **MSRV 1.86 → 1.96 + the egui / wgpu / rfd dependency tier** (v1.3.0 Workstream A).
  The pinned toolchain (`rust-toolchain.toml`), `[workspace.package] rust-version`, the
  two explicit-version crates, and the CI MSRV jobs all move to **Rust 1.96** (latest
  stable). That unblocks the coordinated UI-stack bump: **egui / egui-wgpu / egui-winit
  0.32 → 0.34.3**, **wgpu 25 → 29.0.3** (egui 0.34 requires wgpu 29), naga 25, and
  **rfd 0.14 → 0.17.2**. The wgpu 29 migration is presentation-only (no core/determinism
  surface): `get_current_texture()` now returns the `CurrentSurfaceTexture` enum (handled
  via a small `gfx::PresentError` that preserves the reconfigure-on-lost/outdated
  behavior), `RenderPassColorAttachment` gains `depth_slice`, `RenderPassDescriptor` and
  `RenderPipelineDescriptor` use `multiview_mask`, `PipelineLayoutDescriptor` uses
  `immediate_size` + `Option`-wrapped bind-group layouts, samplers use `MipmapFilterMode`,
  and `InstanceDescriptor` / `DeviceDescriptor` follow the new constructors. egui 0.34
  deprecations were migrated (`Context::run` → `run_ui` with a root `Ui`, `Panel::top/
  bottom(..).show_inside(..)`, `content_rect`, `global_style`, `egui_wants_*_input`,
  `RendererOptions`). The newer 1.96 clippy's lints were cleaned across the workspace
  (collapsible let-chains, `map_unwrap_or`, `manual_is_multiple_of`, etc.). **Verified:**
  AccuracyCoin 100% (139/139), `visual_regression` golden + `nestest` unchanged; clippy
  `-D warnings` clean (native + `scripting,hd-pack` + both wasm flavours); `no_std`
  cross-compile; `trunk build --release` succeeds with the wasm size budget at 3.06 / 5.00
  MiB gzip (wasm-bindgen pin 0.2.125 unchanged).
- **CI: GitHub Actions runner/action versions bumped to latest** (v1.3.0 Workstream A):
  `actions/upload-artifact` v4 → v7; all other actions (`checkout@v6`,
  `configure-pages@v6`, `deploy-pages@v5`, `upload-pages-artifact@v5`,
  `action-gh-release@v3`, `taiki-e/install-action@v2`, `Swatinem/rust-cache@v2`,
  `dtolnay/rust-toolchain@master`) and the `*-latest` runner images already track newest.

### Fixed

- **Mapper 218 (Magic Floor) now loads its real 16 KiB-PRG dumps** (v1.3.0 D1
  verification). The initial sprint8 port required a 32 KiB-multiple PRG and so
  rejected the actual homebrew ROM (16 KiB, NROM-128-style); it now accepts 16 KiB
  and mirrors PRG across the 32 KiB CPU window. Found by a boot-smoke pass over
  homebrew/unlicensed dumps for the 14 new BestEffort families (screenshots under
  `screenshots/besteffort/`; the ROMs stay gitignored). Regression test added.
- **`.zip` / soft-patched ROMs passed on the command line now load** (a v1.2.0 ingest
  bug). The CLI / initial-ROM path (`App::new`, used by `rustynes <rom>`) read the file
  and parsed the raw bytes directly, skipping the `.zip` extraction + same-stem
  `.ips`/`.ups`/`.bps` soft-patching that the menu / drag-drop / recent-ROM path
  (`load_rom_from_path`) performs — so `rustynes game.zip` failed with "rom magic bytes
  do not match `NES\x1A`". The ingest preprocessing is now factored into a shared helper
  that both paths use, so a zipped or patched ROM on argv reaches the deterministic parse
  and the CRC-keyed per-game database as the extracted/patched image, identical to every
  other load path. (Regression-tested.)
- **Performance panel "presented" cadence no longer shows phantom judder** (v1.3.0
  Workstream B1). The present-to-present interval was timestamped *after*
  `surface.present()` returned, so it folded GPU-submit + vsync-gate +
  coalesced-`RedrawRequested` jitter into the series — the graph bottomed out and
  "rushed to catch up" while the steadier "produced" (emulation) series stayed flat.
  It is now timestamped at the `RedrawRequested` display-refresh signal (still only on
  an actual present), so present-to-present deltas reflect the display's true visible
  cadence; any small, steady offset from "produced" is just the NTSC 60.0988 Hz rate
  beating against the display refresh. Measurement-only (presentation/instrumentation;
  no core or determinism surface). The Performance panel's "presented" legend gains a
  tooltip explaining the semantics; see `docs/frontend.md`.
- **Performance panel: present/produce "beat" diagnostic** (v1.3.0 Workstream B, B3
  groundwork). Added duplicate-present and dropped-produce counters (`presented_dups` /
  `produced_dropped`, also in the CSV log) surfaced in the panel beside the existing
  present-mode + pacer-anomaly readouts — read-only instrumentation (no pacer change).
  Under display-sync both stay ~0; under wall-clock they tick ~once every ~10 s for the
  NES 60.0988 Hz vs 60.000 Hz display beat, so a sudden burst (real judder) is now
  distinguishable from the harmless inherent beat. This is the signal for deciding
  whether the deeper present-mode / pacer mitigation is worth its regression risk.

### Security

- **Resolved all 10 open CodeQL code-scanning alerts.** (1) Added a least-privilege
  top-level `permissions: contents: read` to `ci.yml` and `security.yml` — the 9
  `actions/missing-workflow-permissions` alerts (one per job; every job is read-only
  build/test/lint/audit) — so neither workflow's `GITHUB_TOKEN` carries default write
  scopes. (2) Fixed the one high-severity `py/incomplete-url-substring-sanitization` in
  `scripts/download_missing_nesdev_pages.py`: the opensearch-host decision used a
  `"mediawiki.org" in url` substring test (which a crafted host like
  `mediawiki.org.example.com` or a query string would satisfy); it now parses the URL and
  compares the actual hostname (`== "mediawiki.org" or endswith(".mediawiki.org")`).

## [1.2.0] - 2026-06-15 - "Curator" (Feature Release)

**v1.2.0 "Curator" is a broad, additive feature release** on the cycle-accurate v1.0.0
core (shipped through v1.1.0 "Scriptable"). Theme: **library breadth + compatibility +
reach** — getting more games to load, run correctly, and be playable anywhere, plus the
polish to curate them. Mapper coverage rises 51 → **87 families** behind a CI-enforced
accuracy-tiering honesty gate; ROMs now load from `.zip` and auto-apply
`.ips` / `.ups` / `.bps` soft-patches; a per-game database + in-app ROM-Database editor
correct region / mapper / mirroring; the video stack gains live NTSC knobs, a composable
shader stack + CRT preset bank, and a (default-off) HD-pack loader; new peripherals
(Family BASIC keyboard, SNES mouse, Arkanoid-both-ports, Game Genie code DB) join the
input layer; the Lua engine gains `onNmi` / `onIrq` / `setInput`; the menu bar gets
contextual enable/disable, a remappable shortcut registry, and Font Awesome icons; the
web build gains on-screen touch controls + a Power Pad feed + an experimental piccolo
wasm-Lua backend; netplay ships a turn-key `deploy/` bundle; and a manual/release-only
PGO CI promotion gate lands. **Every addition is off-by-default or additive, so the
shipped / native / `no_std` / wasm builds stay byte-identical and AccuracyCoin holds
100% (139/139), `nestest` 0-diff.** The SMB3 World 1-1 sprite flicker (a PPU
OAM-row-corruption model bug) is fixed via a faithful TriCNES eval-pointer port, and
Mapper 89 now models its bus conflict. ADRs **0011** (mapper tiering), **0012** (wasm-Lua
piccolo backend), **0013** (composable shader stack).

### Added

- **PGO / BOLT CI promotion gate** (v1.2.0 Workstream G — performance
  infrastructure, no core behavior change). A new manual-/release-only
  `.github/workflows/pgo.yml` (`PGO`) gates the previously-unused
  `scripts/pgo/run.sh` recipe into CI. Triggered by `workflow_dispatch` (with
  optional `frames` / `run_bolt` inputs) and on push of a release tag (`v*`) —
  never per-PR (the instrument + train + rebuild cycle is too slow for the PR
  gate). It builds the plain-release `full_frame` baseline, runs the PGO recipe
  (instrument → train on the seven committed CC0/MIT ROMs → optimized rebuild),
  re-benches, and **promotes the PGO binary only when BOTH** (a) it beats plain
  release by **> 3%** on the `full_frame` Criterion mean (same-runner A/B) AND
  (b) the full `--features test-roms` oracle — AccuracyCoin 139/139, `nestest`
  0-diff, blargg/kevtris, golden-framebuffer `visual_regression`, the APU
  mixer/volume audio suites — is **byte-identical under the PGO codegen**
  (`cargo pgo optimize test`). An optional Linux-only `bolt` job runs behind the
  same gate (best-effort; skips cleanly when `llvm-bolt` is unavailable). A
  failed gate is informational and never blocks a release. Documented in
  `docs/performance.md` §"Profile-guided optimization (PGO)". No Rust changed;
  the shipped/wasm/`no_std` builds are unaffected.
- **Experimental wasm Lua scripting via a piccolo backend** (v1.2.0 Workstream
  F4; default OFF, behind the new `script-wasm` feature; see
  `docs/adr/0012-wasm-lua-piccolo-backend.md`). `rustynes-script` now sits behind
  a `VmBackend` trait with two compile-time-selected backends: the native
  **mlua** Lua 5.4 engine (the crate-default `mlua-backend` feature, what the
  frontend's `scripting` feature pulls in — **byte-identical to v1.1.0**) and an
  experimental pure-Rust **piccolo** VM that compiles to
  `wasm32-unknown-unknown` with no C toolchain. piccolo's `Fuel` maps onto the
  per-frame instruction budget. On wasm the engine is loadable from the browser
  (`rustynes_load_script` / `rustynes_stop_script` JS bridge) and supports
  `emu.onFrame`, `emu.read`/`peek`/`readRange`, `emu.cpu`/`frame`/`cycle`,
  `emu.log` + `print`, the overlay draws, and gated `emu.write`. It is
  **explicitly NOT byte-parity** with the native mlua engine (a different VM) —
  acceptable because scripts are observational/overlay + gated writes and are
  never part of the framebuffer/audio determinism oracle. The per-access
  (`onExec`/`onRead`/`onWrite`) and per-interrupt (`onNmi`/`onIrq`) replay
  callbacks are a documented native-only limitation (registered as no-ops on
  piccolo). All native builds — shipped, the default wasm flavours, and the
  `no_std` chip stack — are byte-identical to before (piccolo is never pulled
  unless `script-wasm` is explicitly enabled). The native 13-test
  `rustynes-script` suite is unchanged; the piccolo backend adds 4
  backend-specific tests (deferred-write-lands, deferred-write-gated,
  fuel-budget, native-only-callbacks-are-no-ops).
- **Web/wasm input parity — on-screen touch controls + Power Pad** (v1.2.0
  Workstream F1 + F2). The browser build gains a translucent Pointer-Events
  touch overlay (pure DOM/CSS in `web/index.html`, zero Rust binary weight
  beyond the new `web-sys` pointer/touch feature flags): an on-screen D-pad /
  A / B / Start / Select with multi-touch + pointer-capture handling, a
  selectable target **port** (player 1-4, for Four Score), and a 12-button
  **Power Pad** mat. The overlay drives a shared `wasm_touch` thread-local
  bridge that BOTH wasm frontends read at the SAME deterministic late-latch a
  keypress uses — the `wasm-canvas` embed folds it into the per-frame
  `set_buttons` / `set_power_pad` call, and the `wasm-winit` path ORs it into
  `FrameInputs` so it flows through `EmuCore::latch` exactly like a keyboard
  bit. Touch input is therefore recorded/replayed identically by TAS movies +
  netplay and adds no new determinism surface. The native Power Pad latch arm
  (previously `cfg(not(wasm32))`-gated behind the mouse/cursor block) is
  narrowed so the Power Pad — which needs only a `u16` mask — also feeds on
  wasm; Zapper / Vaus stay native-gated. The native build is byte-identical
  (all touch state is wasm-only). On-device touch UX needs a browser on a
  touch device to verify.
- **Menu-bar responsiveness — per-item contextual enable/disable** (v1.2.0
  Workstream H, H1; GeraNES `MenuUI.inl`-inspired). Menu items now grey out
  when the action would be a no-op or unsafe in the current state, instead of
  being always-clickable. Predicates threaded from live app state into the
  shell: with **no ROM loaded** Save/Load State (+ slots + the Save-States
  manager), Reset, Power-cycle, Frame Advance, Speed, FDS disk-swap, Vs. Insert
  Coin, ROM Database, and HD-pack load/unload are disabled; during a **netplay
  session** Open ROM / Open Recent / Reset / Power-cycle / Frame Advance / Vs.
  coin / Movies (TAS) / HD-pack are locked and the Speed submenu allows only
  100% (mirrors GeraNES `netplayRomChangeRestricted` / `isNetplaySpeedRestricted`);
  while a **TAS movie is recording or playing** Load State (+ load-from-slot),
  Reset, Power-cycle, disk-swap, Netplay, and HD-pack are locked, and the
  Movies submenu disables the conflicting Record-vs-Play actions (mirrors
  GeraNES `replayInteractionLocked` / `replayRecordingActive`). Pure UI — the
  `MenuAction` dispatch set is unchanged.
- **Remappable system-hotkey registry surfaced in the rebind UI** (v1.2.0
  Workstream H, H2). The `[input.system]` config section already drives both
  the global hotkey handler and the menu's inline accelerator labels; the
  Settings -> Input rebind panel now exposes **all** of those bindings (Open
  ROM, Pause, Frame Advance, Fast Forward, Fullscreen, Toggle Menu Bar, Speed
  up/down/reset, Movie record/play/branch, FDS disk-swap, Vs. Insert Coin) for
  rebinding, not just the original seven — so a rebind takes effect live and
  the menu accelerator label updates to match. Every field keeps its
  `#[serde(default)]`, so existing configs and the default build are
  byte-identical (a new `shortcut_registry_defaults_are_byte_identical` test
  pins this).
- **Menu icons** (v1.2.0 Workstream H, H3; GeraNES `withMenuIcon`). Font Awesome
  6 Free **Solid** glyphs now precede every top-level menu (File / Emulation /
  Tools / Mod / View / Debug / Help) and their items, alongside the H1
  enable/disable state and H2 accelerator labels. The icon font
  (`assets/fonts/fa-solid-900.ttf`, SIL OFL-1.1, license shipped beside it) is
  embedded via `include_bytes!` and registered with egui as a trailing fallback
  family, so ordinary UI text is untouched and any missing glyph degrades to a
  box rather than crashing (`crate::icons`). Measured against
  `scripts/wasm_size_budget.sh`, the full font **fit** the 5 MiB gzip wasm-deploy
  budget (total 2.78 MiB gzip, 2.22 MiB headroom; ~13 KB gzip added), so the
  **same full font ships on native and both wasm flavours** — no per-target
  subsetting was needed. The lightweight `wasm-canvas` embed has no egui menu and
  is unaffected. Pure UI; the core and the `MenuAction` dispatch set are
  unchanged.

- **Mapper accuracy tiering** (v1.2.0 Workstream A). Every supported mapper
  family is now classified `Core` / `Curated` / `BestEffort` by a single
  `const fn mapper_tier(id, submapper)` (`rustynes-mappers::tier`). The tier is
  an honesty marker — runtime behaviour is identical — that keeps the accuracy
  claim precise as long-tail coverage grows: a CI invariant forbids any
  `BestEffort` mapper from backing an AccuracyCoin / oracle ROM. ADR 0011.
- **Nine curated (Tier-1) mapper families** (v1.2.0 Workstream A): 38 (Bit Corp
  UNL-PCI556), 41 (Caltron 6-in-1), 79 (AVE NINA-03/06), 86 (Jaleco JF-13), 113
  (NINA-006 / MB-91, register-controlled mirroring), 140 (Jaleco JF-11/14), 232
  (Camerica Quattro / BF9096), 240 (C&E), and 241 (BxROM-like). Each is a
  discrete-logic board with register-decode unit tests.
- **Twenty-seven best-effort (Tier-2) mapper families** (v1.2.0 Workstream A):
  the aggressive long-tail sweep ported from the GeraNES / Mesen2 references —
  15, 36, 39, 61, 62, 72, 77, 92, 96, 97, 132, 133, 145, 146 (sprint6) and 147,
  148, 149, 150, 180, 185, 200, 201, 202, 203, 212, 213, 214 (sprint7). Mostly
  multicart / Sachen / discrete boards. Register-decode unit-tested only and
  **not** accuracy-gated (see the tiering note). Total mapper coverage rises
  from 51 to **87 families** (51 Core + 9 Curated + 27 BestEffort).
- **Per-game database — region / mapper / submapper overrides** (v1.2.0
  Workstream B). The CRC32-keyed per-game DB grew from a `(crc, Mirroring)`
  table to a full `GameDbEntry` (region / mapper / submapper / mirroring /
  title) parsed from the vendored TetaNES columns. Region / mapper / submapper
  corrections apply by rewriting the iNES (or NES 2.0) header before the core
  parses it — frontend-only and idempotent, so the determinism firewall holds
  (the core test suites never patch). Mirroring / Vs. corrections continue
  through the existing post-construction setters.
- **In-app ROM-database editor** (v1.2.0 Workstream B, B4): a new **Tools -> ROM
  Database** panel shows the loaded ROM's effective per-game entry (user overlay
  merged over the vendored base, keyed on the ROM CRC32) and lets you edit
  mirroring / region / mapper / submapper / title. Edits persist to an editable
  user-overlay file (`<data-dir>/game_db_user.txt`) that overrides the vendored
  base; the mirroring override applies live, the rest at the next ROM load.
  "Reset to Default" reverts to the vendored entry. Native; frontend-only.
- **ROM soft-patching** (v1.2.0 Workstream B): a same-stem `.bps` / `.ups` /
  `.ips` patch sitting beside a ROM is auto-applied at load (in that
  precedence), before format detection, so the patched image flows through the
  deterministic parse unchanged — save-states / netplay / oracle all see the
  patched bytes. UPS and BPS verify their in-format source and target CRC32s.
  Native; a malformed patch is surfaced and the unpatched ROM still loads.
- **`.zip` ROM loading** (v1.2.0 Workstream B): a `.zip` opened as a ROM has its
  first NES / FDS / NSF entry extracted in memory and loaded as usual (a
  same-stem soft-patch beside the archive still applies). Native-only (`zip`
  crate, `deflate`-only to reuse the existing flate2/miniz_oxide); the wasm
  builds stay byte-identical (the dep is in the `cfg(not(wasm))` table).
- **NTSC per-knob live tuning** (v1.2.0 Workstream C1): the true-composite
  (`composite-rt`, Bisqwit) NTSC filter's Contrast / Saturation / Brightness /
  Hue are now live `[graphics]` settings (`ntsc_contrast`, `ntsc_saturation`,
  `ntsc_brightness`, `ntsc_hue`), adjustable from sliders in Settings -> Graphics
  and persisted to `config.toml`. Promoted from baked WGSL constants to a
  per-frame uniform: the YIQ matrix is rebuilt in-shader from the knobs each
  frame. Output-only (the deterministic core framebuffer and AccuracyCoin /
  oracle results are unaffected). All four default to `0.0` (Bisqwit's neutral
  values), at which the in-shader decode is bit-identical to the previous
  hardcoded coefficients — existing configs and the default build are
  byte-for-byte unchanged.
- **Composable shader stack + CRT preset bank** (v1.2.0 Workstream C2,
  `GeraNES`-`ShaderPass`-inspired). The single-select CRT / NTSC / composite-rt
  post-process filter is now an ordered, composable `ShaderStack`: enabled passes
  render by ping-ponging two NES-resolution intermediate render targets, with the
  final pass blitting the letterboxed image to the swapchain. A new
  Settings -> Graphics -> "Shader stack (composable)" section adds / removes /
  reorders / toggles passes and exposes each pass's tunable knobs as generic
  sliders, parsed from RetroArch-style `#pragma parameter` shader headers (new
  `rustynes-frontend::shader_pass` module). A built-in CRT preset bank (Sharp /
  Classic / Heavy-Aperture, all reusing the existing CRT shader) plus
  Save / Load / Delete persist named stacks under `[graphics.shader_presets]`.
  The Bisqwit `composite-rt` pass is special-cased (it consumes the `R16Uint`
  palette-index texture, so it is only honoured as the first pass). Frontend /
  presentation-only — the deterministic core framebuffer and AccuracyCoin /
  oracle results are unaffected. **An empty / all-disabled stack (the default,
  and what any pre-C2 `config.toml` deserializes to) falls through to the exact
  pre-C2 direct blit — the default presented image is byte-for-byte unchanged.**
  Rejected as out of scope: a RetroArch `.slangp` importer / GLSL->WGSL
  translation. ADR 0013.
- **HD-pack / mod loader — minimal first cut** (v1.2.0 Workstream C3,
  Mesen-HD-pack-inspired; native-only, behind the default-OFF `hd-pack` cargo
  feature). A new per-pixel **tile-source telemetry** export in the PPU
  (`rustynes_ppu::HdTileSource`, gated on `rustynes-ppu/hd-pack`) records, for
  each visible pixel, the CHR pattern-table tile base address, the final
  palette, the sprite flip flags, and whether the pixel came from a sprite or
  the background — written in `emit_pixel` in lockstep with the existing
  `index_framebuffer`, observing only already-computed state. It is
  **output-only**: it reads no new VRAM, issues no new A12 / mapper events,
  mutates no emulation state, and is not part of the save-state. A new frontend
  `hdpack` module loads a Mesen-style `hires.txt` (folder or `.zip`), parses the
  supported subset (`<scale>`, `<patternTable>`, and **unconditional** CHR-hash
  `<tile>` rules), hashes each rendered tile's 16 CHR bytes with the
  Mesen-compatible CRC32, and substitutes hi-res replacement images at blit time
  (a dedicated upscaled-RGBA blit path in `gfx`). A **Mod -> Load HD Pack** menu
  entry enables a pack per-game (keyed on `rom_sha256`, persisted under
  `[graphics] hd_packs`). Out of scope (deferred, not v1.2.0): conditions,
  palette keys, background regions, HD audio, and a `<patternTable>`-bank
  substitution path. **With the `hd-pack` feature OFF the shipped / wasm /
  `no_std` builds are byte-identical to today; with it ON but no pack loaded the
  presentation is also byte-identical** — proven by the full ROM corpus
  (AccuracyCoin 139/139, `nestest` 0-diff, blargg / kevtris green) passing
  identically with the feature on and off.
- **Family BASIC keyboard** (v1.2.0 Workstream D1): a new
  `InputDevice::FamilyKeyboard` core device implements the Famicom keyboard's
  9-row x 2-column-half matrix scan — `$4016` write selects the column-half
  (bit 0), clocks the row counter (bit 1 rising edge advances, low resets), and
  enables the matrix (bit 2); `$4017` read returns the four selected key
  switches on bits 4..1, active-low. A new `Nes::set_family_keyboard` setter
  (mirroring `set_power_pad`) feeds the 72-key bitmap. The frontend maps host
  keys via `input::family_keyboard_index` (a direct, one-to-one passthrough; a
  faithful positional layout is a follow-up) and offers it as the player-2
  device in the input rebind panel. Unit-verified matrix scan; save-state
  round-trips (snapshot tag 5).
- **SNES-style serial mouse + Arkanoid on both ports** (v1.2.0 Workstream D2): a
  new `InputDevice::SnesMouse` core device implements the 32-bit MSb-first D0
  report (signature nibble `0b0001`, sensitivity, left/right buttons, signed
  7-bit X/Y movement, idles high after the report), with a `Nes::set_snes_mouse`
  setter; the frontend derives per-frame movement from the cursor delta and maps
  the left/right mouse buttons. The Arkanoid **Vaus** paddle is now selectable on
  **either** port (the core already permits `set_paddle` on port 0). Both are
  selectable as the player-2 expansion device. Unit-verified serial protocol;
  save-state round-trips (mouse snapshot tag 4). Konami / Bandai Hyper Shot, the
  Family Trainer, and the Subor keyboard are noted follow-ups (not implemented).
- **Game Genie code-name database** (v1.2.0 Workstream D3): a small committed,
  CRC32-keyed asset (`genie_database.tsv`, public Galoob / community code lists)
  maps a ROM to named Game Genie codes. The cheat panel shows a pick-list of the
  loaded ROM's matching codes; selecting one feeds the **existing**
  `GenieCode` decode + cheat persistence. Pure frontend — the core Game Genie
  substitution (`rustynes-core/src/genie.rs`) is unchanged; every offered code is
  validated through `GenieCode::new`, so the no-cheat PRG-read path is
  byte-identical. The asset is compiled in for both native and wasm (modest size;
  commercial ROMs are never committed — only the codes).
- **Determinism contract held for all of Workstream D**: every new input device
  is additive and OFF by default (`ExpansionDevice::None` stays the default; new
  `FrameInputs` / `SharedInput` / config fields are `#[serde(default)]` and
  default to "no device / no keys"), and the core input additions are pure /
  deterministic. The shipped build + AccuracyCoin **(139/139)** + `nestest`
  0-diff + blargg / kevtris are byte-identical to before; the no_std chip stack
  still cross-compiles to `thumbv7em-none-eabihf`.
- **Lua scripting depth — `emu.onNmi` / `emu.onIrq`** (v1.2.0 Workstream E, E1,
  T-110-E1). Two new observational callbacks fire once per interrupt the CPU
  serviced that frame (`fn(vector)`). They tap the **committed** service commit
  point (`Bus::notify_irq_service`, the same point the IRQ trace uses) — not the
  speculative `poll_nmi` / `poll_irq` sampler ADR 0010 flagged as unreliable — so
  a script sees exactly the interrupts that committed, in order, classified by the
  fetched vector (`0xFFFA` ⇒ NMI, `0xFFFE` ⇒ IRQ/BRK, robust under NMI hijack). A
  new `debug-hooks`-gated per-frame interrupt-service log on `Nes` mirrors
  `exec_log` (cleared at the top of `run_frame`; `interrupt_log()` /
  `set_interrupt_logging()` accessors); the engine enables it only while an
  `onNmi`/`onIrq` callback is registered.
- **Lua scripting depth — `emu.setInput` wired through the late-latch** (v1.2.0
  Workstream E, E2, T-110-E2). `emu.setInput(port, buttons)` now applies a per-
  port controller override at the deterministic late-latch (`EmuCore::latch`,
  before `produce_one_frame`) — the *same* point a real keypress enters — so a
  recorded / replayed session stays bit-identical. The override is one-shot per
  call. It is **gated identically to `emu.write`**: the engine drops the command
  at the source under a locked session and the host re-checks the identical
  `netplay_locked || movie_locked` condition (which folds in RA-hardcore) before
  storing it, so a script can never perturb a netplay / TAS-replay / RA-hardcore
  run. (Supersedes the v1.1.0 "accepted but not applied" stub + its one-time
  console warning.)
- **Determinism contract held for Workstream E (E1 + E2)**: the interrupt log is
  `debug-hooks`-gated (a zero-cost no-op when off) and the scripting integration
  is behind the default-OFF `scripting` feature, so the shipped / wasm / no_std
  builds are byte-identical to today. AccuracyCoin **(139/139)** + `nestest`
  0-diff + blargg / kevtris stay green; the no_std chip stack still cross-compiles
  to `thumbv7em-none-eabihf`.

### Changed

- **Hosted browser-netplay deploy made turn-key + honest** (v1.2.0 Workstream
  F3, deploy + verify-prep). The existing `deploy/` bundle (signaling server +
  Caddy TLS proxy + coturn STUN/TURN) is now deployable with no source edits:
  the `Dockerfile` builds the correct `rustynes-netplay` crate (was a stale
  `nes-netplay`), a workspace-root `.dockerignore` (referenced but previously
  missing) keeps `target/` + ROMs + docs out of the image context, and the
  per-deploy values (`DOMAIN`, `TURN_USER`/`TURN_SECRET`/`TURN_REALM`) come from
  a `.env` (new `deploy/.env.example` template; coturn credential/realm injected
  as CLI flags from env so `turnserver.conf` carries no checked-in secret). All
  stale `RustyNES v2` / `nes-frontend` references scrubbed; `README.md` now uses
  clearly-placeholder values (`signaling.example.com`, `TURN_SECRET=changeme`).
  `docs/netplay-webrtc.md` §3.4/§4 flipped from "Pending" to
  **deployment-ready; live verification pending the maintainer's hosted run**
  (explicitly **not** "Verified"), and a copy-pasteable **manual verification
  checklist** (2-tab → 2-machine → 4-player matrix + ops/DNS/TLS steps + the
  TURN-bandwidth ops caveat) added to `deploy/README.md`. Documented that
  browser netplay needs **no COOP/COEP / SharedArrayBuffer** (DataChannel +
  AudioWorklet). Docs/deploy only — no Rust changes; the `[netplay]`
  `signaling_url` / `stun_servers` config defaults were already correct and stay
  byte-identical. Live multi-browser netplay verification remains the
  maintainer's manual step (not run here).

### Fixed

- **PR #75 review hardening (beta.2 bot-review follow-up).** Adopted the
  worthwhile bot-review findings from the merged beta.2 PR:
  - *Security (path traversal):* HD-pack replacement-image filenames parsed
    from `hires.txt` are now sanitised (`hdpack::sanitize_image_name`) — a
    malicious pack can no longer reference `../../etc/passwd`, an absolute path,
    or any path with separators / drive prefixes to escape the pack directory;
    such rules are rejected.
  - *Security (zip bomb / OOM):* `hdpack::read_zip_entry` now caps a single
    archive entry at 64 MiB (declared size **and** actual read bounded),
    matching `app.rs::extract_rom_from_zip`.
  - *Panic guard:* `SnesMouseState::enc_axis` no longer panics on `i16::MIN`
    (used `unsigned_abs` instead of the overflowing `-v`).
  - *Save-state robustness:* `FamilyKeyboardState::from_parts` clamps a restored
    `row` to the matrix bound, so a corrupt/malicious save-state cannot drive an
    out-of-bounds index in `read()`.
  - *Lock discipline:* the HD compositor's CPU-heavy upscale/tile-hash/blit now
    runs **after** the emu lock is dropped — only the framebuffer, PPU
    tile-source telemetry, and the 8 KiB CHR pattern space are snapshotted under
    the lock.
  - *Replay-lock consistency:* the Load-State **hotkey** and `MenuAction`
    dispatch now honour the same movie-record/playback lock the File menu greys
    the item under (previously bypassable via the bound key).
  - *Perf (default path):* the Bisqwit NTSC WGSL skips the per-fragment
    `cos()`/`sin()` hue rotation when `hue == 0` (the default), keeping the
    default present byte-identical (the C1 `default_knobs_match_legacy_matrix`
    guard still passes).
  - *Docs:* corrected the `InputDevice::FamilyKeyboard` doc reference
    (`input::family_keyboard_index`, not a nonexistent `FAMILY_KEYBOARD_KEYMAP`)
    and the `hash_tile` doc to state it hashes raw unflipped CHR bytes (it does
    not consult `flip_h`/`flip_v`).
- **SMB3 (and any MMC3/other game using a mid-scanline `$2001` split) — sprite
  flicker.** The PPU OAM-row-corruption model keyed the corrupted row off the
  raw PPU dot (`dot >> 1`), so *Super Mario Bros. 3*'s mid-scanline rendering
  disable (its HUD split) intermittently flagged Mario's OAM row and wiped his
  sprite, flickering him in/out (~26% of frames) in World 1-1. Replaced with a
  faithful port of TriCNES's eval-pointer model: the corruption index is the
  live secondary-OAM evaluation pointer (`OAM2Address`), captured at the
  rendering-disable edge during dots 1-64 and committed on re-enable. Default
  builds stay byte-identical for games without such a split; AccuracyCoin's
  OAM-Corruption test (0x047B) and the full 139/139 suite, nestest 0-diff, and
  blargg/kevtris remain green. `docs/ppu-2c02.md` edge-case 2 updated.
- **RetroAchievements badge decode** now validates PNG dimensions (from the
  IHDR header) *before* allocating the decode buffer, so a crafted/corrupt
  badge declaring huge dimensions can no longer OOM. (Behind the off-by-default
  `retroachievements` feature.)
- **Lua scripting** surfaces a failed instruction-budget hook install
  (`mlua::set_hook`, fallible since 0.11) as a `ScriptError` instead of
  silently running scripts without the runaway-guard. (Behind the off-by-default
  `scripting` feature.)
- **Mapper 89 (Sunsoft-2) — *Tenka no Goikenban: Mito Koumon*** now models the
  board's **bus conflict** (a register write is ANDed with the PRG-ROM byte at
  the written address, per nesdev `INES_Mapper_089` "BUS CONFLICTS"). Without it
  the raw value selected the wrong CHR/PRG bank and the background nametable
  never latched. Isolated to mapper 89 (its only dump); AccuracyCoin and all
  other titles unaffected.

### Dependencies

- Dependency modernization (v1.2.0 beta.1). Merged the safe Dependabot bumps
  (naga 28, toml 1.1, gilrs 0.11, tokio-tungstenite 0.29) and `cargo update`'d
  the rest of the compatible tree (image 0.25.9, tiff 0.10, windows 0.62, …).
  Migrated the breaking bumps: **png 0.18** (decoder `Read + Seek` + fallible
  `output_buffer_size`), **criterion 0.8** (`std::hint::black_box`), **mlua
  0.11** (`set_hook` now fallible), **cpal 0.18** (`SampleRate` is a `u32` alias;
  `StreamConfig` is `Copy` + passed by value). `rfd 0.17` is deferred — its
  sync xdg-portal/libdbus backend fails to compile on Rust 1.86 (an upstream
  rfd bug), so rfd stays at 0.14 pending a fixed release. The egui / egui-wgpu /
  egui-winit + wgpu UI-stack cluster bump (**egui 0.29 → 0.32, wgpu 22 → 25,
  naga → 25**) landed as one coordinated upgrade; `deny.toml` now allows the
  `Ubuntu-font-1.0` license that egui 0.32's bundled default font declares.

## [1.1.0] - 2026-06-15 - "Scriptable" (Feature Release)

The first feature release after the v1.0.0 production cut. It folds in the
(never-separately-tagged) **v1.0.1** compatibility + hygiene work and all four
**v1.1.0** feature workstreams: visual filters (full NES_NTSC composite + a
CRT / scanline shader pass + `.pal` palette loading), input & peripherals
(NES Power Pad, turbo / autofire, an input-display overlay, and a per-game
nametable-mirroring override database), debugger devtools (breakpoints, a cycle
trace logger, an event viewer), audio (NSF / NSFe music player, 5-band graphic
EQ), and the flagship **Lua scripting engine** (sandboxed
Lua 5.4, Mesen2 / FCEUX-style `emu` API). Additive only — the determinism
contract and **AccuracyCoin 100%** hold; every new state-mutating path
(Lua writes, new cheats) is gated off in netplay / TAS replay / RA-hardcore.

### Added

- **Lua scripting — frontend integration** (v1.1.0 beta.3, Workstream E flagship, T-110-E5 —
  **completes Workstream E**). The Lua engine is now usable from the app (behind the
  default-OFF, native-only `scripting` feature): a **Lua Script console** (Debug → Lua Script)
  loads / reloads / stops a `.lua` file and shows its log + errors + `onFrame` callback count.
  The engine is pumped once per redraw under the emu lock with the live `Nes`; script overlay
  draws (`drawText`/`drawRect`/`drawPixel`) render through the egui pass, and control commands
  apply via the existing pause / save-state path. **Write-gating** is wired: `emu.write` is
  disabled during netplay, TAS replay/record, and RA-hardcore (the cheat-path policy). Ships
  with `examples/scripts/` (`hud.lua`, `ram_watch.lua`) and a full `docs/scripting.md` API
  reference. The feature is off by default, so the shipped/wasm/no_std builds are byte-identical
  (`setInput` application + pixel-perfect overlay mapping + `onNmi`/`onIrq` are documented
  follow-ups).
- **Lua scripting API — callbacks, control & overlay** (v1.1.0 beta.3, Workstream E flagship,
  T-110-E2 — engine surface). Extends the Lua engine with the full callback + control + draw
  surface: `emu.onExec(addr,fn)` / `onRead(addr,fn)` / `onWrite(addr,fn)` (dispatched by
  replaying the frame's exec PCs and a new gated bus-access log — reads + writes + values),
  control commands `emu.pause` / `saveState` / `loadState` / `setInput`, and an overlay draw API
  `emu.drawText` / `drawRect` / `drawPixel`. Control + draw requests are *collected* (drained by
  the host) so the host stays the sole owner of emulator control and can gate state-mutating
  actions. The bus-access log is a new `debug-hooks` tap (output-only, off by default, cleared
  per frame), so determinism / AccuracyCoin are unaffected (reverified byte-identical). The
  frontend script console + overlay rendering + write-gating wiring + `examples/scripts/` +
  `docs/scripting.md` land in the next beta.3 PR.
- **Lua scripting engine** (v1.1.0 beta.3, Workstream E flagship, T-110-E1..E4 — foundation).
  A new `rustynes-script` crate embeds sandboxed **Lua 5.4** (vendored `mlua`) and exposes a
  Mesen2 / FCEUX-style `emu` API: `emu.read` / `readRange` / `write`, `emu.cpu()` (A/X/Y/S/P/PC),
  `emu.frame` / `cycle`, `emu.log`, and `emu.onFrame(fn)` per-frame callbacks. The engine is
  **host-driven** — the frontend calls it once per frame and binds live-`Nes` accessors via
  `mlua::Lua::scope` (see ADR 0010). **Sandboxed** (only `table`/`string`/`math`/`coroutine`;
  no `io`/`os`/`package`/`require`/`debug`/`load`/`dofile`) with a per-frame VM-instruction
  budget against runaway scripts. **Determinism-safe:** the crate is pulled in only behind the
  frontend's (off-by-default) `scripting` feature — so the shipped build, the wasm build, and
  the `no_std` cross-compile never compile it and stay byte-identical — and `emu.write` pokes
  only system RAM after `run_frame` (cheat-path mechanism) and is gated off via
  `set_writes_locked` (netplay / TAS / RA-hardcore). Headless API + sandbox-escape + budget
  tests. (The frontend script console, `onExec`/`onRead`/`onWrite` event callbacks, the overlay
  draw API, save-state/input control, `examples/scripts/`, and `docs/scripting.md` land in the
  next beta.3 PR.)
- **Graphic equalizer** (v1.1.0 beta.2, Workstream D, T-110-D2). An optional 5-band graphic
  EQ (60 / 240 / 1k / 3.8k / 12k Hz, ±12 dB each) in Settings → Audio. It runs as a
  **frontend output stage** — cascaded RBJ peaking biquads applied on the producer side after
  the dynamic-rate resampler, exactly like the existing master-gain stage — so it never
  touches the deterministic core synthesis. Off by default and bypassed when flat, so audio
  is **byte-identical** to before unless you engage it; live-updates while playing (the
  Settings sliders push params through the shared audio queue and the producer rebuilds its
  biquads on the next frame). Native-only for now (a wasm feed is a follow-up).
- **NSF / NSFe music player** (v1.1.0 beta.2, Workstream D, T-110-D1). RustyNES now plays
  `.nsf` chiptune files: drop one in (or File → Open) and an **NSF Player** panel opens with
  the file's title / artist / copyright and a track selector (Prev / Next / Restart + a
  song-index slider). Mechanism follows Mesen2 / FCEUX — a parsed NSF builds a synthetic
  `NsfMapper`: the program image is mapped (with `$5FF8-$5FFF` 4 KiB bank-switching), a tiny
  6502 driver is served at `$5000`, and the reset / NMI vectors point at it. The driver runs
  `init` for the selected song, enables vblank NMI, then spins; the ordinary 60 Hz NMI calls
  `play` once per frame. Playback therefore runs through the **unchanged** `Nes::run_frame`
  lockstep loop (new `Nes::from_nsf` / `nsf_set_song` construction path; cartridge games are
  untouched and stay byte-identical). Scope: base 2A03 APU, NTSC 60 Hz. Expansion-chip audio
  (VRC6/7, MMC5, N163, 5B, FDS), exact non-60 Hz play rates, and a wasm NSF loader are
  documented follow-ups.
- **Event viewer** (v1.1.0 beta.2, Workstream C, T-110-C3). A new **Events** debugger panel
  (Debug → Event Viewer) plots the frame's CPU writes — PPU (`$2000-$3FFF`), APU
  (`$4000-$4017`), and mapper (`$4020-$FFFF`) — on a scanline×dot grid coloured by kind,
  so you can see *when* in the frame a game touches scroll / mapper / APU registers. Built
  on the `debug-hooks` event log (a single tap in the bus write path, tagged with the live
  PPU position, reset per frame); output-only and off by default, so determinism /
  AccuracyCoin are unaffected. (NMI/IRQ markers are a follow-up.)
- **Cycle trace logger** (v1.1.0 beta.2, Workstream C, T-110-C2). A new **Trace** debugger
  panel (Debug → Trace Logger) records each executed instruction's CPU register file +
  cycle into a bounded ring (50k), shows a live disassembled tail, and **exports** the
  full trace to a text file. Built on the `debug-hooks` feature; the ring is output-only
  (bounded, mutates no emulation state) and off by default, so determinism / AccuracyCoin
  are unaffected and headless builds keep a byte-identical hot path.
- **Debugger breakpoints** (v1.1.0 beta.2, Workstream C, T-110-C1). Exec/PC breakpoints:
  add addresses in the CPU debugger panel's new **Breakpoints** section (armed toggle,
  hex add, per-row remove, clear); when the program counter reaches one, emulation
  **pauses and the CPU panel opens** on the stopped PC. Built on a new off-by-default
  `debug-hooks` core feature (a break-check at the top of the `run_frame` loop) — the
  hook is output-only (stops the partial frame + records the PC, mutating nothing), so
  determinism / AccuracyCoin hold even with it on, and the headless test/bench builds
  (which omit the feature) keep a byte-identical hot path. Read/write watchpoints +
  conditions are a follow-up.
- **NES Power Pad — playable** (v1.1.0 beta.1, Workstream B, T-110-B1). The Power Pad /
  Family Fun Fitness mat is now selectable as the player-2 expansion device (Settings →
  Input "Port 2 device"); its 12 mat buttons default to a left-hand grid (`1`–`4` /
  `Q W E R` / `A S D F`, chosen to avoid the P1 and system-speed keys). Completes the
  device shipped in the previous beta entry: the held mat
  buttons flow through `InputState` → `FrameInputs` (+ the emu-thread `SharedInput`) and
  are fed to the device in `EmuCore::latch`. Off by default (standard controller), so
  the default + Four Score paths stay byte-identical. Native path (like Zapper/Vaus);
  rebindable mat keys + a wasm-canvas feed are follow-ups.
- **Per-game database — nametable mirroring override** (v1.1.0 beta.1, Workstream B,
  T-110-B4). A CRC32-keyed game database (vendored from TetaNES, MIT OR Apache-2.0,
  ~2.6k entries) that auto-corrects ROMs whose iNES header carries the wrong mirroring
  flag: at load the frontend computes the ROM's CRC32 (over PRG-ROM+CHR-ROM) and, if
  listed, applies a nametable mirroring override via `Nes::set_mirroring_override`. The
  override is implemented in the bus's nametable translation (uniform across all
  mappers, no per-mapper edits), does not touch mapper-supplied VRAM (4-screen), and is
  persisted in the save-state (so rollback / restore stay consistent). It is
  **frontend-only** and **`None` by default**: the core test suites construct the `Nes`
  directly and never consult the database, so AccuracyCoin / the commercial oracle stay
  byte-identical. Deterministic (same CRC ⇒ same mirroring), so netplay peers agree.
  (Region / mapper overrides and the Game Genie code-name database are tracked
  follow-ups.)
- **NES Power Pad device — core** (v1.1.0 beta.1, Workstream B, T-110-B1, core of 2).
  The 12-button Power Pad / Family Fun Fitness mat as an opt-in per-port `InputDevice`
  overlay: `PowerPadState` implements the dual-8-bit-shift-register serial protocol
  (the `NESdev` / Mesen bit layout — buttons read out LSb-first on `$4017` bits 3 and
  4, with the standard `$4016` strobe), exposed via `Nes::set_power_pad(port, buttons)`
  and round-tripped in the save-state. Opt-in: with no device attached the
  standard-controller + Four Score read paths stay **byte-identical**, so determinism /
  AccuracyCoin / the oracle are unaffected and the `no_std` core is unchanged. Protocol
  unit-verified (button→serial-position mapping, strobe-reload, save-state round-trip).
  (The frontend key mapping that drives the mat landed in the same beta — see "NES
  Power Pad — playable" above.)
- **Turbo / autofire** (v1.1.0 beta.1, Workstream B, T-110-B2). The A and/or B button
  can rapid-fire while held, configurable via `[input] turbo_a` / `turbo_b` and a
  `turbo_period` (frames per on/off half-cycle; Settings → Input "Turbo / autofire").
  **Off by default** (empty mask = byte-identical input). The strobe is applied where
  input meets the NES — keyed on the **emulated frame number** (a new pure
  `Nes::frame()` accessor) — in `EmuCore::latch` and on the local input in both netplay
  paths, so the **gated bits are what get latched / recorded / sent**: it is
  deterministic and rollback / TAS / netplay-safe, AccuracyCoin / the oracle are
  unaffected, and the `no_std` core is unchanged. (The lightweight `wasm-canvas` embed
  does not apply turbo; the native + wasm-winit paths do.)
- **Input-display overlay** (v1.1.0 beta.1, Workstream B, T-110-B3). A new
  read-only tool panel (`debugger/input_display_panel.rs`) that draws a stylized NES
  controller per active player — D-pad, Select/Start, B/A — with each currently-held
  button lit. Open from **Tools → Input Display** or the debugger toolbar's "Input
  HUD" checkbox; shows P1+P2 (and P3/P4 with Four Score). Useful for TAS authoring and
  streaming. It reads the same held-button snapshot the emulator is fed (pushed each
  frame via `DebuggerOverlay::set_input_display`), so it is frontend-only with no core,
  produce-path, or determinism impact.
- **True composite NES_NTSC filter** (v1.1.0 beta.1, T-110-A1, stage 2 of 2 — the
  shader). A faithful GPU port of Bisqwit's `nes_ntsc` algorithm
  (`crates/rustynes-frontend/src/ntsc_bisqwit.rs`): it reconstructs the analog
  composite signal from the PPU's palette-index framebuffer and demodulates it back to
  RGB with a windowed Y/I/Q filter, so genuine NTSC artifacts (chroma dot-crawl, colour
  fringing on vertical edges, the saturated-hue "checkerboard") fall out of the math
  rather than being faked. Selected via a new `[graphics] ntsc_filter = "composite-rt"`
  mode (Settings → Display); the existing `"composite"` / `"rgb"` remain the simplified
  blur. The index framebuffer is uploaded as an `R16Uint` texture (read via
  `textureLoad`, no sampler — WebGL2-safe) only while the filter is active, snapshotted
  under the same brief present lock as the framebuffer; all signal/sine/YIQ/emphasis
  tables are computed in Rust and baked into the WGSL as `var<private>` arrays (no
  storage buffers → WebGL2-safe). Render priority is CRT > true-NTSC > simplified blur.
  Off by default = byte-identical presentation; frontend-only (the parallel index
  buffer is read-only output) → AccuracyCoin / determinism / the commercial oracle are
  unaffected, and the `no_std` core is unchanged. WGSL parse+validate and LUT/sine unit
  tests in CI; native + both wasm flavours clippy clean. Uses Bisqwit's neutral
  defaults (per-knob tuning is a later pass).
- **NES_NTSC composite filter — core foundation** (v1.1.0 beta.1, T-110-A1, stage 1
  of 2). The cycle-accurate PPU now emits, alongside the RGBA framebuffer, a parallel
  per-pixel **palette-index** framebuffer (256×240 `u16`s, each the 9-bit
  `(emphasis << 6) | colour` value) plus a per-frame **NTSC colour phase** (0..=2, the
  `videoPhase` source of the dot-crawl, derived from a master-cycle counter). Exposed
  as `Ppu::index_framebuffer()` / `Ppu::ntsc_phase()`, routed through
  `Bus`/`Nes::index_framebuffer()` + `ntsc_phase()`. These are **output-only**: they
  carry exactly the LUT index used to produce the displayed RGBA (proven by a unit
  test that `rgba_lut[index] == framebuffer[pixel]` for every emitted pixel), feed no
  emulation logic, and are not part of the save-state — so the determinism /
  AccuracyCoin contract is unaffected, and the `no_std` chip stack still cross-compiles
  against `core` + `alloc`. The composite encode→decode WGSL shader that consumes this
  index buffer follows in stage 2.
- **CRT / scanline video filter** (v1.1.0 beta.1). A new presentation-layer wgsl
  post-pass (`crates/rustynes-frontend/src/crt.rs`) that applies source-row-space
  scanlines (a parabolic per-row brightness profile) + a subtle RGB aperture-grille
  mask, with a live **scanline-intensity** slider (`[graphics] crt_scanline`,
  default 0.5) and an on/off toggle (`[graphics] crt_filter`, default off) in the
  Settings → Display tab. Off by default = byte-identical presentation; mutually
  exclusive with the NTSC filter (CRT wins). A frontend-only effect — no core /
  framebuffer change, so AccuracyCoin + determinism are unaffected. The embedded
  WGSL is parse+validate-tested in CI.
- **Custom `.pal` palette loading** (v1.1.0 beta.1). Load a 64-entry `.pal` palette
  file (192-byte form; longer files use the first 64 colours) to re-tint the
  display via the PPU's colour LUT. `rustynes-ppu` gains `build_rgba_lut_from_base`
  - `Ppu::set_custom_palette` (applying the standard 2C02 composite emphasis to a
  custom base table), routed through `Nes::set_custom_palette`. The frontend adds a
  `[graphics] palette_file` config, a Settings → Display **Load .pal… / Built-in**
  picker (native), and re-applies the configured palette on every ROM load. Default
  (none) is **byte-identical** to the built-in palette — proven by a unit test that
  `build_rgba_lut_from_base(&NES_PALETTE)` equals the built-in composite LUT — so
  AccuracyCoin + the commercial oracle are unaffected. Native-only file I/O (a
  no-op on wasm).

### Fixed

- **Lua callback registry is isolated Rust-side (structural crash-proofing).** The
  `onFrame`/`onExec`/`onRead`/`onWrite` callbacks are stored Rust-side as `mlua::RegistryKey`s
  (a `Vec` + per-address `HashMap`), **not** in a script-visible Lua global. A script can register
  callbacks but cannot inspect, clobber, or inject junk into the registry, so no malformed
  registry value can ever error the host pump — the protection is *structural*. The per-address
  callback replay gates on the Rust `HashMap` keys (an O(1) check, no Lua FFI for the ~75k
  non-matching exec/access events per frame). Test `callback_registry_is_not_script_visible`
  asserts `__rustynes` is `nil` to scripts. (This was reached via a review cycle, #49–#57, that
  first hardened a script-visible `__rustynes` global traversal-by-traversal against junk keys /
  values / non-functions, then moved the registry Rust-side to remove the surface entirely.)
  Core untouched (AccuracyCoin byte-identical); ADR 0010 + `docs/scripting.md` updated.
- **Lua scripting — self-review hardening (M2/L1–L4).** A code-review pass on the Workstream-E
  code: (M2) `pump_scripts` holds the emulator lock only around `on_frame` (the log/control/
  draw drains moved outside it) and the default per-frame instruction budget dropped 5M→1M, so a
  runaway script stalls emulation far less; (L1/L3) the script overlay now maps onto the actual
  letterboxed game rect (8:7 PAR + overscan aware) instead of stretching to the window, and the
  NSF panel notes its ~60 Hz tempo approximation; (L2) `emu.setInput` logs a one-time
  "accepted but not yet applied" notice instead of being a silent no-op; (L4) non-primitive
  `emu.log` args render as their Lua type name, not a `{:?}` debug dump. Core untouched
  (AccuracyCoin byte-identical).
- **Bot-review sweep round 2 (PRs #38–#48)** — applied the actionable
  `gemini-code-assist` / Copilot suggestions from the v1.1.0 feature PRs:
  - **`emu.read` / debugger peek no longer has side effects** (Copilot #46): `Nes::peek`
    now uses the bus's `debug_peek_cpu`, so a script/debugger read of `$2002` doesn't clear
    the VBL flag and `$2007` doesn't advance the read buffer — restoring the determinism
    guarantee. (AccuracyCoin reverified byte-identical.)
  - **`onExec` no longer replays stale/duplicate PCs** (gemini #47, *critical*): the Lua
    `onExec` callback now reads a dedicated **per-frame exec-PC log** (`Nes::exec_log`, cleared
    each frame) instead of the 50k rolling trace buffer, and is **independent** of the Trace
    Logger panel's recording (Copilot #48).
  - **Lua replay no longer crosses the FFI per instruction/access** (gemini #47): a Rust-side
    active-address set gates the Lua lookup, so only addresses with a registered callback pay
    the cost.
  - **`emu.readRange` is bounded** (gemini/Copilot #46): capped at 64 KiB with `wrapping_add`,
    so a script can't OOM/overflow the host; control/draw command queues are per-frame capped.
  - **Breakpoints fire at a frame's starting PC** (gemini #41): replaced the blind
    "skip first iteration" with a precise `skip_breakpoint_at`, so a breakpoint at the PC after
    a reset / save-state load / manual change still triggers; and a hit now **stops the
    catch-up burst** so the core doesn't advance past the stop frame (Copilot #41).
  - **Event viewer captures `$4014` OAM DMA + `$4016` strobe** (Copilot #43): the tap now covers
    the whole `$4000-$4017` I/O window its legend advertises.
  - **Trace panel disassembly can't panic** (gemini/Copilot #42): the 3-byte peek window uses
    `.get().unwrap_or(0)` instead of an out-of-range `& 3` index.
  - **NSF programs loaded at `$6000-$7FFF` now play** (gemini #44): a non-bankswitched NSF
    serves its program there before falling back to WRAM.
  - Lua engine hardening: `pump_scripts` takes a single emu lock (gemini #48), a failed
    (re)load clears the previous script first (gemini #48), the overlay honours a non-zero
    `screen_rect().min` (gemini #48), the dead `ScriptError::Runtime` variant was removed, and
    the wasm/native `cfg` gates on the overlay path + console availability were made consistent
    (Copilot #48). Default build remains byte-identical.
- **CRT / NTSC filters now letterbox + aspect-correct like the main blit**
  (v1.1.0 beta.1, review feedback on T-110-A1/A2). The CRT, simplified-NTSC, and
  true-composite-NTSC post-passes scaled the oversized fullscreen triangle's clip-space
  **position**, which re-introduced the bottom/fullscreen edge-smear the main blit
  documents (and they ignored 8:7 pixel-aspect correction + overscan crop). All three
  now use the same proven UV-space letterbox + clip-to-black + overscan-crop as the main
  blit (the shared `gfx::letterbox_uniform`), so a filtered picture has correct aspect,
  honours "Hide Overscan" / "8:7 Pixel Aspect", and shows clean black bars instead of
  smeared edge texels.
- **Mapper 89 (Sunsoft-2) — Mito Koumon background rendering.** The `$8000-$FFFF`
  register decode had bit 7 and bit 3 swapped: RustyNES used bit 7 for the
  one-screen mirroring select and bit 3 for the CHR-bank high bit, but the
  hardware layout (`CPPP MCCC`, per nesdev `INES_Mapper_089` and Mesen2's
  `Sunsoft89`) is bit 7 = CHR high bit (A16), bit 3 = mirroring. The wrong
  mirroring displayed the empty single-screen B while the game wrote its
  background to single-screen A, so the background appeared blank (only sprites
  rendered). *Tenka no Goikenban: Mito Koumon* now renders its title screen and
  backgrounds correctly. Isolated to mapper 89; AccuracyCoin 100% and every other
  game are unaffected.

### Changed (docs / hygiene)

- **v1.0.1 compatibility + hygiene patch** (docs-only; zero core change, AccuracyCoin stays
  100%). Audited the v1.0.1 plan: most hygiene was already complete (phase-7/8 to-do folders
  archived, all 24 `#[ignore]`s carry permanent-by-design reasons, the roadmap banner frames the
  `v1.x`/`v2.x` markers as engine lineage, `deny.toml` allows the shipped license set, no open
  Dependabot PRs). This patch closes the remainder: the present-tense `[→]` ticket markers under
  the already-"SUPERSEDED" Phase 6 roadmap section are flipped to historical `[~]` with a note
  that they record the *then-current* state (not live TODOs), so the roadmap no longer reads as
  if accuracy work is ongoing. The three flagged compatibility gaps (Mito Koumon mapper-89 render,
  FDS *Kid Icarus* side-B stall, GxROM "Mario flashing") are documented (not force-fixed — their
  dumps are gitignored / they need interactive verification and the fixes would risk the 100%
  core). Full audit in `to-dos/v1.0.1-compat-hygiene/00-patch.md`.
- Reconciled `docs/STATUS.md` to the shipped v1.0.0 reality: the master-clock core
  is the default and only scheduler (the `mc-r1-full-cpu` umbrella was promoted to
  default and the flag removed), so the AccuracyCoin row now reads **100.00%**
  (not the pre-promotion 90.65%), the "Known residuals (deferred to v2.0)" section
  is re-titled **CLOSED**, and the removed feature-flag rows are marked historical.
- Archived the stale `to-dos/phase-7-*` + `phase-8-*` accuracy plans under
  `to-dos/archive/` and stood up release-named `to-dos/v1.0.1-compat-hygiene/` +
  `to-dos/v1.1.0-features/` (the live forward roadmap); updated `ROADMAP.md` +
  `to-dos/README.md` pointers.
- Re-worded the 7 "pin-the-floor" `#[ignore]` unit probes (cpu/apu/ppu) whose
  reasons cited the removed `mc-r1-full-cpu` flag as opt-in — they now state plainly
  that they pin superseded pre-master-clock unit behaviour (real coverage is the
  now-green `cpu_interrupts_v2` 5/5 + AccuracyCoin 100%); scrubbed the stale
  "known fail on the default lockstep build" comments in `cpu_interrupts_v2.rs`.

---

## [1.0.0] - 2026-06-13 - "Cycle-Accurate" (Production Release)

**The headline: RustyNES's emulation core has been replaced with a new
cycle-accurate, master-clock-precise engine, and the original RustyNES desktop
experience has been ported onto it.** This is the first 1.0 release: a
hardware-accurate NES emulator with a polished desktop shell, a live in-browser
WASM demo, and a synthesized documentation set.

The `v0.9.x` sections below (`v0.9.0` -> `v0.9.7`, all dated this same synthesis
day) are the **documentary lineage** of the new engine — they record, re-versioned
onto RustyNES's numbering, the work that produced the core now shipping in 1.0.0.
This `[1.0.0]` entry is the synthesis itself: the new core plus the v1.0.0-specific
shell, docs, and web work.

### The new core (replaces the entire v0.8.6-and-earlier emulation engine)

- **Master-clock-precise lockstep scheduler.** The PPU is the master clock; the
  CPU advances on its region-correct divider (3:1 NTSC/Dendy, 3.2:1 PAL), the APU
  every other CPU cycle — one shared timebase rather than the old
  PPU-stepped-around-CPU-instructions model. Mid-instruction PPU events
  (sprite-zero hit at an exact dot, MMC3 IRQ at a precise dot, mid-scanline scroll
  writes) are visible to subsequent CPU code without per-quirk patches.
- **AccuracyCoin 100.00% (139/139).** The kevtris AccuracyCoin hardware-accuracy
  battery passes completely (the prior core's accuracy ceiling was the Blargg
  suite; this clears the substantially harder AccuracyCoin gate, RAM-direct
  decoded). nestest matches the golden log with zero diff.
- **Determinism is a hard contract.** Same seed + ROM + input sequence yields a
  bit-identical framebuffer and audio stream — the foundation for save-state
  round-trips, regression oracles, TAS movies, and rollback netplay.
- **Band-limited audio.** A polyphase BLEP / windowed-sinc synthesizer
  (256 phases x 32 taps, ~81 dB SFDR) with a lookup-table non-linear mixer and a
  3-stage analog filter chain.
- **51 mapper families** (up from the original 5), including the expansion-audio
  chips (VRC6, VRC7 OPLL FM, Sunsoft 5B, Namco 163, MMC5), the full VRC family,
  Sunsoft FME-7, and the Taito / Sunsoft / Irem / Jaleco / Bandai / Konami long tail.
- **Famicom Disk System** (real-BIOS boot, read/write drive, writable `.fds.sav`,
  2C33 wavetable audio) and **Nintendo Vs. System / PlayChoice-10** RGB-PPU
  arcade support (2C03/2C04/2C05 palettes).
- **Rollback netplay** (GGPO-style, 2-4 players, native UDP + browser WebRTC mesh),
  **TAS movie** record/playback, **Game Genie + raw-RAM cheats**, **rewind**, and
  an opt-in native **RetroAchievements** integration (achievements, leaderboards,
  rich presence, hardcore mode).
- **WebAssembly target** with a live in-browser build, plus an integrated egui
  debugger overlay (CPU / PPU / APU / memory).
- **Performance-tuned frontend:** display-sync pacing, a lock-free audio ring with
  dynamic rate control, run-ahead to cancel game-internal input lag, and a
  dedicated emulation thread — the smoothest, lowest-latency playback path
  RustyNES has shipped.

### Added — v1.0.0 synthesis (the desktop shell + web + docs ported onto the new core)

- **Desktop UX shell.** An always-on egui menu bar (File / Emulation / Tools /
  View / Debug / Help) and status bar frame the NES image independently of the
  `` ` `` debugger overlay:
  - **File** — Open ROM (`F12`), Open Recent (missing files greyed out, with
    Clear Recent), save / load state, a ten-slot (0–9) Save Slot picker (set
    active slot + save/load to a specific slot), a thumbnail **Save States…**
    manager, Take Screenshot, Copy Screenshot to Clipboard (native), and the FDS
    Swap Disk Side (`F9`) item when an FDS game is loaded.
  - **Emulation** — Pause/Resume (`Space`, disabled during netplay), Reset (`F2`),
    Power Cycle (`F3`), Frame Advance (`\`, steps one frame while paused), a
    hold-`Tab` Fast Forward hint, **Speed presets** (25 / 50 / 75 / 100 / 150 /
    200 / 300 %, keys `=` / `-` / `0`), Run-Ahead 0–3, a read-only region label,
    and Vs. System Insert Coin (`F10`) for Vs. titles.
  - **Tools** — Cheats, TAS Movies (record `F6` / play `F7` / branch `F8`),
    Netplay, RetroAchievements, and the Performance Monitor, each opened as a
    floating window without needing the debugger overlay.
  - **View** — the tabbed Settings window (Display / Audio / Input / Advanced),
    Light / Dark / System themes, an 8:7 pixel-aspect toggle, a Hide Overscan
    toggle, Fullscreen (`F11`), Window Size presets (1x / 2x / 3x / 4x of the NES
    resolution), a Show FPS toggle, a Pause When Unfocused toggle (auto-pause on
    focus loss), and Show Menu Bar (`M`).
- **Audio, speed, and presentation controls.**
  - **Master volume** slider + mute, and **per-APU-channel mutes** (Pulse 1 /
    Pulse 2 / Triangle / Noise / DMC / Mapper Audio) in the Settings Audio tab —
    all-on by default (a playback overlay; the deterministic core output is
    byte-identical).
  - **Emulation-speed presets** (25 %–300 %) with pitch-shifted, glitch-free audio
    at non-100 % speeds; the status bar shows the speed when it is not 100 %.
  - A **thumbnail save-state manager** (File → Save States…) showing each slot's
    frame thumbnail + timestamp with per-slot save / load.
  - **Optional overscan cropping** (`[graphics] hide_overscan`), a **pause-dim**
    overlay, a **gamepad deadzone slider**, **controller hot-plug** toasts,
    **screenshot-to-clipboard**, a **Reset-to-Defaults** button per Settings
    section, and a frame-time **sparkline** in the Performance panel.
- **Frontend quality-of-life keys.** Hold-`Tab` fast-forward (runs the emulator
  unthrottled with audio muted) and `\` frame-advance (steps exactly one frame
  while paused), both default-bound in `[input.system]` and rebindable.
  - **Debug / Help** — the debugger overlay and per-chip panels; Keyboard
    Shortcuts and About.
- **Status bar** showing the ROM name, region, mapper, run-ahead depth, the
  Running / Paused / Netplay state, and the FPS readout, plus a translucent
  "PAUSED" overlay over the picture when paused.
- **First-run Welcome modal** with a quick-start shortcut list, shown once on a
  fresh install.
- **Live WebAssembly / GitHub Pages demo.** A playable in-browser build hosted on
  GitHub Pages, with the egui debugger overlay, AudioWorklet audio, and rAF
  display-sync.
- **Synthesized documentation set.** Per-subsystem specifications
  (CPU 6502, PPU 2C02, APU 2A03, mappers, cartridge format, scheduler), the
  cross-cutting architecture / testing-strategy / performance / compatibility
  docs, the Architecture Decision Records, and a unified user guide — re-grounded
  on the new core.
- **Branding and imagery** refreshed for the 1.0.0 release (logo, screenshots,
  showcase montage).

### Fixed

- **8:7 pixel-aspect and fullscreen now letterbox correctly** — the NES image is
  framed with clean black bars instead of garbage / smeared edges around it.
- **File / Tools submenus auto-close on hover-away**, matching standard menu-bar
  behaviour.
- **About window** credits "Created by DoubleGate" and its **GitHub** link opens
  the project page in a browser.
- **Pause is reachable and reversible** — a `Space` keybind toggles pause/resume,
  and the menu bar keeps repainting (~30 Hz) while paused so Resume is always
  clickable (previously the parked emulator left the menu frozen).
- **View -> Window Size keeps the chrome readable** — the size presets scale only
  the emulated picture (which letterboxes), with the menu / status bars held at a
  fixed width, so the menu no longer clips and the mouse stays aligned with its
  hit-areas.
- **Game Genie cheats apply reliably** — enabled codes are re-synced to the live
  core every frame the Cheats panel is open and re-applied after Reset / Power
  Cycle, so they keep working across a soft reset and a cold boot.

### Changed

- The desktop frontend now targets the new core's deterministic, lockstep API
  surface. Save-state, configuration, and input handling are re-wired onto it.
- Audio path reworked end-to-end (lock-free ring + dynamic rate control) and
  display pacing made display-sync-aware; input is latched immediately before
  each emulated frame to cut button-to-pixel latency.

### Notes

- The `v2.x` numbers referenced as "engine lineage" inside the `v0.9.x` entries
  are the upstream cycle-accurate-engine version tags, recorded for traceability
  only; they are not RustyNES release numbers.
- A small set of deep edge cases (a handful of internal-bus / IRQ-sample-timing
  AccuracyCoin sub-tests) remain documented in `docs/compatibility.md`;
  AccuracyCoin still reports a clean 139/139 on the shipping configuration.

---

## [0.9.7] - 2026-06-13 - Optimized Performance (documentary lineage)

*Documentary lineage of the cycle-accurate engine (integrated from the engine's
v2.8.0 work). Delta versus the prior engine stage — the stock-NES per-frame output
is byte-identical (AccuracyCoin 100.00% / 139/139; commercial oracles unchanged):
dynamic rate control lives in a frontend resampler stage and run-ahead is frontend
orchestration of the existing snapshot/restore, so the core is untouched.*

### Added

- **Display-sync pacing matrix** (`pacing_mode = auto | display | vrr | wallclock`,
  default `auto`): `display` makes vsync the clock (one emulated frame per refresh
  when the panel is within 0.5% of the console rate, the audio DRC absorbing the
  sub-percent bend) with an occlusion watchdog and a sustained-miss sticky fallback;
  `vrr` is vsync + a wall-clock pacer for G-Sync/FreeSync; `wallclock` is the
  sleep-then-spin pacer. Configurable swapchain depth (`max_frame_latency`).
- **Run-ahead** (`run_ahead`, default 1, 0-3): runs hidden frames each visible
  frame to remove the game's own internal input lag; the persistent timeline stays
  byte-identical, auto-disabled during netplay / movies / rewind, median-cost
  budget-throttled.
- **Dynamic rate control** (4-tap Hermite resampler nudging the output rate +/-0.5%
  from audio-queue occupancy) so audio never underruns/overruns from host-vs-DAC
  clock drift; off is a bit-exact passthrough.
- **Dedicated emulation thread** (default-on native) so UI / egui / wgpu-submit /
  file-I/O stalls no longer disturb emulation cadence, with best-effort Linux
  thread-priority elevation; inputs read from a lock-free `SharedInput` so the late
  latch survives.
- **Debugger Performance panel** with produced-vs-presented interval histograms
  (p50/p95/p99/max), audio-queue health, pacer-anomaly counters, optional GPU pass
  timing, and an opt-in 1 Hz CSV performance log.
- **Browser AudioWorklet** output (replacing the deprecated `ScriptProcessorNode`)
  - **rAF display-sync** on ~60 Hz panels (eliminating the wall-clock-vs-rAF beat).

### Changed

- Audio output is a hand-rolled lock-free SPSC ring with an allocation-free
  callback, start-gating, and a hard resync.
- Input is latched immediately before each emulated frame (the late latch),
  removing up to a frame of button-to-emulation latency.
- Core micro-optimizations: a `MapperCaps` capability cache skips unused
  per-CPU-cycle virtual calls, a 512-entry `(emphasis, color) -> RGBA` LUT for pixel
  emission, fat LTO, and auto-vectorized BLEP-scatter / rewind-XOR loops — all
  byte-identical by construction.

### Performance

- Rendering-heavy bench -26.0%, nestest bench -16.0%; snapshot fast path
  36 -> 14.6 us. Comfortably inside the 16.64 ms NTSC budget (~5-8x realtime).

---

## [0.9.6] - 2026-06-13 - Platform Expansion + RetroAchievements (documentary lineage)

*Documentary lineage of the cycle-accurate engine (integrated from the engine's
v2.6.0 -> v2.7.1 work). No accuracy/behaviour change to stock NES play
(AccuracyCoin 100.00%); every new path is gated on mapper / console-type /
player-count.*

### Added

- **RetroAchievements** (opt-in, native-only): achievements, leaderboards, rich
  presence, and hardcore mode via the vendored MIT `rcheevos` C library wrapped in
  a safe `RaClient` (hand-written FFI, no bindgen). Login with persisted token, an
  achievements panel with measured progress, unlock toasts, badge images fetched
  off-thread, and per-game progress persisted outside the deterministic snapshot.
  Hardcore mode disables save-load / rewind / cheats / frame-advance / debugger
  memory.
- **Vs. System / PlayChoice-10 RGB game-verification** (mapper 99 + clean-iNES
  byte-7 detection immune to the `0x0A` corruption) — VS Excitebike / Clu Clu Land /
  Castlevania and the PC10 dumps render through the 2C03 RGB palette; a SHA-256
  Vs.-game database supplies DIP presets and exact 2C04-000x palettes.
- **+13 mapper families (38 -> 51):** Taito (TC0190 / TC0690 / X1-005 / X1-017),
  Sunsoft-1/2/3R, Irem G-101, Jaleco, Bandai, Konami-VS, and more — all visually
  verified.
- **N-peer netplay:** a UDP multi-joiner roster handshake (2-4 players) plus a
  full browser **WebRTC mesh** (one data channel per peer) with a reference
  signaling server and a deployable `deploy/` bundle (signaling + `wss://` proxy +
  STUN/TURN).

### Fixed

- **Real-BIOS Famicom Disk System boot now works** (the device previously hung on
  "NOW LOADING"): the read engine synthesizes the on-disk wire format the `.fds`
  container omits (gaps / start-mark / CRC-16) and corrects four masked drive
  registers plus a `$4025` bit-3 mirroring inversion (the "DISK TROUBLE ERR.20"
  root cause). Verified across all three BIOS revisions.
- **Rollback netplay now works in real two-instance sessions** (native and browser):
  the root cause was that `power_cycle()` was not a true cold boot — it left the
  master clock (`ppu_clock`) and several DMA/latch fields carrying residual
  run-history phase, so two peers that had run a different number of frames diverged
  from frame 0. Power-cycle now fully cold-boots (including rebuilding stateful
  mappers), with an input-resend + cumulative-ack reliability layer and exact
  one-frame-per-pace driving on both transports.
- An NTSC-filter shader crash (dynamic indexing of `let` arrays rejected by naga)
  and several RetroAchievements login / badge-lock-state / User-Agent issues.

---

## [0.9.5] - 2026-06-13 - Netplay (documentary lineage)

*Documentary lineage of the cycle-accurate engine (integrated from the engine's
v2.3.0 -> v2.5.0 work). No accuracy/behaviour change to single-player NES play
(AccuracyCoin 100.00%); single-player frame output is byte-for-byte unchanged.*

### Added

- **GGPO-style rollback netplay.** Each peer runs the bit-deterministic core
  locally, predicts the remote input, advances, and on a misprediction rolls back
  to a save-state and re-simulates — the determinism contract guaranteeing the
  re-sim matches. A new netplay crate provides `RollbackSession` (per-frame input
  history + save-state ring + periodic checksum desync detection + seeded RNG, no
  wall-clock), a `Transport` trait (in-memory for tests + non-blocking UDP that
  never panics on hostile packets), and a host/join connection with a `Sync`
  handshake + ROM-hash check. The cross-peer digest is `framebuffer ^ cycle`, not
  the full snapshot (which carries audio-drain transients that never affect future
  frames).
- **Up to 4-player rollback** (generalized session + a fully-connected mesh
  transport; Four Score auto-enabled above 2 players), with native frontend host /
  join UI and a netplay HUD.
- **Internet-netplay + arcade groundwork:** a STUN client (RFC 5389) and a hole-punch
  state machine; the netplay crate made `wasm32`-buildable with a WebRTC
  `Transport`; and the Vs. System / PlayChoice-10 RGB-PPU foundation (2C03/2C04/2C05
  palettes, NES 2.0 byte-13 parsing, Vs. DIP switches + coin/service inputs).

---

## [0.9.4] - 2026-06-13 - Coverage + Input + FDS (documentary lineage)

*Documentary lineage of the cycle-accurate engine (integrated from the engine's
v2.1.0 -> v2.2.0 work). No accuracy regression (AccuracyCoin 100.00%; the
default/standard-pad paths are byte-identical — new paths are opt-in / separate
parse).*

### Added

- **+13 mapper families (25 -> 38):** Bandai FCG (+minimal EEPROM), Jaleco
  SS88006, Tengen RAMBO-1, Irem H3001, Sunsoft-3/4, Bandai discrete, Konami VRC3,
  Holy Diver, Namco 118 / DxROM / TxSROM, Namco 175/340 — spec-implemented with
  register/IRQ unit tests and boot smokes.
- **Expansion input devices** (opt-in per-port overlay; the default pad / Four
  Score path stays byte-identical): the **Arkanoid Vaus** paddle (game-ROM-verified)
  and the **Zapper** light gun (framebuffer-luma light detection).
- **Famicom Disk System support.** The FDS RAM adaptor (32K PRG-RAM + 8K CHR-RAM +
  a user-supplied `disksys.rom` BIOS), the `$4020-$4026`/`$4030-$4033` register map
  with the per-CPU-cycle timer IRQ, the disk read+write drive with multi-side
  eject/insert, writable-disk persistence (`.fds.sav`), and the 2C33 wavetable
  audio. New `Nes::from_disk` API; frontend `.fds` open / drag-drop + a one-time
  BIOS prompt + a disk-swap key. (The BIOS is never committed; the device and audio
  are unit-tested.)
- Substantial test-ROM coverage growth (the blargg PPU/APU/timing suites and the
  expansion-audio corpus wired in).

---

## [0.9.3] - 2026-06-13 - Master-Clock Scheduler -> 100% Accuracy (documentary lineage)

*Documentary lineage of the cycle-accurate engine (integrated from the engine's
v2.0.0 -> v2.0.1 work). This is the stage where the master-clock scheduler became
the only path and the AccuracyCoin gate was fully cleared.*

### Changed

- **The master-clock-precise scheduler became the default and only path.** The
  prior integer-lockstep scheduler — and the umbrella of accuracy feature gates it
  hid behind — were removed entirely once the master-clock path proved itself,
  collapsing dozens of experiment flags into unconditional code with zero
  behaviour change on the shipping build.
- **Region-exact CPU:PPU ratios** parametrized: 3:1 for NTSC/Dendy and **3.2:1 for
  PAL** (the prior core only modelled 3:1).

### Fixed / Accuracy

- **AccuracyCoin reached 100.00% (139/139).** A long accuracy program closed via a
  unified per-cycle DMA engine (replacing the two-driver span split), a put-cycle
  end-flip parity fix, delayed-`$4015` and edge-arm-suppress handling, and the
  OAM-DMA open-bus rule — plus the master-clock-precise CPU/PPU phase relationship
  that finally closed the long-standing IRQ-sample-timing residuals. nestest 0-diff;
  the blargg `cpu_interrupts_v2` suite passes; region-exact CPU timing verified.

---

## [0.9.2] - 2026-06-13 - Accuracy Hardening + Frontend Features (documentary lineage)

*Documentary lineage of the cycle-accurate engine (integrated from the engine's
v1.5.0 -> v1.7.0 work). Additive — accuracy held while frontend features and a
nesdev-accuracy pass landed.*

### Added

- **Nesdev accuracy-hardening pass:** vendored/wired blargg `instr_misc` /
  `instr_timing` / `cpu_reset` suites, VRC2/4 register fixtures, automated
  PAL/Dendy region-timing gates, a seeded power-on RAM fill (developer mode; the
  default path stays byte-identical), and documented scope closure across the
  mapper / input / region matrix.
- **Game Genie + raw-RAM cheats.** Clean-room 6/8-char Game Genie decoding applied
  as a runtime overlay (off by default, not captured in the deterministic
  snapshot/movie), plus `$addr=$value [if $compare]` raw-RAM cheats — both with a
  debugger cheat panel persisted per-ROM.
- **Four Score** (4-controller adapter) support over the canonical 24-read serial
  sequence (opt-in; the standard two-controller read stays byte-identical), with
  P3/P4 keyboard + gamepad rebinding.
- **Config-driven gamepad rebinding UI** (per-player keyboard + pad bindings,
  analog-stick-as-D-pad) and an egui graphics / audio / rewind **settings panel**.
- Browser persistence: in-browser TAS movie download/upload and `localStorage`
  save-states keyed by ROM hash.

---

## [0.9.1] - 2026-06-13 - Expansion Audio + Web + TAS (documentary lineage)

*Documentary lineage of the cycle-accurate engine (integrated from the engine's
v1.1.0 -> v1.4.0 work). Additive over the new core's v0.9.0 baseline.*

### Added

- **VRC7 OPLL FM audio** via a clean-room pure-Rust port of `emu2413` (MIT),
  completing the expansion-audio family (VRC6, Sunsoft 5B, Namco 163, MMC5, and now
  VRC7 FM) — *Lagrange Point* plays with in-game audio.
- **The WebAssembly target.** The frontend builds for `wasm32-unknown-unknown` in
  two flavours: a full winit + wgpu + egui browser app (WebGPU with a WebGL2
  fallback, the egui debugger overlay, an NTSC filter) and a lightweight canvas-2D
  embed mode, with shared Web Audio and a GitHub Pages deploy.
- **TAS movie record/playback.** A versioned `.rnm` movie format (deterministic
  start point + a raw per-frame input stream; optional save-state start), recorder /
  player, and save-state branching — proven by byte-identical round-trip tests
  (framebuffer + audio + cycle count). Record / play / branch hotkeys in the
  frontend.
- A canonical get/put-cycle DMC-DMA scheduler model (introduced alongside the
  existing scheduler under the parallel-implementation pattern).

---

## [0.9.0] - 2026-06-13 - Cycle-Accurate Core Engine + Frontend MVP (documentary lineage)

*Documentary lineage of the cycle-accurate engine (integrated from the engine's
v1.0.0 work). This is the new hardware-accurate core that replaces RustyNES's
original emulation engine — the baseline the rest of the `v0.9.x` lineage builds on.*

### Added

- **A new master-clock-precise, lockstep-scheduled core** (CPU + PPU + APU on one
  shared timebase; the PPU is the master clock, the CPU advances on its region
  divider, the APU every other CPU cycle). Mid-instruction PPU events are visible to
  subsequent CPU code without per-quirk patches. The Bus owns all mutable state; the
  workspace dependency graph is one-directional so each chip is independently
  fuzzable and benchmarkable.
- **Band-limited audio synthesis** (polyphase BLEP / windowed-sinc, ~81 dB SFDR,
  lookup-table non-linear mixer, 3-stage analog filter chain).
- **15 mappers** including MMC1-MMC5 (with MMC5 audio + ExGrafix), the VRC family
  (1/2/4/6/7), Sunsoft FME-7, and Namco 163 — with the expansion-audio extension
  family (VRC6, Sunsoft 5B, Namco 163, MMC5; VRC7 FM landed in the v0.9.1 stage).
- **Frontend MVP** (winit + wgpu + cpal + egui): save-states + **rewind**, TOML
  input rebinding with an in-app rebind modal, an **egui debugger overlay** (CPU /
  PPU / APU / memory, strictly read-only to preserve determinism), a simplified
  NTSC post-pass, `rfd` file-open + drag-and-drop ROM loading, and gilrs gamepad
  support.
- A six-layer testing strategy with the blargg / kevtris / AccuracyCoin suites as
  the closed-form definition of "cycle-accurate," plus a commercial-ROM regression
  oracle (ROM hash + framebuffer hash + audio hash + cycle count) and a visual
  baseline corpus.

---

## [0.8.6] - 2025-12-29 - Sub-Cycle Accuracy Improvements

**Status**: Phase 1.5 Stabilization - M11 Sub-Cycle Accuracy (Sprints 3-5 Complete, 83%)

This release implements critical sub-cycle accuracy features including DMC DMA cycle stealing, NES open bus behavior, and per-CPU-cycle mapper clocking.

### Highlights

- **DMC DMA Cycle Stealing:** Proper CPU stall handling during DMC sample fetches
- **NES Open Bus Behavior:** `last_bus_value` tracking for hardware-accurate unmapped reads
- **Controller Open Bus:** Bits 5-7 correctly mixed from open bus in controller reads
- **Per-Cycle Mapper Clocking:** `mapper.clock(1)` in `on_cpu_cycle()` for IRQ accuracy
- **Test Suite:** 522+ tests passing (0 failures, 1 ignored doctest, 2 new tests)
- **100% Blargg Pass Rate:** All 90/90 Blargg tests continue to pass

### Added

#### DMC DMA Cycle Stealing (Sprint 3)

- **New Field:** `dmc_stall_cycles` in Bus struct tracks pending CPU stalls
- **Purpose:** DMC sample fetches steal 4 CPU cycles per sample
- **Implementation:** CPU halted during stalls while PPU/APU/mapper continue running
- **Method:** `take_dmc_stall_cycles()` for Console integration
- **Console Integration:** tick() handles stalls with proper component stepping

#### NES Open Bus Behavior (Sprint 4)

- **New Field:** `last_bus_value` in Bus struct tracks last value on data bus
- **Purpose:** NES hardware returns last bus value for unmapped reads (not 0)
- **Unmapped Regions:** $4018-$401F and $4020-$5FFF return open bus value
- **Controller Reads:** $4016/$4017 mix bits 5-7 from open bus with controller data bits 0-4
- **Writes:** Also update bus value (hardware-accurate behavior)
- **New Tests:** `test_open_bus_behavior()`, `test_controller_open_bus_bits()`

#### Per-Cycle Mapper Clocking (Sprint 5)

- **New Call:** `self.mapper.clock(1)` in `on_cpu_cycle()` callback
- **Purpose:** Mappers clocked once per CPU cycle for cycle-accurate timing
- **Critical For:** VRC cycle-based IRQs, MMC3 A12 detection
- **Effect:** Maintains sub-cycle accuracy for all mapper types

### Fixed

- Controller reads now correctly mix bits 5-7 from open bus with controller data
- Unmapped regions return last bus value instead of 0 (hardware-accurate)
- DMC sample fetches now properly stall CPU while other components continue

### Changed

- Bus struct tracks `last_bus_value` for open bus emulation
- Bus struct tracks `dmc_stall_cycles` for DMC DMA handling
- Console tick() handles DMC stalls with proper PPU/APU/mapper stepping
- Mappers clocked via `on_cpu_cycle()` callback for sub-cycle accuracy
- Consolidated unmapped read ranges for cleaner match arm

### Quality Metrics

- cargo clippy: PASSING (zero warnings)
- cargo fmt: PASSING
- cargo test: 522+ tests passing
- cargo build --release: SUCCESS
- Zero unsafe code maintained

---

## [0.8.5] - 2025-12-29 - Cycle-Accurate CPU/PPU Synchronization

**Status**: Phase 1.5 Stabilization - M11 Sub-Cycle Accuracy (Sprints 1 & 2 Complete)

This release implements true cycle-accurate CPU/PPU synchronization, enabling VBlank timing tests to pass with zero-cycle accuracy.

### Highlights

- **Cycle-Accurate Synchronization:** CpuBus trait with on_cpu_cycle() callback for PPU stepping
- **VBlank Timing Tests:** Now pass with +/-0 cycle accuracy (previously failed with +/-51 and +/-10 cycles)
- **cpu.tick() Method:** Cycle-by-cycle CPU execution for sub-instruction timing precision
- **Test Suite:** 520+ tests passing (0 failures, 1 ignored doctest)
- **100% Blargg Pass Rate:** All 90/90 Blargg tests continue to pass

### Added

- **CpuBus Trait:** extends `Bus` with `on_cpu_cycle()` callback so the PPU is stepped 3 dots per CPU cycle before each memory access; NMI captured during the callback and delivered via `cpu.trigger_nmi()`.
- **cpu.tick() Cycle-by-Cycle Execution:** `cpu.tick(&mut bus)` executes one CPU cycle, exposing internal cycle state for sub-instruction-accurate timing.

### Fixed

- **ppu_02-vbl_set_time:** Was +/-51 cycles off, now exact (0 cycles).
- **ppu_03-vbl_clear_time:** Was +/-10 cycles off, now exact (0 cycles).
- **Root Cause:** PPU was only stepped after full CPU instructions completed; solution steps PPU 3 dots BEFORE each CPU memory access via the callback.

### Changed

- Test harness uses CpuBus for cycle-accurate validation while existing Bus-trait tests continue to work.

### Quality Metrics

- cargo clippy / fmt: PASSING; 520+ tests passing; zero unsafe code.

---

## [0.8.4] - 2025-12-28 - CPU/PPU Timing & Version Consistency

**Status**: Phase 1.5 Stabilization - Timing Improvements & Bug Fixes

### Highlights

- **CPU/PPU Timing:** PPU now stepped BEFORE CPU cycle in tick() for accurate $2002 reads at the VBlank boundary.
- **Version Consistency:** Fixed About window and Settings showing outdated version numbers.
- **Documentation:** Fixed clone_mapper doctest by implementing full Clone for mapper Box types.
- **Test Suite:** 517+ tests passing (0 failures, 2 ignored for known architectural limitations); 100% Blargg pass rate.

### Fixed

- **VBlank boundary reads:** reordered tick() to step the PPU 3 dots before each CPU cycle so `$2002` status reads at frame boundaries are accurate (2 VBlank timing tests remain ignored pending a full cycle-by-cycle CPU refactor).
- **Version strings:** updated About window, Settings dialog, CLI `--version`, and Cargo.toml files to 0.8.4.
- **clone_mapper doctest:** implemented Clone for BoxedMapper types so the doctest compiles.

### Quality Metrics

- cargo clippy / fmt: PASSING; 517+ tests passing; zero unsafe code.

---

## [0.8.3] - 2025-12-28 - Critical Rendering Bug Fix

**Status**: Phase 1.5 Stabilization - Critical Bug Fix Release

### Highlights

- **Critical Rendering Fix:** Fixed a framebuffer display showing "4 faint postage stamp copies."
- **Palette Index to RGB Conversion:** NES palette indices (0-63) now correctly converted to RGB via the 64-entry NES_PALETTE lookup table in `update_framebuffer()`.
- **Documentation Improvements:** Changed 3 doctests from `ignore` to `no_run` for compile-time verification.
- **Zero Regressions:** 516+ tests passing, 100% Blargg pass rate maintained.

### Fixed

- **Root Cause:** the framebuffer passed raw palette indices directly as RGBA instead of converting them; the fix performs index -> RGB conversion, restoring proper full-window rendering with the correct palette.

### Changed

- `lib.rs` / `rom.rs` (rustynes-mappers) doctests changed from `ignore` to `no_run` so they are compile-checked during `cargo test`.

### Quality Metrics

- cargo clippy / fmt: PASSING; 516+ tests passing; zero unsafe code.

---

## [0.8.2] - 2025-12-28 - M10-S1 UI/UX Improvements

**Status**: Phase 1.5 Stabilization - M10 Sprint 1 Complete (UI/UX Polish)

This release completes M10-S1, delivering comprehensive desktop GUI polish: theme support, status bar, tabbed settings, keyboard shortcuts, modal dialogs, and visual feedback.

### Highlights

- **Theme Support:** Light/Dark/System themes with persistence and real-time switching.
- **Status Bar:** FPS counter, ROM name display, color-coded status messages with auto-expiry.
- **Tabbed Settings Dialog:** Video/Audio/Input/Advanced tabs with comprehensive tooltips.
- **Keyboard Shortcuts:** Ctrl+O/P/R/Q, F1-F3, M, Escape with consistent behavior.
- **Modal Dialogs:** Welcome screen, error dialogs, confirmation prompts, help window.
- **Zero Regressions:** 508+ tests passing, 100% Blargg pass rate maintained.

### Added

- **Themes:** Light/Dark/System via the egui Visuals API, saved to RON config, applied in real time (`ctx.set_visuals()`), with OS dark-mode detection.
- **Status bar:** real-time FPS (500 ms), ROM filename, color-coded auto-expiring status messages, responsive layout.
- **Tabbed settings:** Video (theme, scale 1-8x, fullscreen, VSync, 8:7 PAR, FPS counter), Audio (mute, volume, sample rate, buffer size), Input (P1/P2 keyboard bindings with reset), Advanced (debug, recent ROMs, app info); Save/Reset buttons; per-setting tooltips.
- **Keyboard shortcuts:** Ctrl+O/Q, Ctrl+P, Ctrl+R/F2, F1/F2/F3 debug windows, M mute, Escape.
- **Modal dialogs:** first-run welcome screen (tracks `first_run`), error dialogs, confirmation prompts, and a Help window with shortcut reference.

### Changed

- Guard-pattern input-handler refactor; improved settings-renderer borrowing; removed unused code; new `AppTheme` enum + `first_run` config field with theme persistence.

### Quality Metrics

- cargo clippy / fmt: PASSING; 508+ tests passing; zero unsafe code.

---

## [0.8.1] - 2025-12-28 - M9 Known Issues Resolution (85% Complete)

**Status**: Phase 1.5 Stabilization - M9 Sprints 1-3 Core Implementation Complete

### Highlights

- **Audio Improvements (S1 Complete):** Two-stage decimation via rubato (1.79 MHz -> 192 kHz -> 48 kHz), A/V sync with adaptive speed adjustment (0.99x-1.01x), dynamic buffer sizing (2048-16384), hardware-accurate mixer with NES filter chain.
- **PPU Edge Cases (S2 Complete):** Sprite overflow bug emulation (false positive/negative matching hardware), palette RAM mirroring at $3F10/$3F14/$3F18/$3F1C, mid-scanline write detection for split-screen, attribute-byte extraction verified.
- **Performance Optimization (S3 Core Complete):** `#[inline]` hints on CPU/PPU hot paths (step(), execute_opcode(), handle_nmi(), handle_irq(), step_with_chr()).
- **Zero Regressions:** 508+ tests passing, 100% Blargg pass rate maintained.

### Quality Metrics

- cargo clippy / fmt: PASSING; 508+ tests passing; zero unsafe code.

---

## [0.8.0] - 2025-12-28 - Rust 2024 Edition & Dependency Modernization

**Status**: Phase 1.5 Stabilization - M9 Sprint 0 Complete (Dependency Upgrade)

Comprehensive dependency modernization: Rust 2024 Edition (MSRV 1.88), eframe/egui 0.33, cpal 0.16, rubato 0.16, ron 0.12, thiserror 2.0, bitflags 2.10.

### Highlights

- **Rust 2024 Edition** adopted across all crates (MSRV 1.75 -> 1.88).
- **GUI/Audio:** eframe 0.33 + egui 0.33, cpal 0.16, NEW rubato 0.16 high-quality resampling for flexible output sample rates.
- **Config/Errors:** ron 0.12, thiserror 2.0, bitflags 2.10.
- **Performance:** `#[inline]` hints on critical audio/rendering paths, buffer-reuse patterns, optimized frame-timing accumulator.
- **Test Suite:** 508+ tests passing (0 failures, 0 ignored).

### Added

- **Audio Resampling:** rubato 0.16 integration for high-quality sample-rate conversion and better cross-platform audio.

### Changed

| Component | Previous | New |
|-----------|----------|-----|
| eframe / egui | 0.29 | 0.33 |
| cpal | 0.15 | 0.16 |
| ron | 0.8 | 0.12 |
| thiserror | 1.x | 2.0 |
| bitflags | 2.4 | 2.10 |
| rubato | - | 0.16 (NEW) |

### Migration Notes

- No breaking changes to user-facing functionality; config files and save states remain compatible. Developers need Rust 1.88+ (`rustup update`); a clean rebuild is recommended.

### Quality Metrics

- cargo clippy / fmt: PASSING; 508+ tests passing; zero unsafe code.

---

## [0.7.1] - 2025-12-27 - Desktop GUI Framework Migration

**Status**: Phase 1.5 Stabilization - GUI Reimplementation Complete

Complete migration of the desktop frontend from Iced+wgpu to eframe+egui for a simpler, more maintainable GUI layer.

### Changed

- **Desktop Frontend:** Iced 0.13 -> eframe 0.29 / egui 0.29 (immediate mode GUI); cpal ring-buffer audio (8192 samples); gilrs gamepad support with hotplug; RON config via the `directories` crate; native file dialogs via `rfd`.

### Added

- **Debug Windows:** CPU (registers, flags, cycle counter), PPU (frame/state overview), APU (audio info, sample buffer, channel overview), and a hex Memory viewer with navigation + ASCII.
- **Menu System** (File, Emulation, Options, Debug, Help), **Settings Dialog** (video/audio/input/debug), accumulator-based 60.0988 Hz NTSC frame timing.

### Removed

- Custom wgpu shader pipeline, Iced view components, the ROM library scanner (to be reimplemented), and unused modules (runahead, metrics, theme, palette).

### Fixed

- Resolved wgpu version conflicts (pixels 0.17 vs egui-wgpu 22) via eframe's bundled solution; clippy warnings; simplified event loop with full frame-timing control.

---

## [0.7.0] - 2025-12-21 - "Perfect Accuracy" (Milestone 8: Test ROM Validation Complete)

**Status**: Phase 1.5 Stabilization - Milestone 8 COMPLETE (100% Blargg Pass Rate)

This release marks the historic completion of Milestone 8 (Test ROM Validation), achieving a **100% pass rate** across all Blargg test suites (CPU, PPU, APU, Mappers) — 500 tests passing, zero failures.

### Highlights

- **100% Blargg Test Pass Rate:** CPU 22/22, PPU 25/25, APU 15/15, Mappers 28/28 (90 total).
- **Cycle-Accurate CPU State Machine:** Complete `tick()` implementation with dummy read timing.
- **CPU Interrupt Handling:** NMI hijacking during BRK; all 5 cpu_interrupts sub-tests passing.
- **PPU Open Bus Emulation:** Data latch with decay counter, correct read-only register handling.
- **CHR-RAM Support:** Fixed a critical design flaw enabling pattern-table writes to mappers.
- **APU Frame Counter:** Immediate clocking on $4017 write; DMC IRQ/DMA fixes.
- **Test Suite:** 500 tests passing (0 failures, 0 ignored).

### Added

- **CPU (22/22):** cycle-accurate `tick()` (1-access-per-cycle discipline, dummy read/write cycles, hardware-accurate RMW timing); NMI hijacking during BRK; IRQ polling between instructions. All cpu_instr / dummy-write / interrupt / timing tests passing.
- **PPU (25/25):** open-bus data latch with 1-second decay; correct $2002 / write-only register behavior; CHR-RAM write routing to the mapper; frame-accurate VBlank/NMI timing; OAM attribute-bit masking; VRAM read-buffer palette behavior.
- **APU (15/15):** frame-counter immediate clocking on $4017; DMC sample-buffer refill + IRQ-acknowledge fixes + 2-stage sample pipeline; timer-period off-by-one and clock-parity fixes.
- **Mappers (28/28):** Holy Mapperel suite across NROM, MMC1, UxROM, CNROM, MMC3 (banking, mirroring, IRQ timing).
- Comprehensive test harnesses (blargg CPU/PPU/APU, holy_mapperel) and an 800+ line M8 technical analysis doc.

### Changed

- CPU refactored from `step()` (instruction-level) to `tick()` (cycle-level); PPU gained open-bus + CHR-RAM routing; APU frame-counter / DMC fixes; test suite at 500 passing with all prior "known limitations" resolved.

### Quality Metrics

- cargo clippy / fmt: PASSING; 500 tests passing, 0 failures, 0 ignored; zero unsafe across all 6 crates.

---

## [0.6.0] - 2025-12-20 - "Accuracy Improvements" (Milestone 7: Complete + M8 Progress)

**Status**: Phase 1.5 Stabilization In Progress - Milestone 7 Complete, Milestone 8 In Progress

Completion of Milestone 7 (Accuracy Improvements) — timing refinements across CPU, PPU, APU, and bus synchronization — plus M8 progress (Blargg CPU tests 90% pass rate). 469 tests passing, zero regressions.

### Highlights

- **M7 Complete:** APU frame-counter precision, hardware-accurate mixer, OAM DMA 513/514-cycle timing.
- **M8 Progress:** Blargg CPU tests 18/20 (90%), up from 13/20 (65%).
- **CPU Timing Fixes:** hardware-accurate dummy read/write cycles, IRQ handling, illegal-opcode fixes.

### Added

- **M8 progress:** all 11 cpu_instr tests + dummy-writes + all-instrs + official-only + instr-timing passing; hardware-accurate dummy reads for implied mode + RMW dummy writes for indexed modes; ATX/LXA (0xAB) fix; RTI IRQ-ack timing; NMI hijacking for BRK. (cpu_dummy_reads + cpu_interrupts #2 documented as known limitations pending cycle-by-cycle CPU.)
- **M7-S1 (CPU):** verified all 256 opcodes' cycle counts, page-crossing penalties, branch timing, RMW dummy writes.
- **M7-S2 (PPU):** `scanline()`/`dot()` accessors, VBlank race-condition handling, exact VBlank flag timing, sprite-0 hit 2/2.
- **M7-S3 (APU):** 4-step quarter-frame timing fix (22371 -> 22372), verified 4/5-step sequences, hardware-accurate non-linear mixer (TND divisors), triangle linear counter.
- **M7-S4 (Timing):** exact 513/514-cycle OAM DMA based on CPU parity; CPU cycle-parity tracking; verified 3:1 CPU/PPU sync.

### Quality Metrics

- cargo clippy / fmt: PASSING; 469 tests passing, 0 failures, 8 ignored; all 136 APU tests passing; zero unsafe code.

---

## [0.5.0] - 2025-12-19 - "Phase 1 Complete" (Milestone 6: Desktop GUI)

**Status**: Phase 1 MVP Complete - All 6 milestones finished

This release marks the historic completion of Phase 1 MVP, delivering the `rustynes-desktop` application — a fully playable NES emulator — 6+ months ahead of the original June 2026 target.

### Highlights

- Cross-platform desktop application with egui/wgpu.
- Real-time 60 FPS NES rendering; cpal audio output with ring buffer.
- Keyboard and gamepad input (gilrs); ROM file browser with format validation.
- Configuration persistence (JSON); playback controls (pause, resume, reset).
- Complete Phase 1 MVP (all 6 milestones); zero unsafe code; 400+ tests passing.

### Added - Desktop GUI (rustynes-desktop)

- **egui application framework:** menu bar (File, Emulation, Help), ROM browser with iNES validation, playback controls, video/audio settings, About dialog.
- **wgpu rendering backend:** Vulkan/Metal/DX12/WebGPU, real-time 256x240 framebuffer, configurable window scaling, VSync, 60 FPS target.
- **cpal audio output:** 48 kHz, configurable buffer, A/V sync, volume control.
- **gilrs gamepad support:** auto-detection, button mapping, dual controllers, keyboard fallback.
- **Configuration system:** JSON persistence for video/audio/input settings, recent ROMs, last directory, window size/position.

### Test Results

| Component | Tests | Pass Rate |
| --------- | ----- | --------- |
| CPU | 47/47 | 100% |
| PPU | 85/87 | 97.7% (2 ignored) |
| APU | 136/136 | 100% |
| Mappers | 78/78 | 100% |
| Core | 18/18 | 100% |
| **Total** | **400+** | **100%** |

### Notes

- Phase 1 MVP achieved 6+ months ahead of schedule; 77.7% game compatibility with 5 essential mappers; cross-platform (Linux, Windows, macOS); zero unsafe code.

---

## [0.4.0] - 2025-12-19 - "All Systems Go" (Milestone 5: Integration Complete)

**Status**: Phase 1 In Progress - CPU, PPU, APU, Mappers, and Core Integration complete

Completion of Milestone 5 (Integration), delivering the `rustynes-core` layer that connects all subsystems into a functional NES emulator core (ROM load, frame execution, input, framebuffer output).

### Highlights

- Complete integration layer connecting CPU, PPU, APU, and Mappers.
- Hardware-accurate bus with the full NES memory map; cycle-accurate OAM DMA (513-514 cycles).
- Console coordinator with proper timing synchronization; input shift-register protocol.
- Save-state framework (serialization deferred to Phase 2); 22 new integration tests (398 total); zero unsafe code.

### Added - Core Integration Layer

- **Bus system:** full $0000-$FFFF memory map (RAM + mirrors, PPU/APU registers + mirrors, OAM DMA, controller ports, cartridge space), cycle-accurate OAM DMA with dummy-cycle alignment, mapper integration (mirroring conversion, scanline IRQ, A12 edge notification).
- **Console coordinator:** 3 PPU dots per CPU cycle, APU step per cycle, NMI (VBlank) + mapper IRQ + DMA-stall handling; public API for ROM load, single-step, frame-step, framebuffer access, dual-controller input, reset.
- **Input system:** hardware-accurate shift-register protocol (strobe latch, serial readout, open-bus behavior); dual-controller state management.
- **Save-state framework:** 64-byte header format ("RNES" magic, version, CRC32, ROM SHA-256, timestamp, frame count) with mismatch/checksum/I-O error handling.

### Test Results

| Component | Tests | Pass Rate |
| --------- | ----- | --------- |
| CPU | 46/46 | 100% |
| PPU | 83/83 | 100% |
| APU | 150/150 | 100% |
| Mappers | 78/78 | 100% |
| Core | 41/41 | 100% |
| **Total** | **398/398** | **100%** |

### Notes

- The emulator core is now functionally complete; save-state serialization deferred to Phase 2; zero unsafe code across all 5 crates.

---

## [0.3.0] - 2025-12-19 - "Mapping the Path Forward" (Milestone 4: Mappers Complete)

**Status**: Phase 1 In Progress - CPU, PPU, APU, and Mappers implementation complete

Completion of Milestone 4 (Mappers): a complete mapper subsystem enabling 77.7% NES game-library compatibility via the 5 most important mappers.

### Highlights

- Trait-based mapper framework with 5 mappers: NROM (0), MMC1 (1), UxROM (2), CNROM (3), MMC3 (4).
- 77.7% NES game-library coverage; full iNES + NES 2.0 ROM support; scanline IRQ (MMC3 A12 edge detection); battery-backed SRAM interface.
- 78 tests passing; zero unsafe code; 3,401 lines of production code.

### Added - Mappers Implementation

- **Framework:** 13-method Mapper trait (PRG/CHR read/write with banking, dynamic mirroring, IRQ generation/ack, CPU/PPU clock callbacks, PRG-RAM for battery saves); complete iNES + NES 2.0 parser (12-bit mapper numbers, size/battery/trainer/mirroring detection); all mirroring modes; a factory registry.
- **NROM (0)** - 9.5% coverage: no banking, 16/32KB PRG, 8KB CHR-ROM/RAM.
- **MMC1 (1)** - 27.9% coverage: 5-bit serial shift register, 4 PRG modes, 2 CHR modes, programmable mirroring, 8KB battery SRAM.
- **UxROM (2)** - 10.6% coverage: 16KB switchable + fixed PRG, 8KB CHR-RAM, bus-conflict emulation.
- **CNROM (3)** - 6.3% coverage: fixed PRG, 8KB switchable CHR, bus-conflict emulation.
- **MMC3 (4)** - 23.4% coverage: 8 bank-select registers, 2 PRG + 2 CHR modes, scanline-counter IRQ with A12 edge detection, PRG-RAM protection, 8KB battery SRAM.

### Test Results

| Component | Tests | Pass Rate |
| --------- | ----- | --------- |
| CPU | 46/46 | 100% |
| PPU | 83/83 | 100% |
| APU | 150/150 | 100% |
| Mappers | 78/78 | 100% |
| **Total** | **357/357** | **100%** |

### Notes

- Library-only release (no GUI yet); hardware-accurate mapper implementation with cycle-accurate MMC3 A12 IRQ timing; zero unsafe code across all 4 crates.

---

## [0.2.0] - 2025-12-19 - "The Sound of Innovation" (Milestone 3: APU Complete)

**Status**: Phase 1 In Progress - CPU, PPU, and APU implementation complete

Completion of Milestone 3 (APU): a complete, hardware-accurate 2A03 Audio Processing Unit with all 5 channels, a non-linear mixer, and a configurable resampler — cycle-accurate, zero unsafe code.

### Highlights

- Complete 2A03 APU with all 5 audio channels; hardware-accurate non-linear mixer with lookup tables.
- Configurable resampler (1.79 MHz -> 48 kHz) with low-pass filter; 4-step and 5-step frame-counter modes.
- Flexible DMA interface for DMC sample playback; complete $4000-$4017 register implementation.
- 150 tests passing (136 unit + 14 doc); zero unsafe code.

### Added - APU Implementation

- **Core components:** frame counter (4-/5-step, cycle-accurate, optional IRQ), envelope generator, length counter (32-entry table), sweep unit.
- **Channels:** Pulse 1 & 2 (4 duty cycles, 11-bit timer, envelope, sweep, length counter), Triangle (32-step sequence, linear counter, ultrasonic silencing), Noise (15-bit LFSR, long/short modes, 16-entry period table), DMC (1-bit delta modulation, 7-bit output, 16 sample rates, DMA interface, IRQ, loop, $FFFF->$8000 wrap).
- **Output:** non-linear mixer (hardware mixing formulas, range 0.0-~2.0), linear-interpolation resampler with ring buffer, optional low-pass filter.
- **System integration:** $4015 status register, $4017 frame-counter control, memory-callback DMA interface.

### Changed

- Reorganized `to-dos/` into a phase-based hierarchy (4 phases, 18 milestones).

### Test Results

| Component | Tests | Pass Rate |
| --------- | ----- | --------- |
| CPU | 46/46 | 100% |
| PPU | 83/83 | 100% |
| APU | 150/150 | 100% |
| **Total** | **279/279** | **100%** |

### Notes

- Library-only release; APU is hardware-accurate and cycle-precise; DMC DMA via a simple memory callback; zero unsafe code across CPU/PPU/APU.

---

## [0.1.0] - 2025-12-19 - "Precise. Pure. Powerful." (First Official Release)

**Status**: Phase 1 In Progress - CPU and PPU implementation complete

The **first official release** of RustyNES, completing Milestone 1 (CPU) and Milestone 2 (PPU) — a world-class foundation with 100% CPU test pass rate and 97.8% PPU pass rate.

### Highlights

- World-class CPU with 100% nestest.nes validation; cycle-accurate PPU at 97.8% pass rate.
- Complete 6502 CPU with all 256 opcodes (151 official + 105 unofficial); full 2C02 PPU with VBL/NMI timing and sprite rendering.
- 144 comprehensive tests passing (56 CPU + 88 PPU); zero unsafe code; 44 test ROMs acquired for validation.

### Added

- **Milestone 1 (CPU):** cycle-accurate 6502/2A03, all 256 opcodes, all 13 addressing modes with cycle-accurate timing, complete interrupt handling (NMI, IRQ, BRK, RESET), page-crossing penalties, 100% nestest.nes golden-log match, 46 unit tests.
- **Milestone 2 (PPU):** dot-level 2C02 rendering (341 dots x 262 scanlines), background rendering with Loopy scrolling, sprite rendering (8-per-scanline), sprite-0 hit, sprite overflow, cycle-accurate VBlank/NMI timing, OAM DMA, complete register implementation, palette RAM with mirroring, VRAM nametable mirroring, 83 unit tests.
- **Documentation & Organization:** Phase 1 TODO tracking, M1/M2 milestone docs, M3 (APU) / M4 (Mappers) overviews, CPU test-ROM README, `game-roms/` directory.

### Changed

- Reorganized test-ROM structure (CPU files to `test-roms/cpu/`); updated documentation references, README status, and .gitignore.

### Test Results

| Component | Tests | Pass Rate |
| --------- | ----- | --------- |
| CPU | 56/56 | 100% |
| PPU | 88/90 | 97.8% (2 ignored) |
| **Total** | **144/146** | **98.6%** |

### Notes

- Library-only release (no GUI yet); two PPU tests ignored pending cycle-accurate timing refinement (not failures); CPU is world-class (100% nestest golden-log match); zero unsafe code.

---

### Project Setup - 2025-12-18

#### Added

- Initial project structure with 10 workspace crates.
- Comprehensive documentation suite (73 markdown files, 52,402 lines): CPU/PPU/APU/mapper specifications, API reference, development guides (BUILD, CONTRIBUTING, TESTING, DEBUGGING), format specifications (iNES, NES 2.0, NSF, FM2).
- GitHub project templates (issue/PR templates, Code of Conduct, contributing guidelines, security policy, support docs).
- Development infrastructure (Dependabot, CODEOWNERS) and project documentation (README, ROADMAP, ARCHITECTURE, OVERVIEW, CHANGELOG).

---

## Development Phases

RustyNES followed a phased development approach. See [ROADMAP.md](ROADMAP.md) for complete details.

### Phase 1: MVP (delivered v0.1.0 -> v0.5.0)

- [x] Cycle-accurate 6502/2A03 CPU implementation
- [x] Dot-level 2C02 PPU rendering
- [x] Hardware-accurate 2A03 APU synthesis
- [x] Mappers 0, 1, 2, 3, 4 (77.7% game coverage)
- [x] Cross-platform desktop GUI (egui + wgpu)
- [x] Save states and battery saves
- [x] Gamepad support

### Phase 1.5: Stabilization (delivered v0.6.0 -> v0.8.6)

- [x] 100% Blargg test-ROM pass rate
- [x] Cycle-accurate CPU/PPU synchronization
- [x] Sub-cycle accuracy (DMC DMA, open bus, per-cycle mapper clocking)
- [x] Rust 2024 Edition modernization
- [x] Desktop UI/UX polish

### Phase 2 and beyond: Cycle-Accurate Core (delivered v0.9.0 -> v1.0.0)

The original Phase 2-4 roadmap goals were delivered by integrating a new
cycle-accurate engine (the `v0.9.x` documentary lineage above):

- [x] Master-clock-precise scheduler, 100% AccuracyCoin (139/139)
- [x] Integrated debugger (CPU, PPU, APU viewers)
- [x] Rewind, run-ahead, dynamic-rate audio
- [x] RetroAchievements integration (rcheevos)
- [x] GGPO rollback netplay (2-4 players, native + WebRTC)
- [x] TAS recording/playback
- [x] WebAssembly build + live GitHub Pages demo
- [x] Expansion audio (VRC6, VRC7, MMC5, FDS, N163, 5B)
- [x] Famicom Disk System + Vs. System / PlayChoice-10
- [x] 51 mapper families
- [ ] Mobile platform support (Android, iOS) - next

---

## Changelog Conventions

### Categories

- **Added**: New features
- **Changed**: Changes to existing functionality
- **Deprecated**: Soon-to-be removed features
- **Removed**: Removed features
- **Fixed**: Bug fixes
- **Security**: Security vulnerability fixes

### Versioning

RustyNES follows [Semantic Versioning](https://semver.org/):

- **MAJOR** version (X.0.0): Incompatible API changes, major features
- **MINOR** version (0.X.0): Backward-compatible functionality additions
- **PATCH** version (0.0.X): Backward-compatible bug fixes

---

## Links

- [Project Repository](https://github.com/doublegate/RustyNES)
- [Issue Tracker](https://github.com/doublegate/RustyNES/issues)
- [Discussions](https://github.com/doublegate/RustyNES/discussions)
- [Documentation](https://github.com/doublegate/RustyNES/tree/main/docs)

---

**Note**: This changelog is actively maintained as the project progresses, following the Keep a Changelog format.
</content>
</invoke>
