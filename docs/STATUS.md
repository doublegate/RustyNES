# RustyNES — Project Status Matrix

> **Current release: v2.2.3** (2026-07-23) — **"Datum"**, a **performance and
> accuracy-closure patch** on top of v2.2.2 (below), produced by a measure-first
> appraisal that profiled the emulator and acted on what it found.
>
> **Performance.** The specialized PPU fast dot path is promoted to the
> **default** and exposed to users for the first time — `Nes::set_fast_dotloop`
> previously had **no caller outside the core**, so a **−11.3%** frame-time win
> (fresh clean-host Criterion, reproducing v2.1.8's +12.3% by a different
> method) shipped switched off and unreachable. It has been differential-tested
> bit-identical every frame — framebuffer, palette-index framebuffer, audio,
> CPU-cycle count and full core snapshot — since v2.1.8. Release builds now ship
> **PGO-optimized** Linux binaries when the existing >3%-and-byte-identical gate
> passes, falling back to the plain build otherwise; and CI gained a same-runner
> **relative** frame-time regression gate, closing a hole where a 2.5x slowdown
> passed the deliberately-loose absolute ceiling.
>
> **Two optimizations were measured and REJECTED**, documented in
> `docs/performance.md` with their numbers per that file's convention. P3
> (`emit_pixel` bounds-check elision) made the shipped default **slower**
> (+4.32% / +3.35% on the two `_fast` workloads, p ≤ 0.02); P4 (`cpu_clock`)
> found both textbook optimizations already implemented and the one remaining
> lever capped at **≤1.9%**, below the 3% bar.
>
> **Accuracy — the last two Holy Mapperel residuals are closed**, so all 17 ROMs
> now report `detail=0000` (was 15/17). MMC1's two software WRAM write-protect
> layers (`$E000` bit 4 and SNROM's CHR-register layer) and FME-7's open bus on
> the RAM-selected-but-disabled window are both modelled, routed through the
> trait's existing `cpu_read_unmapped` contract. MMC1 is the change Holy
> Mapperel's README calls a game-compatibility hazard, so it was validated
> before landing: **60/60** commercial ROMs (including seven battery-backed MMC1
> saves — Zelda, Metroid, Final Fantasy, Mega Man 2, Castlevania II, Ninja
> Gaiden, Kid Icarus) and **138/138** extended.
>
> The **Sunsoft 5B absolute level** is calibrated against Mesen2, which required
> widening `Mapper::mix_audio` to `i32` — the correct full-scale 5B tone
> (`1882 * 18.471 = 34,761`) does not fit `i16`. A **save-state schema gap** is
> fixed (`PPU_SNAPSHOT_VERSION` 8 carrying sprite-eval + OAM-bus state, plus an
> APU v4 tail), which is what made AccuracyCoin report **141/141 through
> run-ahead** as well as without it; a new standing field-vs-schema audit found
> it and the two APU gaps mechanically. A **Zapper beam-relative light model**
> lands opt-in / default-off (no pass-fail light-gun ROM exists to adjudicate
> it). **AccuracyCoin holds 141/141 (100.00%)**, nestest 0-diff.
>
> Also: the eleven `sprintN.rs` mapper modules (27,631 lines, ~110 boards) are
> renamed for the boards they emulate with `mNNN_` mapper-number prefixes,
> verified content-preserving by a byte-for-byte item comparison (930 items, 0
> altered) and an identical 172-ID dispatch table.
>
> The prior release, **v2.2.2** (2026-07-21) — **"Conduit"**, a **build,
> distribution, and CI-integrity patch**: the libretro buildbot recipe from 1 of
> 10 jobs green to **all ten building**, a GitHub Actions supply-chain hardening
> pass, and the toolchain collapsed to a single pinned source of truth with no
> `nightly` on any build path. Zero emulation-core changes.
>
> The prior release, **v2.2.1** (2026-07-15) — a **housekeeping patch** on top of
> v2.2.0 "Capstone" (below): archives two batches of dev/research tooling (the
> Game Genie header-robust re-key's six research/verification scripts in
> `scripts/gg/`, and the 2A03-revision DMA-divergence probe in `scripts/probes/`),
> consolidates six open Dependabot PRs with **zero source changes** (`pollster`
> 0.4→1.0, `wide` 0.7→1.5, `tungstenite`/`tokio-tungstenite` 0.29→0.30,
> `bytemuck`/`cc` patch, `actions/setup-python` v5→v6), and wires four gitignored,
> `RUSTYNES_FDS_BIOS`-gated smoke tests against `TakuikaNinja`'s FDS `$4023` /
> mirroring / audio-register / DRAM-watchdog hardware-verification probes
> (regression insurance for behavior RustyNES already models correctly — not a
> fix). **Zero accuracy, feature, or core changes** — the deterministic
> `#![no_std]` chip stack, save-state / TAS / netplay-replay formats, and every
> golden vector are untouched; **AccuracyCoin holds 141/141 (100.00%)**,
> unchanged from v2.2.0.
>
> The prior release, **v2.2.0 "Capstone"** (2026-07-12), was the **milestone cut** that
> **closes the v2.1.5 → v2.2.0 "deepen the existing project"** run on the v2.0.0 "Timebase"
> core, landing its two remaining marquees — the **netplay matchmaking / lobby** stack and the
> **FDS medium model** — atop a peripherals + quality/security pass (the joint iOS App Store /
> Google Play launch stays deferred to v2.3.0). Every change is **additive or default-off**, so
> the deterministic core is unchanged on the shipped default: **AccuracyCoin 141/141 (100.00%,
> RAM-authoritative)**, nestest 0-diff, `blargg_apu_2005` / `pal_apu_tests` (10/10) /
> `visual_regression` / the 60-ROM commercial oracle unchanged, the `#![no_std]` chip stack
> untouched, save-state / TAS / netplay replay bit-identical. Two PRs (#290, #291; cut #297). It
> lands: **(1)** a **netplay lobby + matchmaking** stack over the existing room-code / TURN
> transport — new `SignalMessage` variants `ListRooms` → `RoomList` (a browse-and-join,
> optionally game-filtered room directory whose `RoomInfo` carries code / player-count / capacity
> / `rom_hash` and *no* SDP/ICE/identity) and `QuickMatch` → `Matched` (server-side quick-play:
> join any open room via the shared `add_to_room` primitive, or mint a fresh `QM-NNNNNN`), the
> `room-list` frame parsed by a brace-depth walk bounded at `MAX_ROOM_LIST` = 256; **signaling
> only**, the determinism / rollback contract untouched. **(2)** **Delayed-stream spectators** —
> `SpectatorConfig.delay_frames` (clamped to `MAX_DELAY_FRAMES` = 512 ≈ 8.5 s), a pure
> *presentation* hold (`reveal_horizon()`) that cannot perturb the match. **(3)** a **hardened
> desync + peer-liveness surface** — a graded `DesyncStatus` { InSync / Suspect / Desynced } with
> a hysteresis threshold (`DEFAULT_DESYNC_THRESHOLD` = 3 ≈ 1.5 s) + a sticky peak-run rule, and a
> graded `PeerLink` { Live / Interrupted / TimedOut } driven by `last_recv` against 2 s / 5 s
> timeouts with a terminal `DisconnectReason::PeerTimeout` — pure telemetry over the existing
> `NetMessage::Checksum` digests. **(4)** the **FDS medium model completion** (F4.3, a marquee) —
> a per-block **CRC-16/KERMIT** re-emitted on every BIOS write (`resynth_block_crc`) over a
> synthesized gap/mark wire image, an opt-in default-OFF continuous analog head-seek / velocity
> model (`Fds::set_analog_head_seek`) replacing the flat `HEAD_RESEEK_CYCLES`, and a BIOS-free
> synthetic write-verify oracle (`Fds::medium_write_verify`) as the CI-verifiable half (the
> real-BIOS write-CRC path needs a copyright `disksys.rom` and runs only from a gitignored local
> dump); an additive **v4** FDS save-state tail, **byte-identical** with the head-seek model off
> (AccuracyCoin has no FDS ROM, so 141/141 is unaffected). **(5)** **peripherals** — a Famicom
> controller-2 **microphone** on `$4016` bit 2 (`Nes::set_microphone`, never `$4017`) and a
> **3×3-aperture** Zapper light-timing model (`ZAPPER_APERTURE_*`), both additive / default-off.
> **(6)** a **quality / security** pass — cargo-fuzz targets grown **3 → 8** (`ppu_reg_io` /
> `apu_reg_io` / `netplay_message` / `save_state` / `movie`), the `movie` target surfacing and
> fixing **two real OOM-DoS paths** in `Movie::deserialize` (byte-identical for valid input;
> regression `deserialize_hostile_frame_count_does_not_oom`); a read-only **Tools → ROM Info**
> browser over `&Nes` that never mutates; and four new MkDocs handbook pages.
> See `CHANGELOG.md` `[2.2.0]` + `.github/release-notes/v2.2.0.md`.
>
> **Earlier in the Fathom line: v2.1.10 "Fathom" ("Loom")** (2026-07-12) — the **creator-tools &
> web-parity** step of the **v2.1.5 → v2.2.0 "deepen the existing project"** run on the v2.0.0
> "Timebase" core. All work is **opt-in / additive**,
> so the deterministic core is unchanged on the shipped default: **AccuracyCoin 141/141
> (100.00%, RAM-authoritative)**, nestest 0-diff, `blargg_apu_2005` / `pal_apu_tests` (10/10) /
> `visual_regression` / the 60-ROM commercial oracle unchanged, the `#![no_std]` chip stack
> untouched, save-state / TAS / netplay replay byte-identical. It lands: **(1)** **TAStudio
> greenzone depth** — a "Force GZ" toggle in the piano-roll header guarantees a cached
> save-state at *every* frame in a bounded range (up to `MAX_FORCED_GREENZONE_FRAMES` = 10,800
> ≈ 3 min at 60 fps) so scrubbing / rewinding anywhere inside it is instant; forced frames are
> non-evictable anchors (marker / branch anchors untouched), and it stays a pure caching
> optimisation, so a seek into the range is bit-identical to a linear replay (the TAS /
> determinism contract is unchanged). **(2)** **Lua API breadth** — a fourth HUD primitive
> `emu.drawLine`; `memory:read_palette(idx)` (`$3F00-$3F1F`) + `memory:read_chr(addr)`
> (`$0000-$1FFF`, mapper-banked) via the side-effect-free debug-peek path; new lifecycle events
> `reset` / `spriteZeroHit` / `codeBreak` on `emu.addEventCallback`; and a **piccolo (wasm)
> parity uplift** closing most of the read + HUD gap (ADR 0012 carve-out now only per-access /
> per-interrupt replay callbacks + host-fired events); three well-commented example scripts
> (`hud_graph.lua` / `palette_viewer.lua` / `lifecycle_events.lua`) ship compile-time embedded.
> **(3)** **Browser RetroAchievements auth-proxy deploy stack** (ADR 0015) — the remaining
> deployable half of the browser-RA carryover: a first-class `ra-proxy` compose service
> (`deploy/Dockerfile.raproxy`, the stdlib-only reference stub, configured purely from env —
> `RA_USER_AGENT` / `RA_ALLOWED_ORIGINS` / `RA_UPSTREAM` / `RA_ENFORCE_CASUAL`, holding no RA
> secret) behind the shared Caddy TLS proxy at `https://<DOMAIN>/ra/*`, injecting RA's identity
> `User-Agent` server-side (browsers forbid scripts from setting it); marshalling-contract tests
> added to `wasm_cheevos.rs`. **Code-complete + compose/config validated**; standing the stack on
> a live host, the RA-team `User-Agent` coordination, and a real browser login are the un-CI-able
> acceptance gate.
> **(4)** **Vs. `DualSystem` presentation in the libretro core** — the RetroArch core
> (`crates/rustynes-libretro`) now presents Vs. `DualSystem` arcade cabinets (Balloon Fight /
> Wrecking Crew / Tennis / Baseball) at parity with the desktop frontend: it steps **both**
> cross-wired consoles each `retro_run` and composes their two 256×240 framebuffers into a single
> **512×240** XRGB8888 side-by-side image (MAIN left, SUB right); previously a `DualSystem` dump
> booted a single console that hangs on its absent partner. **Code-complete + builds**; a live
> RetroArch run with a real cabinet dump is the maintainer's manual check. The **wasm
> desktop-style dual present remains deferred** (the CPU compositor + core are cross-platform, but
> the wasm ROM-load detection + un-gating `Gfx::render_dual` are held). Determinism preserved
> throughout — reads are debug-peeks, writes stay gated / deferred, the deterministic `#![no_std]`
> core is untouched. **v2.2.0 "Capstone" has since shipped — see the top block.**
> See `CHANGELOG.md` `[2.1.10]` + `.github/release-notes/v2.1.10.md`.
>
> **Earlier in the Fathom line: v2.1.9 "Fathom" ("Aperture")** (2026-07-12) — a
> **presentation & signal** cut, all opt-in and **default byte-identical** (the shipped
> presentation is untouched, so `visual_regression` stays byte-identical and AccuracyCoin holds
> **141/141**). It lands a **raw NTSC composite core** — a new `rustynes-ppu::raw_signal`
> module that keeps the 2C02 composite waveform *un-decoded* (for every `(index, emphasis)` pair
> it emits the twelve per-subcarrier-phase voltages, so a decoder can reproduce composite colour
> bleed / dot crawl / dither transparency a per-colour palette cannot; canonical Bisqwit
> `nes_ntsc` / Mesen2 "raw palette" model, `f32` byte-identical across x86 / aarch64 / wasm /
> `thumbv7em` under a `GOLDEN_SIGNAL` cross-target lock; additive + default-OFF) — plus a
> **marquee CRT shader stack**: three single-pass WGSL ports of the reference libretro *slang*
> presets behind a `CrtStackShader` registry (**CRT-Royale**, **CRT Guest Advanced /
> guest-dr-venom**, **Sony Megatron** — the last with an HDR-headroom + SDR-Reinhard fallback),
> all four new shaders (the three CRT + the raw signal-decode pass `signal_decode.wgsl`, which
> reconstructs the 2C02's two-level chroma square wave and demodulates it with a windowed
> quadrature filter) gate-validated as compilable WGSL by the same **naga** front-end wgpu runs
> at pipeline creation. Selectable from **Settings → Shaders** as first-class `ShaderStack`
> passes with `#pragma parameter` sliders + per-game presets. Also: **GIF / animated capture +
> WAV audio export** (the `av-record` feature, driven from a read-only framebuffer / audio tap so
> capture never perturbs the deterministic timeline), and a **live generated-palette preview**
> (a 16×4 swatch of the generated NTSC base that regenerates from the current
> saturation / hue / contrast / brightness / gamma params as you drag the sliders; purely visual,
> the shipped palette unchanged until explicitly enabled). See `CHANGELOG.md` `[2.1.9]` +
> `.github/release-notes/v2.1.9.md`.
>
> **Earlier in the Fathom line: v2.1.8 "Fathom" ("Tempo")** (2026-07-12) — a
> **performance** cut (default build unchanged / byte-identical; the deterministic core is
> untouched). Profiling shows `Ppu::tick` is the emulator's single hottest function
> (**~46% of frame self-time**), the overwhelming majority of its 89,342 per-frame invocations
> being visible-scanline background-render dots whose event/bookkeeping branches are statically
> dead. A **runtime knob** (`Nes::set_fast_dotloop`, default **false**) dispatches those "clean"
> dots (visible scanline, dots `1..=256`, rendering stably enabled, no sub-dot disturbance in
> flight) to `Ppu::tick_visible_render_fast`, a straight-line handler running the **identical**
> helper sequence with the dead branches pruned; any disturbance drops instantly back to the exact
> per-dot path. **Byte-identical (proven, not assumed):** the default is byte-identical to a build
> without the field, and with the knob ON a new differential test (`fast_dotloop_diff`) asserts a
> corpus (`nestest`, `flowing_palette`, `oam_stress`, `AccuracyCoin`, the Holy Mapperel MMC1/MMC3
> boards, a mid-frame raster demo) is bit-for-bit identical through **both** paths every frame —
> including under the opt-in `Rp2c02G` die revision. **Measured** (interleaved per-frame A/B):
> rendering-enabled content is **~+12.3% faster per frame** (4.54 → 3.98 ms), well above the
> project's >3%-Criterion adoption bar; rendering-disabled content is neutral. A *whole-scanline
> batch* is **precluded** by the v2.0.0 "Timebase" every-cycle-bus-access scheduler (the CPU
> observes A12 / NMI / sprite-0 / `$2002` at 3-dot granularity), so this is a per-dot
> specialization, not a dot-batch. Also lands a frontend-only **vectorized software
> palette-index → RGBA blitter** (`gfx_blit`, scalar / scalar-`u32` / portable-SIMD paths, all
> validated byte-for-byte equal by `simd_equals_scalar_byte_identical` over the full `0..512`
> domain; a memory-bound LUT gather, so the SIMD path is within noise of scalar and **scalar stays
> the default**, documented honestly) plus a **wasm size / startup pass** (3.99 MiB gzip).
> **Shipped default-OFF** (recommended for promotion after maintainer review + a clean-host
> Criterion confirmation). See `CHANGELOG.md` `[2.1.8]` + `.github/release-notes/v2.1.8.md`.
>
> **Earlier in the Fathom line: v2.1.7 "Fathom" ("Stepping")** (2026-07-12) — a
> **hardware-revisions & DMA-frontier** cut, every knob **opt-in and default-off** so the
> deterministic core stays **byte-identical** at the default (**AccuracyCoin 141/141 (100%)**,
> nestest 0-diff, `visual_regression` / `pal_apu_tests` 10/10 and save-state round-trip
> unchanged). On the **PPU side** it adds a selectable 2C02 die revision + power-on hardware model
> (P5): a `PpuRevision` enum (default `Rp2c02H`, opt-in `Rp2c02G` gating the `$2003`
> write-during-render OAM row-copy corruption — the *Huge Insect* glitch — with no snapshot-format
> change), a power-up palette-RAM model (`PaletteInit`; default `Zeroed`, opt-in `Blargg` loading
> the canonical blargg dump), and a deterministic power-on work-RAM model (`PowerOnRam`:
> `Zeroed` / `Seeded(u64)` / `Filled(u8)` via `Nes::from_rom_with_power_on_config`, for titles
> that read uninitialized RAM — *Final Fantasy* RNG seed, *River City Ransom*, *Cybernoid*), all
> exposed through additive `Nes` setters + default-off `[emulation]` config keys. On the **2A03
> side** it adds a `Cpu2A03Revision { Rp2A03G (default), Rp2A03H }` config gating the DMA unit's
> "unexpected DMA" extra parked-address re-read on a DMC-halt-overlaps-OAM-halt cycle. The existing
> DMC↔OAM collision, OAM alignment, aborted DMC-DMA, and the `$2007`/`$4015`/`$4016`/`$4017`
> DMC-glitch register corruption all stay green (the five `dmc_dma_during_read4` ROMs, both
> `sprdma_and_dmc_dma` variants, and `dma_timing_pin` all `Pass`). The RP2A03G-vs-RP2A03H revision
> is modeled by **no** public reference emulator and verified by **no** test ROM; on this engine
> the revision gate fires but is a **documented no-op on every committed oracle** (`Rp2A03H` is
> byte-identical to `Rp2A03G` today — a mechanism-level model whose direction is an unverified
> hypothesis, **recorded not faked** in **ADR 0033** + `docs/scheduler.md` §"Unexpected DMA"; no
> `dmc_dma_during_read4` sub-test is failed or newly `#[ignore]`'d). See `CHANGELOG.md` `[2.1.7]` +
> `.github/release-notes/v2.1.7.md`.
>
> **Earlier in the Fathom line: v2.1.6 "Fathom" ("Timbre")** (2026-07-11) — the
> **expansion-audio fidelity** step. The base **2A03 NTSC output stays byte-identical** —
> expansion audio is a separate additive `mix_audio` term (0 for non-expansion mappers) — so the
> deterministic core is unchanged on the shipped default: **AccuracyCoin 141/141**, nestest
> 0-diff, `blargg_apu_2005` / `pal_apu_tests` (10/10) / `visual_regression` / the 60-ROM
> commercial oracle unchanged, the `#![no_std]` chip stack untouched. It lands: **(1)** an
> **expansion-audio decibel oracle** — the bbbradsmith `db_*` comparison ROMs are upgraded from
> pure `insta` snapshots into a real accuracy oracle (`audio_expansion.rs` `level_db_*` tests
> measure the peak amplitude of the reference-2A03-square and expansion-square segments and
> **assert** the ratio against the Mesen2 / hardware target — APU triangle ÷ square ≈ 0.524,
> VRC6 ≈ 1.506, MMC5 ≈ 1.000, N163 1-channel ≈ 6.02; the 19 `insta` snapshots retained as
> byte-exact guards). **(2)** **hardware/Mesen2 channel-level calibration** — VRC6 square
> `256 → 979`, MMC5 pulse/PCM `256/16 → 650/40`, and **Namco 163 `64 → 261`** (1-channel mode was
> **~12 dB too quiet, now fixed**, the fix bit-shared with the NSF path; only the three
> `db_vrc6a/b` / `db_mmc5` expansion snapshots re-blessed, audio-hash-only). **(3)** **VRC7
> patch-set verification** — all 15 melodic (+ 3 rhythm) OPLL patches pinned to the canonical
> Nuke.YKT dump (+ 5B log-DAC / N163 long-wave unit tests). **(4)** a frontend **Audio Mixer**
> panel (Tools → Audio Mixer — per-source gain sliders + mute, presets, live oscilloscope + VU
> meters via a determinism-safe `channel_gain` / `channel_mask` overlay that is byte-identical at
> unity and never serialized into the save state). **Sunsoft 5B absolute level** and **VRC7 FM
> level** stay honest documented gaps (`docs/accuracy-ledger.md` §Expansion-audio levels),
> snapshot-guarded rather than faked. See `CHANGELOG.md` `[2.1.6]` +
> `.github/release-notes/v2.1.6.md`.
>
> **Earlier in the Fathom line: v2.1.5 "Fathom" ("Vernier")** (2026-07-11) — the
> **regression-net & residual** step that opened the **v2.1.5 → v2.2.0 "deepen the existing
> project"** run (the deterministic NTSC core byte-identical, **AccuracyCoin 141/141**). Five
> workstreams across
> six PRs (#268/#270, #272, #273, #274, #275). The deterministic NTSC core stays
> **byte-identical** on the shipped default: **AccuracyCoin 141/141 (100.00%,
> RAM-authoritative)**, nestest 0-diff, `blargg_apu_2005` 11/11, the 60-ROM commercial
> oracle + `visual_regression` unchanged, the `#![no_std]` chip stack untouched. It lands:
> **(1)** a **mapper bank-reachability + IRQ regression net** wiring the tepples **Holy
> Mapperel** cartridge-PCB-assembly ROMs into CI (`holy_mapperel.rs`, `--features
> test-roms`) — each of 17 committed zlib-licensed ROMs is driven to its settled result
> screen and pinned by an `insta` framebuffer-hash snapshot behind *settled* + *non-blank*
> structural guards; 15/17 detect the correct mapper and reach every PRG/CHR bank with code
> `0000`, and the two MMC1 + two FME-7 ROMs surface an honestly-pinned **WRAM-protection
> residual** whose cause differs per mapper (MMC1 genuinely omits the software WRAM
> write-protect — a widely-shared simplification; FME-7's is a narrow
> "RAM-selected-but-disabled → open-bus" edge). Purely additive — no core behavior change,
> AccuracyCoin stays 141/141; residuals recorded in `docs/accuracy-ledger.md`. **(2)** the
> **first PAL-region APU oracle** — the PAL (2A07) frame-counter step positions modeled and
> blargg's `pal_apu_tests` (10 PAL-calibrated sub-ROMs) wired into CI → **10/10**. It
> **corrects a false oracle** (the prior `$6000`-status runner passed these no-PRG-RAM
> 2005-era NROM ROMs vacuously; the suite now decodes the real **on-screen** `PASSED` /
> `FAILED: #n` verdict via a new `run_nes_screen` harness runner) and carries a genuine
> **length halt/reload write-ordering fix** — the length counter was applying a `$4000`
> halt and `$4003` reload *immediately* on the CPU write instead of *after* the half-frame
> length clock (the inverse of silicon); it now defers both, promoted once per CPU cycle
> after the half-frame clock. **This bug was latent on NTSC too**, but the region-gated PAL
> positions and the by-construction-byte-identical ordering fix keep the NTSC default
> byte-identical. **(3)** an **MMC3 R1/R2 residual A12-phase instrumentation study** (F5.0,
> ADR 0002; default-off `mmc3-a12-phase-probe` + `mmc3_r1r2_phase_probe.rs`) that
> *refines* the F5.0 finding: the two `scanline_timing` residuals have zero post-access
> IRQ-clocking rises (confirming Session B's earlier indirect result), but the two
> "reload/set-IRQ-every-clock" residuals — never tested before — have **4** post-access
> rises each, so the "no post-access rise" premise is ROM-specific, not a structural
> NTSC-MMC3 property. No production / scheduler / MMC3 default behavior changed; all four
> residuals stay `#[ignore]`'d and the ares-style M2-edge low-time *filter* is recorded as
> the one genuinely-untested axis-B lever, deferred to a maintainer decision. **(4)** a
> **measured fat-LTO A/B validation** (`docs/performance.md`) retroactively backing the
> already-shipped `lto = "fat"` + `codegen-units = 1` release default (+8.4% / +20.8% on
> cross-crate hot paths, zero byte-identity cost) and correcting stale `139/139` / `lto =
> "thin"` text. **(5)** a **real symmetric-NAT netplay production-bug fix** — the native
> TURN client (`relay.rs`) now retransmits each `Allocate` / `CreatePermission` every
> 250 ms (a fixed RTO, not the RFC 5389 §7.2.1 default of 500 ms + exponential backoff) instead of sending once, so a single dropped datagram no longer
> hard-fails NAT traversal (and de-flakes the `windows-latest` `relay_loopback` CI test
> that was blocking `release-auto`); the session-digest determinism contract is unchanged.
> See `CHANGELOG.md` `[2.1.5]` + `.github/release-notes/v2.1.5.md`.
>
> **Earlier in the Fathom line: v2.1.4 "Fathom" ("Caliper")** (2026-07-11) — an
> **accuracy-hardening** cut (deterministic core byte-identical, **AccuracyCoin 141/141**):
> an **opt-in, default-OFF OAM decay** model (the 2C02's dynamic sprite RAM decaying to a
> fixed garbage pattern when un-refreshed, modeled exactly like Mesen2 — a 3000-CPU-cycle
> per-8-byte-row refresh window; enable via **Settings → Emulation → "OAM decay
> (accuracy)"**, the `[emulation] oam_decay` config bool, or `Nes::set_oam_decay(true)`;
> byte-identical to a decay-free build with the default off, deterministic when on, NTSC/Dendy
> only, its per-row state round-tripping an additive **`PPU_SNAPSHOT_VERSION` 6 → 7** tail);
> a **CI boot-smoke sweep of all 26 `BestEffort` mapper families** (`v21_best_effort_sweep.rs`,
> `--features test-roms`) building each into a synthetic minimal iNES / NES 2.0 image and
> running ~60 deterministic headless frames (a pure safety net — promotes nothing, no real
> panics found); and a **shared MMC3-clone A12/IRQ timing oracle** (`mmc3_clone_a12.rs`)
> proving the reusable `Mmc3Clone` core reproduces MMC3's A12-clocked scanline-counter IRQ
> timing **bit-for-bit** for all eleven `Mmc3CloneMapper` boards (44, 49, 52, 115, 134, 189,
> 205, 238, 245, 348, 366); no tier move, no source change. See `CHANGELOG.md` `[2.1.4]`.
>
> **Earlier in the Fathom line: v2.1.3 "Fathom" ("Codex")** (2026-07-11) — a
> **quality-of-life** cut (the deterministic core is byte-identical, AccuracyCoin **141/141**):
> an **APU audio filter-model selector** (Settings → Audio → Filter model / `[audio]
> filter_model` — `nes` authentic default / `famicom` / `clean`, fixing the "thin / missing
> bass channel" character while keeping the default byte-identical to the audio oracle),
> **Game Genie per-game code nomination + a bulk code database** (`genie_database_full.tsv`,
> ~10,800 codes across ~520 USA/World games, plus a universal header-robust
> `genie_database_headerless.tsv` re-keyed to the header-excluded `rom_crc32` for ~521 games
> so a re-headered dump still matches — all three catalogs ship on wasm inside the 5 MiB
> budget), and a **Material-for-MkDocs documentation handbook** served at `/docs/` on GitHub
> Pages (the deployment now serves three sections: the wasm demo at `/`, the rustdoc at
> `/api/`, and the handbook at `/docs/`). See `CHANGELOG.md` `[2.1.3]`.
>
> **Earlier: v2.1.2 "Fathom" ("Prism")** (2026-07-11) — a **display-fidelity** cut (the
> deterministic core is byte-identical, AccuracyCoin **141/141**, presentation-only):
> **Vs. `DualSystem` second-screen desktop presentation** (both cross-wired consoles run and
> present together — side-by-side or stacked via `[graphics] dual_screen_layout`; the advanced
> single-`Nes` features are scoped out in dual mode per ADR 0032), the **NTSC composite-shader
> ladder completed** (simplified blur `Ntsc` → LMP88959 `Lmp88959` → Bisqwit per-dot
> `CompositeRt`, with live emulator-synced dot-crawl now wired to LMP88959), an in-core
> **generated NTSC palette** (`rustynes_ppu::generate_base_palette`, every transcendental routed
> through `libm` so the output is byte-identical across x86 / aarch64 / wasm / `thumbv7em`, off
> by default), and **NSF non-60 Hz playback + NSFe** container support. See `CHANGELOG.md`
> `[2.1.2]`.
>
> **The preceding patch: v2.1.1 "Fathom"** (2026-07-10) — a **bug-fix patch** on
> v2.1.0. The cycle-accurate core is unchanged (**AccuracyCoin 141/141**, nestest
> 0-diff, no oracle move). The **root-cause fix** is a **game-database mirroring
> gate**: `game_database.txt` (vendored TetaNES) was force-applying its `mirroring`
> column to every matched ROM via `Nes::set_mirroring_override`, including mappers
> that control their own mirroring at runtime. Wizards & Warriors (AxROM/mapper 7)
> flips single-screen A↔B mid-frame to draw its status bar; the DB's spurious
> `Horizontal` pinned it, killed the sprite-0 split, and hung the game — a hard,
> deterministic freeze on desktop and WASM while a headless core played it perfectly.
> Fix: **`Mapper::has_hardwired_mirroring()`** (default `false` = mapper controls
> its own mirroring → override declined; `true` only on NROM/UxROM/CNROM/GxROM),
> gating both `App::apply_game_db` and the per-game overlay. **Additionally
> hardened**: **`PPU_SNAPSHOT_VERSION` 5 → 6**, an additive tail serializing
> per-sprite shifter-halt flags, the 1-dot-delayed rendering gate, and OAM-corruption
> arming state that were previously missing from the run-ahead `snapshot`/`restore`
> path (pre-v6 `.rns` still load, upconverting to power-on defaults — not an
> ADR-0028 epoch break); also hardens netplay rollback + manual save/load. GitHub-safe
> regression test (skips when the commercial dump is absent). Version bump
> `2.1.0 → 2.1.1`. See `CHANGELOG.md` `[2.1.1]` + `.github/release-notes/v2.1.1.md`.
>
> **Earlier — the base of the Fathom line: v2.1.0 "Fathom"** (2026-07-09) — the
> **accuracy-remediation** release, and the first of the "Fathom" line. A core / desktop cut
> that lands **ahead of** the joint mobile store launch (which moved from v2.1.0 to **v2.3.0**,
> so the Android + iOS apps re-release on this improved core). The deterministic core
> is unchanged except one **display-only** PPU fix, so AccuracyCoin stays **141/141
> (100.00%, RAM-authoritative)**, nestest 0-diff, the `#![no_std]` chip stack
> untouched; no save-state/format bump. It lands: **(F1.1)** the **PPU palette
> backdrop-override** — rendering-disabled + `v` in `$3F00-$3FFF` outputs
> `palette[v & 0x1F]` not the backdrop, **byte-exact with TriCNES**, fixing the
> `full_palette` / `flowing_palette` demos (9 snapshots re-blessed: 2 demos + 7
> commercial games, all converging with the TriCNES oracle; `external_real_games`
> 60/60 byte-identical); **(F1.2/F1.3)** OAM + open-bus audits regression-locked;
> **(F3)** the **mapper completion** — **86** families promoted BestEffort → Curated
> with a commercial-ROM boot-snapshot oracle (57 already-staged + 29 from GoodNES
> v3.23b), so the tier split is **51 Core + 95 Curated + 26 BestEffort** and
> oracle-gated coverage rises **60 → 146** of 172 (the 26 left have no
> cleanly-booting dump); **(F5)** the **MMC3 R1/R2 scanline-IRQ residual CLOSED** by-design-permanent
> (ADR 0002 F5.0 — differential 1-dot deficit, structurally unreachable, zero game
> impact), with all **20** `#[ignore]`'d tests catalogued in the new
> `docs/accuracy-ledger.md` (none an accuracy gap); and **(F0)** doc reconciliation.
> Version bump (workspace `2.0.8 → 2.1.0`; mobile `MARKETING_VERSION`s unchanged —
> the apps re-release at v2.3.0). See `CHANGELOG.md` `[2.1.0]` +
> `docs/accuracy-ledger.md` + `docs/adr/0002-irq-timing-coordination.md` +
> `to-dos/plans/v2.1.0-fathom-accuracy-remediation-plan.md`.
>
> **Earlier: v2.0.8 "Harbor"** (2026-07-09) — the eighth release of the v2.0.x
> mobile-finalization train, and the **iOS release candidate** ("Harborlight"), the final
> release of the iOS finalization window (v2.0.5–v2.0.8). A **host / iOS-only** cut: the
> cycle-accurate emulation core is **unchanged and byte-identical to v2.0.7** —
> AccuracyCoin still **141/141 (100.00%, RAM-authoritative)**, nestest 0-diff, the
> `#![no_std]` chip stack untouched. It stages the App Store scaffolding a maintainer
> needs for the v2.1.0 launch: **(1)** version-controlled **App Store Connect listing
> metadata** (`fastlane/metadata/ios/{en-US,es-ES}/` — name / subtitle / promotional text
> / keywords / description / release notes / URLs + copyright), mirroring the Android
> tree, files-only, no upload; **(2)** a **dormant App Store `release` lane** in
> `fastlane/Fastfile` that stages the build + listing and **does not submit**
> (`submit_for_review: false`), **not** wired into CI — the interim channel stays
> **TestFlight** (the `beta` lane); and **(3)** an **App-Review §4.7 self-audit** (no
> bundled / downloadable ROMs, no in-app ROM links, no Nintendo branding, in-app
> ownership notice, searchable library, 4+ age rating) recorded in
> `docs/ios-v2.0.8-readiness.md`. Version bump (workspace `2.0.7 -> 2.0.8`; iOS
> `MARKETING_VERSION -> 2.0.8`). **No store submission** (that is v2.1.0); screenshots,
> real signing, the listing upload, and the App-Review submission are the **maintainer /
> v2.0.9 / v2.1.0** closeout. See `CHANGELOG.md` `[2.0.8]` + `docs/ios-v2.0.8-readiness.md` +
> `to-dos/plans/v2.0.5-v2.0.8-ios-finalization-plan.md`.
>
> **Earlier in the train: v2.0.7 "Harbor"** (2026-07-09) — the seventh release of the v2.0.x
> mobile-finalization train, and the **third iOS finalization release** ("Trim"),
> continuing the iOS window (v2.0.5–v2.0.8). A **host / iOS-only** cut: the
> cycle-accurate emulation core is **unchanged and byte-identical to v2.0.6** —
> AccuracyCoin still **141/141 (100.00%, RAM-authoritative)**, nestest 0-diff, the
> `#![no_std]` chip stack untouched. It wires the **App Store submission floor**: Apple
> mandates the **iOS 26 SDK / Xcode 26** for every App Store Connect upload from
> **2026-04-28**, so the tag-gated iOS CI now selects the newest Xcode 26.x on the runner
> (a build-SDK pin, separate from the minimum OS; non-breaking fallback + warning on older
> images). It **reconciles the deployment target `iOS 15.0 → 17.0`** to match the code's
> real API floor (`NavigationStack` iOS 16 + `.topBarTrailing` iOS 17, unguarded at 12+
> sites — the prior 15.0 was never actually buildable), and **re-audits
> `PrivacyInfo.xcprivacy`** against the v2.0.6 crash reporter (no new data type, no new
> required-reason API — it is local-only / backup-excluded / off by default, and
> `UserDefaults` is already declared `CA92.1`). Version bump (workspace `2.0.6 -> 2.0.7`;
> iOS `MARKETING_VERSION -> 2.0.7`). **TestFlight-only**; App Store + AltStore PAL deferred
> to v2.1.0; on-device profiling + the Xcode-26 archive are a **maintainer / v2.0.9** step.
> See `CHANGELOG.md` `[2.0.7]` + `docs/ios-v2.0.7-readiness.md` +
> `to-dos/plans/v2.0.5-v2.0.8-ios-finalization-plan.md`.
>
> **The release before that: v2.0.6 "Harbor"** (2026-07-09) — the sixth release of the v2.0.x
> mobile-finalization train, and the **second iOS finalization release** ("Parity"),
> continuing the iOS window (v2.0.5–v2.0.8). A **host / iOS-only** cut: the
> cycle-accurate emulation core is **unchanged and byte-identical to v2.0.5** —
> AccuracyCoin still **141/141 (100.00%, RAM-authoritative)**, nestest 0-diff, the
> `#![no_std]` chip stack untouched — so no accuracy / save-state / determinism number
> moves. It adds a **new opt-in, privacy-first crash-reporting surface** (off by
> default, the iOS analogue of the Android v1.8.8 `CrashReporter`, closing the v1.9.9
> iOS-applicable deferral): **Settings → Diagnostics** installs an
> uncaught-`NSException` handler that writes **local** crash logs the user can view +
> copy in-app — **nothing is uploaded**, so the "Data Not Collected" privacy label is
> unchanged (EN + ES); the handler re-checks the live opt-in at crash time so opting
> out stops new logs immediately. It also records the **feature-parity
> re-verification** of the v1.9.x host features (Game Center, CloudKit save sync, MFi
> controllers, capture / PiP, accessibility) against the unchanged v2.0.0 bridge
> surface. Version bump (workspace `2.0.5 -> 2.0.6`; iOS `MARKETING_VERSION -> 2.0.6`).
> **TestFlight-only**; App Store + AltStore PAL deferred to v2.1.0; on-device
> crash-capture verification is a **maintainer / v2.0.9** step. See `CHANGELOG.md`
> `[2.0.6]` + `docs/ios-v2.0.6-readiness.md` +
> `to-dos/plans/v2.0.5-v2.0.8-ios-finalization-plan.md`.
>
> **Earlier in the train: v2.0.5 "Harbor"** (2026-07-09) — the fifth release of the v2.0.x
> mobile-finalization train, and the **first iOS finalization release** ("Landfall"),
> opening the iOS window (v2.0.5–v2.0.8) that mirrors the Android v2.0.1–v2.0.4 window.
> This is a **host / iOS-only** cut: the cycle-accurate emulation core is **unchanged and
> byte-identical to v2.0.4** — AccuracyCoin still **141/141 (100.00%, RAM-authoritative)**,
> nestest 0-diff, the deterministic `#![no_std]` chip stack untouched — so no accuracy /
> save-state / determinism number moves. It re-ports the frozen v1.9.9 SwiftUI / Metal app
> onto the v2.0.0 "Timebase" core: **(1)** the pre-Timebase movie warning is surfaced +
> localized on iOS (a non-blocking notice on its own channel — not an error, **EN + ES**,
> drained via `EmulatorCore.drainWarnings()` → `NesController.drainWarningCodes()`, wording
> byte-identical to the Android v2.0.4 string) so loading a pre-v2.0.0 `.rnm` tells the user
> byte-exact framebuffer/audio reproduction isn't guaranteed across the ADR-0028 timebase
> change; **(2)** the UniFFI-Swift binding surface (`drainWarningCodes` /
> `HostWarning.preTimebaseMovie` / `moviePlay`) is re-confirmed against the v2.0.0 bridge
> (host-verified Swift emit); and the **version bump** (workspace `2.0.4 -> 2.0.5`; iOS
> `MARKETING_VERSION 1.9.1 -> 2.0.5`, realigned from the frozen v1.9.x default). The core is
> untouched, so the chip stack / `no_std` / AccuracyCoin contract is unaffected.
> **TestFlight-only**; App Store + AltStore PAL stay deferred to the v2.1.0 joint launch.
> The **on-device closeout is a maintainer / v2.0.9 step** — the xcframework build on macOS
> (**Xcode 26 / iOS 26 SDK**, the App Store submission floor that lands as a hard gate at
> v2.0.7), save-state migration from a v1.9.x install, and the AccuracyCoin / SMB / Zelda
> determinism smoke on Apple silicon. See `CHANGELOG.md` `[2.0.5]` +
> `docs/ios-v2.0.5-readiness.md` + `to-dos/plans/v2.0.5-v2.0.8-ios-finalization-plan.md`.
>
> **Earlier in the train: v2.0.4 "Harbor" ("Slipway")** (2026-07-08) — the fourth release of the v2.0.x
> mobile-finalization train, and the **Android release-candidate** milestone. This is a
> **host / Android-only** cut: the cycle-accurate emulation core is **unchanged and
> byte-identical to v2.0.3** — AccuracyCoin still **141/141 (100.00%, RAM-authoritative)**,
> nestest 0-diff, the deterministic `#![no_std]` chip stack untouched — so no accuracy /
> save-state / determinism number moves. What v2.0.4 adds is the release-candidate
> scaffolding a maintainer needs to stage the Android app onto a Play Console testing
> track: **(1)** the `release` build type wired to the upload keystore with a **graceful
> debug-signing fallback** (a keyless CI / local `assemble{Foss,Play}Release` still
> produces an installable — debug-signed, never shippable — RC artifact; a real store
> upload uses the `keystore.properties` / `RUSTYNES_UPLOAD_*` maintainer secret);
> **(2)** debug-only **StrictMode** diagnostics (`DebugStrictMode`, thread + VM
> log-only, `BuildConfig.DEBUG`-guarded, inert in release) as the host-side complement
> to the on-device crash-free-rate / ANR gate, alongside the existing opt-in
> `CrashReporter`; **(3)** version-controlled **fastlane Play Console listing metadata**
> (`fastlane/metadata/android/{en-US,es-ES}/` — title / short + full description /
> `changelogs/20004.txt`); **(4)** an **R8 / ProGuard final hardening review** confirming
> the release keep set complete (no keep loosened); and the **version bump** (workspace
> `2.0.3 -> 2.0.4`, Android `versionCode 20003 -> 20004` / `versionName -> 2.0.4`). The
> `foss` (F-Droid / GitHub-sideload) flavor stays **behaviour-identical**. **No store
> submission** (that is the future v2.1.0 joint launch); the **on-device closeout is a
> maintainer / v2.0.9 step** — the actual Play Console listing upload, signing with the
> real keystore, the internal/closed testing track, the crash-free-rate + ANR gate on
> real hardware, live monetization runtime verification, and the deferred per-feature
> gate migration (keep the existing `LicenseManager` freemium gates — do **not** rewire
> them to the monetization `feature_enabled` façade without on-device validation). See
> `CHANGELOG.md` `[2.0.4]` + `to-dos/plans/v2.0.4-android-rc-plan.md`.
>
> **Earlier in the train: v2.0.3 "Harbor" ("Keel")** (2026-07-08) — the third release of the v2.0.x
> mobile-finalization train, and the one that makes the octal-latch accuracy work
> real at the shipped default. The **2-cycle-ALE PPU fetch model is promoted from the
> experimental `mc-ppu-2cycle-ale` flag to the unconditional, only PPU fetch path**
> (ADR 0030), so the shipped default now scores **AccuracyCoin 141/141 (100.00%,
> RAM-authoritative)** — both **"ALE + Read"** (`$0491`) and **"Hybrid Addresses"**
> (`$0492`) pass out of the box (previously an honest 139/141). This replaces v2.0.2's
> whole-dot `+1 coarse-X` stand-in with a genuine two-dot fetch (even-dot ALE-drive +
> `octal_latch` load; odd-dot `(address & 0x3F00) | octal_latch` splice + read),
> where the latch *naturally* carries the stale byte: the delayed `CopyV`
> (`copy_v_delay = 4`) makes the NT read splice `$2F19` for Hybrid Addresses, and the
> `$2007`-ALE overlap freezes the latch on `$FF` → `$0FFF` for ALE + Read. **Both
> experimental flags are retired** (`mc-ppu-2cycle-ale` + the superseded
> `mc-ppu-bus-addr-hybrid`), and the v2.0.2 stand-in code (`octal_effective`, the
> `OctalFetch` enum, `copy_v`, the coarse-X reconstruction) is deleted; the
> `octal_trace` calibration ring survives behind a new default-off dev feature
> `ppu-octal-trace`. Verified: **60-ROM commercial oracle 60/60** with exactly two
> documented re-blesses (SMB3, Uchuu Keibitai SDF — single-tile `$2006`-during-render
> shifts, *more* TriCNES-faithful, audio/cycle byte-identical), nestest 0-diff, mmc3
> A12-IRQ 18/18, `ppu_sprites` 19/19; ~10% headless frame-cost rise (~4.15 ms/frame,
> ~4x realtime). **Save-state:** promotion made the model the netplay-rollback
> substrate, which exposed a determinism gap in the five in-flight fetch fields; they
> are now serialized in an additive **`PPU_SNAPSHOT_VERSION` v5 tail** (pre-v5 `.rns`
> still load; a v2.0.3 state is forward-incompatible with <= v2.0.2 but this is an
> in-train sub-version bump, not an ADR-0028 epoch break). Also this release: the
> **Harbor Android foss/play monetization glue** (step 5 — the `play`
> `MonetizationGate` with AppLovin MAX + RevenueCat 8.10.0, `feature_enabled` gating,
> run-out paywall + countdown, session/progress/offline-grace; a no-op `foss` twin so
> the F-Droid artifact stays behaviour-identical; both flavors assemble, monetization
> behaviourally dormant pending v2.0.9 on-device verify) and a **host-localizable
> mobile bridge-warning** API (`rustynes-mobile` `HostWarning` enum +
> `drain_warning_codes()`; `drain_warnings()` preserved). No store submission (that is
> the future v2.1.0 joint launch); Android continues as GitHub-sideload. See
> `CHANGELOG.md` `[2.0.3]` + `to-dos/plans/v2.0.3-2cycle-ale-plan.md`.
>
> **The preceding release: v2.0.2 "Harbor" ("Soundings")** (2026-07-08) — the second release of the v2.0.x
> mobile-finalization train, and Harbor's **headline accuracy release**: the two new
> upstream AccuracyCoin PPU tests v2.0.1 documented as honest gaps — **"ALE + Read"**
> (`$0491`) and **"Hybrid Addresses"** (`$0492`) — are now **solved flag-on** by a
> whole-dot port of TriCNES's **octal-latch multiplexed-bus PPU model** (ADR 0030,
> commit `27c103c`), behind the pre-existing default-off `mc-ppu-bus-addr-hybrid`
> feature flag. The NES 2C02 multiplexes its low eight VRAM address pins with the
> eight data pins (PA0-7 = AD7-0), so an external octal latch holds A7-A0 across the
> ALE transition; when the halves desync, a fetch reads a hybrid address it never
> coherently output. The model adds an 8-bit `octal_latch` + 14-bit `address_bus` +
> a `copy_v`/`pattern_latch_stale` one-shot pair (all `#[cfg]`-gated) and resolves
> every background fetch through `octal_effective` — the TriCNES `FetchPPU` splice
> `(address_bus & 0x3F00) | octal_latch` — transparent except on the two modeled
> corruption events (`$2007`R freezing the latch on its DATA byte → pattern read
> `$0FFF`; `$2006`W2 capturing the stale low byte → NT read `$2F19`). A12/MMC3
> notification stays on the intended (un-spliced) fetch address, so IRQ timing is
> unchanged, and no snapshot-format bump is needed (`PPU_SNAPSHOT_VERSION` stays 4).
>
> **Shipped default stays honest 139/141; the flag-on model is verified 141/141.**
> Per the *Feature-Flag Additive Change* + "bake, then promote" guardrails and the
> maintainer's **refine-then-promote** decision (ADR 0030), v2.0.2 ships the model
> **default-off**: the shipped default build is **byte-identical to v2.0.1** —
> AccuracyCoin **139/141 (98.58%)**, nestest 0-diff — while flag-on it reaches
> **141/141 (100.00%)** with a clean battery (framebuffer 100%, nestest 0-diff, mmc3
> A12 clocking + IRQ all pass, `ppu_sprites` 19/19, `mmc1_a12` pass). The campaign
> (`docs/audit/v2.0.2-octal-latch-campaign-2026-07-08.md`) corrected two ADR 0030
> premises: **Mesen2 does NOT pass these tests** (both bytes read `0x0A` = corruption
> not reproduced — the correct oracle is TriCNES, the AccuracyCoin author's own MIT
> emulator, `ref-proj/TriCNES`, commit `9199870`), and **a whole-dot port suffices**
> (the full 2-cycle-ALE refactor was not required). **Promotion to default (shipped
> 141/141) is the deliberate v2.0.3 step** — after the Hybrid path's `+1 coarse-X`
> approximation is reworked to a first-principles latch-carry model and gated on the
> 60-ROM commercial byte-identity oracle + a CI job asserting flag-on 141/141. **This
> release does not claim the shipped build is 141/141, nor that the flag is promoted.**
> No store submission (that is the future v2.1.0 joint launch); Android continues as
> GitHub-sideload. See `CHANGELOG.md` `[2.0.2]` + `to-dos/plans/v2.0.2-harbor-plan.md`.
>
> **The preceding release: v2.0.1 "Harbor" ("Mooring")** (2026-07-08) — the first release of the v2.0.x
> mobile-finalization train onto the v2.0.0 "Timebase" core (`to-dos/ROADMAP.md`),
> bundling the Android core re-port + `foss`/`play` split scaffolding, the **AccuracyCoin
> oracle re-sync** (catalog 144→146 rows / 139→141 assigned; measured honestly at
> **139/141, 98.58%**), the **CI cost optimization** (heavy suite gated to `release/*` +
> a weekly cron), the **dependency sweep** (uniffi 0.32 / mlua 0.12 / wgpu-naga 29.0.4 /
> cc 1.2.66; wgpu 30 deferred on the egui 0.35 pin), and the **`mc-r1-dmc-abort-probe`
> housekeeping removal**. Every core change is behaviour-neutral, so the deterministic
> core is byte-identical to v2.0.0 and host CI accuracy is unchanged. The Android re-port
> is host-shell-only. Landed:
> the Android `versionCode`/`versionName` → `20001`/`2.0.1`; a **movie-epoch warning on
> the mobile bridge** (`NesController::movie_play` peeks `recorded_before_v2_timebase`
> and queues a drainable warning via the new `drain_warnings`, mirroring desktop/wasm —
> ADR 0028; a pre-v2.0.0 `.rns` restore already maps cleanly to `MobileError::SaveState`);
> and the **structural start of the `foss`/`play` product-flavor split** (ADR 0025) — a
> `distribution` flavor dimension (`foss` default + `play`), every proprietary Google SDK
> moved to `playImplementation`, the six Google-touching glue classes moved to `src/play/`
> with **no-op façades in `src/foss/`** (so `foss` links zero Google SDKs), the Play
> manifest meta-data moved to `src/play/AndroidManifest.xml`, and the
> `installDebug`→`installFossDebug` alias. Ad / RevenueCat glue stays dormant; the ad
> wiring, on-device dual-flavor verification, and F-Droid submission are the v2.1.0 step.
> See `docs/android.md` + `to-dos/v1.8.x-on-device-verification.md`. android.yml CI (the
> NDK cross-build + both-flavor Gradle package) is the compile gate for this change.
>
> **The preceding release: v2.0.0 "Timebase"** (2026-07-03) — the **one-clock,
> every-cycle-bus-access scheduler rewrite** (ADR 0002 / ADR 0029): the old
> five-counter, dot-lockstep model (`tick_one_dot`) is replaced by a single
> canonical cycle counter, every CPU cycle a real bus access (the historical
> 9,795-cycle busless surface → 0), and a split-around-the-access
> `start_cycle`/`end_cycle` PPU catch-up mirroring Mesen2's structure. This is
> RustyNES's designated **MAJOR-boundary release** (ADR 0003): `.rns` save-state
> and `.rnm` TAS-movie format epochs bump (ADR 0028) — a pre-v2.0.0 slot file now
> fails to load with a clear error instead of silently misinterpreting stale
> bytes. Landed across five betas + rc.1 (PRs #217-223): beta.1 the counter
> collapse, beta.2 every-cycle-bus-access (the STOP-OR-GO gate), beta.3 the
> cycle-accurate reset (closing residual R4), beta.4 THE PROMOTE (the feature
> flag deleted — one-clock is now the only path), beta.5 full Vs. `DualSystem`
> dual-console support (core-and-harness-only; the frontend does not yet consume
> it), and rc.1 the format break + the canonical architecture ADR. The R1/R2 MMC3
> IRQ-timing residual was investigated under a maintainer-authorized
> bounded-effort campaign and is **by-design-deferred beyond v2.0.0** with a
> mechanism-level finding recorded in ADR 0002 (differential-interval invariance
> to batch re-phasing) — not silently dropped. **AccuracyCoin now measures
> 139/141 (98.58%)**: the v2.0.1 upstream AccuracyCoin re-sync grew the catalog
> to 146 rows / 141 assigned tests, adding two new PPU tests ("ALE + Read",
> "Hybrid Addresses") that the v2.0.0 core does not yet pass (deep
> sub-instruction PPU-fetch-corruption timing — known gaps, deferred to a future
> accuracy session). It held **100% (139/139)** throughout every v2.0.0 beta and
> on the final cut; mapper breadth stays
> frozen at **172 families** for this cut. The
> preceding **v1.10.0 "Arcade"** (2026-07-01) — the native **Libretro core**:
> `crates/rustynes-libretro` builds the `rustynes_libretro` shared library (`.so` /
> `.dylib` / `.dll` by platform, per the crate `Makefile`) that plugs the
> byte-identical cycle-accurate engine into **RetroArch** — allocation-free
> XRGB8888 (RGBA8-swizzled) video, batched interleaved-stereo `i16` audio with the
> frontend's dynamic-rate sync, WRAM/SRAM memory maps for zero-cost
> **RetroAchievements** scanning, deterministic `retro_serialize` save-states (GGPO
> rollback-ready), and a `Makefile` cross-compile translation layer staged for
> upstream `libretro-super`. Plus the **egui 0.34.3 → 0.35.0** dependency-tier
> refresh (no wgpu/winit movement) and the iOS release-workflow signing-secret gate
> fix. Additive / host-only: the deterministic core / chip crates are untouched, so
> byte-identity + **AccuracyCoin 139/139** hold. The
> preceding **v1.9.9 "Workshop"** (2026-06-26) — the iOS creator / power-tools
> release + the final pre-Timebase readiness gate: a **Cheats** editor (Game Genie +
> raw-RAM peek/poke), a FOSS-build-gated **read-only debugger inspector** (CPU regs /
> disassembly / hex / frame-step), a touch **TAStudio piano-roll**, **foreign movie
> import** (`.fm2`/`.bk2`/`.fcm`/`.fmv`/`.vmv` → native `.rnm`), host-side **audio
> depth** (5-band EQ / panning / reverb / crossfeed, bypassable to bit-identical), and
> host-side **symbol maps** (`.sym`/`.mlb`/`.nl`). The bridge gains only **additive
> forwarding fns** over existing core APIs — the deterministic core / chip crates are
> untouched, so byte-identity + **AccuracyCoin 139/139** hold. Mobile ROM loading is
> **iNES / NES 2.0 only** (FDS / NSF + 20-band EQ + `.dbg` source maps are post-v2.0.0
> carryovers). This release shipped a full readiness pass (host gates, security audit,
> gap analysis, completeness-critic) — see `docs/ios-v1.9.9-readiness.md`. The
> preceding **v1.9.8 "Horizon"** — the iOS store-readiness pass: **accessibility**
> (VoiceOver / Dynamic Type / high-contrast + Okabe-Ito palettes), **EN/ES i18n**,
> **ReplayKit** capture, opt-in **Game Center** sign-in, a **§4.7 +
> `PrivacyInfo.xcprivacy`** disclosure pass, and the **dormant StoreKit / `foss`-vs-
> App-Store seam** (the real split is v2.1.0 per ADR 0027). The preceding **v1.9.7 "Relay"** —
> **room-code (CGNAT/TURN) netplay**, **robust GameController hot-plug**, and opt-in
> **iCloud save-state sync** (CloudKit). The preceding **v1.9.6 "Link"** — a **Lua**
> console,
> **RetroAchievements** (Keychain login + hardcore + unlock-toast HUD + list), and
> **direct-IP / LAN netplay** (the rollback `npAdvanceFrame` loop). The preceding
> **v1.9.5 "Curator"** — TAS `.rnm`, `.pal`
> palettes, `.zip` ROMs, a per-game overrides DB, **HD-pack** load + composited-HD
> rendering (a `cfg(ios)` `rustynes-ios` HD path), and iCloud config sync. The
> preceding **v1.9.4 "Lens"** — the full **`wgpu→Metal` shader stack**
> (None/Scanlines/CRT/NTSC/**Bisqwit**) with per-filter **shader-param controls**,
> **ProMotion** pacing, and a hardened surface lifecycle. The preceding
> **v1.9.3 "Workshop-lite"** — an organized
> native **Settings sheet**, **4 save-state slots per ROM** (SHA-256-keyed `.rns`),
> an in-game **pill menu**, first-run **onboarding + About**, iPad multitasking
> polish. The preceding **v1.9.2 "Input"** — a true
> **multi-touch** on-screen pad (a `UIView` responder over all active touches,
> replacing the v1.9.0 single `DragGesture`), a faithful **Android-parity NES-001**
> render with **glyph-identical bundled fonts**, responsive sizing, **GameController
> P1–P4** + remap, and optional haptics. The preceding **v1.9.1 "Patch"** — a
> TestFlight **build-refresh
> cron** (builds expire 90 days after upload) + a **dormant freemium-gate scaffold**
> (`ios/RustyNES/Entitlements.swift`, fully unlocked through v1.9.x; the
> present-but-inert seam the v2.1.0 monetization wiring drops into). The preceding
> **v1.9.0 "Sunrise"** (2026-06-25) — the first **iOS / iPadOS**
> release: the foundation slice of the v1.9.0 → v1.9.9 TestFlight train (mirroring
> the Android v1.8.0 → v1.8.9 arc), a native **SwiftUI** shell over the
> byte-identical core via the shared `rustynes-mobile` UniFFI bridge. New crate
> **`rustynes-ios`** (the wgpu→Metal + cpal-CoreAudio shim, the Apple analogue of
> `rustynes-android`) + the `ios/` SwiftUI app + the xcframework build + a
> tag-gated `macos-latest` TestFlight CI + fastlane; ADRs **0026** (host + shim) /
> **0027** (distribution + §4.7). The iOS code is a host shell off-device
> (`#[cfg(target_os = "ios")]`), so it is **additive / off-by-default** — the
> shipped / native / `no_std` / wasm core stays byte-identical (only
> `CARGO_PKG_VERSION` moves 1.8.9 → 1.9.0) and AccuracyCoin holds 100% (139/139).
> Lua / RetroAchievements / netplay are deferred to the v1.9.x train; distributed
> as interim **TestFlight** (App Store deferred to v2.1.0). The preceding
> **v1.8.9 "Backlog"** (2026-06-25) — the carryover / backlog
> increment on the **v1.8.x** line, landed as a phased beta train: creator tooling
> (a desktop Virtual Pad, TAStudio Input Macros, A/V-codec depth, an FDS Firmware
> Manager, a multi-disk side-selector, a headless batch runner, optional SQLite
> userdata), debugger depth (BasicBot save-state-anchored input search + detachable
> multi-viewport panels + the RetroAchievements leaderboard-trackers HUD), **full
> Mesen2 HD-pack parity** (the Legend-of-Zelda texture-mapping fix taken to every
> HD-pack form, including `<addition>` and the CHR-RAM tileData-hash conditions), a
> Game Genie database, NSF waveform-viz depth, mapper breadth **168 → 172 families**
> (m193/204/221/299 + ~35 UNIF board aliases), plus the 13-PR Dependabot
> consolidation and the dormant `rustynes-monetization` build-out. All additive /
> off-by-default, so the shipped / native / `no_std` / wasm core stays byte-identical
> and AccuracyCoin holds 100% (139/139). The preceding **v1.8.8 "Atlas"** (2026-06-20)
> was the Google-Play-launch +
> Android-native-excellence increment on the first **platform** (not accuracy) release
> line **v1.8.0 "Android"**: a complete Android app (shared `rustynes-mobile` UniFFI
> bridge + `rustynes-android` platform crate + a Jetpack Compose shell; audio /
> touch+gamepad input / save-states+SRAM+recent-ROMs / pause-FF-mute /
> foldable+immersive UI; a freemium $2.99 Play unlock + an 8-minute demo — interim
> builds are full-featured via `PLAY_BUILD=false`), verified on a Galaxy Z Fold 7
> (SMB / Zelda / AccuracyCoin). **v1.8.8** modernizes the toolchain to the **Android 16
> / API 36 target mandate** (AGP 9.2.1 / Gradle 9.4.1 / compileSdk 37 / targetSdk 36,
> Compose BOM 2026.06) and lands **adaptive / foldable / TV** layouts + modern UX
> (edge-to-edge, predictive back, splash), **Material You** + **EN / ES** i18n, a
> **box-art ROM library** (SHA-256-keyed grid + libretro / TheGamesDB scrapers +
> Keystore AES-256-GCM secrets), a **performance** pass (a `:baselineprofile` module,
> R8 full-mode keeps, Compose stability), **capture / share** (screenshot + MP4) +
> **platform surfaces** (PiP, a Quick-Settings tile, app shortcuts, a Glance widget),
> **Android TV / Leanback** + **accessibility** (high-contrast + Okabe-Ito colorblind
> palettes), and the **Google-Play-integration readiness** — Play Games cloud-save
> Snapshots, achievements / leaderboards (distinct from RetroAchievements), Play
> Integrity, and in-app update / review — **all default-off** (`PGS_ENABLED` /
> `PLAY_INTEGRITY_ENABLED`) until the maintainer wires the Play projects. All
> presentation / Gradle-side; AccuracyCoin holds 100% (139/139) on host CI. The
> **sideload** build ships to GitHub Releases now; the **Google Play production**
> promotion is a maintainer step gated on a batched on-device pass. **v1.8.7** completed
> the connectivity work: **CGNAT / TURN room-code netplay** (a `NatConnect` STUN /
> hole-punch / `PublicAddr`-signaling / TURN-relay-fallback orchestrator + room-code
> `np_host_room` / `np_join_room` + an "Online (room code)" UI; loopback / mock-verified,
> live cross-NAT a maintainer carryover), **robust hardware controllers** (the
> `onGenericMotionEvent` fix for analog-stick / d-pad-as-HAT pads + per-port P1–P4
> masks + hot-plug + remapping + turbo), a **controller-aware UI** (auto-hide the
> on-screen pad + a Guide / Start+Select menu), and **Chromecast prep** behind a
> default-off `CHROMECAST_ENABLED` flag. **v1.8.6** brought **Lua** (the sandboxed desktop engine over the bridge,
> made `Send`), **RetroAchievements** (login + hardcore + unlock toasts + a `.rap`
> sidecar; the new shared `rustynes-ra` crate, `ureq` + `rustls` + `ring` TLS), and
> **direct-IP / LAN netplay** (GGPO rollback over `rustynes-netplay`), plus an Open /
> Close ROM toggle. **v1.8.5** adds custom
> **`.pal` palettes**, compressed **`.zip` ROM loading** (extract the first NES entry,
> like desktop), **Bisqwit composite NTSC on the GPU** (desktop WGSL shared via
> `rustynes-gfx-shaders`, an `R16Uint` palette-index pipeline fed by the bridge),
> **TAS `.rnm` movies** (record / play / save via SAF), a **per-game settings DB**
> (each game reopens with its last video filter, keyed by ROM SHA), and **HD-packs** —
> the HD-pack loader + compositor + HD audio **extracted into a new shared
> `rustynes-hdpack` crate** (the core is `#![no_std]`) so the bridge composites the
> upscaled picture (Bitmap path). **v1.8.4** brought the native **wgpu `SurfaceView`
> renderer** reusing the desktop CRT / scanline / LMP88959-NTSC shaders (shared via
> `rustynes-gfx-shaders`) with per-filter tuning sliders, on top of v1.8.3's authentic
> NES-004 controller + casting + polish and v1.8.2's multi-touch pad. The pure-Rust
> core is **byte-identical on ARM** (additive host only), so **AccuracyCoin holds
> 100% (139/139)** and every desktop gate is unchanged. Built on
> the patches **v1.8.1** / **v1.7.1** on **v1.7.0 "Forge"** (framed below). v1.7.1's seven fixes: #1 ROM-close GPU abort
> (skip the `write_texture` upload on a pixel-slice length mismatch; `close_rom`
> presents a clean blank frame) · #6 clean pause/unpause (pacing-timer
> `break_phase()` reset on resume + a sticky audio pause gate → no `produced_max_ms`
> spike, zero underruns) · #4 Documentation-pane overhaul (word-wrap at any UI scale,
> collapsible multi-level sidebar tree with intra-doc links, `[Unreleased]` ordered
> last) · #7 exhaustive README rewrite · #2 Tools-menu NSF Player / Pixel Inspector
> icon glyph swap (dodges egui's Ubuntu-Light `fi`/`fl` PUA-ligature collision) · #5
> removed the vestigial "Show Debugger" checkbox + dead plumbing · #3 HD-pack tile
> substitution now applies in the debugger / tool render branch.
>
> **v1.7.0 "Forge"** (released 2026-06-19) — the "writable +
> programmable tools" release, MAXIMAL scope (full A–H + an H1–H9 reach wave) on the
> current dot-lockstep scheduler. **beta.1** (#134, F accuracy hardening + G1 ASIC
> mappers 150→**168** + H7 measure-first perf) · **beta.2** (#135, A editing-capable
> tools + C debugger depth) · **beta.3** (#136, B scriptable TAStudio with Lua parity +
> E host IPC/automation behind `script-ipc`) · **beta.4** (#137, D rewind/Zwinder +
> G2/G3 expansion-audio + G4 movie-import + G5 HD-Pack Builder) · **beta.5 wave-1**
> (HD-pack loader real-Mesen-format fix #56 / ADR 0018; UI overhaul #51/#52/#53/#55) ·
> **beta.5 wave-2** (#142 browser-RA finish + RA HUD + spectator netplay; #143
> coverage-harness `.zip`/`.7z`/`.fds` discovery; #144 per-game `<rom>.json`
> overrides + DIP editor + lag counter; #145 audio depth; #146 web/wasm parity; #147
> i18n; #148 `full` build + `cargo full-run`). Additive / off-by-default → byte-identical with
> features off; **AccuracyCoin 100% (139/139)** held at every beta. New default-off
> feature flag `script-ipc`; new ADRs 0016 (IPC) · 0017 (HD-Pack Builder) · 0018
> (HD-pack real-Mesen format) · 0019 (per-game config overlay) · 0020 (mono→stereo
> output widening) · 0021 (File System Access fallback) · 0022 (settings share-link) ·
> 0023 (i18n string-catalog).

**The v2.0.0 "Timebase" baseline** — the MAJOR-boundary release beneath the current
v2.1.x "Fathom" line — was the one-clock, every-cycle-bus-access
scheduler rewrite (ADR 0002 / ADR 0029: a single canonical cycle counter, every
CPU cycle a real bus access, split-around-the-access PPU catch-up, replacing the
old five-counter dot-lockstep model; RustyNES's designated MAJOR-boundary release,
so `.rns` save-state and `.rnm` movie format epochs bump per ADR 0028; landed
across five betas + rc.1, PRs #217-223, with Vs. `DualSystem` dual-console support
added in beta.5; the R1/R2 MMC3 IRQ-timing residual is by-design-deferred beyond
this release per ADR 0002's mechanism-level finding; AccuracyCoin 139/139 held
throughout, mapper breadth frozen at 172 families), on the cycle-accurate v1.0.0
production core. The preceding **v1.10.0 "Arcade"** was the native **Libretro
core** release —
`crates/rustynes-libretro` builds the `rustynes_libretro` shared library (`.so` /
`.dylib` / `.dll` by platform) that plugs the byte-identical engine into RetroArch:
XRGB8888 swizzled video, batched `i16` stereo audio with dynamic sync, WRAM/SRAM
memory maps for RetroAchievements, deterministic `retro_serialize` save-states /
rollback, and a `Makefile` cross-compile layer staged for upstream `libretro-super` —
plus the egui 0.34.3→0.35.0 dependency-tier refresh; additive/host-only, core
byte-identical, AccuracyCoin 139/139; the preceding **v1.9.9 "Workshop"** was the iOS creator / power-tools release + the
final pre-Timebase readiness gate — a Cheats editor, a FOSS-gated read-only debugger
inspector, a touch TAStudio piano-roll, foreign movie import, host-side audio depth,
and symbol maps, framed in the blockquote above (additive bridge forwarding fns only);
the preceding **v1.9.8 "Horizon"** was the iOS store-readiness pass
(accessibility, EN/ES i18n, ReplayKit, Game Center, the §4.7 + privacy-manifest pass,
and the dormant StoreKit / foss-seam); the preceding **v1.9.7 "Relay"** was the connectivity-completion
release (room-code CGNAT/TURN netplay, controller hot-plug robustness, iCloud
save-state sync via CloudKit); the preceding **v1.9.6 "Link"** was the iOS
connectivity release (Lua console,
RetroAchievements, direct-IP / LAN netplay); the preceding **v1.9.5 "Curator"** was the
iOS power-user / library release (TAS `.rnm`, `.pal` palettes, `.zip` ROMs, a per-game
overrides DB, HD-pack, iCloud config sync), **v1.9.4 "Lens"** the renderer-completion
release (full wgpu->Metal shader stack + ProMotion pacing + hardened surface
lifecycle), **v1.9.3 "Workshop-lite"** the settings / save-state-slots / onboarding
release, **v1.9.2
"Input"** the input release (multi-touch pad, Android-parity NES-001 render +
glyph-identical fonts, GameController P1–P4 + remap, haptics), **v1.9.1 "Patch"** an
iOS-line patch (TestFlight build-refresh cron + a dormant freemium-gate scaffold), and
**v1.9.0 "Sunrise"** the first iOS /
iPadOS release (a native SwiftUI shell + the new `rustynes-ios` Metal/CoreAudio shim);
on the **v1.8.x "Android"** line **v1.8.9 "Backlog"** was the
carryover creator-tooling / debugger-depth / full-HD-pack-parity / mapper-breadth
increment, and **v1.8.8 "Atlas"** the Google-Play-launch-readiness increment, on the
**v1.8.x "Android"** platform line. The desktop-feature baseline below traces
back through **v1.7.1 — a bugfix / polish patch on v1.7.0 "Forge"** (the
writable + programmable tooling, accuracy, mapper-breadth, and reach feature release
it patches, described below). A
NES/Famicom emulator with the Mesen2 / higan / ares accuracy bar, shipped as a
polished desktop application and an Android app. **AccuracyCoin 100.00% (139/139)**, the 60-ROM
`external_real_games` + 52-entry `external_extended` oracles byte-identical,
nestest 0-diff. v1.7.0 makes the tools *writable* and *programmable* — A
editing-capable tools (palette/nametable/CHR/OAM writeback + iNES/NES 2.0 header
editor + inline 6502 assembler), B scriptable TAStudio (`tastudio.*` Lua + full
Lua parity), C Mesen2-class debugger depth (CallstackManager + step modes,
MemoryAccessCounter + uninit-read, ca65/cc65 `.dbg` source maps), D rewind
(HistoryViewer + Export-Last-30s `.rnm` + Zwinder XOR-delta+LZ4 greenzone), E host
IPC/automation behind the off-by-default `script-ipc` feature, F accuracy hardening,
G expansion-audio + movie-import + HD-Pack Builder, plus the H1–H9 reach wave
(browser-RA finish + RA HUD, spectator netplay, audio depth, per-game `<rom>.json`,
i18n, web/wasm parity, a `full` native build) — and takes mapper breadth to **168
families**. All additive/off-by-default, so with the new features off the
shipped/wasm/`no_std` code paths are byte-identical to v1.6.0 (the only build delta
is the embedded `CARGO_PKG_VERSION` string bumping 1.6.0 → 1.7.0) and AccuracyCoin
holds 100% (139/139). The preceding v1.6.0 "Studio" landed the TAStudio piano-roll
editor, `.fm2`/`.bk2` movie interop, Mesen2-class debugger depth, off-axis-accuracy
verification, mapper breadth to **150 families**, FDS-proper, A/V recording, HD
audio, and a shader/filter ecosystem. The preceding v1.5.0 "Lens" lands eight
additive workstreams: **A** debugger
visualization (an Input Miniatures overlay, a graphical PPU event-viewer heatmap,
a per-scanline trace viewer, an HD-pack per-pixel inspector — behind
`debug-hooks`/`hd-pack`); **B** Lua dev/TAS API depth (memory peek/poke/range,
cart/system queries, in-memory save-state scripting, breakpoint/symbol hooks,
bundled example scripts — gated like `emu.write`); **C** creator/TAS tooling (a
TASVideos compatibility pass, replay/TAS-window polish with device topology +
timebase, an NSF waveform scope); **H** frontend pacing & audio-sync perf
(triple-buffer framebuffer handoff, a pacer stall phase-break, audio DRC/buffer
tuning, GPU pass timing `gpu_ms`, perf-log↔panel parity + a perf-capture
regression gate, and a bit-identical allocation-only rewind-keyframe-cache core
tweak); **I** a native-UI overhaul + in-app Documentation pane (clipboard
screenshot fix, frame-advance/fast-forward fix, Settings polish, a Cheats game-DB
picklist, Input Display colors, a ROM-DB standalone-open fix, RA-to-status-bar, a
deeper Mapper panel, a Keyboard-Shortcuts player selector, Help → Documentation);
**D** UX polish (a named-palette editor, per-side overscan WYSIWYG, an
"Enhancements" settings group with sprite-limit-disable/overclock staged-but-inert
pending v2.0 per ADR 0002, device-config controls); **E** accessibility
(configurable UI scaling, high-contrast + Okabe-Ito colorblind themes,
keyboard-only menu nav with Esc-closes-modal); **F** mapper breadth 113 → **123
families** (+10 BestEffort: m40/81/95/112/137/156/162/178/244/250, honesty-gated);
and **G** casual-mode browser RetroAchievements scaffolding (ADR 0015) behind an
off-by-default `browser-cheevos` feature — an emcc-built rcheevos wasm side
module, structural casual-only gating, an auth-proxy contract/stub, and a loud UI
caveat, with live-browser verify, proxy deploy, and trampoline marshalling left as
maintainer-manual carryovers. Every addition is additive / off-by-default, so the
shipped / native / `no_std` / wasm builds stay byte-identical and AccuracyCoin
holds 100% (139/139); the accuracy residuals all converge on the future v2.0
fractional-master-clock refactor (ADR 0002).

**v1.7.0 "Forge" — the writable + programmable tooling / accuracy / mapper-breadth /
reach release.** Like every release on the v1.0.0 core it is additive /
off-by-default: with the new features off, behaviour is unchanged — the native /
`no_std` / wasm code paths are byte-for-byte the same as v1.6.0 (the lone build
difference is the embedded `CARGO_PKG_VERSION` string) — and AccuracyCoin holds
**100% (139/139)**. *Shipped (full A–H + the H1–H9 reach wave):* **F** accuracy
hardening (battery-save round-trip oracle + length-halt/reload + DMC even/odd-defer
audit pins + off-by-default PPU extra-scanlines overclock); **G1** reusable-ASIC
mappers 150 → **168 families** (FK23C/COOLBOY/MINDKIDS/Sachen/Waixing/Kaiser,
BestEffort honesty-gated); **A** editing-capable tools (palette/nametable/CHR/OAM
writeback editors gated like `emu.write`, iNES/NES 2.0 header editor, inline 6502
assembler); **C** debugger depth (CallstackManager + step modes, a
MemoryAccessCounter + uninit-read, ca65/cc65 `.dbg` source maps); **B** scriptable TAStudio (`tastudio.*`
Lua + full Lua parity); **E** host IPC/automation (`comm.*`/`client.*`/`userdata.*`)
behind the off-by-default `script-ipc` feature (host-mediated sandbox, ADR 0016);
**D** rewind (HistoryViewer + Export-Last-30s `.rnm` + Zwinder XOR-delta+LZ4
greenzone); **G2/G3** expansion-audio (NSF router reusing VRC6/7/FDS/MMC5/N163/5B;
MMC5 audio); **G4** movie import (`.fcm`/`.fmv`/`.vmv`; `.fm2`/`.bk2` export hashing);
**G5** HD-Pack Builder (ADR 0017) + the HD-pack loader real-Mesen-`<tile>`-format fix
(ADR 0018); a UI overhaul (consolidated Input Display, menu/status-bar
modernization + full icon coverage, Documentation-pane polish, backtick toggle of
the status-bar RA readout); and the **H1–H9 reach wave** — browser-RA finish + RA
HUD, spectator netplay + Game-Genie-encoder/`.srt`/`.tbl`, coverage-harness
`.zip`/`.7z`/`.fds` discovery (7z-bomb OOM cap), per-game `<rom>.json` config
overrides + DIP editor + lag counter (ADR 0019), audio depth (stereo
panning/Schroeder-reverb/crossfeed/output-device-picker/20-band-EQ, ADR 0020),
web/wasm parity (browser Lua/File-System-Access-API/Gamepad-API/PWA-offline/
`?settings=` share-links, ADRs 0021+0022), an i18n framework (compile-time string
catalog + language picker, English+Spanish, English byte-identical, ADR 0023), and a
`full` maximal-native-feature build + `cargo full-run` alias. The headline mapper
count is the released figure (**168**). Planning lives in `to-dos/ROADMAP.md` (the
entry point) + `to-dos/plans/`.

**v1.6.0 "Studio" — the studio / TAS-tooling / debugger-depth / accuracy-and-breadth
release.** Like every release on the v1.0.0 core it is additive / off-by-default:
with the new features off, behaviour is unchanged — the native / `no_std` / wasm
code paths are byte-for-byte the same as v1.5.0 (the lone build difference is the
embedded `CARGO_PKG_VERSION` string) — and AccuracyCoin holds **100% (139/139)**.
*Shipped (full A–I scope):* the **TAStudio piano-roll TAS editor** (editor model
A1/A3/A4 + FCEUX `.fm2` interop B1, #118; the A2 piano-roll editor, #122) + **B2**
Lua movie driving + `.bk2` interop; **J.Y. Company ASIC mappers** m90/209/211
(BestEffort, #123) + the **UNIF (`.unf`) cartridge loader** (Workstream E2, #117);
the data-driven external-coverage harness + per-mapper screenshot coverage +
BestEffort consolidation (#114/#115/#116/#125); **Lua data breadth** (memory
domains, sized reads, joypad — Workstream B3, #128); the **m30/m80/m185 blank-boot
mapper fixes** (#127); **C** Mesen2-class debugger depth (expression/conditional
breakpoints + R/W/X watchpoints + a full hex editor + RAM search); **D** off-axis
accuracy verification; **E** mapper breadth → **150 families**; **F** FDS-proper;
**G** A/V recording; **H** HD audio; **I** the shader/filter ecosystem; a **CI
paths-filter gate** for doc-only PRs (#124); the ROADMAP/to-dos refresh (#129/#130);
and README freshness (#126). The headline mapper count is the released figure
(**150**). Planning lives in `to-dos/ROADMAP.md` (the entry point) +
`to-dos/plans/`.

The preceding **v1.4.1** (a patch on
**v1.4.0 "Fidelity" — the compatibility-and-finish release**) added four BestEffort
mapper boot/decode fixes (m92 / m94 / m145 / m147) from the boot-smoke-vs-real-dumps
pass and reorganized the boot-smoke screenshot corpus to mirror the per-mapper
`tests/roms/` tier layout. **v1.4.0 "Fidelity"**
polishes accuracy (triangle ultrasonic silence, DMC-DMA ↔ controller-read conflict
verified), adds per-channel audio mixing, finishes the devtools (symbol-file
`.sym`/`.mlb`/`.nl` loading + event breakpoints), adds browser QoL (wasm `.rnm`
movie I/O + IndexedDB save-states), runs a measure-first perf pass (−8% on the
rendering-heavy bench), ships a colorful `rustynes help` TUI + styled `--help`, and
takes mapper coverage 101 → **113 families** (boot-smoke verified, with reset-vector /
decode fixes to m132/m143/m225/m226/m233/m242/m246).
Built on top of v1.3.0 "Bedrock" (toolchain + breadth) and v1.2.0
"Curator", which was a broad library / compatibility / reach release on v1.1.0
"Scriptable":
mapper coverage rises 51 → **87 families** behind a CI accuracy-tiering honesty
gate (Core / Curated / BestEffort, ADR 0011); `.zip` loading + `.ips`/`.ups`/`.bps`
soft-patching; a per-game DB + in-app ROM-Database editor; live NTSC knobs + a
composable shader stack + CRT preset bank (ADR 0013) + a default-off HD-pack loader;
Family BASIC keyboard / SNES mouse / Arkanoid-both-ports / Game Genie code DB; Lua
`onNmi`/`onIrq`/`setInput`; menu-bar contextual enable/disable, remappable shortcuts,
and Font Awesome icons; web touch controls + Power Pad + an experimental piccolo wasm-Lua
backend (ADR 0012); a turn-key netplay `deploy/` bundle; and a manual/release-only PGO
CI promotion gate. **Every addition is off-by-default or additive — the shipped /
native / `no_std` / wasm builds stay byte-identical and AccuracyCoin holds 100%.** The
SMB3 World 1-1 sprite flicker (a PPU OAM-row-corruption model bug) is fixed and Mapper
89 now models its bus conflict. (v1.1.0 added visual filters, input & peripherals,
debugger devtools, an NSF/NSFe player + 5-band EQ, and the flagship Lua engine.) The
v1.0.0 core baseline:

- **Cycle-accurate core** — PPU-master-clock lockstep scheduling at dot
  resolution, the unified per-cycle DMA engine, region-exact CPU:PPU (3:1
  NTSC/Dendy, 3.2:1 PAL), band-limited polyphase-BLEP audio synthesis.
- **51 mapper families** including MMC1-MMC5, the full VRC family (1/2/4/6/7
  with VRC6/VRC7-OPLL/Sunsoft-5B/Namco-163/MMC5 expansion audio), FME-7, and
  the Vs. System / PlayChoice-10 RGB-PPU boards.
- **Famicom Disk System** — real-BIOS `.fds` boot (user-supplied `disksys.rom`),
  read/write drive, multi-side eject/insert, 2C33 wavetable audio. **FDS-proper
  (v1.6.0 Workstream F, after puNES `fds.c`)** adds a timed disk-head position
  (a motor restart rewinds the belt-driven disk and the head re-seeks across a
  short deterministic `HEAD_RESEEK_CYCLES` not-ready window before re-reading,
  rather than teleporting to track 0), the `$4032` drive-status / auto-insert
  presentation driven by those windows, and a per-game CRC quirk table
  (`quirk_for_crc`) for titles needing extra re-seek slack. The general timed
  head-position model closes the **Kid Icarus side-B post-registration** replay
  (the BIOS re-read loop now observes the not-ready -> ready edge it waits for).
  Cycle-count-based — NOT the v2.0 master-clock axis; determinism intact.
- **Vs. System / PlayChoice-10** — 2C03/2C04/2C05 hardware RGB palettes, DIP
  switches, coin/service inputs. **Vs. DualSystem** (two-CPU/two-PPU arcade
  boards) is *detected* — from both the SHA-256 game DB and, as of v1.3.0 (D2),
  the NES 2.0 byte-13 high nibble (Vs. hardware type 5/6) — and, as of **v2.0.0
  beta.5**, genuinely *emulated* at the core level: a new `Emu::Dual` front door
  (`VsDualSystem`, `rustynes-core`) owns two complete `Nes` instances sharing a
  2 KiB WRAM mailbox and a cross-wired `$4016` bit-1/IRQ line, using a MAME
  `.share("nvram")`-style level-driven convergence model. **Vs. Balloon Fight
  boots to a correct attract-mode screen** on both consoles against the
  maintainer's own legitimately-owned MAME romset; Vs. Wrecking Crew is
  inconclusive (cross-wiring active, no confirmed title screen); Vs. Tennis and
  Vs. Mahjong remain infrastructure-only (no local sub-CPU dump). **This is
  core-and-test-harness-only** — `rustynes-frontend` still constructs `Nes`
  directly and does not yet consume `Emu::Dual`, so the feature is unreachable
  from the shipped desktop/mobile UI pending a follow-up release that wires
  dual-console rendering + 4-port input routing (see
  `docs/audit/vs-dualsystem-design-2026-06-11.md` for the original design and
  `docs/audit/vs-dualsystem-combined-dumps-2026-07-02.md` for the
  boot-verification campaign).
- **Rollback netplay** — GGPO-style 2-4 player over UDP (native) and WebRTC
  (browser), live-verified two-instance sessions. As of v1.3.0 (Workstream G1)
  the netplay panel surfaces a read-only **desync-diagnostics** section (a
  `GeraNES`-style `DesyncMonitor`): the room / input topology (who drives which
  port), the in-sync / desynced-at-frame-N status, lifetime checksum-compare +
  mismatch counts, the consecutive-mismatch counter, the most recent local-vs-
  remote CRC, and a rolling CRC-match history. It is purely observational — it
  reads the digests the session already exchanges and never feeds back into the
  rollback algorithm, so the determinism contract is intact.
- **RetroAchievements** — achievements, leaderboards, rich presence, hardcore
  mode via the canonical `rcheevos` library (opt-in, native).
- **TAS movie recording/playback** — deterministic `.rnm` record/replay with
  save-state branching; save states + rewind.
- **Performance + desktop UX** — a display-sync pacing matrix
  (`auto|display|vrr|wallclock`) with a late input latch, a lock-free audio
  ring with dynamic rate control, run-ahead (default 1), a dedicated emulation
  thread, plus a Performance panel and an **always-on egui shell**: a menu bar
  (File / Emulation / Tools / View / Debug / Help), status bar, tabbed Settings
  window, light/dark/system themes, 8:7 pixel-aspect correction, fullscreen
  (`F11`), save-state slots, a recent-ROMs list, and the surfaced tool panels
  (Cheats / Movies / Netplay / RetroAchievements / Performance) — all layered
  over the toggleable debugger overlay.
- **WebAssembly** — a browser build (`wasm-winit` / `wasm-canvas`) with an
  AudioWorklet audio path and rAF display-sync.

The determinism contract is a hard invariant: same seed + ROM + input ⇒
bit-identical framebuffer and audio (dynamic rate control is a frontend
resampler stage, never the core synthesis rate; run-ahead is frontend
orchestration of the existing snapshot/restore — the core per-frame output is
untouched). fmt / clippy (1.86 + stable + wasm32) / doc / no_std / native +
both-wasm all clean. See `CHANGELOG.md` `[1.0.0]`.

> **Engine lineage.** RustyNES v1.0.0 is the production cut of an accuracy
> program that was developed across an internal engine line (v0.9 cycle-accurate
> core → master-clock 100% → FDS → netplay → platform + RA → performance pass).
> The deep per-suite / accuracy-program history below keeps the engine's
> version markers (e.g. "the master-clock work landed in the engine's v2.0
> line") as historical anchors — they document *how* the technology was built,
> not RustyNES release numbers. The per-version notes under
> `docs/release-notes/` are retained as that engine-lineage history; the
> current RustyNES release is **v1.2.0 "Curator"** (the second feature release on
> the v1.0.0 production core; v1.1.0 "Scriptable" preceded it).

---

The remainder of this section preserves the engine-lineage milestone history
(version markers below are the engine line that produced the v1.0.0 technology,
not RustyNES releases of their own).

**Engine v2.8.0 line — "optimized performance".** A frontend +
build performance pass — the smoothest-playing, lowest-latency increment — with
**no accuracy or behaviour change to stock NES play**: AccuracyCoin
**100.00% (139/139)**, the 60-ROM `external_real_games` + 52-entry
`external_extended` oracles byte-identical, nestest 0-diff. The determinism
contract holds because dynamic rate control lives in a frontend resampler
stage (never the core's synthesis rate) and run-ahead is frontend
orchestration of the existing snapshot/restore — the core's per-frame
framebuffer + audio are untouched. Six phases: (0) a Performance panel with
produced-vs-**presented** histograms + audio health + a CSV "Logging"
checkbox; (1) a lock-free SPSC audio ring + alloc-free cpal callback + Near's
**dynamic rate control**; (2) a **display-sync pacing matrix**
(`[graphics] pacing_mode = auto|display|vrr|wallclock`) + **late input
latch**; (3) a **snapshot fast path** (36→14.6 µs) + **run-ahead** (default
1, persistent timeline byte-identical); (4) **mapper capability flags** + a
512-entry pixel LUT + **fat LTO** + SIMD-shaped loops (**−26%** on the
rendering-heavy bench, −16% nestest); (5) a **dedicated emulation thread**
(default-ON `emu-thread`, `Arc<Mutex<EmuCore>>`, lock-free `SharedInput`,
netplay-pauses-the-thread with an under-lock TOCTOU close, RA stays
winit-side) with best-effort Linux priority elevation (SCHED_RR → nice →
timer-slack, degrades silently without the `realtime` rlimit); (6) a browser
**AudioWorklet** (off the main thread, `Blob:`-URL JS, postMessage
sample/occupancy transport, the same Hermite DRC, `ScriptProcessorNode`
fallback) + **rAF display-sync** + a wasm Performance HUD. See
`docs/release-notes/v2.8.0.md` for the engine-line detail.

**v2.7.1 (released 2026-06-12) — netplay-hardening + live-verification patch.**
The headline is that GGPO-style **rollback netplay now works in real two-instance
sessions — native *and* browser — and is live-verified**. The root cause of the
remaining desync was that **`Bus::power_cycle` was not a true cold boot**: it left
`ppu_clock` (the master clock) and several other run-history fields carrying the old
phase, so two peers that had each run a *different* number of single-player frames
booted with different timing and diverged from frame 0. `power_cycle` now resets all
of those fields and **rebuilds the mapper** from the stored ROM bytes (a true cold boot
for stateful-mapper games too), so netplay is correct for all cartridge mappers; the
`power_cycle_result_is_independent_of_prior_history` determinism test is un-ignored as a
regression guard. Plus: an input-resend + cumulative-`InputAck` reliability layer (a new
600-frame two-peer test over a 25%-loss + heavy-reorder link converges byte-identically),
**one-frame-per-pace** session driving on both the native and wasm pacers, a >2-player
browser **WebRTC mesh** (2-4 players, slot-routed signaling + `WebRtcMeshTransport`), RA
fixes (login chicken-and-egg, badge lock-state via the canonical `unlocked` field, badge
images, a `RustyNES/<ver>` User-Agent), an MMC6 byte-10 high-nibble PRG-RAM fix, an
NTSC-filter WGSL crash fix (+ shader-validation tests), and Vs. DualSystem detection
groundwork. **No accuracy/behaviour change to stock NES play** — AccuracyCoin
**100% (139/139)**, the 60-ROM + 52-entry oracles byte-identical, the default build
identical to v2.7.0. See `CHANGELOG.md` `[2.7.1]` + `docs/release-notes/v2.7.1.md`.

**v2.7.0 (released 2026-06-11) — RetroAchievements + the v2.6.0 known-gap closeout.**
The headline is a full **RetroAchievements** integration (achievements, leaderboards,
rich presence, hardcore mode) via the canonical MIT `rcheevos` library, in a new
native-only, opt-in `crates/rustynes-cheevos` FFI crate + the frontend (login, panel, HUD,
toasts, hardcore gating). Plus a **Vs.-System per-game database** (DIP presets so
single-CPU Vs. games boot + exact 2C04 palettes), **deployable browser WebRTC netplay**
(a `deploy/` Docker/compose bundle + a wired wasm lobby), and a regenerated, re-sorted
**screenshot corpus** + montage. RA is opt-in/native-only and frontend-side, so stock
NES play is unchanged: AccuracyCoin **100% (139/139)**, 60-ROM + 52-entry oracles
byte-identical, the default build identical to v2.6.0. Known: RA needs a live account
to verify + is native-only; the still-black VS games are Vs. DualSystem (two CPUs/PPUs);
browser netplay needs the deploy stack hosted. See `CHANGELOG.md` `[2.7.0]` +
`docs/release-notes/v2.7.0.md`.

**v2.6.0 (2026-06-11) — Vs./PC10 RGB game-verified, +11 mappers, N-peer
netplay, working real-BIOS FDS.** A large compatibility + platform release, all gated
so stock NES is byte-identical (AccuracyCoin 100% (139/139); 60-ROM + existing-39
oracles byte-identical). Mapper 99 (Vs. System) → real in-game 2C03 RGB (VS Excitebike
/ Clu Clu Land); +11 mapper families (38→51: 32/33/48/80/82/87/89/93/151/152/184;
rustynes-mappers 398 tests); PlayChoice-10 + VS Castlevania/Pinball in RGB via clean-iNES
byte-7 0x01/0x02 detection (immune to the 0x0A corruption); N-peer UDP multi-joiner
roster handshake (3/4-player loopback == reference) + reference signaling server + wasm
WebRTC wiring (rustynes-netplay 59+1 tests); and **real-BIOS FDS boot now works** (Zelda/
Metroid/etc. across all 3 BIOS revisions — disk wire-format synthesis + a $4025 bit-3
mirroring-inversion fix; FDS 56+6 tests). Known gaps: Vs. DIP-dependence, iNES-1.0
2C03-default, Kid Icarus FDS side-B stall, Mito Koumon (m89) backdrop-only, browser
WebRTC needs deployment. See `CHANGELOG.md` `[2.6.0]` + `docs/release-notes/v2.6.0.md`.

**v2.5.0 (2026-06-11) — Vs. System / PlayChoice-10 + multiplayer & internet
netplay groundwork.** Two v3-tier platform initiatives, both gated so stock NES play
is unchanged (AccuracyCoin 100% (139/139); 60-ROM + 39-title oracles byte-identical).
**(A)** Vs./PC10 RGB-PPU support — 2C03/2C04/2C05 hardware palettes (`PpuPalette` enum;
default `Composite2C02` == the legacy path byte-for-byte), the 2C05 `$2000/$2001` swap

- `$2002` ID, NES 2.0 byte-13 detection, Vs. DIP switches + coin inputs (20 unit tests;
in-game RGB unverified — no iNES Vs. ROMs). **(B)** N-player netplay (up to 4) — N-player
`RollbackSession` (`num_players`, default 2 = byte-identical), `[PlayerInput;4]` history,
Four Score for >2, a new `MeshTransport`; +8 tests incl. a 3/4-player determinism harness
== a no-rollback reference. **(C)** Internet groundwork (scaffold+docs) — a STUN client
(RFC 5389) + hole-punch state machine, `rustynes-netplay` now compiles on wasm32 + a wasm-only
`WebRtcTransport`, `docs/netplay-webrtc.md` (real NAT traversal + browser netplay pending
external infra). See `CHANGELOG.md` `[2.5.0]` + `docs/release-notes/v2.5.0.md`.

**v2.4.1 (2026-06-11) — VRC2a (mapper 22) register-select fix.** A patch
to the v2.4.0 VRC2 fix: v2.4.0 swapped the A0/A1 register-select pins for VRC2c
(m25) but left VRC2a (m22) straight; per nesdev, VRC2a swaps A0/A1 the same way
(chip A0←CPU A1), so TwinBee 3's background tiles stayed scrambled (the sprite
slots happened to land right). Fix = `22 => (bit(1), bit(0))`; visually verified.
Isolated to m22 (m23/m25 byte-identical); AccuracyCoin 100% unchanged. See
`CHANGELOG.md` `[2.4.1]` + `docs/release-notes/v2.4.1.md`.

**v2.4.0 (2026-06-11) — Compatibility & rendering-accuracy.** A 99-title
commercial-ROM survey (the 60-ROM `external_real_games` gate + a new 39-title
`external_extended` oracle, visually verified) surfaced + FIXED two rendering bugs
the byte-identical oracle had locked into its baselines: **VRC7** (mapper 85,
Lagrange Point) rendered blank gray (unbacked `$6000-$7FFF` WRAM failed the boot
self-test → CPU spin lock; fix = WRAM backing) and **VRC2/VRC4** (mappers 21/22/23/25)
garbled in-game tiles (wrong `vrc_a_bits` A0/A1 register-line decode for submapper-0
ROMs + the VRC2a CHR `>>1` quirk). Also: mapper 119 TQROM (**39 families**), netplay
host-learns-joiner-address, a regenerated/audited 107-frame screenshot corpus + a
README showcase montage, and CI maintenance (all stable-clippy lints fixed; clean on
1.86 + current stable; `test-roms` job `--release`; `actions/checkout@v6`).
**AccuracyCoin 100.00% (139/139)** holds; the ~95 unaffected oracle games stay
byte-identical. SMB3 "sprite flashing" is **resolved** in v1.2.0 (it was the PPU
OAM-row-corruption model, not MMC3 — see `compatibility.md`). The "GxROM-66"
report is the same SMB3 title under a misattributed board label and is covered by
that fix; no separate GxROM defect reproduced. See `CHANGELOG.md` `[2.4.0]` +
`docs/release-notes/v2.4.0.md`.

**v2.3.0 (2026-06-10) — Netplay (rollback netcode).** Two-player online
via GGPO-style rollback over UDP: each peer runs the bit-deterministic core,
predicts the remote input, and rolls back + re-simulates on a misprediction (the
determinism contract guarantees the re-sim matches). New `crates/rustynes-netplay`
(`RollbackSession` + a `Transport` trait + `UdpTransport`/`NetplayConnection`;
seeded RNG, `#![forbid(unsafe_code)]`) + a native frontend (`NetplayUi` + a
"Netplay" debugger panel; host/join). 22 rustynes-netplay tests (incl. a 600-frame
two-peer harness proving both peers == a no-rollback reference + a real-UDP
loopback) + 7 frontend tests. Note: **Native-only** (UDP); 2-player; the single-player
path is byte-for-byte unchanged. **No accuracy/behaviour change** — AccuracyCoin
**100.00% (139/139)**, oracle 60/60 byte-identical (netplay is a new crate + a
native frontend path; no core/chip change). See `CHANGELOG.md` `[2.3.0]` +
`docs/release-notes/v2.3.0.md`.

**v2.2.0 (2026-06-10) — Famicom Disk System (FDS).** v2.1.0 detected
`.fds` images and refused them; v2.2.0 plays them — the FDS RAM adaptor (iNES 20:
PRG-RAM + CHR-RAM + user-supplied `disksys.rom` BIOS), the register map + 16-bit
timer IRQ, the disk **read + write** drive, multi-side **eject/insert**, writable
`.fds.sav` persistence, and the **2C33 wavetable audio** (behind `mapper-audio`).
API `Nes::from_disk` (+ frontend `.fds` loading, BIOS prompt, F9 side-swap).
Workspace `--features test-roms`: 876 → **937 strict + 16 ignored** (56 FDS unit
tests). Note: **The BIOS is never committed (Nintendo copyright); real-BIOS FDS boot
is unverified in CI by design** — the device + audio are unit-tested, but in-game
boot needs a user `disksys.rom`. **No accuracy regression** — AccuracyCoin
100.00% (139/139), 60-ROM oracle 60/60 byte-identical (FDS is a separate
parse/construct path). See `CHANGELOG.md` `[2.2.0]` + `docs/release-notes/v2.2.0.md`.

**v2.1.0 (2026-06-10) — coverage + expansion.** +13 mapper families
(25 → **38**), the Arkanoid Vaus paddle + Zapper light gun (opt-in per-port
`InputDevice` overlay; default controller/Four-Score path byte-identical), and
**+195 strict test-ROM coverage** (681 → 876 strict) by wiring previously-unrun
`nes-test-roms/` suites; `ppu-state-trace` compiles again (RECORD_SIZE 111→113).
Mapper verification is spec (nesdev) + register/IRQ unit tests + boot-smoke (no
behavioral fixtures exist for the new boards). Documented expected-fails:
`apu_reset` len_ctrs_enabled + 4017_written (rustynes-apu reset semantics),
`mmc3_test_2/4` #3 (ADR-0002 axis). See `CHANGELOG.md` `[2.1.0]` +
`docs/release-notes/v2.1.0.md`.

**Scheduler (since v2.0.1) — the R1 master clock is the ONLY scheduler** (the
legacy integer-lockstep path was removed; the `mc-r1-*` flags no longer exist).
One validated configuration:

- **R1 master clock — AccuracyCoin 100% (141/141)**: built
  unconditionally (the `mc-r1-full-cpu` umbrella + its closure are now permanent
  code, not feature flags). As of **v2.0.3**, all 141 assigned tests pass — the two
  upstream PPU tests "ALE + Read" and "Hybrid Addresses" added by the v2.0.1 catalog
  re-sync now pass via the promoted genuine two-dot 2-cycle-ALE fetch model (ADR 0030;
  previously honest 139/141 gaps). nestest 0-diff; blargg cpu_interrupts_v2 **5/5 strict**;
  SH\* 6/6; ppu_vbl_nmi 10/10; visual_regression 7/7; **region-exact CPU:PPU** (3:1
  NTSC/Dendy, 3.2:1 PAL — region_timing 4/4); 60-ROM oracle 60/60 (byte-identical
  across the v2.0.1 refactor); save-state determinism round-trips green (CPU snapshot
  v2 + the `ppu_clock`/`dma_mc_consumed` pair, APU/BUS trailing tails, PPU snapshot
  v5 as of v2.0.3). R1 audio coverage: AccuracyCoin APU 100% + apu_test 8/8 + dmc_dma 5/5 + the
  60-ROM oracle `audio_fnv1a64`. (The bbbradsmith `audio_tests` framebuffer corpus
  was legacy-only — it asserted pre-R1 DMC audio hashes — and was removed with the
  legacy path.) See `docs/audit/v2.0-phase7f-r1-default-promotion-2026-06-10.md`
  for the R1 promotion (v2.0.0) and `CHANGELOG.md` `[2.0.1]` for the legacy removal.

---

**RustyNES version of record:** **v1.2.0 "Curator"** (the second feature release on the v1.0.0 production cut; v1.1.0 "Scriptable" preceded it). The
milestone paragraphs that follow are engine-lineage history — they describe
increments of the internal engine line (the "niceties", "frontend-polish",
"TAS", "WebAssembly" etc. milestones) that together produced the v1.0.0
technology, and keep the engine's version markers as historical anchors.

**Engine v1.7.0 line — niceties milestone**: Four
Score 4-player support (core `$4016`/`$4017` 24-read multiplex + signature, opt-in
and off by default = byte-identical two-controller reads + a P3/P4 rebind UI),
GameShark-style raw RAM cheats (a `Nes::poke_ram` applied caller-side after
`run_frame`, alongside the engine v1.6.0 Game Genie support), and an in-app
graphics/audio/rewind settings panel. **Additive, independent of the deferred
engine v2.0 master-clock axis** — AccuracyCoin held **90.65%**, oracle 60/60, sacred
trio + B4 byte-identical, determinism preserved. Workspace `--features
test-roms`: **702 strict + 10 ignored** (+14 over the engine v1.6.0 line). See `CHANGELOG.md`
`[1.7.0]`.

**Previous:** **v1.6.0** (released 2026-05-25) — **frontend-polish milestone**
(the v2.0.0 plan's original v1.5.0 content, deferred when Phase 7 took that slot;
see `docs/audit/gap-analysis-remediation-plan-2026-05-25.md` §2). **Additive
only, independent of the deferred v2.0 master-clock axis** — AccuracyCoin held at
**90.65%** (126/139), 60-ROM oracle 60/60, sacred trio + B4 byte-identical, the
determinism contract preserved. Landed: Game Genie cheats (core
`rustynes-core/src/genie.rs` runtime overlay, off by default + a debugger cheat panel
with per-ROM persistence); in-app gamepad rebinding UI (config-driven
`[input.gamepad1/2]`, P2 keyboard rows, axis-as-dpad; default reproduces the old
Xbox layout); browser (wasm) `.rnm` movie I/O + localStorage save-states; a
non-flaky frame-time regression CI gate + a rendering-heavy `flowing_palette`
bench; and the `x86_64-apple-darwin` release-target sunset (ADR 0009). Workspace
`--features test-roms`: **688 strict + 10 ignored** (+27 over v1.5.0). See
`CHANGELOG.md` `[1.6.0]`.

**Previous:** **v1.5.0** (released 2026-05-24) — **Phase 7: Nesdev Accuracy
Hardening**. Coverage + region validation + developer ergonomics + documented
scope closure (the ROADMAP's genuinely-skipped Phase 7). **Additive only** —
AccuracyCoin held at **90.65%** (126/139), 60-ROM oracle 60/60, sacred trio + B4
byte-identical. Landed: blargg `instr_misc`/`instr_timing`/`cpu_reset` corpus
wired (+8 strict); seeded power-on RAM randomization developer mode
(`Nes::from_rom_with_power_on_seed`); NMI/IRQ B-flag + `$4015` open-bus guards;
automated PAL/Dendy timing gates; VRC2/4 + NINA-001 submapper fixtures
(replacing the rotted `vrc24test`); and `compatibility.md` scope closure (FDS
plan, Vs/PC10, PPU variants, input devices, long-tail policy). Deferred to v2.0
(master-clock axis): C1 IRQ-sample, `$2002` sub-cycle, SH\* internal-bus,
stale-shifter, FDS code, PAL 3.2:1 CPU:PPU ratio. Workspace `--features
test-roms`: **661 strict + 10 ignored**. See `CHANGELOG.md` `[1.5.0]` +
`docs/audit/phase-7-*`.

**Previous:** **v1.4.0** (released 2026-05-24) — **TAS movie recording &
playback**. Frame-perfect deterministic input recording + replay with
save-state branching. **Core** (`rustynes-core`, no_std): a versioned binary
`.rnm` format (ADR 0008 — `RNESMOV1` header + ROM SHA-256 + region +
optional `.rns` save-state start point + raw per-frame `[p1,p2,expansion]`
input stream), `MovieRecorder`/`MoviePlayer`, `serialize`/`deserialize`; a
movie = a reproducible start point + the input stream, replay re-derives
every pixel/sample bit-for-bit. **Frontend**: record/play/branch hotkeys
(`F6`/`F7`/`F8`, rebindable), a `MovieUi` state machine in the frame loop
(record captures held input; playback overrides + auto-stops), native
`.rnm` save/load (`rfd` + `<data_dir>/movies/`), read-only egui REC/PLAY
overlay indicator. **No API break** — the only core addition is the
additive read-only `Nes::buttons` getter; `run_frame` is byte-for-byte
unchanged → oracle 60/60, AccuracyCoin 90.65%, B4 + sacred trio
unaffected. Determinism **proven** by byte-identical round-trip tests
(framebuffer + audio FNV-1a + cycle count) on a committed CC0 ROM.
Workspace `--features test-roms`: **636 strict + 8 expected-fail
`#[ignore]`** (616 at v1.3.3 + 13 TAS-core + 7 TAS-frontend; the 8
ignored = 5 expected-fail strict probes + 3 rustdoc doc-test examples);
under `--features test-roms,dmc-get-put-scheduler`: **637 strict + 9
ignored**. wasm movie file I/O is a follow-up (UI compiles + no-ops on
wasm; native is the v1.4.0 TAS surface). Clean-room from Mesen2
`Core/Shared/Movies/` + FCEUX `.fm2` + TetaNES `.replay` + nesdev TAS. See
`docs/adr/0008-tas-movie-format.md` + `CHANGELOG.md` `[1.4.0]`.

**Previous:** v1.3.3 (released 2026-05-24) — bug-fix patch closing two
wasm/GitHub-Pages issues (wasm idle-path `Poll`→`Wait` + rAF heartbeat
fixing the v1.3.2 stutter/freeze regression; WebGL2 UNORM palette fix) +
a native sleep-overshoot pacing tweak. Native pixel-identical. See
`CHANGELOG.md` `[1.3.3]`.

**Earlier:** v1.3.2 (legacy keycode aliases fixing post-migration dead
input + first wasm rAF attempt), v1.3.1 (left-edge BG attribute-shifter
palette fix + native stutter fix + `config.toml` migration; PPU save-state
v1→v2; AccuracyCoin-neutral; MM3 MMC3 shear confirmed not-a-regression,
deferred to v2.0). See `CHANGELOG.md` `[1.3.2]` / `[1.3.1]`.

**Older:** v1.3.0 — **WebAssembly target** (`wasm-winit` default +
`wasm-canvas` embed; GitHub Pages deploy; CI wasm clippy + 5 MiB size
budget). v1.2.0 — DMC DMA get/put scheduler (ADR 0007). v1.1.0 — VRC7
OPLL FM audio (ADR 0006). v1.0.0 final — AccuracyCoin 90.65% (126/139).
See `CHANGELOG.md` for each.

Phase 6 v1.0.0-final closures landed: Phase 1a/b/d (internal/external
bus split + SH* unstable stores + `$4015` bit-5 Open Bus #9), Phase 0
(Mesen2 `EventType::PpuCycle` patch), Phase 3a/b (sprite-eval base
from OAMADDR + OAM-corruption row tracking).  4 Track C1 IRQ-timing
residuals (`cpu_interrupts_v2/{2,3,5}` + `mmc3_test_2/4` sub-test #3)
deferred to **v2.0** with documented architectural rationale —
empirical falsification of Option (a) PPU re-baseline in Session-29
demonstrated that closure requires the master-clock-precise
scheduling refactor (Option b), targeted for v2.0.

**Source of truth for:** per-test-ROM-suite pass count, mapper coverage,
feature flag state.
**Updated:** 2026-05-25 (doc-hygiene reconciliation pass — see `docs/audit/gap-analysis-remediation-plan-2026-05-25.md`)

This page replaces the prior practice of scattering pass/fail counts
across `CLAUDE.md`, `CHANGELOG.md`, and the per-sprint `to-dos/*.md`
files. When those files cite numbers, they should link here. The
gap-analysis remediation plan that produced this file lives at
`/home/parobek/.claude/plans/linked-puzzling-sutherland.md` (Track D5).

---

## Test ROMs

Counts are with `--features test-roms`. "Strict pass" means
`assert_eq!(status, 0, ...)`; surprise failures fail CI loudly.
"`#[ignore]` expected-fail" tests are accessible via
`cargo test --features test-roms -- --ignored` and have companion
`*_currently_fails` probes that fail loudly on either surprise-pass
("please flip to strict") or failure-shape change ("please re-diagnose").
"Smoke" tests assert that the emulator advances the frame counter
without panicking (no `$6000` status protocol).

| Suite | ROM count | Strict pass | `#[ignore]` expected-fail | Smoke | Notes |
|-------|-----------|-------------|---------------------------|-------|-------|
| `nestest` | 1 | 1 | — | — | PC=$C000 automation; compared against ~8,991 lines of Nintendulator-generated golden log. Zero-diff. |
| `instr_test_v5` | 18 | 18 | — | — | All 16 sub-ROMs + `all_instrs` + `official_only` aggregates. `all_instrs` / `official_only` exercise MMC1 banking. |
| `instr_misc` | 5 | 5 | — | — | **Vendored + wired in Phase 7 (T-71-003).** blargg aggregate + 4 sub-ROMs (`01-abs_x_wrap`, `02-branch_wrap`, `03-dummy_reads`, `04-dummy_reads_apu`). MMC1. All strict-pass on the **full** lockstep `Nes` (`run_nes_blargg`) — `04-dummy_reads_apu` needs the real APU and cannot pass on the CPU-only `BlarggBus`. |
| `instr_timing` | 2 | 2 | — | — | **Vendored + wired in Phase 7 (T-71-003).** blargg `1-instr_timing` + `2-branch_timing`. MMC1. Both strict-pass on the full `Nes` (the timing harness depends on APU frame-counter cadence). `1-instr_timing` completes ~frame 1016. |
| `cpu_reset` | 2 | 1 | 2 | — | **Wired in Phase 7 (T-71-002).** ROMs were vendored at `assorted/cpu_reset_{registers,ram_after_reset}.nes` but unused. `cpu_reset_registers_power_on_state` strict-passes by asserting the ROM's power-on register dump `A X Y P S = 00 00 00 34 FD`. The two `_full_protocol` tests are `#[ignore]`'d — these are interactive ("Press reset AFTER this message disappears") and the headless `0x81`-handler can't supply the externally-timed reset; reset register/RAM semantics are covered by `Cpu::power_on` / `Nes::reset` unit tests. |
| `cpu_timing_test6` | 1 | 1 | — | — | NROM; runs through `nes_blargg.rs` (`cpu_timing_test_phase1_deferred`) and `blargg_cpu.rs` (boot-completes smoke). |
| `branch_timing_tests` | 3 | 3 | — | — | `Branch_Basics`, `Backward_Branch`, `Forward_Branch`. |
| `cpu_dummy_reads` | 1 | 1 | — | — | NROM. |
| `cpu_dummy_writes_oam` | 1 | 1 | — | — | NROM. |
| `cpu_dummy_writes_ppumem` | 1 | 1 | — | — | NROM. Passes strictly today; may re-orient when Track C1 lands. |
| `cpu_interrupts_v2` | 5 | **5** | — | — | All five sub-ROMs (`1-cli_latency`, `2-nmi_and_brk`, `3-nmi_and_irq`, `4-irq_and_dma`, `5-branch_delays_irq`) strict-pass on the current default (master-clock) build. The former "default-only" C1 IRQ-sample residual on `{2,3,5}` is **closed** (the `mc-r1-full-cpu` umbrella is the default core; the old `cfg_attr(not(feature), ignore)` gating is gone). |
| `ppu_open_bus` | 1 | 1 | — | — | NROM. |
| `ppu_vbl_nmi` | 10 | 10 | — | — | All ten sub-ROMs (`01-vbl_basics` through `10-even_odd_timing`) pass strictly. |
| `sprite_overflow_tests` | 5 | 5 | — | — | `1.Basics` through `5.Emulator`. |
| `sprite_hit_tests` | 11 | 11 | — | — | blargg `sprite_hit_tests_2005.10.05`. `01.basics` through `11.edge_timing`. |
| `oam_read` | 1 | 1 | — | — | `assorted/oam_read.nes`. |
| `oam_stress` | 1 | 1 | — | — | `assorted/oam_stress.nes`. Long-running (~30 s NES time); test gives 3000-frame budget. |
| `apu_test` | 8 | 8 | — | — | `1-len_ctr` through `8-dmc_rates`. All sub-ROMs pass strictly including the IRQ-flag and jitter tests. |
| `apu_mixer` | 4 | 4 | — | — | `square`, `triangle`, `noise`, `dmc`. Validates the lookup-table non-linear mixer. |
| `pal_apu_tests` | 10 | 10 | 0 | — | **PAL-region APU oracle (v2.1.5).** blargg's PAL-calibrated APU length/frame-IRQ/timing rebuild, forced to PAL region. These 2005-era ROMs predate the `$6000` protocol (plain NROM, no PRG-RAM → `$6000` reads 0 forever), so the suite decodes the **on-screen** `PASSED`/`FAILED: #<n>` verdict via the `run_nes_screen` harness runner — correcting a prior *false* `$6000`-based oracle that vacuously reported all 10 as passing. **All 10 strict pass**: the three region-independent checks (`01.len_ctr`, `02.len_table`, `03.irq_flag`); the five PAL frame-counter-timing checks (`04.clock_jitter`, `05`/`06.len_timing_mode0`/`1`, `07.irq_flag_timing`, `08.irq_timing`) that flipped to PASS in v2.1.5 when the frame counter gained **region-gated PAL step positions** (2A07 sequencer at 8313/16627/24939/33252-33254 4-step, 8313/16627/24939/41565-41566 5-step); and `10.len_halt_timing` / `11.len_reload_timing` (were `FAILED: #3` / `#4`), closed in v2.1.5 by the **length halt/reload write-ordering** fix — the length counter now defers the halt (`new_halt`) and reload (`reload_val` + `previous_count`), promoted per CPU cycle in `Apu::tick_with_external` after the half-frame clock and before the mixer sample (mirrors `TetaNES`/Mesen2; see `docs/apu-2a03.md` + `docs/accuracy-ledger.md`). NTSC byte-identity preserved: the PAL positions are `Region::Pal`-only and the ordering fix settles in-cycle on non-coincident writes (NTSC/Dendy unchanged; AccuracyCoin 141/141, `apu_test` 8/8, `blargg_apu_2005` 11/11 exact). |
| `dmc_dma_during_read4` | 5 | 5 | — | — | `dma_2007_read`, `dma_2007_write`, `dma_4016_read`, `double_2007_read`, `read_write_2007`. |
| `mmc3_test_2` | 6 | 4 | 2 | — | `1-clocking`, `2-details`, `3-A12_clocking`, `5-MMC3` strict. `4-scanline_timing` `#[ignore]` (post-step-B4 + post-mid-cycle-snapshot rollback: sub-tests #1 + #2 PASS via the B4 reload-pending discriminator + post-fix trace at cycle 1,370,110 / scanline 0; sub-test #3 is the residual, a 1-CPU-cycle bracket empirically grounded as cross-cycle physics on the canonical CPU `T_last - 1` IRQ-sample-point axis — **CLOSED by-design-permanent** (v2.1.0 "Fathom" F5.0; ADR 0002): a differential 1-dot deficit structurally unreachable on the one-clock batched-catch-up scheduler, 21+ falsified levers, zero production-ROM impact. Stays `#[ignore]`'d permanently with a fail-loud `_currently_fails` companion). `6-MMC3_alt` `#[ignore]` by design (NEC rev B; project defaults to Sharp rev A). |
| `mmc3_irq_tests` | 6 | — | — | 6 | Visual-only protocol (no `$6000` status byte). Smoke-tested only. |
| `mmc5` (smoke) | 3 | — | — | 3 | `mapper_mmc5test_v1.nes`, `mapper_mmc5test_v2.nes`, `mapper_mmc5exram.nes` from `christopherpow/nes-test-roms/mmc5test/`. Visual-only; smoke-tested. Deep features (split-screen ExGrafix, audio extension) tested via in-tree mapper unit tests. |
| `holy_mapperel` | 17 | 17 | — | 17 | Damian Yerrick / tepples cartridge-PCB-assembly test (zlib license). 17 ROMs across mappers 0/1/2/3/4/7/9/10/34/66/69. Visual-only protocol → smoke-tested only. Track B1. |
| `vrc24test` | — | — | — | — | **Skipped (Track B1)**: link rot. AWJ's original forum attachment (id=10017 on forums.nesdev.org/viewtopic.php?p=203716) is auth-walled; the deletion is documented at archive.nes.science. No GitHub mirror found. |
| `AccuracyCoin` | 1 | 1 | — | — | 100thCoin / Chris Siebert single-NROM accuracy battery (MIT license, 146 tests across 20 suites + 5 visual-only `Power On State` tests sharing `$03FF`; the v2.0.1 upstream re-sync grew the catalog from 144 to 146 rows / 139 to 141 assigned tests, adding the PPU "ALE + Read" and "Hybrid Addresses" tests). Interactive (D-Pad menu); the harness presses `START` to "run all tests on the ROM" then takes two parallel measurements. **(1) Framebuffer decoder** reads the 10×16 on-screen result grid by exact-pixel colour (5-colour palette: `#64A0FF` = pass, `#4F1000` = fail, `#DC834C` = partial-pass, `#4C4C4C` = no-test / not-run, `#FFFFFF` = border); this is the legacy path and has a known grid-stride bug that under-samples by ~31 cells. **(2) RAM-direct decoder** reads each test's result byte from its fixed CPU-RAM address (catalogued from upstream `AccuracyCoin.asm` in `crates/rustynes-test-harness/src/accuracy_coin_catalog.rs` and `tests/roms/AccuracyCoin/SOURCE_CATALOG.tsv` — 146 `(suite, name, addr)` triples) and decodes per-test pass/fail/error-code names + per-suite breakdowns. This is the authoritative path. **Current measured pass rate (RAM-direct): 100.00% (141/141)** on the default build — the two upstream PPU tests "ALE + Read" and "Hybrid Addresses" (briefly the only gaps at 139/141 after the v2.0.1 catalog re-sync) were **closed in v2.0.3** by promoting the 2-cycle-ALE PPU fetch model to the unconditional default. The `90.65%`, `84.17%` and the trajectory figures below are historical engine-lineage milestones (the pre-promotion v1.0.0-rc2 / Session-26 era), retained as history. Historical trajectory: `64.03%` (post-D2 baseline) → `67.63%` (post-D3, 7 6502 bus-pattern fixes) → `69.06%` (post-Phase-3 OAM DMA parity fix, +1 strict test flipped) → `69.78%` (post-FSM-fix recovery, +1 sprite-related sub-test flipped as a side-benefit of the `crates/rustynes-ppu/src/ppu.rs` dot-64 reset removal) → `76.98%` (post-Cascade-B DMC DMA scheduler, commit `9b0c81c` — closes all 8 tests in the `APU Registers and DMA tests` suite + 3 net elsewhere as side-benefits; +11 tests flipped) → `78.42%` (post-Cascade-A OAMADDR-during-rendering reset, commit `f29f7ca` — hardware-accurate per nesdev: OAMADDR is reset to 0 during dots 257-320 of every rendered scanline; +2 tests flipped — Sprite overflow behavior PASSES, Sprite 0 Hit advances from error 1 → error 13) → `79.14%` (post-session-7 OAMADDR-walks-during-eval + $4-aligned `$2004` write, commit `c230489` — closes `Address $2004 behavior` with code 16; +1 net flip) → `79.86%` (post-session-7 RMW ABS,X/Y unfixed-address dummy read, commit `32d5b18` — 18 RMW opcodes get the canonical cycle-4 unfixed-address dummy; flips `APU Tests :: Controller Clocking` and advances `Implied Dummy Reads` 2→3 + `Frame Counter IRQ` 6→7 via the SLO $4015,X bracket; +1 net flip) → `82.73%` (post-session-8 BG-pipeline cycle-9 reload + post-emit shift, commit `086ce4d` — fixes the long-standing 1-column BG pixel off-by-one identified in `docs/audit/cascade-a-investigation-2026-05-19.md`; flips `Sprite 0 Hit behavior` + `Sprite overflow behavior` + `Suddenly Resize Sprite` + `$2007 read w/ rendering`; +4 net flips, +2.87pp) → `83.45%` (post-session-24 Controller Strobing M2-low-defer write, Session-24 Phase 3 — deferred `$4016` commit buffer on `LockstepBus` mirrors Mesen2's `NesControlManager::ProcessWrites`; flips `APU Tests :: Controller Strobing` from `[error 4]` to PASS; +1 net flip) → **`84.17%` (post-session-26 Sprint 2 iter 5 Frame-Counter-IRQ split, 2026-05-23 — separates `FrameCounter::irq_flag` ($4015 bit 6 visibility) from `FrameCounter::irq_line_active` (CPU IRQ source driver) so Tests I/J/K/L/M/N/O all PASS without spuriously asserting the CPU IRQ line on inhibited frame-counter cycles; flips `APU Tests :: Frame Counter IRQ` from `[error 19]` to PASS; +1 net flip)**. Session-26 Sprint 2 iter 4 (APU Register Activation OAM-DMA chip-select gate) advanced the same suite's APU Register Activation entry internally from `[error 4]` to `[error 6]` but did not flip the catalog-headline metric. The previous `75.93%` headline reflected the framebuffer decoder's stride bug, not real accuracy. Strict floor in CI is **60%** — see `crates/rustynes-test-harness/tests/accuracycoin.rs::MIN_PASS_RATE`. the v0.9.x 80% target and the v1.0.0 90% gate were both cleared, and the default build now measures **100.00% (141/141)** — the master-clock core is the default, the former C1 + sub-cycle residuals are closed, and the two v2.0.1 PPU tests were closed in v2.0.3 (see "Accuracy residuals" below). There are no open AccuracyCoin gaps. Implementation in `crates/rustynes-test-harness/src/accuracy_coin.rs` + `accuracy_coin_catalog.rs`. Phase D1 / D2 / D3. **The battery is now also run under the frontend's run-ahead snapshot/restore cycle** (`accuracycoin_runahead.rs`, depths 1 and 2) — the plain `run_frame` driver cannot see save-state gaps, and a real one cost three tests (141/141 headless vs 138/141 on the desktop app at the default `run_ahead = 1`) until the sprite-evaluation FSM + OAM-data-bus state was serialized in the `PPU_SNAPSHOT_VERSION` **v8** tail. That tail is the one non-additive save-state change since ADR 0028: the `.rns` container is version-exact per section, so pre-v8 save states no longer load. |

**Top-line counts (workspace + `--features test-roms`): the suite has grown substantially across the v1.6.0 → v1.8.8 train — `cargo test --workspace --features test-roms -- --list` currently enumerates ~**2030** tests workspace-wide (AccuracyCoin holds **100.00% / 141-141** since v2.0.3; host CI is green). The per-release figures cited in this section (661 strict + 10 ignored at v1.5.0; 545 strict / 605-with-`commercial-roms` at v1.0.0-rc2 / Session-26; etc.) are point-in-time historical provenance, NOT the current count — see `CHANGELOG.md` per-version entries and CI for authoritative per-release / per-suite numbers:

- Strict pass (not `#[ignore]`'d): **545** as of 2026-05-23 Session-26 (unchanged from iter 4; Session-26 Sprint 2 iter 5 lands the `FrameCounter::irq_flag` vs `irq_line_active` split, an internal-refactor with no new dedicated unit tests — the 4 MMC3 commercial canary ROMs + the custom Frame Counter IRQ ROM are the load-bearing assertions). Session-26 iter 4 (OAM-DMA chip-select gate) is unchanged from Session-25 baseline 545 too. Was 541 pre-Session-25; +4 then from the lazy-clear contract unit tests in `crates/rustynes-apu/src/frame_counter.rs` and `crates/rustynes-apu/src/snapshot.rs` that landed alongside the Frame Counter IRQ Test 7 architectural fix. Was 540 pre-Session-18; +1 then from the `vbl_race_window_2002_read_sweep` PPU-unit test; was 537 pre-Session-13; +3 then from the `Cpu::power_on`-path unit tests for the cold-boot SP fix; was 510 pre-Cascade-B; +35 net since the v1.0.0-rc1 tag. The C1 Phase 3 (2026-05-15) OAM-DMA alignment audit flipped `cpu_interrupts_v2/4-irq_and_dma` from `#[ignore]` (was paired with `_currently_fails` probe) to strict-pass + deleted the probe. **With `--features test-roms,commercial-roms`**: + 60 strict commercial-ROM tests (= **605 total**); audio FNV-1a + cumulative cycle-count invariants preserved across the session-8 BG re-baseline (only framebuffer FNV-1a hashes shifted there, all 60 visually verified) and PRESERVED byte-identical across Session-13 (SP delta not observable at the framebuffer / audio / cycle invariant layer), Session-18 (no chip-stack code change), Session-24 Phase 3 ($4016 strobe defer doesn't affect game ROMs that strobe with multi-cycle STA), and Session-25 (the Frame Counter IRQ lazy-clear surfaces only when ROMs do back-to-back $4015 reads at sub-3-cycle gaps — vanishingly rare in production game code).
- `#[ignore]` expected-fail (run via `-- --ignored`): **20**, fully catalogued in
  `docs/accuracy-ledger.md` — **none is an open accuracy gap.** They group (matching
  that ledger's "Ignored-test dispositions" table exactly) as:
  **7 permanent historical pins** — the APU `$4015`-load / reload-arm / `put_cycle`
  assertions (`apu.rs`), the three CPU interrupt-dispatch pins (`opcodes.rs`), and the
  PPU BG-shifter pin (`ppu.rs`), all superseded pre-master-clock mock-bus unit
  assertions (real coverage: AccuracyCoin 100%, `cpu_interrupts_v2` 5/5,
  `visual_regression` 7/7); **5 by-design** — the 4 MMC3 R1/R2 scanline-IRQ brackets
  closed by-design-permanent per ADR 0002 / v2.1.0 F5.0 plus the NEC-rev-B
  `mmc3_test_2/6-MMC3_alt` (only one of the two opposite silicon revisions can pass;
  the project defaults to Sharp rev A); **5 Vs. `DualSystem` fixture-blocked** — the GVS
  boots need a combined dual-CPU dump (the staged dumps are the MAME `maincpu` half
  only); and **3 CI/fixture** — the `stun`/`turn` live-network probes (kept hermetic)
  plus the `hdpack` copyrighted-pack test (run manually with `--ignored`). Each
  by-design/historical `#[ignore]` carries a fail-loud `*_currently_fails` companion so
  a surprise pass or a failure-shape change fails CI loudly.

**Counts with `--features test-roms,commercial-roms`** (user-supplied
ROM dumps under `tests/roms/external/`, not committed):

- **+60 strict pass + 0 `#[ignore]`'d** = 60 new commercial-ROM
  regression tests across all 15 supported mappers
  (`crates/rustynes-test-harness/tests/external_real_games.rs`, 757 LoC,
  landed 2026-05-17 with 54 strict + 6 `#[ignore]`'d; all 6 ignored
  ROMs were un-ignored later that same day via T-60-003 and
  T-60-003b — see the `2026-05-17 un-ignored set` bullet below).
  Each test asserts ROM SHA-256 + framebuffer FNV-1a 64-bit +
  cumulative CPU cycles + audio FNV-1a + audio sample count
  against a committed `insta` snapshot (60 `.snap` files under
  `crates/rustynes-test-harness/tests/snapshots/`). Verified 60/60
  strict on every v1.0.0 / v1.1.0 release gauntlet run.
- **2026-05-17 un-ignored set** (all 6 originally `#[ignore]`'d
  ROMs, now strict-pass at v1.1.0; see
  `docs/audit/sprint-2.5-commercial-rom-closure-2026-05-25.md`
  for the full closure record):
  - `external_mmc3_tiny_toon_adventures_2` (long intro / MMC3 edge case)
  - `external_mmc4_fire_emblem_gaiden` (long pre-title sequence)
  - `external_vrc4_ganbare_goemon_2` (mapper-023 sub-variant decoder)
  - `external_vrc6b_esper_dream_2` + `external_vrc6b_madara`
    (mapper-026 pinout decoder — structural; both ROMs share the
    failure mode, so worth ONE bug fix)
  - `external_fme7_mr_gimmick` (long FME-7 splash + intro animation)
- **21-ROM committed permissive baselines** at
  `crates/rustynes-test-harness/tests/{audio_tests,m22,mmc1_a12}.rs` (landed
  2026-05-17 / `6b3a818`; counts roll into the standard `--features
  test-roms` total above).
- **81-PNG visual baseline corpus** committed at `screenshots/`
  (`audio_tests/` × 19 + `m22/` × 1 + `mmc1_a12/` × 1 + `external/
  mapper-NNN-NAME/` × 60). Human-readable companion to the machine-
  readable `*.snap` hashes; regenerated via `RUSTYNES_DUMP_FRAMES=1
  RUSTYNES_DUMP_DIR=$PWD/screenshots cargo test ...`.
- **Permanent bisect tooling** at `scripts/regression-bisect/`. Drove
  the May-2026 recovery in 5 iterations
  (`0b1d4b66..HEAD` → `63d8dea` first-bad → `834be9e` fix).

---

## Mapper coverage

| iNES # | Name | Status | Audio | IRQ family | Notes |
|--------|------|--------|-------|------------|-------|
| 0 | NROM | landed (Phase 1) | — | — | 247 commercial titles. Trivial; no banking. |
| 1 | MMC1 (SUROM, SXROM, …) | landed (Phase 2) | — | — | Serial 5-write protocol; consecutive-write bug. |
| 2 | UxROM | landed (Phase 2) | — | — | UNROM, UOROM. CHR-RAM only. |
| 3 | CNROM | landed (Phase 2) | — | — | Bus conflict modeled. |
| 4 | MMC3 (Sharp rev A default; NEC rev B available) | landed (Phase 4 S1) | — | A12 edge | Default revision is **Sharp** (`Star Trek: 25th Anniversary` requires it). 4 of 6 `mmc3_test_2/*` sub-ROMs pass strictly; `4-scanline_timing` sub-test #3 is the 1-dot IRQ-sample bracket, **closed by-design-permanent** (ADR 0002 / v2.1.0 F5.0), and `6-MMC3_alt` is the by-design NEC-rev-B skip. |
| 5 | MMC5 | landed (Phase 4 S4 v0+v1) | **landed** (`mapper-audio`, Track C2 / Phase 2.3; 2 pulse + raw PCM) | Scanline (PPU dot 0 + scanline 241 dot 1) | Banking, ExRAM modes 10/11 + multiplier, scanline IRQ, dual sprite/BG CHR for 8×16, 4-byte fill mode, ExGrafix (mode 01), vertical split-screen (`$5200-$5202`), `$5113` PRG-RAM bank select. Save-state v3. (Phase 7 T-74-002: MMC5 confirmed feature-complete for v1.x; >8 KiB multi-chip PRG-RAM configs are long-tail, no corpus fixture.) |
| 7 | AxROM | landed (Phase 2) | — | — | Single-screen mirroring control. |
| 9 | MMC2 | landed (Phase 4 S2) | — | — | Punch-Out; latched CHR per fetch (`$FD`/`$FE`). |
| 10 | MMC4 | landed (Phase 4 S2) | — | — | Like MMC2 with full PRG banking. |
| 11 | Color Dreams | landed (Phase 4 S2) | — | — | Unlicensed; bus conflict. |
| 13 | CPROM | landed (Phase 4 S2) | — | — | Videomation. |
| 19 | Namco 163 | landed (Phase 4 S3) | **landed** (Track C2 / Phase 2.2; 1-8 wavetable channels via 128 B internal RAM; `mapper-audio` feature default ON) | CPU cycle | Banking + IRQ + nametable mode select. Mappy-Land, King of Kings, Final Lap, Rolling Thunder, Megami Tensei II. |
| 21 | VRC4a / VRC4c | landed (Phase 4 S3) | — | CPU cycle | Konami. |
| 22 | VRC2a | landed (Phase 4 S3) | — | — | Konami. |
| 23 | VRC4e / VRC4f / VRC2b | landed (Phase 4 S3) | — | CPU cycle | Konami. |
| 24 | VRC6a | landed (Phase 4 S3) | **landed** (Track C2; 2 pulse + sawtooth; `mapper-audio` feature default ON) | CPU cycle | Akumajou Densetsu. |
| 25 | VRC4b / VRC4d / VRC2c | landed (Phase 4 S3) | — | CPU cycle | Konami. |
| 26 | VRC6b | landed (Phase 4 S3) | **landed** (Track C2; same channels as 24; A0/A1 swap) | CPU cycle | Madara, Esper Dream 2. |
| 34 | BNROM / NINA-001 (variant-detected) | landed (Phase 4 S2) | — | — | M34 variant detection per NES 2.0 submapper. |
| 66 | GxROM | landed (Phase 2) | — | — | Bus conflict modeled. |
| 69 | Sunsoft FME-7 | landed (Phase 4 S3) | **landed** (Track C2 / Phase 2.1; 3 squares + envelope generator + LFSR noise; `mapper-audio` feature default ON) | CPU cycle | Gimmick! |
| 71 | Camerica BF9093 | landed (Phase 4 S2) | — | — | |
| 75 | VRC1 | landed (Phase 4 S2) | — | — | |
| 85 | VRC7 | landed (Track C2 / Phase 2.4; banking + IRQ) | **landed** (FM synthesis; v1.1.0; ADR-0006 supersedes 0004) | CPU cycle | Lagrange Point (JP). YM2413 OPLL-derived 6-channel FM audio **landed in v1.1.0** via a clean-room pure-Rust port of `emu2413 v1.5.9` (MIT) at `crates/rustynes-apu/src/opll.rs`; *Lagrange Point* plays with in-game audio. Banking + IRQ are identical in shape to VRC6's (mapper 24/26). |

> **Engine-lineage note.** The table above is the **original 15-mapper**
> coverage from the early engine line (the top-25-by-title-count tranche).
> RustyNES **v1.0.0 shipped 51 mapper families**; **v1.2.0 extended this to
> 87**, **v1.3.0 "Bedrock" to 101**, **v1.4.0 "Fidelity" to 113**, and
> **v1.5.0 "Lens" to 123**, **v1.6.0 "Studio" to 150** — the J.Y. Company ASIC
> mappers (m90/209/211) + the UNIF loader + Workstream E's MMC3-clone batch — and
> **v1.7.0 "Forge" to 168** (Workstream G1's reusable-ASIC batch), and
> **v1.8.9 "Backlog" beta.6 to 172** (the current count; the final NTDEC/TXC/BMC
> NTDEC/TXC/BMC multicart batch — m193/204/221/299 — plus a UNIF board-map
> breadth pass). See
> `docs/mappers.md` §Mapper coverage matrix +
> §Mapper accuracy tiering for the full current list. The "out of scope" notes
> below were the early-engine scoping; they are retained as history and
> annotated with what has since shipped.

**Mapper count.** 15 distinct mappers in the early engine line (>95% of the
licensed library by title count) → **51 families at v1.0.0** → **87 families at
v1.2.0** → **101 families at v1.3.0 "Bedrock"** → **113 families at v1.4.0
"Fidelity"** → **123 families at v1.5.0 "Lens"** → **150 families at v1.6.0
"Studio"** (the J.Y. Company ASIC sweep 35/90/209/211 +
Workstream E's batch: MMC3-clones, Sachen 8259 A/B/C, discrete
multicarts) → **168 families at v1.7.0 "Forge"** (Workstream G1's reusable-ASIC
reusable-ASIC batch: FK23C, COOLBOY/MINDKIDS, Sachen 9602/3011, Waixing
164/253/286, Kaiser 56/142/303/305/306/312, and BMC multicarts
261/289/320/336/349) → **172 families at v1.8.9 "Backlog" beta.6** (the current
count; the final NTDEC/TXC/BMC multicart batch — NTDEC TC-112 m193, BMC
2-in-1 m204, NTDEC N625092 m221, TXC/BMC-11160 m299 — plus a UNIF board-map
breadth pass wiring well-known board aliases to already-implemented families),
tiered for accuracy honesty:

| Tier | Families | Accuracy-gated? | Evidence |
|------|----------|-----------------|----------|
| **Core** | 51 | Yes (AccuracyCoin + commercial oracle) | spec-implemented, oracle-locked |
| **Curated** (v1.2.0 + **v2.1.0 "Fathom" F3**) | 95 | Yes | notable games + decode spec; register-decode unit tests **+ byte-identity boot-snapshot oracle** (`external_extended.rs`) |
| **BestEffort** | 26 | **No** | reference-ported long-tail with **no cleanly-booting ROM dump** (16 NES 2.0 high-id boards + 8 with no matching cart + 2 whose only dump jams at boot); register-decode + save-state unit tests only |

The **v2.1.0 "Fathom" F3** sweep promoted **86** families BestEffort → Curated
(57 already-staged + 29 sourced from GoodNES v3.23b), taking accuracy-gated
coverage from **60 → 146** of the 172 families. The 26 that remain BestEffort are
uncoverable (no cleanly-booting redistributable dump).

A CI-checkable invariant forbids any `BestEffort` mapper from backing an oracle
ROM (`rustynes-mappers::mapper_tier`; ADR 0011). The remaining tail (unlicensed
pirate carts, niche boards) is documented in `docs/compatibility.md`.

**Early-engine "not supported" list (all since resolved in v1.0.0 unless noted):**

- ~~VRC7 (mapper 85) FM audio~~ — **shipped** (clean-room `emu2413` OPLL port
  per `docs/adr/0006-vrc7-audio-landed.md`, supersedes ADR 0004).
- ~~FDS (Famicom Disk System)~~ — **shipped** (real-BIOS `.fds` boot; see
  `docs/compatibility.md`).
- ~~VS. System, PlayChoice-10 arcade variants~~ — **shipped** (game-verified
  2C03/2C04/2C05 RGB PPU).
- Unlicensed pirate cart mappers (113, 116, etc.) — still tracked
  case-by-case if user demand surfaces.

---

## Feature flags

> **v2.1.x "Fathom" update:** the accuracy line added its capabilities as **runtime,
> default-off toggles** (Settings / config, not cargo features), so the shipped default build
> stays byte-identical and no cargo-feature flag was added. Runtime toggles: **OAM decay**
> (`[emulation] oam_decay` / `Nes::set_oam_decay`, v2.1.4), the **APU filter model**
> (`[audio] filter_model` — `nes` / `famicom` / `clean`, v2.1.3), the **generated NTSC
> palette** (Settings → Palette, v2.1.2), and the **Vs. `DualSystem` dual-screen layout**
> (`[graphics] dual_screen_layout`, v2.1.2). The only new *cargo* feature in the line is the
> pre-existing default-off dev-diagnostic `ppu-octal-trace`. Save-state tail progression across
> the line: **`PPU_SNAPSHOT_VERSION` 5 → 6** (v2.1.1, run-ahead PPU-state completeness) **→ 7**
> (v2.1.4, per-row OAM-decay age); both tails are additive, pre-version `.rns` still load, and
> neither is an ADR-0028 epoch break.
>
> **v2.0.3 update:** BOTH octal-latch experiment flags are now **retired**. The
> genuine two-dot `mc-ppu-2cycle-ale` model (the v2.0.3 first-principles rework of the
> v2.0.2 whole-dot stand-in) is **promoted to the unconditional, only PPU fetch path**
> (shipped AccuracyCoin **141/141**, 60-ROM oracle 60/60), so `mc-ppu-2cycle-ale` and
> the superseded `mc-ppu-bus-addr-hybrid` are both deleted from `rustynes-ppu` /
> `rustynes-core` / `rustynes-test-harness`, along with the v2.0.2 stand-in code
> (`octal_effective`, `OctalFetch`, `copy_v`, the coarse-X reconstruction). A new
> default-off dev feature **`ppu-octal-trace`** keeps the `octal_trace` calibration
> ring (no-op stub when off; does not gate emulation behaviour, keeps the `no_std`
> chip stack building). Promotion required an ADDITIVE **`PPU_SNAPSHOT_VERSION` 4 -> 5**
> tail (the five in-flight fetch fields, for netplay-rollback determinism; pre-v5
> `.rns` still load — see the top-of-file current-release block).
>
> **v2.0.2 update:** the `mc-ppu-bus-addr-hybrid` experiment — the octal-latch
> multiplexed-bus PPU model (ADR 0030, commit `27c103c`) — is now **verified at
> AccuracyCoin 141/141 (100.00%) flag-on** (a whole-dot port of TriCNES's `FetchPPU`
> octal-latch splice; the two new PPU tests "ALE + Read" / "Hybrid Addresses" pass,
> framebuffer 100%, no regression). It stays **default-off** in v2.0.2 so the shipped
> default build is byte-identical to v2.0.1 at 139/141; per the maintainer's
> refine-then-promote decision (ADR 0030) it is **promoted to default in v2.0.3**
> after the Hybrid `+1 coarse-X` approximation is reworked to a first-principles
> latch-carry model and gated on the 60-ROM commercial byte-identity oracle. No
> snapshot-format bump — `PPU_SNAPSHOT_VERSION` stays 4.
>
> **v2.0.1 update:** the last diagnostic hold-over from the v2.0.0 Timebase work —
> the DMC-DMA-abort probe `mc-r1-dmc-abort-probe` — has now been **removed** (it
> gated only default-off pub-static atomic counters and a diagnostic bin block, no
> shipped behaviour; the default build is byte-identical, AccuracyCoin unchanged).
> Earlier in the v2.0.0 collapse, most of the `mc-r1-*` / `mc-ppu-*` family +
> `cpu-{stack,implied}-dummy-reads` were already retired or unconditionalised:
>
> - the PPU-accuracy trio (`ppu-oam-data-bus`, `ppu-sprite-shifter-counter`,
> `ppu-2002-read-end-flags`) + `accuracycoin-sprite-eval-base-from-oamaddr` are
> **no longer feature flags** — they were unconditionalised when the legacy
> integer-lockstep path was removed (the R1 master clock is now the only path).
> The four dead experiments (`cpu-c1-attempt-17-access-reorder`,
> `mc-r1-coldboot-ppuoffset`, `mc-r1-bp-ppu-offset`, `dmc-get-put-scheduler`) were
> deleted. The rows below for any removed flag are **historical** (v2.0.0-era). The
> flags that REMAIN: `std`, `serde`, `test-roms`, `commercial-roms`, `mapper-audio`,
> `wasm-winit`/`wasm-canvas`, the diagnostics `irq-timing-trace`, `ppu-state-trace`,
> `cpu-boot-trace`, `cpu-instr-cycle-trace`, the new default-off dev-diagnostic
> `ppu-octal-trace` (the `octal_trace` calibration ring; no-op when off), and
> `mmc3-m2-phase-irq` (the still-open R1/R2 MMC3-IRQ-timing residual experiment per
> ADR 0002 — retained because it gates real alternate behaviour and the residual is
> by-design-deferred, not closed). **The two octal-latch experiment flags
> `mc-ppu-2cycle-ale` and `mc-ppu-bus-addr-hybrid` were both retired in v2.0.3** when
> the 2-cycle-ALE model was promoted to the default (see the v2.0.3 update above).

| Flag | Crate(s) | Default | Purpose |
|------|----------|---------|---------|
| `test-roms` | `rustynes-test-harness` | off | Gates integration tests that depend on vendored test ROMs under `tests/roms/`. CI enables it. |
| `mapper-audio` | `rustynes-mappers` | **on** | Gates on-cart audio extensions. Post-tag v0.9.x ships VRC6 (mappers 24/26), Sunsoft 5B (mapper 69, Phase 2.1), Namco 163 (mapper 19, Phase 2.2), and MMC5 (mapper 5, Phase 2.3); VRC7 (mapper 85) FM audio **landed in v1.1.0** (`crates/rustynes-apu/src/opll.rs`; ADR-0006 supersedes 0004). With the flag off, register decoders still latch state (preserves save-state round-trip) but channel oscillators do not advance and `mix_audio` returns 0. |
| `irq-timing-trace` | `rustynes-core`, `rustynes-test-harness` | off | Track C1 per-CPU-cycle IRQ tracing fixture. See `crates/rustynes-core/src/irq_trace.rs` and ADR-0002 §"Test fixture". CI does not enable it (the fixture is heavy: ~3-4 M records per ROM × 6 ROMs ≈ 160 MB peak). Enabled by `cargo test --features test-roms,irq-timing-trace --test irq_trace_fixture`. |
| `ppu-state-trace` | `rustynes-ppu`, `rustynes-core`, `rustynes-test-harness` | off | Session-10 per-PPU-dot state-tracing fixture. See `crates/rustynes-ppu/src/state_trace.rs`, ADR-0005, `docs/ppu-trace-tooling.md`. When OFF, every byte of overhead is gone via `#[cfg]` gates inside `Ppu::tick` and on the storage field. CI does not enable it; the fixture is heavy (180 MB / 10-frame visible-only window). Enabled by `cargo test --features test-roms,ppu-state-trace --test ppu_state_trace_fixture`. |
| `ppu-idle-line-fast` | `rustynes-ppu`, `rustynes-core`, `rustynes-test-harness` | off | v2.2.3 P2. The specialized **idle-line** PPU dot path (`Ppu::tick_idle_line_fast`) for the post-render line + vblank lines 242..=260 — 6,820 of the 89,342 NTSC dots, where the per-dot body provably reduces to three rendering-flag assignments. **Byte-identical** (pinned by `fast_dotloop_diff`, incl. a vblank-I/O torture ROM that forces the guard's fall-through arms), but measured **below the >3% adoption bar** — a same-session A/B against a ±0.7% noise floor gives −1.3%/−1.5% on rendering-*disabled* content and +0.2%/+0.4% on the rendering-heavy case that dominates real play. Kept behind a flag rather than deleted; compile-time rather than runtime because the cost *is* the per-dot guard. With it off the field, guard and handler are all absent. See `docs/performance.md` §P2. |
| `commercial-roms` | `rustynes-test-harness` | off | 60-ROM regression bisect harness against user-supplied dumps at `tests/roms/external/`. Snapshots committed; ROM dumps gitignored. Enables `cargo test --features test-roms,commercial-roms --test external_real_games`. |
| `cpu-implied-dummy-reads` | `rustynes-cpu`, `rustynes-core`, `rustynes-test-harness` | off | Sprint 2.3 (v1.2.0). Enables canonical cycle-2 PC dummy reads for the 23 implied/accumulator/transfer/flag opcodes per nesdev §6502 cpu cycle reference. Default-off pending DMC scheduler co-fix (see `dmc-get-put-scheduler` row below + ADR 0007). With the flag ON in isolation, the `Implied Dummy Reads` AccuracyCoin test still does NOT flip to PASS — the cascade-target needs the get/put scheduler interaction. |
| `dmc-get-put-scheduler` *(removed)* | — | **removed** | **Historical (v2.0.0-era); the flag was deleted.** Engine-lineage Phase 8 Sprint 3 (v1.2.0). Replaces the v1.1.0 phase-agnostic "noop loop + compensating delays" DMC scheduler in `rustynes-core::bus::service_dmc_dma` with Mesen2's canonical get/put cycle alternation model (`NesCpu.cpp:399-447`). Default-off via parallel-implementation pattern (ADR 0007). This parallel experiment reached only **6/10** on the AccuracyCoin DMA cluster (4 failures in the DMC abort path) and was **superseded and removed**: the default master-clock core closes that DMA cluster **10/10** (see the AccuracyCoin 100% breakdown above). |
| `mc-r1-full-cpu` *(removed)* | — | **removed → default** | **Historical (v2.0.0-era); the flag no longer exists.** This was the v2.0 master-clock umbrella (W3-Stage-4 promotion, 2026-06-10) that reached **AccuracyCoin 100.00% (139/139)** on this one flag; it has since been **promoted to the default core and deleted** — RustyNES v1.0.0 ships this behaviour unconditionally (it is the only scheduler). The composition it bundled, for the record: composes the R1 floor substrate (substrate + dmc-idle-halt + unified APU clock + one-clock parity + stack/implied dummy reads + the promoted DMC-abort stack) PLUS the Stage-4 fold: `mc-r1-branch-poll-points` (Interrupt-flag-latency), `mc-ppu-2007-render-buffer` (`$2007` Stress), `mc-r1-dmc-delayed-4015` (the delayed-`$4015` status on the unified single-driver DMA engine at the breakthrough parity — DMC+OAM / Explicit + Implicit Abort / Delta-Mod / Implied-Dummy), and `mc-r1-oam-dma-reg-window` (APU Register Activation). nestest 0-diff; cpu_interrupts_v2 5/5 strict; SH\* 6/6; save-state round-trips hold (the gated state is serialized). Scope: NTSC-only (PAL/Dendy frame-structure tests ignored under the flag); the `audio_tests` corpus is default-build-only (R1 changes DMC audio timing by design). Default-build promotion is the later Phase-7/F program. See `docs/audit/v2.0-stage4-promotion-2026-06-10.md`. |

---

## Accuracy residuals — CLOSED by the v1.0.0 master-clock core

> **Authoritative scoreboard: `docs/accuracy-ledger.md`.** That ledger is the
> single source of truth for the current per-suite pass counts and the full
> `#[ignore]` catalogue. This section is the narrative companion; where the two
> ever disagree, the ledger wins.

**These are closed.** The master-clock-precise scheduler that the engine lineage
called the "v2.0 refactor" shipped as the **default and only** core in RustyNES
v1.0.0 — the `mc-r1-full-cpu` umbrella was promoted to default and the feature
flag no longer exists. The subsequent v2.0.0 "Timebase" one-clock rewrite and the
v2.0.3 2-cycle-ALE PPU promotion further hardened it. On the current default build
(`--features test-roms`):

- **AccuracyCoin 141/141 (100.00%)** (RAM-direct decoder), 0 fail. The two PPU
  tests the v2.0.1 upstream re-sync added ("ALE + Read" `$0491`, "Hybrid
  Addresses" `$0492`) — briefly the only two open gaps at 139/141 — were **closed
  in v2.0.3** by promoting the 2-cycle-ALE PPU fetch model to the unconditional
  default. nestest is 0-diff.
- **`cpu_interrupts_v2` 5/5 strict** — the `2-nmi_and_brk` / `3-nmi_and_irq` /
  `5-branch_delays_irq` sub-ROMs this section formerly listed as "deferred to
  v2.0" pass strictly on the default build. `ppu_sprites` 19/19.
- **The MMC3 R1/R2 scanline-IRQ residual is CLOSED — by-design-permanent**
  (v2.1.0 "Fathom" F5.0, 2026-07-09; `docs/adr/0002-irq-timing-coordination.md`).
  The remaining `mmc3_test_2/4` sub-test #3 (and the sibling `mmc3_test_v1/*`
  brackets) are a **differential 1-dot IRQ-sample deficit that is structurally
  unreachable** on the one-clock batched-catch-up scheduler — 21+ falsified
  levers, **zero production-ROM impact**. They stay `#[ignore]`'d **permanently**
  with fail-loud `*_currently_fails` companions; this is a closed decision, not a
  deferral.

The ROM-level edge cases that remain `#[ignore]`'d are **documented-by-design or
externally-fixture-blocked, not deferred to any future refactor** (see the full
20-test catalogue in `docs/accuracy-ledger.md`): the MMC3 scanline-IRQ brackets +
NEC-rev-B `mmc3_test_2/6` (by-design), the pre-master-clock mock-bus unit pins
(`apu_reset` / interrupt-dispatch / BG-shifter — superseded by AccuracyCoin 100%),
the Vs. `DualSystem` GVS boots (need a combined dual-CPU dump), and the live-STUN /
TURN + HD-pack fixture tests. Each carries a permanent or external-blocker
`#[ignore]` reason at its test site.

> **Superseded note (kept for lineage).** An earlier v1.3.0 re-baseline framed the
> MMC3 sub-test #3 + two `apu_reset` cases as "keep deferring, pending a
> fractional-master-clock + cycle-accurate-reset refactor." That verdict is
> **superseded**: v2.1.0 F5.0 investigated the MMC3 axis under a bounded-effort
> campaign, falsified 21+ levers, and **formally closed it by-design-permanent**
> (ADR 0002) — a fractional-master-clock rewrite is *not* planned. The `apu_reset`
> pins are likewise permanent (mock-bus assertions the master-clock core no longer
> exercises). See `docs/accuracy-ledger.md`.

The detailed engine-lineage attempt-log below is retained as **historical
provenance only** — it documents how the master-clock axis was investigated
before it landed. The "deferred to v2.0" language inside it is history, not a plan.

1. **`cpu_interrupts_v2/{2-nmi_and_brk, 3-nmi_and_irq,
   5-branch_delays_irq}`** — 3 sub-ROMs fail on `test_jmp` / NMI-BRK
   shape. `#[ignore]`'d strict probes + `*_currently_fails` companions
   in `crates/rustynes-test-harness/tests/cpu_interrupts_v2.rs`.
2. **`mmc3_test_2/4-scanline_timing` sub-test #3** (post-step-B4
   residual, 2026-05-14). The original sub-test #2 failure ("Scanline 0
   IRQ should occur LATER when `$2000=$08`") is now CLOSED by the C1
   step B4 landing — see CHANGELOG `[Unreleased]` → "Fixed (Phase 4 /
   Track C1 — step B4 landing, MMC3 reload-pending Sharp
   discriminator)". The remaining sub-test #3 ("Scanline 0 IRQ should
   occur SOONER when `$2000=$08`") is a 1-CPU-cycle bracket residual
   distinct from the structural reload-pending discriminator step B4
   closed; it shares the same architectural surface as
   `cpu_interrupts_v2/{2..5}` above (CPU per-cycle IRQ sample point /
   bus poll location). `#[ignore]`'d strict probe + `_currently_fails`
   companion in `crates/rustynes-test-harness/tests/m004_mmc3.rs`.

CHANGELOG `[Unreleased]` → "Investigated and rolled back" documents
**seven** prior code attempts (the original 4 from v0.9.0-rc prep,
Phase B4's sub_dot-aware MMC3 filter threshold from 2026-05-14, the
post-B4 mid-cycle mapper-IRQ-snapshot experiment from 2026-05-15,
and the M2-low CPU IRQ sample from 2026-05-15); all rolled back as
negative results because they regressed orthogonal surfaces or were
dead-ended by empirical evidence. The diagnosis converges on:
**CPU per-cycle IRQ sample point, LockstepBus IRQ poll point, and
PPU A12 emission dot need to be re-aligned together, not
independently.** ADR `docs/adr/0002-irq-timing-coordination.md`
captures the constraint set, the proposed coordinated approach, and
(in its "Empirical refinement (2026-05-14)" subsection) the refined
direction for the next attempt; no code attempt should land until
that ADR is reviewed.

The 7th attempt's empirical evidence is the FIRST positive signal
in this series: switching `idle_tick`'s IRQ poll from `M2Phase::High`
to `M2Phase::Low` flipped `cpu_interrupts_v2/5-branch_delays_irq`'s
`test_jmp` sub-test CK values to silicon-matching patterns (the
remaining cpu_interrupts_v2/5 residual is page-cross dummy-read
cycle accounting in a LATER sub-test, not test_jmp). The M2-low
IRQ axis is therefore confirmed load-bearing for cpu_interrupts_v2;
landing it requires the coordinated bundle (IRQ + NMI edge latch
restructure + OAM/DMC DMA cycle audit + mmc3 sub-test #3 audit),
estimated 1-2 weeks.

**Track C1 pre-work landed (2026-05-13):** ADR-0002's "Decision
(revised, 2026-05-13)" section now defines the coordinated change
concretely (M2-phase reference enum in the scheduler; CPU / Bus / PPU
sample-point re-derivation; per-attempt differentiation from each of
the four rolled-back approaches). The empirical oracle — a per-CPU-
cycle IRQ tracing fixture gated on the `irq-timing-trace` cargo feature
— lives in `crates/rustynes-core/src/irq_trace.rs` with a corresponding
integration test in `crates/rustynes-test-harness/tests/irq_trace_fixture.rs`.
Baseline traces are committed at
`crates/rustynes-test-harness/golden/irq_trace/` for each of the 5 target
ROMs + the `1-cli_latency` control.

**Track C1 Phases A + B1 + B2/B3 landed (2026-05-14):** the bus / CPU
plumbing for the M2-phase reference is in place. **Phase A** (d7d4c98)
rewrote the trace fixture's `CycleRecord` to carry two-phase IRQ
snapshots (`_at_low` + `_at_high`) and regenerated all 6 baseline CSVs;
empirical finding: `_at_low` == `_at_high` byte-identical across every
baseline row (the asymmetry is not visible at the bus-snapshot level).
**Phase B1** (12949c3) promoted `M2Phase::Low/High` to `rustynes-cpu::scheduler`
(re-exported from `rustynes-core::scheduler` to preserve workspace dep
direction) and exposed `LockstepBus::current_m2_phase()`. **Phase B2 +
B3** (c8b7ce6) added 4 unconditional snapshot fields
(`irq_snapshot_{mapper,apu}_at_{low,high}`) on the bus and
`Bus::poll_irq_at_phase(M2Phase) -> bool` on the `rustynes_cpu::Bus` trait;
`Cpu::idle_tick` now reads via `bus.poll_irq_at_phase(M2Phase::High)`.
Pure no-op-behavior plumbing per Phase A's empirical finding; production
`poll_irq` semantics preserved; +1 unit test (`scheduler::tests::
m2_phase_as_str_round_trips`).

**Track C1 Phase B4 attempt + rollback (2026-05-14):** the sub_dot-aware
MMC3 A12 filter threshold was prototyped in two iterations (iter 1:
sub_dot 0/1 require gap >= 4, sub_dot 2 requires gap >= 3; iter 2:
inverted) and both rolled back (`git checkout -- .`). df07ae3 ships
only a CHANGELOG diagnostic entry. **Empirical finding**: the failing
IRQ assertion at cycle 1,369,997 (frame 46, scanline 261, dot 259,
sub_dot 0) fires after a ~900,000-cycle rendering-disabled phase; the
gap to the prior A12 fall is enormous and any reasonable threshold
accepts identically. The MMC3 filter *threshold* is NOT the load-
bearing axis for sub-test #2 — the discriminator must be the rise's
**context** (pre-render vs visible scanline) or a counter-clock-pipeline
mechanism distinct from Attempts 2/3.

The actual coordinated change itself (PPU-side pre-render A12 emission
audit OR sub_dot-aware MMC3 counter-clock pipeline — see ADR-0002's
"Empirical refinement (2026-05-14)" subsection for full design notes)
remains the next work item; the trace fixture + the M2-phase plumbing
are the mandatory pre-requisite infrastructure.

The 6th `#[ignore]`'d test (`mmc3_test_2/6-MMC3_alt`) is **by design**:
sub-ROM 6 exercises NEC rev B MMC3, sub-ROM 5 exercises Sharp rev A,
and the two are mutually exclusive. RustyNES defaults to Sharp (Star
Trek: 25th Anniversary's canary requirement), so sub-ROM 6 must fail
unless the default flips.

---

## Version policy

**RustyNES ships at v1.0.0** (the production cut), with the additive,
off-by-default feature releases **v1.1.0 "Scriptable" → v1.2.0 "Curator" →
v1.3.0 "Bedrock" → v1.4.0 "Fidelity" (+ the v1.4.1 patch) → v1.5.0 "Lens" →
v1.6.0 "Studio" → v1.7.0 "Forge" → v1.7.1 (patch)** on top, then the **v1.8.x
"Android"** platform line (v1.8.0 → … → v1.8.7 "Android" (Connectivity completion) →
**v1.8.8 "Atlas" (Google Play launch readiness)** → **v1.8.9 "Backlog"** (the
carryover beta train that closed the Android line). **v1.8.9** added the creator-tooling /
debugger-depth / full-Mesen2-HD-pack-parity / mapper-breadth (168 → 172) work — see
the blockquote at the top + `CHANGELOG.md` `[1.8.9]` — plus the 13-PR Dependabot
consolidation (jni 0.21 → 0.22, zip 2 → 8.6, naga 25 → 29, sha1 / md-5 0.10 → 0.11,
pollster 0.3 → 0.4, android_logger 0.14 → 0.15, lz4_flex 0.11 → 0.13, plus the GitHub
Actions bumps) and the **monetization build-out** (the new, dormant
`rustynes-monetization` crate — the shared ad-supported / freemium policy core); the
emulation core stays byte-identical and AccuracyCoin holds 100% (139/139). The
table below is the **engine-lineage** version history
— the internal engine line whose increments produced the v1.0.0 technology. Its
`v0.9.x` / `v1.x` / `v2.x` markers are the engine's own line, retained as
historical anchors documenting *how* each capability was built; they are **not**
RustyNES releases of their own. When RustyNES makes a release it does so under its
own semantic-version line starting at **v1.0.0**.

> **Two distinct "v2.0"s — do not conflate them (both now shipped, but at
> different times, for different reasons).** The **engine-lineage v2.0**
> master-clock work (which took AccuracyCoin to **100.00%**) is *upstream engine
> history* that shipped as the **v1.0.0 production core** (2026-06-13) — the
> dot-lockstep scheduler that was the *only* scheduler through v1.10.0. The
> **RustyNES v2.0.0 "Timebase"** release (2026-07-03, the MAJOR-boundary release
> beneath the current v2.1.x "Fathom" line) is a
> *different* milestone that *replaces* that dot-lockstep scheduler outright: the
> **one-clock + every-cycle-bus-access collapse** (a single canonical cycle
> counter + a split-around-the-access `start_cycle`/`end_cycle` PPU catch-up,
> mirroring Mesen2's structure), full Vs. `DualSystem` dual-console emulation, and
> the breaking save-state / cross-version format changes that it entailed
> (ADR 0002 / ADR 0028 / ADR 0029). The R1/R2 hard-tier IRQ-timing residual was
> investigated under a bounded-effort campaign and is by-design-deferred beyond
> this release, not closed. The full path that got here is tracked in
> `to-dos/ROADMAP.md` + `to-dos/plans/`: v1.7.0 "Forge" shipped → the
> **v1.8.x "Android"** sideload line
> (through **v1.8.9 "Backlog"**) → the **v1.9.0 → v1.9.9**
> interim-TestFlight iOS train (mirroring the Android v1.8.0–v1.8.9 arc:
> **v1.9.0 "Sunrise"** foundation → the wgpu→Metal renderer → connectivity +
> scripting → **v1.9.8 "Horizon"** store-readiness + the §4.7 compliance pass →
> **v1.9.9 "Workshop"** creator tools; ADRs 0026 + 0027; plan
> `to-dos/plans/v1.9.x-ios-train-plan.md`) →
> **v1.10.0 "Arcade"** (the native Libretro / RetroArch core) →
> **v2.0.0 "Timebase"** (the MAJOR-boundary scheduler rewrite) → the
> **mobile-finalization train** (maintainer replan 2026-06-23: both app-store
> launches held to post-2.0.0) — **v2.0.1–v2.0.4** final Android, **v2.0.5–v2.0.8**
> iOS finalization, **v2.0.9** both-apps readiness, all shipped → the
> **v2.1.x "Fathom"** accuracy line: **v2.1.0** accuracy remediation → **v2.1.1** the
> Wizards & Warriors game-DB freeze fix → **v2.1.2 "Prism"** display fidelity →
> **v2.1.3 "Codex"** quality-of-life → **v2.1.4 "Caliper"** accuracy hardening →
> **v2.1.5 "Vernier"** the regression-net & residual step →
> **v2.1.6 "Timbre"** the expansion-audio fidelity step →
> **v2.1.7 "Stepping"** hardware revisions & DMA frontier →
> **v2.1.8 "Tempo"** performance →
> **v2.1.9 "Aperture"** presentation & signal →
> **v2.1.10 "Loom"** creator tools & web parity →
> **v2.2.0 "Capstone"** the milestone cut closing the run (**the current release**).
> The **v2.1.5 → v2.2.0** line is a **"deepen the existing project"** run —
> accuracy / performance / features / quality; v2.1.5 opened it (the tepples **Holy
> Mapperel** mapper bank-reachability / IRQ regression net, the first PAL-region APU oracle
> at `pal_apu_tests` 10/10, the MMC3 R1/R2 F5.0 A12-phase study, a measured fat-LTO A/B, and
> a real TURN NAT-traversal retransmit production fix — all NTSC-byte-identical), v2.1.6
> continued it (the expansion-audio decibel oracle, the hardware/Mesen2 channel-level
> calibration incl. the Namco 163 ~12 dB fix, VRC7 patch-set verification vs Nuke.YKT, and a
> frontend Audio Mixer panel — the base 2A03 NTSC output byte-identical), v2.1.7 added
> opt-in PPU / 2A03 die-revision + power-on RAM/palette hardware models (the DMA
> "unexpected read" frontier proven a documented no-op on every oracle, ADR 0033 — honest, not
> faked), v2.1.8 the default-OFF specialized fast PPU dot path (~12% rendering-heavy,
> differential-tested bit-identical) + a SIMD-validated software blitter + a wasm size pass,
> v2.1.9 the marquee CRT shader stack (CRT-Royale / crt-guest-advanced / Sony Megatron) + a raw
> NTSC composite signal-decode path + GIF/WAV capture + a palette editor, and v2.1.10 the
> TAStudio greenzone + Lua API breadth + the browser-RA auth-proxy deploy stack + Vs.
> `DualSystem` libretro presentation, and v2.2.0 closed the run (the netplay matchmaking / lobby
> stack + the FDS medium model + a peripherals & quality/security pass — cargo-fuzz targets 3 → 8,
> a `Movie::deserialize` OOM-DoS fix, a read-only Tools → ROM Info browser) — all
> NTSC-byte-identical, AccuracyCoin 141/141 throughout. The **JOINT mobile store launch** (Google Play + Apple App
> Store + AltStore PAL + F-Droid), and with it the `rustynes-monetization` activation, is the
> future **v2.3.0** (moved from the earlier v2.1.0 / v2.2.0 targets). The monetization model
> is ad-supported with a **$3.99** premium unlock (AppLovin MAX + RevenueCat, a reward-ad
> +11-minute × 2 demo extension, 6 premium features) under a **`foss` / `play` flavor split**
> (ADR 0025); the shared policy core is the `rustynes-monetization` crate. See
> `to-dos/plans/v2.0.x-mobile-finalization-plan.md`.

| Version | Status | Bar |
|---------|--------|-----|
| **RustyNES v1.0.0** | **PRODUCTION BASE** (the current release is **v2.2.0**; see the top block) | The production cut. AccuracyCoin **100.00% (139/139)**; 60-ROM `external_real_games` + 52-entry `external_extended` oracles byte-identical; nestest 0-diff. Ships the cycle-accurate master-clock core, 51 mapper families (incl. VRC6/VRC7-OPLL/Sunsoft-5B/Namco-163/MMC5 expansion audio + Vs./PC10 RGB boards), real-BIOS FDS, 2-4-player rollback netplay (native UDP + browser WebRTC), RetroAchievements (opt-in/native), TAS movie record/replay + save states/rewind, the performance + desktop-UX shell (display-sync pacing matrix, lock-free audio ring + DRC, run-ahead, dedicated emulation thread, and an always-on egui shell — menu bar / status bar / tabbed Settings / themes / fullscreen / save-state slots / surfaced tool panels), and a WebAssembly build. All quality gates green (fmt / clippy 1.86+stable+wasm32 / doc / no_std / native + both-wasm). See `CHANGELOG.md` `[1.0.0]`. The rows below this one are the engine-lineage history that produced this release. |
| *(engine lineage)* **v0.9.0** | superseded (historical) | 393 strict pass + 6 documented `#[ignore]`. Frontend MVP (window, audio, save state, rewind, debugger overlay, NTSC filter, rebind modal). 14 mappers. CHANGELOG dated; quality gates green. **Post-tag landings on `main`** raise this to **510 strict pass + 6 expected-fail `#[ignore]`'d** across Tracks C2 (VRC6 + 5B + N163 + MMC5 audio; VRC7 banking+IRQ with FM deferred per ADR-0004), C3 (polyphase BLEP / windowed-sinc synthesis with SFDR 81.61 dB), C4 (cargo-fuzz), C5 (`no_std + alloc` chip stack), C6 (thumbnails + ADR-0003 migration policy), B8 (cycle-resolution sprite-eval FSM), C1 pre-work (ADR-0002 Decision section + M2-phase IRQ tracing fixture + 6 golden baseline traces), C1 Phases A + B1 + B2/B3 (two-phase IRQ trace snapshots + `M2Phase` enum on `rustynes-cpu::scheduler` + `LockstepBus::current_m2_phase()` + `irq_snapshot_{mapper,apu}_at_{low,high}` + `Bus::poll_irq_at_phase`; +1 unit test), C1 Phase B4 success (cycle-precise MMC3 reload-pending Sharp discriminator flipping `mmc3_test_2/4` sub-test #2; `_currently_fails` probe now expects sub-test #3 residual), Phase 1A (AccuracyCoin pass-rate harness), and Phase D2 (AccuracyCoin RAM-direct per-test diagnostic decoder + 144-entry name/address catalog, surfacing the true `64.03%` pass rate across 50 named failing tests grouped by 20 upstream suites). **Post-Phase-D2 sequence on `main`** (chronological) then carries the row forward through Session-13: Phase D3 7-fix 6502 canonical bus-pattern landing (unofficial NOP DOP/TOP dummy reads; `$4020-$5FFF` open-bus floating-latch via new `Mapper::cpu_read_unmapped` trait method; absolute,X/Y page-cross dummy at unfixed address; canonical JSR cycle order with dummy stack read at `$0100\|S`; branch cycle-3 PC dummy + cycle-4 unfixed page-cross dummy; STA-family always-dummy at final address +`addr_ind_y` unfixed dummy; `$4015 / $4016 / $4017` open-bus semantics) lifting AccuracyCoin `64.03% → 67.63%` via the RAM-direct decoder; then C1 **Phase 3** OAM-DMA alignment audit (2026-05-15) flipping `cpu_interrupts_v2/4-irq_and_dma` from `#[ignore]` + `_currently_fails` probe to strict-pass and DELETING the probe (`67.63% → 69.06%`); then the 2026-05-17 recovery on `main` after the v0.9.0-rc accuracy-stabilization branch regressed SMB / Excitebike / Kid Icarus PAL — `main` rolled back to `10995f1`, `git bisect run` pinned `63d8dea` (B8b: flip sprite-eval FSM default to cycle-resolution) as first-bad, fix `834be9e` removed a destructive dot-64 reset that zeroed `spr_count` + `spr_shift_lo/hi[..]` + `spr_attr[..]` + `spr_x[..]` + `spr_zero_in_line` mid-scanline, and the sacred-trio boot-and-play was restored (`69.06% → 69.78%`); the same recovery cycle landed permanent regression-prevention infrastructure — 21-ROM permissive baseline harnesses at `crates/rustynes-test-harness/tests/{audio_tests,m22,mmc1_a12}.rs` (commit `6b3a818`), `scripts/regression-bisect/` turn-key `git bisect run` wrapper, the 60-ROM commercial-ROM oracle at `crates/rustynes-test-harness/tests/external_real_games.rs` (54 strict + 6 ignored, feature-gated on `commercial-roms`; commit `86691c8`) covering all 15 supported mappers each asserting ROM SHA-256 + framebuffer FNV-1a + audio FNV-1a + cumulative cycle count against committed `insta` snapshots, an 81-PNG visual baseline corpus at `screenshots/` (19 audio_tests + 1 m22 + 1 mmc1_a12 + 60 external across 19 mapper subdirs), and the new `docs/audit/` decision-rationale documentation tier with the WHY-axis `rom-library-buildout-2026-05-17.md` (commit `8eb66d6`) preserving the May-2026 audit including a 9-entry iNES-header-mismatch table; then Cascade B DMC DMA scheduler (commit `9b0c81c`, +11 AccuracyCoin tests: closes the entire `APU Registers and DMA tests` suite plus 3 side-benefits elsewhere, `69.78% → 76.98%`); then Cascade A OAMADDR-during-rendering reset (commit `f29f7ca`, hardware-accurate per nesdev "OAMADDR reset to 0 during dots 257-320 of every rendered scanline", +2 tests, `76.98% → 78.42%`); then session-7 OAMADDR-walks-during-eval + $4-aligned `$2004` write (commit `c230489`, closes`Address $2004 behavior` with code 16, `78.42% → 79.14%`); then session-7 RMW ABS,X/Y unfixed-address dummy read (commit `32d5b18`, 18 RMW opcodes get the canonical cycle-4 unfixed-address dummy via the SLO `$4015,X` bracket flipping `APU Tests :: Controller Clocking` + advancing `Implied Dummy Reads` 2→3 + `Frame Counter IRQ` 6→7, `79.14% → 79.86%`); then session-8 BG-pipeline cycle-9 reload + post-emit shift (commit`086ce4d`, architectural closure for the Cascade A`VerifySpriteZeroHits` step-2 geometric puzzle per `docs/audit/cascade-a-investigation-2026-05-19.md` — flipped Sprite 0 Hit behavior + Sprite overflow behavior + Suddenly Resize Sprite + $2007 read w/ rendering; +4 tests, +2.87pp the largest single-commit jump since Cascade B; 60-ROM commercial oracle re-baselined with framebuffer FNV-1a hashes shifted 1 column right, audio + cycle invariants byte-identical, `79.86% → 82.73%`); then Session-13 coordinated cold-boot alignment with Mesen2 —`Cpu::power_on()` reset SP from `0xFD` → `$00` + 8-cycle reset (Phase B Option B, commit `ea3cc4c`) and PPU scheduler power-up position aligned from dot=0 → dot=340 (the +344-dot empirical hypothesis from Sessions 10-12, commit`eb37ff8`) — provides the first contamination-free foundation for the next Track C1 IRQ-sample-point attempt by eliminating the SP-divergent stack writes + the +344-dot drift that masked all 11 prior C1 attempts; +3 new`cpu::power_on` unit tests in `crates/rustynes-cpu/tests/opcodes.rs`. **Final count:** **541 strict pass + 5 expected-fail`#[ignore]`'d** across 34 suites with`--features test-roms` (the 6th `#[ignore]` was deleted when `cpu_interrupts_v2/4-irq_and_dma` flipped to strict-pass in C1 Phase 3; the +1 from 540 → 541 is the Session-18 PPU `$2002`-race-window oracle unit test`vbl_race_window_2002_read_sweep`, the empirical truth-record that informed the Session-18 / C1-attempt-16 rollback); **with`--features test-roms,commercial-roms`: +60 commercial-ROM strict pass = 601 total**. Lines 56 and 267 are now consistent. Session-18 (2026-05-22) attempted C1 attempt 16 (PPU-axis`$2002` race-window predicate narrowing `dot <= 1` → `dot == 0` to match Mesen2 + nesdev) under feature flag `ppu-c1-attempt-16` and ROLLED IT BACK because the failing `cpu_interrupts_v2/{2,3,5}`reads land at scanline 241 dot 0 (not dot 1, where the predicate change only differs); the actual load-bearing axis is the intra-cycle CPU-vs-PPU access interleaving (`read1` reads BEFORE PPU ticks vs Mesen2's `MemoryRead` PPU-then-read order). See `docs/audit/session-18-c1-attempt16-ppu-axis-rollback-2026-05-22.md` and ADR-0002 §"Decision update (2026-05-22, Session-18)". |
| **v1.0.0** | **RELEASED 2026-05-23** | **AccuracyCoin RAM-direct 90.65%** (126/139 — gate cleared by 0.65pp). Workspace: **545 strict + 5 ignored** across 34 suites with `--features test-roms`; **+60 strict commercial-ROM oracle** with `--features test-roms,commercial-roms` (605 total, 60/60). All 10 validation gauntlet gates green (fmt / clippy / doc / no_std cross-compile / sacred trio SMB+Excitebike+Kid Icarus PAL preserved / B4 invariant preserved / ppu_vbl_nmi 10/10 / sprite_hit_tests 11/11 / sprite_overflow_tests 5/5 / apu_test 8/8 / dmc_dma_during_read4 5/5). Phase 6 v1.0.0-final closures: Phase 1a/b/d (internal/external bus split + SH* unstable stores +5 + `$4015` bit-5 Open Bus #9 +1), Phase 0 (Mesen2 `EventType::PpuCycle` patch documentation), Phase 3a/b (sprite-eval base from OAMADDR +2: Arbitrary Sprite zero + Misaligned OAM behavior, plus OAM-corruption row tracking +1: cleared the 90% gate). Cumulative AccuracyCoin progress 84.17% → 90.65% (+9 tests). 4 Track C1 IRQ-timing residuals (`cpu_interrupts_v2/{2,3,5}` + `mmc3_test_2/4` sub-test #3) deferred to **v2.0** with documented architectural rationale per `docs/audit/session-29-c1-axis-final-conclusion-2026-05-23.md` + `docs/audit/session-29-option-a-empirical-falsification.md`: Session-29 empirically demonstrated that Option (a) "comprehensive PPU re-baseline" (global +2 PPU dot init shift) does NOT close the C1 axis (cpu_interrupts_v2/{2,3,5}_strict still failed with `--include-ignored` because the global shift moves VBL set position and BIT $2002 read position uniformly, preserving the race-window relationship). Closing C1 requires changing the per-cycle PHASE RELATIONSHIP between CPU and PPU — Option (b) master-clock-precise scheduling refactor targeted for v2.0 (replace integer-PPU-dot-per-CPU-cycle model with Mesen2's fractional 12-master-clocks-per-CPU-cycle model). Permanent v2.0 infrastructure landed in v1.0.0: `cpu-c1-attempt-17-access-reorder` cargo feature on `rustynes-cpu` + `rustynes-core` + `rustynes-ppu` (φ1/φ2 split scaffold, default OFF), `rustynes-core::irq_trace` + 6 golden traces, `rustynes-cpu::M2Phase` + per-phase IRQ snapshots, `rustynes-ppu::vbl_race_window_2002_read_sweep` permanent oracle, Mesen2 source patch `EventType::PpuCycle` for per-PPU-cycle Lua callbacks (89342 events/frame verified), `scripts/cpu_boot_trace_pc_align.py` + `cpu_boot_trace_diff` + `mesen2_cpu_boot_trace.lua` cross-emulator diff tooling. |
| **v1.1.0** | **RELEASED 2026-05-25** | VRC7 OPLL FM audio (mapper 85) via a clean-room pure-Rust port of `emu2413 v1.5.9` (MIT) at `crates/rustynes-apu/src/opll.rs`; *Lagrange Point* produces in-game audio. ADR 0006 supersedes ADR 0004. Workspace **600 strict + 5 ignored** at the tag. AccuracyCoin 90.65% preserved. See `CHANGELOG.md` `[1.1.0]`. |
| **v1.2.0** | **RELEASED 2026-05-24** | DMC DMA get/put scheduler under default-off `dmc-get-put-scheduler` cargo feature (ADR 0007) — Mesen2's canonical get/put alternation alongside the v1.1.0 phase-agnostic model via the parallel-implementation pattern. Default build bit-identical to v1.1.0. **599 strict + 6 ignored** (default). AccuracyCoin 90.65% preserved. See `CHANGELOG.md` `[1.2.0]`. |
| **v1.4.0** | **RELEASED 2026-05-24** | **TAS movie recording & playback.** Versioned binary `.rnm` format (ADR 0008: ROM SHA-256 + optional `.rns` start point + per-frame input stream), `MovieRecorder`/`MoviePlayer` in `rustynes-core` (no_std), record/play/branch hotkeys (`F6`/`F7`/`F8`) + REC/PLAY overlay in the frontend; native `.rnm` save/load (wasm I/O is a follow-up). No API break (additive `Nes::buttons` getter; `run_frame` unchanged). Determinism proven by byte-identical round-trip tests. **636 strict + 8 ignored** (616 + 13 TAS-core + 7 TAS-frontend; 8 ignored = 5 strict probes + 3 doc-test examples); oracle 60/60; AccuracyCoin 90.65%; B4 + sacred trio unaffected. See `CHANGELOG.md` `[1.4.0]` + ADR 0008. |
| **v1.7.0** | **RELEASED 2026-05-25** | **Niceties milestone** (gap-analysis plan §2 follow-on). **Additive, independent of the v2.0 master-clock axis**; AccuracyCoin held **90.65%**, oracle 60/60, sacred trio + B4 byte-identical, determinism preserved. Landed: **Four Score 4-player** (bus `$4016`/`$4017` 24-read multiplex of 4 controllers + adapter signature `0x08`/`0x04`, nesdev/Mesen2/TetaNES; `Nes::set_four_score`, `set_buttons` ports 2/3; opt-in, OFF by default = byte-identical two-controller reads; Four Score state appended to the BUS save-state section with trailing-default-zero back-compat; a P3/P4 keyboard + gamepad rebind UI + a "Four Score" toggle); **raw RAM cheats** (`Nes::poke_ram` writing `$0000-$1FFF`, applied caller-side after `run_frame` — empty list = byte-identical; a `RawCheat` `$addr=$value [if $compare]` section in the cheat panel, persisted per-ROM as a `#[serde(default)]` `[[raw]]` array alongside Game Genie); an **in-app graphics/audio/rewind settings panel** (NTSC-filter + rewind-enable apply live; present-mode/sample-rate/rewind-sizing persisted + "restart to apply"). **702 strict + 10 ignored** (`--features test-roms`); +60 commercial oracle = 762 total. See `CHANGELOG.md` `[1.7.0]`. |
| **v1.6.0** | **RELEASED 2026-05-25** | **Frontend-polish milestone** (the v2.0.0 plan's original v1.5.0 content; gap-analysis plan §2). **Additive, independent of the v2.0 master-clock axis**; AccuracyCoin held **90.65%**, oracle 60/60, sacred trio + B4 byte-identical, determinism preserved. Landed: Game Genie cheats (core `rustynes-core/src/genie.rs` runtime overlay — off by default, NOT in the save-state — + a debugger cheat panel with per-ROM `<data_dir>/cheats/<sha>.toml` persistence); in-app gamepad rebinding UI (config-driven `[input.gamepad1/2]` + P2 keyboard rows + axis-as-dpad; serde-default reproduces the legacy Xbox layout); browser (wasm) `.rnm` movie download/upload + localStorage save-states; a non-flaky frame-time regression CI gate (`scripts/bench_regression_check.sh` + `bench` job, absolute 10 ms ceiling) + a rendering-heavy `nes_run_frame_flowing_palette` bench; `x86_64-apple-darwin` release target dropped (ADR 0009, Aug-2027 runner sunset). **688 strict + 10 ignored** (`--features test-roms`); +60 commercial oracle = 748 total. See `CHANGELOG.md` `[1.6.0]`. |
| **v1.5.0** | **RELEASED 2026-05-24** | **Phase 7 — Nesdev Accuracy Hardening** (the ROADMAP's genuinely-skipped phase). Coverage + region validation + developer ergonomics + documented scope closure; **additive only**, AccuracyCoin held **90.65%**, oracle 60/60, sacred trio + B4 byte-identical. Landed: blargg `instr_misc`/`instr_timing`/`cpu_reset` wired (+8 strict); `Nes::from_rom_with_power_on_seed` seeded power-on RAM randomization (developer mode; default path unchanged); NMI/IRQ B-flag + `$4015` open-bus guards; PAL/Dendy timing gates (per-region constant table + frame-structure integration test); VRC2/4 + M34 NINA-001 submapper fixtures (replacing the rotted `vrc24test`); `compatibility.md` platform-scope closure (FDS plan, Vs/PC10, PPU variants, input devices, long-tail policy). Deferred to v2.0 (master-clock axis): C1, `$2002` sub-cycle, SH\* internal-bus fix, stale-shifter, `$2007` rendering, FDS code, PAL 3.2:1 CPU:PPU ratio. **661 strict + 10 ignored** (`--features test-roms`); +60 commercial oracle = 721 total. See `CHANGELOG.md` `[1.5.0]` + `docs/audit/phase-7-*`. |
| **v1.3.3** | **RELEASED 2026-05-24** | Bug-fix patch (frontend-only; native unchanged): (1) wasm/Pages severe stutter + freezes (v1.3.2 regression) — wasm idle path `Poll`→`Wait` + unconditional rAF `request_redraw` re-arm; (2) wasm/WebGL2 palette wrong — GL pipeline kept UNORM (zero conversion, matches canvas-2D) since wgpu-hal double-encodes sRGB on the GL surface; native stays sRGB → pixel-identical; (3) native residual stutter — chunked pacer sleep + 2 ms spin margin. **616 strict + 6 ignored** (unchanged); oracle 60/60; AccuracyCoin 90.65%. Both wasm fixes need browser confirmation. See `CHANGELOG.md` `[1.3.3]`. |
| **v1.3.2** | **RELEASED 2026-05-24** | Bug-fix patch closing two v1.3.1 follow-ups: (1) dead keyboard input after the config migration — `parse_keycode` now accepts legacy winit-0.29 keycode names as aliases (repairs already-migrated configs with no manual action); the migration canonicalizes written values; (2) wasm/Pages stutter — the `wasm-winit` build now paces production from the rAF-synced `RedrawRequested` instead of `WaitUntil`/`setTimeout` (native pacing untouched; browser-confirmation pending). **616 strict + 6 ignored**; oracle 60/60; AccuracyCoin 90.65%. See `CHANGELOG.md` `[1.3.2]`. |
| **v1.3.1** | **RELEASED 2026-05-24** | Bug-fix patch: (1) green/garbage left-edge column — BG attribute shifters one tile out of phase with the pattern shifters (`086ce4d` regression), now 16-bit + lockstep (AccuracyCoin-neutral; PPU save-state v1→v2); (2) stutter — configurable present mode + native sleep-then-spin pacer replacing jittery `WaitUntil`; (3) legacy `config.toml` now migrated in place (backup + loud summary) instead of silently dropped. MM3 MMC3 stage-select shear confirmed not-a-regression, deferred to v2.0 (C1 axis). **608 strict + 6 ignored**; oracle 60/60; AccuracyCoin 90.65%. See `CHANGELOG.md` `[1.3.1]`. |
| **v1.3.0** | **RELEASED 2026-05-24** | **WebAssembly target.** `wasm32-unknown-unknown` frontend in two flavours (`wasm-winit` default = full winit+wgpu+egui, 2.12 MiB gzip; `wasm-canvas` ~316 KB embed); GitHub Pages deploy (`https://doublegate.github.io/RustyNES/`); CI `wasm` clippy job + 5 MiB size budget. No API/accuracy change (chip stack already `no_std + alloc`). **599 strict + 6 ignored** (unchanged). AccuracyCoin 90.65% preserved. See `CHANGELOG.md` `[1.3.0]`. |
| **v1.x (legacy v1.0.0 plan, archived)** | superseded | Coordinated IRQ-timing rework (Track C1) flips the 3 remaining `cpu_interrupts_v2/*` `#[ignore]`'d probes (`2-nmi_and_brk`, `3-nmi_and_irq`, `5-branch_delays_irq`; `4-irq_and_dma` already flipped to strict-pass by C1 Phase 3, 2026-05-15) + `mmc3_test_2/4` #3 (sub-test #2 already flipped by Phase B4). AccuracyCoin pass rate ≥ 90% (currently measured at **82.73%** on `main` via the RAM-direct decoder in `crates/rustynes-test-harness/src/accuracy_coin_catalog.rs`; trajectory `64.03% → 67.63% → 69.06% → 69.78% → 76.98% → 78.42% → 79.14% → 79.86% → 82.73%` across Phase D3 → Phase-3 OAM-DMA → FSM-fix recovery → Cascade B DMC DMA scheduler (commit `9b0c81c`) → Cascade A OAMADDR-during-rendering reset (commit `f29f7ca`) → session-7 OAMADDR-walks + $4-aligned `$2004` write (commit `c230489`) → session-7 RMW ABS,X/Y unfixed-address dummy read (commit`32d5b18`) → session-8 BG-pipeline cycle-9 reload + post-emit shift (commit`086ce4d`, Cascade A geometric puzzle closed at the architectural level — flipped 4 sprite-eval / PPU-behavior tests including Sprite 0 Hit behavior, Sprite overflow behavior, Suddenly Resize Sprite, $2007 read w/ rendering); the named failing tests are printed per-CI-run by`crates/rustynes-test-harness/tests/accuracycoin.rs` for actionable per-test fix loops). The remaining gap to 90% is 24 failing tests: 4 sprite-eval (post-BG-pipeline-fix residuals: $2002 flag timing, Arbitrary Sprite zero, Misaligned OAM behavior, OAM Corruption), 6 PPU misc (post-BG-fix residuals; Stale BG/Sprite Shift Regs + BG Serial In + Sprites On Scanline 0 + $2004/$2007 Stress Tests), 3 `cpu_interrupts_v2` + 1 `mmc3_test_2/4` #3 (Track C1 axis, 11 prior rollbacks), 5 SH* unstable stores (internal-bus model, deferred to v1.x), 1 `CPU Behavior :: Open Bus [error 9]`, 1`Implied Dummy Reads [error 3]`, and 4 APU residuals (Frame Counter IRQ #7, DMC, APU Reg Activation, Controller Strobing). Multi-OS release-artifact smoke test (T-51-009) closed. Real-game regression recovery (2026-05-17) landed on`main`: SMB / Excitebike / Kid Icarus PAL boot-and-play restored via fix`834be9e` (removed the destructive dot-64 sprite-eval reset introduced by B8b / first-bad `63d8dea`); the rolled-back commit range is preserved on the dedicated`accuracy-stabilization`branch, and the regression surface is now permanently guarded by the 60-ROM commercial-oracle harness (`crates/rustynes-test-harness/tests/external_real_games.rs`,`--features commercial-roms`) + the 81-PNG visual baseline corpus at`screenshots/`. Coordinated CPU/PPU power-up alignment (Session-13, commit`eb37ff8`) landed on`main` post-recovery, aligning the cold-boot CPU SP + PPU scheduler position with Mesen2 (clean foundation for the next Track C1 IRQ-sample-point attempt). |
| **v1.x** | future | **Partially landed post-v0.9.0:** VRC6 audio via `mapper-audio` feature (Track C2, mappers 24/26). Sunsoft 5B audio via the same flag (Track C2 / Phase 2.1, mapper 69; 3 squares + envelope generator + LFSR noise). Namco 163 wavetable audio via the same flag (Track C2 / Phase 2.2, mapper 19; 1-8 channels playing 4-bit wavetables from 128 B internal sound RAM). MMC5 audio via the same flag (Track C2 / Phase 2.3, mapper 5; 2 pulses + raw PCM). VRC7 banking + IRQ (Track C2 / Phase 2.4, mapper 85; FM synthesizer deferred per ADR-0004). Save-state thumbnails (Track C6, `THM` tagged section + ADR-0003 migration policy). `cargo-fuzz` harnesses for cartridge / CPU / mapper writes (Track C4). `no_std + alloc` migration of the chip stack **complete** (Track C5; `thiserror = "2.0"` bump, `#![no_std]` + `extern crate alloc;` on `rustynes-cpu` / `rustynes-ppu` / `rustynes-apu` / `rustynes-mappers` / `rustynes-core`, CI cross-compile gate `cargo build -p rustynes-core --target thumbv7em-none-eabihf --no-default-features`; `rustynes-frontend` stays `std`-only). **Shipped since:** VRC7 FM synthesis (YM2413 OPLL clean-room port, v1.1.0); polyphase BLEP audio (Track C3); WebAssembly target (v1.3.0). **Still deferred:** mobile / netplay. (TAS shipped v1.4.0; WebAssembly v1.3.0.) |
| **post-v1.0 (all delivered in v1.0.0)** | delivered | The entire engine-lineage "v2.0" slate landed in the v1.0.0 production cut: the **master-clock-precise scheduler** (now the default and only core — the 4 C1 residuals are closed), **FDS**, **rollback netplay** (native UDP + browser WebRTC), **TAS recording**, AND **Vs. System / PlayChoice-10** (no longer out of scope — RGB-PPU boards ship). Forward work is tracked in [`to-dos/v1.0.1-compat-hygiene/`](../to-dos/v1.0.1-compat-hygiene/overview.md) + [`to-dos/v1.1.0-features/`](../to-dos/v1.1.0-features/overview.md). Still out of scope: pirate / multicart long-tail mappers (per-title demand only). |
