# Frontend audit — disposition ledger

Report: [`frontend-audit-report.md`](frontend-audit-report.md). Verdict vocabulary and
the closing procedure: [`README.md`](README.md). Target release per
[ADR 0041](../adr/0041-hardware-release-is-v3.0.0.md).

Rows are generated from the report's §8 table, so the ids and severities are the
report's own. The report itself retracted a 49th candidate, CON-02, as a false positive.

**Mobile scope (maintainer decision, 2026-09-22):** all tiers are implemented --
the Rust bridge, Kotlin and Swift. What CI and an Android emulator can reach is
verified here; the maintainer runs a per-platform device checklist before the
v2.7.4 PR merges, and anything left unverified is labelled so in the release notes.

| Id | Severity | Finding | Verdict | Evidence | Release | PR |
| --- | --- | --- | --- | --- | --- | --- |
| DESK-01 | High | Egui `textures_delta` dropped on swapchain acquire failure (`Lost`/`Outdated`/`Timeout`), permanently corrupting font atlas and leaking GPU texture memory. | CONFIRMED | Main window: `paint_shell` (uploads `textures_delta.set` at `debugger/mod.rs:3175`) runs only inside the overlay closure (`app.rs:10057`), after `get_current_texture` succeeds (`gfx.rs:1242`); `detached.rs:355-363` uploads first on purpose | v2.7.3 | |
| DESK-02 | Medium | Keycode collision: Famicom microphone (`MICROPHONE_KEY = KeyCode::KeyM`) and menu bar toggle (`default_toggle_menu_bar = "KeyM"`) share the same key. | CONFIRMED | Worse than stated: `input.rs:734` `MICROPHONE_KEY = KeyM`, `config.rs:692-693` menu-bar toggle `KeyM`, **and** `config.rs:496` P2 Select `KeyM` -- three defaults on one key; the comment at `input.rs:730` claiming it avoids controller keys is false | v2.7.3 | |
| DESK-03 | Medium | In fast-forward mode, `proxy.send_event(AppEvent::EmuFrame)` floods Winit event loop at thousands of FPS, causing high `self.emu.lock()` contention. | UNTRIAGED |  | v2.7.3 | |
| DESK-04 | High | CPAL stream errors (device disconnect, route change) are ignored, permanently muting audio until application restart. | UNTRIAGED |  | v2.7.3 | |
| DESK-05 | High | Low latency false underrun oscillation: setting `latency_ms = 20` when OS audio period is 1024 frames trips false underrun muting cycle. | UNTRIAGED |  | v2.7.3 | |
| DESK-06 | Medium | Web canvas `#nes-canvas` has fixed 512x480 dimensions without responsive CSS, overflowing mobile screens. | UNTRIAGED |  | v2.7.3 | |
| DESK-07 | Low | Web Audio AudioWorklet Blob URL is never revoked, leaking memory in browser context. | UNTRIAGED |  | v2.7.3 | |
| DESK-08 | Low | Unbounded texture cache accumulation in `BadgeCache` across games. | UNTRIAGED |  | v2.7.3 | |
| AND-01 | High | Surface destruction race: 800ms join timeout causes zombie thread condition; subsequent `surfaceCreated` fails to start render thread, causing permanent black screen. | UNTRIAGED |  | v2.7.4 | |
| AND-02 | High | 60 FPS Compose recomposition storm: updating `frame = reuse.asImageBitmap()` forces full-tree recomposition of `EmulatorScreen` and redundant CPU pixel packing. | UNTRIAGED |  | v2.7.4 | |
| AND-03 | High | Background emulation and audio leak: `onPause` does not pause `emulator.paused`, coroutine runs at 100% CPU, audio continues playing, and zero AudioFocus handling exists. | UNTRIAGED |  | v2.7.4 | |
| AND-04 | High | Synchronous main-thread disk I/O and SHA-256 computation in `loadRom` and save-state UI clicks risk ANRs. | UNTRIAGED |  | v2.7.4 | |
| AND-05 | High | Non-atomic save writes using `File.writeBytes` risk file truncation and corruption on process kill. | UNTRIAGED |  | v2.7.4 | |
| AND-06 | High | Phantom AAudio sink: `Cargo.toml` claims an AAudio sink, but Rust code implements zero audio symbols. Kotlin uses blocking `AudioTrack.write` that conflicts with frame pacing. | UNTRIAGED |  | v2.7.4 | |
| AND-07 | Medium | High-frequency touch and analog motion events trigger unthrottled synchronous JNI calls and Rust mutex contention. | UNTRIAGED |  | v2.7.4 | |
| AND-08 | Medium | CPU-spinning `Thread.sleep(2)` in `NesSurfaceView` render loop generates up to 500 wakeups/sec when idle. | UNTRIAGED |  | v2.7.4 | |
| AND-09 | Medium | Missing standard cartridge battery PRG-RAM (`.sav`) persistence. | CONFIRMED | Same root as MOB-05: the bridge exposes no battery SRAM, so the Android host cannot persist it | v2.7.4 | |
| AND-10 | Low | Absence of `onTrimMemory` and `onLowMemory` callbacks increases LMK kill score. | UNTRIAGED |  | v2.7.4 | |
| IOS-01 | High | CADisplayLink configured for 120 Hz instead of 60 Hz causes 3:2 pull-down micro-judder and doubles GPU/display power consumption. | UNTRIAGED |  | v2.7.4 | |
| IOS-02 | High | Total absence of Dynamic Rate Control (DRC) causes systematic clock drift and buffer dropouts every 30-60 seconds. | UNTRIAGED |  | v2.7.4 | |
| IOS-03 | Medium | Per-sample atomic load/store in real-time CoreAudio callback degrades thread determinism and burns mobile CPU cycles. | UNTRIAGED |  | v2.7.4 | |
| IOS-04 | High | CloudKit uploads and SRAM auto-save on scene backgrounding lack `beginBackgroundTask`, risking termination by iOS. | UNTRIAGED |  | v2.7.4 | |
| IOS-05 | High | Virtual control pad positioned over Home Indicator zone without `preferredScreenEdgesDeferringSystemGestures` triggers accidental app minimization. | UNTRIAGED |  | v2.7.4 | |
| IOS-06 | Medium | `GCController` background thread invokes `@MainActor` state without actor isolation, causing data races. | UNTRIAGED |  | v2.7.4 | |
| IOS-07 | High | Missing autorelease pool in CADisplayLink 60 Hz frame tick causes 14.7 MB/s memory churn and Jetsam risk. | UNTRIAGED |  | v2.7.4 | |
| IOS-08 | High | Audio stream death on interruptions (phone calls) and route changes; unhandled `mediaServicesWereResetNotification`. | UNTRIAGED |  | v2.7.4 | |
| IOS-09 | Medium | Surround sound channels erroneously fed only Left channel audio instead of mono center average. | UNTRIAGED |  | v2.7.4 | |
| IOS-10 | Medium | Fragile CloudKit conflict resolution based purely on wall-clock time is vulnerable to clock skew. | UNTRIAGED |  | v2.7.4 | |
| IOS-11 | High | Complete absence of `ProcessInfo.thermalState` observers and Low Power Mode handling. | UNTRIAGED |  | v2.7.4 | |
| MOB-01 | High | Monolithic `self.inner` mutex lock contention: `run_frame` holds mutex for entire frame execution, starving UI thread `set_buttons` calls and causing input lag / ANRs. | UNTRIAGED |  | v2.7.4 | |
| MOB-02 | High | Raw uncompressed `.nes` ROM buffers bypass `MAX_ROM_BYTES` validation, allowing memory exhaustion and process termination. | PARTIAL | `rustynes-mobile/src/lib.rs:1963` returns non-zip bytes before the `MAX_ROM_BYTES` check. Low impact: the caller has already allocated the buffer | v2.7.4 | |
| MOB-03 | High | Missing `catch_unwind` on C-ABI and JNI platform shims causes fatal process aborts on any panic. | UNTRIAGED |  | v2.7.4 | |
| MOB-04 | High | Synchronous `worker.join()` in `Drop` blocks calling UI thread for 10-30s on unreachable networks. | UNTRIAGED |  | v2.7.4 | |
| MOB-05 | Medium | Complete absence of battery SRAM (`.sav`) export/import methods in mobile bridge. | CONFIRMED | No `sram`/`battery` API anywhere in `rustynes-mobile`, `rustynes-android`, `rustynes-ios` | v2.7.4 | |
| MOB-06 | Medium | Per-frame `Vec<u8>` allocation & UniFFI copy generates ~15 MB/s GC churn across FFI. | UNTRIAGED |  | v2.7.4 | |
| MOB-07 | Medium | Transparent mutex poison recovery ignores state corruption in netplay and emulation. | UNTRIAGED |  | v2.7.4 | |
| MOB-08 | Medium | Partial state corruption when snapshot restore fails after bus stage. | UNTRIAGED |  | v2.7.4 | |
| MOB-09 | Medium | Synchronous blocking DNS resolution in `np_join` and `to_nat_config` blocks UI thread. | UNTRIAGED |  | v2.7.4 | |
| SEC-01 | High | HD-pack PNG decompression bomb: `output_buffer_size()` called without clamping dimensions allows crafted packs to cause OOM crash. | CONFIRMED | `rustynes-hdpack/src/hdpack.rs:869` allocates `output_buffer_size()` with no dimension clamp; png 0.18 `Limits` reserve one line only; `(w * h * 4)` is also a `u32` multiply that can overflow | v2.7.3 | |
| SEC-02 | High | Missing Lua memory limit allows scripts to consume unbounded system memory. | CONFIRMED | No `set_memory_limit` anywhere in `crates/rustynes-script/src/` | v2.7.3 | |
| SEC-03 | Medium | Lua debug hook budget error is caught by `pcall`, allowing malicious or buggy scripts to hang the emulator thread while holding `self.emu` lock. | UNTRIAGED |  | v2.7.3 | |
| SEC-04 | Medium | SSRF and unbounded response memory in host HTTP IPC (`CommCmd::Http`). | UNTRIAGED |  | v2.7.3 | |
| SEC-05 | Medium | Synchronous `recorder.stop()` waits on `ffmpeg` process on Winit UI thread, freezing interface. | UNTRIAGED |  | v2.7.3 | |
| SEC-06 | Medium | FDS disk saves, RetroAchievements progress, and TAS `.rnm` movies use bare `std::fs::write` instead of `write_atomic`. | UNTRIAGED |  | v2.7.1 | |
| CON-01 | Low | Missing compile-time guard for `wasm-canvas` vs `wasm-winit` conflict. | UNTRIAGED |  | v2.7.3 | |
| CON-03 | Low | Degenerate sample rate ratio causes excessive memory allocation in `resample_linear`. | UNTRIAGED |  | v2.7.3 | |
| CON-04 | Low | Corrupted `config.toml` silently discarded on load without `.bak` preservation. | UNTRIAGED |  | v2.7.1 | |
| CON-05 | Low | Dead/unwired `host` field in `RetroAchievementsConfig`. | UNTRIAGED |  | v2.7.3 | |
