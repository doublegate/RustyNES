# RustyNES — Deferred / Carryover / Incomplete Feature Catalogue

> **Purpose.** A single follow-up backlog that reconciles **every** deferred,
> carried-over, manual-verify, staged-but-inert, and not-yet-implemented feature
> surfaced across the whole RustyNES development history (v1.0.0 synthesis →
> v1.2.0 "Curator" → v1.3.0 "Bedrock" → v1.4.0 "Fidelity" → v1.5.0 "Lens" →
> v1.6.0 "Studio" → v1.7.0 "Forge" → v1.8.0 … v1.8.8 "Atlas" (the Android train)
> → the v1.9.0 iOS plan → v2.0.0 "Timebase" → the v2.0.x mobile-finalization
> train) against the **current code on `main`**. Items already shipped since they
> were written are excluded; everything below was verified still-outstanding.
>
> **Reconciled against `main` @ `de682d8` on 2026-06-19** (the v1.7.0 "Forge"
> baseline; the entries below remain the authoritative deferred backlog).
>
> **Current state (2026-06-23).** v1.7.0 "Forge" (+ the v1.7.1 patch) and the full
> Android train **v1.8.0 … v1.8.8 "Atlas"** have since **shipped**; **v1.8.9** is
> now in development (Dependabot consolidation #180, the dormant
> `rustynes-monetization` crate, a held UX fix). Mapper coverage is **168
> families**. Most of the v1.7.0 H reach/polish items that this file lists as
> "beta.5 / not yet on `main`" (audio depth, per-game `<rom>.json` overrides + DIP
> editor + lag counter, web/wasm parity, i18n, spectator netplay, browser-RA HUD,
> the `full` native build) **landed in v1.7.0**; the genuinely-future items
> (v2.0.0 accuracy residuals, mobile follow-ups, long-tail mappers, the
> maintainer-manual verifies) remain outstanding as listed. The **mobile-launch
> deferral** + the **`foss`/`play` flavor split** below are the current canonical
> forward framing.
>
> **v1.8.9 reconciliation (2026-06-23).** A full re-verification of §1–§5, §7, §8,
> §11, §12 against current `main` reclassified every item; the per-item markers
> below are updated to match. Three buckets:
>
> - **Shipped (now `[x]`):** movie import `.fcm`/`.fmv`/`.vmv`/`.fm2`/`.bk2` (§1i;
>   `.mc2` rejected by design — PCE format), wasm-Lua done-as-designed (§2b, ADR
>   0012), spectator netplay (§3c), the RA `ra_glue.js` trampoline (§4a), audio
>   depth/H3 (§5a; per-APU-channel volume is an intentional v2.0 deferral),
>   mappers→168 + zero-library present + m176 FK23c defined + m30/m80/m185
>   blank-boot + `.zip`/`.7z`/`.fds` harness (§7a/b/c/d/g), per-game/DIP/lag H4 +
>   i18n H5 + **web/wasm H6** (§8a/b/c), the `full` alias (§11f), Kid Icarus FDS
>   (§12b).
> - **Open + CI-doable → the v1.8.9 "Backlog" beta train** (additive/off-by-default,
>   core byte-identical, AccuracyCoin 139/139 held): the §1 creator tooling (Virtual
>   Pad, Input Macros, BasicBot, multi-viewport, A/V codec depth, FDS Firmware
>   Manager, Multi-Disk Bundler, Batch Runner, `.fm2` rerecord-count export), §2
>   `userdata` SQLite, §4d RA-HUD visual finish, §5b HD-pack spatial conditions, §7
>   UNIF board-map + remaining broken-boots + ASIC-cluster continuation, §11 CI
>   hardening (cargo-hack / arm64 leg / per-job skips / nextest), §12a NSF-viz depth,
>   plus the `deny.toml` stale-advisory cleanup.
> - **`[M]` maintainer-manual (cannot be CI-certified):** live WebRTC matrix +
>   cross-NAT UDP (§3a/b), RA auth-proxy deploy + live-account verify + allowlisting
>   (§4b/c/e), A/V-recording + shader/NTSC + HD-audio output verifies (§5c/d/e), m176
>   `.WXN` re-staging + remaining-broken-boot snapshot re-bless (§7), the `merge_group`
>   adoption decision (§11d) — listed under their themes below.

## How to read this

- Each item: **name** — what it is · why deferred · where tracked (plan / ADR /
  doc) · target release · relevant files/crates.
- Status markers:
  - `[ ]` open / not started
  - `[~]` in flight or partially landed (scaffolding present, finish pending)
  - `[M]` maintainer-manual — cannot be CI-self-certified (needs a device,
    a live account, a GPU/audio device, or a hosted deploy)
  - `[x]` resolved / rejected — already landed, or deliberately decided
    against (kept here for the record)
- Plan cross-links are relative to this file (`to-dos/`):
  [v1.6.0](plans/v1.6.0-studio-plan.md) ·
  [v1.7.0](plans/v1.7.0-forge-plan.md) ·
  [v1.8.0](plans/v1.8.0-android-plan.md) ·
  [v1.9.0](plans/v1.9.0-ios-plan.md) ·
  [v2.0.0](plans/v2.0.0-master-clock-plan.md) ·
  [v2.0.x mobile finalization](plans/v2.0.x-mobile-finalization-plan.md).

> **Mobile-launch deferral (maintainer, 2026-06-23).** The Android (v1.8.x) and iOS
> (v1.9.0) **app-store launches** are deferred to **after v2.0.0 "Timebase"**:
> Android finalized across v2.0.1–v2.0.4, iOS across v2.0.5–v2.0.8, both verified in
> v2.0.9, and **launched jointly at v2.1.0**. Until then the apps ship as Android
> **GitHub-sideload** + iOS **TestFlight** only; the Play-services / Integrity /
> Chromecast flags stay default-off. The old Google-Play "Workstream P" task is now a
> v2.1.0 line item. See
> [v2.0.x mobile finalization](plans/v2.0.x-mobile-finalization-plan.md).
>
> **`foss` / `play` Android flavor split (v2.1.0, ADR 0025).** Split the Android build into
> a **`foss`** flavor (default — no Google SDKs, no ads, no tracking; the **F-Droid** +
> sideload artifact) and a **`play`** flavor (all proprietary SDKs — Billing / Cast / Play
> Games / Integrity / update-review **+ AppLovin + RevenueCat** — for Google Play). F-Droid
> requires a Google-/ad-free build, so the flavor is the only way to reach that channel. The
> five existing proprietary subsystems move behind `src/play/` façades (no-op in `src/foss/`),
> with an `installDebug` alias; both flavors verified on-device at v2.0.9. **Done in v1.8.9
> (dormant):** the clean `rustynes-monetization` crate `.so` + UniFFI bindings are wired into
> the Android build. Target: **v2.0.1–v2.0.4 (lands by v2.1.0)**.

---

## 1. Creator / TAS tooling (v1.7.0 H9 + breadth)

These are the v1.7.0 "Forge" **H9 power-user niceties** the plan scopes as S–M
each but that are **not present in `crates/` on `main`** (verified absent: no
`virtual_pad`, `basic_bot`, `multi-viewport`, `firmware_manager`, `multi_disk`,
`batch_runner` symbols). They are slated for beta.5 (the H reach/polish train).

- `[ ]` **Virtual Pad** — a clickable on-screen controller that feeds
  `SharedInput` (BizHawk parity). Source: [v1.7.0](plans/v1.7.0-forge-plan.md) H9.
  Target: **v1.7.x (beta.5)**. Files: `crates/rustynes-frontend/src/` (new
  input-overlay module) + `SharedInput`.
- `[ ]` **Input Macros / templates** — record/replay short input macros that feed
  the TAStudio piano-roll pattern-paint. Source: [v1.7.0](plans/v1.7.0-forge-plan.md)
  H9 (pairs with the v1.6.0 piano-roll). Target: **v1.7.x (beta.5)**. Files:
  `crates/rustynes-frontend/src/tastudio*` + input layer.
- `[ ]` **BasicBot** — a save-state-anchored brute-force search tool (pairs with
  the B1 `tastudio.*` API + Lua `frameadvance`). Source:
  [v1.7.0](plans/v1.7.0-forge-plan.md) H9. Target: **v1.7.x (beta.5)**. Files:
  frontend + `crates/rustynes-script`.
- `[ ]` **Multi-monitor / detachable tool windows** — egui multi-viewport so the
  debugger/TAStudio panels can pop out into OS windows. Source:
  [v1.7.0](plans/v1.7.0-forge-plan.md) H9. Target: **v1.7.x (beta.5)**. Files:
  `crates/rustynes-frontend/src/debugger/`.
- `[ ]` **A/V dump-options depth (codec / sync)** — extend the v1.6.0 `av_record`
  ffmpeg tap with codec selection + A/V-sync controls. Source:
  [v1.7.0](plans/v1.7.0-forge-plan.md) H9 (`av_record.rs` exists; the depth does
  not). Target: **v1.7.x (beta.5)**. Files:
  `crates/rustynes-frontend/src/av_record.rs`.
- `[ ]` **FDS Firmware Manager** — FDS BIOS hash-verify/resolve UI (supports the
  v1.6.0 FDS-proper work). Source: [v1.7.0](plans/v1.7.0-forge-plan.md) H9.
  Target: **v1.7.x (beta.5)**. Files: frontend FDS handling.
- `[ ]` **Multi-Disk Bundler** — named FDS / multicart slot management. Source:
  [v1.7.0](plans/v1.7.0-forge-plan.md) H9. Target: **v1.7.x (beta.5)**. Files:
  frontend FDS / cart loader.
- `[ ]` **Batch Runner (headless)** — consolidate the ad-hoc per-mapper
  screenshot/movie-verify scripts into a first-class headless mode (reuses the
  boot-smoke harness + screenshot corpus). Source:
  [v1.7.0](plans/v1.7.0-forge-plan.md) H9 + the Batch-Runner reuse note. Target:
  **v1.7.x (beta.5)**. Files: `scripts/`, `crates/rustynes-test-harness/`.
- `[x]` **Broad movie-format import (G4 remainder)** — *(shipped; v1.8.9 reconcile:
  `.fcm`/`.fmv`/`.vmv` in `legacy_movie.rs`, `.fm2`/`.bk2` in `movie_interop.rs`; `.mc2`
  rejected by design as a PC-Engine format. The only remainder — `.fm2` rerecord-count
  on export — is a v1.8.9 beta.2 item.)* — the G4 commit landed the
  legacy importer, but the full TASVideos pre-`.fm2` corpus breadth (`.fcm`,
  Famtasia `.fmv`, VirtuaNES, Mednafen `.mc2`) + `.fm2`/`.bk2` export hardening
  (rerecord count, MD5/SHA hashing) should be confirmed/finished. Source:
  [v1.7.0](plans/v1.7.0-forge-plan.md) G4. Target: **v1.7.x**. Files:
  `crates/rustynes-frontend/src/movie.rs`.

---

## 2. Debugger / scripting depth

The v1.7.0 A/B/C/E debugger + scripting workstreams are merged on `main`; what
remains are the optional/SQLite tails and any beta.5 polish.

- `[ ]` **`userdata.*` SQLite backend (E3 optional)** — the `userdata` KV store is
  in scope; the optional SQLite-backed persistence was scoped "optional / later"
  by the maintainer. Source: [v1.7.0](plans/v1.7.0-forge-plan.md) E3 + Maintainer
  decisions. Target: **TBD (v1.7.x or later)**. Files: `crates/rustynes-script`.
- `[x]` **Browser / wasm Lua maturity** — *(done-as-designed; v1.8.9 reconcile: the
  `piccolo` backend ships in `piccolo_backend.rs` + `wasm_script.rs`; per ADR 0012 the
  per-access callbacks are intentionally no-ops on wasm — non-parity is the design, not a
  finish-pending.)* — the native mlua engine is
  feature-complete; the wasm piccolo hooks are **no-ops in the browser** (ADR
  0012, explicitly *not* byte-parity with native mlua). v1.7.0 H6 targets
  "Lua-in-browser." Source: [v1.7.0](plans/v1.7.0-forge-plan.md) H6 +
  [ROADMAP](ROADMAP.md) "Beyond v2.0.0". Target: **v1.7.x (beta.5) / TBD**.
  Files: `crates/rustynes-frontend` (wasm), `crates/rustynes-script` (piccolo),
  `docs/adr/0012-wasm-lua-piccolo-backend.md`.

---

## 3. Netplay

- `[M]` **Live 2–4-player browser WebRTC netplay matrix** — the full deploy
  bundle (signaling + Caddy TLS + coturn) builds and is turn-key, and the wire
  protocol/lobby are unit-tested, but a live N-browser run on a real host/domain
  has **no headless path**. Walk the `deploy/README.md` checklist (2-tab →
  2-machine → 4-player + ops/DNS/TLS/TURN-bandwidth) and flip
  `docs/netplay-webrtc.md` §4 "Pending" → "Verified". Source:
  `docs/netplay-webrtc.md` §4; the long-standing v1.2.0-era **F3** carryover.
  Target: **maintainer-manual**. Files: `deploy/`, `docs/netplay-webrtc.md`,
  `crates/rustynes-netplay`.
- `[M]` **Real cross-NAT UDP traversal** — needs a STUN server + two real NATs;
  unverifiable in CI. Source: `docs/netplay-webrtc.md` §4. Target:
  **maintainer-manual**.
- `[x]` **Spectator netplay (H8)** — *(shipped; v1.8.9 reconcile: complete in
  `crates/rustynes-netplay/src/spectator.rs` — `SpectatorSession<T: Transport>`,
  receive-only, determinism-safe, `MAX_SPECTATOR_FRAME_LOOKAHEAD` DoS bound, unit-tested;
  wired in `netplay_ui.rs`.)* a determinism-safe read-only extension of the
  rollback stack (pairs with the live-matrix verify + adaptive input buffer +
  auto config-resync). Source: [v1.7.0](plans/v1.7.0-forge-plan.md) H8.
  Files: `crates/rustynes-netplay`, frontend Netplay panel.

---

## 4. RetroAchievements (browser — the ADR 0015 carryover)

Native RA is shipped and unaffected. The browser (casual-only) path landed as
*scaffolding* in v1.5.0 behind off-by-default `browser-cheevos`; finishing it is
the v1.7.0 **H1/H2** workstream + a maintainer-manual deploy/verify.

- `[x]` **`ra_glue.js` rc_client trampoline marshalling** — *(shipped; v1.8.9
  reconcile: `crates/rustynes-frontend/web/cheevos/ra_glue.js` implements all
  three trampolines — read-memory, server-call, event-handler — via `addFunction`, plus the
  session-epoch use-after-free guard, `ra_init`/`ra_do_frame`/`ra_begin_login`/`ra_load_game`.
  The auth-proxy DEPLOY + live verify remain `[M]` below.)* Source: ADR 0015;
  `docs/cheevos-browser.md` §Status; [v1.7.0](plans/v1.7.0-forge-plan.md) H1.
  Files: `crates/rustynes-frontend/web/cheevos/ra_glue.js`, `scripts/cheevos/`.
- `[M]` **Auth-proxy deploy** — stand up a host + TLS + hardened CORS origin and
  point `RA_PROXY_BASE` at it (until set, `proxy_configured()` is `false`).
  Source: `docs/cheevos-browser.md` §Auth proxy contract; ADR 0015. Target:
  **maintainer-manual**. Files: `scripts/cheevos/auth-proxy.example.toml`,
  `scripts/cheevos/auth_proxy_stub.py`, `crates/rustynes-frontend/web/cheevos/ra_glue.js`.
- `[M]` **Live-browser verify with a real RA account** — no headless path;
  mirrors the v1.2.0 F1/F3 carryovers the maintainer accepted. Source: ADR 0015;
  `docs/cheevos-browser.md` §Status. Target: **maintainer-manual**.
- `[ ]` **RA HUD completion (H2)** — surface data RustyNES already decodes then
  drops: leaderboard scoreboard (#N of M), progress/challenge indicators,
  progress-bars/buckets/rarity, hardcore **pause-gating** (`rc_client_can_pause`).
  Source: [v1.7.0](plans/v1.7.0-forge-plan.md) H2. Target: **v1.7.x (beta.5)**.
  Files: `crates/rustynes-cheevos`, frontend RA panel.
- `[ ]` **Live RA-account allowlisting pass** — the `RustyNES/<ver> rcheevos/<ver>`
  User-Agent is already sent; allowlisting with the RA team is a request, not a
  code change. Source: [ROADMAP](ROADMAP.md) "Beyond v2.0.0". Target:
  **maintainer-manual**.

---

## 5. Audio / Video (v1.7.0 H3 + HD-pack parity)

- `[x]` **Audio depth (H3)** — *(shipped; v1.8.9 reconcile: stereo panning + Schroeder
  reverb + crossfeed in `audio_dsp.rs`, the 20-band ISO third-octave EQ in `eq.rs`, the
  output-device picker in `audio.rs` + `settings_panel.rs`; all bypass to byte-identical at
  defaults. Per-APU-channel context volume is an intentional v2.0 deferral, not a gap.)*
  stereo panning, reverb/crossfeed, output device picker, 20-band EQ. Source:
  [v1.7.0](plans/v1.7.0-forge-plan.md) H3. Files:
  `crates/rustynes-frontend/src/{audio,audio_dsp,eq}.rs` +
  `crates/rustynes-frontend/src/debugger/settings_panel.rs`.
- `[ ]` **Full Mesen HD-pack parity** — beyond the shipped `<condition>` /
  `<background>` rules + HD audio: neighbor predicates / palette-key matching /
  the remaining Mesen rule set. Source:
  [v1.5.0](plans/v1.5.0-lens-plan.md) + [v1.4.0](plans/v1.4.0-fidelity-plan.md)
  deferrals. Target: **TBD**. Files: `crates/rustynes-frontend/src/hdpack.rs`,
  `crates/rustynes-core` PPU tile-source export.
- `[M]` **A/V recording playback verify** — the v1.6.0 `av_record` tap is gated
  off and CI cannot exercise the codec/mux path; live recording playback is a
  maintainer manual-check. Source: CHANGELOG `[Unreleased]`; v1.6.0 release notes.
  Target: **maintainer-manual**. Files: `crates/rustynes-frontend/src/av_record.rs`.
- `[M]` **Shader / NTSC visual output verify** — shader and filter output can't be
  verified headlessly; documented as a maintainer manual-verify. Source: CHANGELOG
  `[Unreleased]`. Target: **maintainer-manual**.
- `[M]` **HD-audio (`<bgm>`/`<sfx>` OGG) playback verify** — no audio device in
  CI. Source: CHANGELOG `[Unreleased]`. Target: **maintainer-manual**.

---

## 6. Accuracy → v2.0.0 "Timebase" (the master-clock rewrite, ADR 0002)

All remaining hard-tier accuracy residuals share **one root cause** and converge
on the v2.0.0 one-clock + every-cycle-bus-access refactor. They are **outside the
AccuracyCoin oracle** (zero production-ROM impact; AccuracyCoin is 100% / 139/139
on the shipping default core). The maintainer's standing decision through v1.7.0
is "keep deferring" point-fixes (ADR 0002 stop-condition; 15+ documented
rollbacks); v2.0.0 is the one release licensed to break save-state/determinism and
take this on. See [v2.0.0 plan](plans/v2.0.0-master-clock-plan.md) and
`docs/adr/0002-irq-timing-coordination.md`.

### 6a. The timebase rewrite itself

- `[ ]` **One monotonic master clock (A1)** — collapse the five-counter substrate
  (`Cpu::master_clock`, `Cpu::cycles`, `LockstepBus::cycle`/`ppu_clock`,
  `Apu::cpu_cycle` + `apu_phase`/`put_cycle` parity + DMC byte-timer) to a single
  `master_clock: u64` with everything else derived by fixed arithmetic. Target:
  **v2.0.0**. Files: `crates/rustynes-cpu/src/cpu.rs`,
  `crates/rustynes-core/src/bus.rs`, `crates/rustynes-apu/src/apu.rs`.
- `[ ]` **Every cycle is a bus access (A2)** — replace the `dispatch()`-length +
  `idle_tick` burn-loop and the `dma-cycle-budget` hack with a per-cycle
  read/write/dummy-read model (interleaved DMA). The make-or-break beta.2
  stop-or-go gate. Target: **v2.0.0**. Files: `crates/rustynes-cpu/src/cpu.rs`,
  `crates/rustynes-core/src/bus.rs`.
- `[ ]` **Reload arm invisible to its own cycle (A3)** — `pending_dmc_dma_next`
  latch promoted at the next boundary. Target: **v2.0.0**. Files:
  `crates/rustynes-apu/src/apu.rs`.
- `[ ]` **Cycle-accurate reset (A4)** — replace the function-call `Nes::reset()`
  with a real reset sequence (reset-vector-delay cycles + frame-counter re-arm).
  Target: **v2.0.0**. Files: `crates/rustynes-core/src/nes.rs`.

### 6b. The residuals it unlocks (R1–R5)

- `[ ]` **R1 — `mmc3_test_2/4` #3 (1-CPU-cycle "IRQ sooner" bracket)** — the
  CPU `T_last-1` IRQ-sample M2 sub-cycle phase; the integer 3-dots-per-cycle
  timebase **cannot represent** it. **The 17-rollback graveyard / hard target with
  a bounded-effort escape hatch** (fall back to by-design `#[ignore]` rather than
  risk a 16th rollback of the sacred 100%). Site: `tests/m004_mmc3.rs:64,167`. Target:
  **v2.0.0 (escape-hatch-able)**.
- `[ ]` **R2 — `mmc3_test_2/4` #2 reload-to-0 cadence + MMC6 variant** — same M2
  sub-cycle axis as R1. Site: `tests/m004_mmc3.rs:187,207`. Target: **v2.0.0
  (escape-hatch-able)**.
- `[ ]` **R3 — `apu_reset/len_ctrs_enabled` (FAIL #3)** — needs A4's
  cycle-accurate reset. Site: `tests/apu_reset.rs:113`. Target: **v2.0.0**.
- `[ ]` **R4 — `apu_reset/4017_written` (FAIL #3)** — same cycle-accurate-reset
  axis. Site: `tests/apu_reset.rs:138`. Target: **v2.0.0**.
- `[ ]` **R5 — DMC reload-DMA span `Y=3` vs hardware `Y=4`** — five-counter parity
  drift; falls out naturally once A2+A3 hold. Target: **v2.0.0**.
- `[ ]` **`$2002` / NMI-suppression sub-cycle race** — part of the same fractional
  timebase; representable only post-rewrite. Source: [v2.0.0
  plan](plans/v2.0.0-master-clock-plan.md) Out-of-scope; ADR 0002. Target:
  **v2.0.0**.
- `[ ]` **`$2007` rendering blocking-read sub-cycle** — the PPUDATA state-machine
  reload / `v`-increment glitch. Source: CHANGELOG `[Unreleased]`; ADR 0002.
  Target: **v2.0.0**.
- `[ ]` **Exact PAL 3.2:1 fractional alignment** — already integer-correct on the
  shipping core; the v2.0.0 plan preserves the dividers exactly (NTSC ÷12/÷4, PAL
  ÷16/÷5). Listed as a residual closed-by-construction. Target: **v2.0.0**.
- `[ ]` **Sprite-0 stale-shifter / internal-vs-external bus-split** — lowest-value;
  ares omits it; partly entangled with the v2.0 axis. Attempt only if a Mesen2
  single-cycle trace oracle is wired and a real game demands it. Source:
  [v1.5.0](plans/v1.5.0-lens-plan.md); [v2.0.0
  plan](plans/v2.0.0-master-clock-plan.md). Target: **v2.0.0 / leave documented**.

### 6c. Other v2.0-axis items

- `[ ]` **CPU-multiplier overclock** — distinct from the F3 dot-resolution
  scanline-insert overclock (which shipped off-by-default in v1.7.0 beta.1);
  needs the timebase rewrite. The v1.5.0 "Enhancements" group's
  **sprite-limit-disable + overclock** controls are **staged-but-inert** pending
  this. Source: [v1.5.0](plans/v1.5.0-lens-plan.md) D; [v1.7.0
  plan](plans/v1.7.0-forge-plan.md) F3 note; ADR 0002. Target: **v2.0.0**. Files:
  frontend Enhancements settings group; `crates/rustynes-cpu`.
- `[ ]` **Full Vs. DualSystem dual-core (C)** — second CPU + PPU + bus
  arbitration, surfaced via an `Emu { Single, Dual }` enum API break. Detection
  shipped (v1.3.0 D2) + a frontend note; full emulation has no committable
  test-ROM oracle. Design: `docs/audit/vs-dualsystem-design-2026-06-11.md`.
  Source: [v2.0.0 plan](plans/v2.0.0-master-clock-plan.md) C. Target: **v2.0.0
  (or a v1.x point release — open maintainer decision)**. Files: new
  `crates/rustynes-core/src/vs_dualsystem.rs`.
- `[ ]` **Breaking-API + save-state v3 cleanup (D)** — CPU section v2→v3, `.rns`
  `FORMAT_VERSION` bump with clean-reject of v1.x slots (no migration code), `.rnm`
  honest verify-replay break, retire the dead experiment feature flags
  (`cpu-c1-attempt-17-access-reorder`, `ppu-2002-read-end-flags`, the `mc-r1-*` /
  `dmc-get-put-scheduler` stubs), ADRs 0016/0017 + a 0002 update. Source:
  [v2.0.0 plan](plans/v2.0.0-master-clock-plan.md) D; ADR 0003. Target:
  **v2.0.0**.
- `[ ]` **OAM / open-bus DRAM decay** — by-design omission; no game depends.
  Document only; do not implement. Source:
  [v1.4.0](plans/v1.4.0-fidelity-plan.md), [v1.5.0](plans/v1.5.0-lens-plan.md).
  Target: **by-design (documented)**.

### 6d. By-design non-targets (recorded for completeness — do NOT implement)

- `mmc3_test_2/6` (NEC rev B) — RustyNES defaults to Sharp rev A; mutually
  exclusive (R6).
- `cpu_reset` full-protocol ×2 (interactive) — needs an externally-timed reset the
  headless handler can't supply (R7).

---

## 7. Mapper / coverage gaps

Mapper coverage is **168 families** on `main` (BestEffort, honesty-gated). Gaps
are ROM-availability/coverage and a detection follow-up — none affect the oracle.

- `[~]` **Next reusable-ASIC BMC/pirate cores (G1 continuation → ~170–185)** —
  *(150 → 168 shipped in v1.7.0 beta.1; the → ~170–185 continuation is a
  v1.8.9 beta.6 item where free dumps exist.)*
  FK23C / COOLBOY / MINDKIDS / Sachen / Waixing / Kaiser clusters, honesty-gated.
  The long-tail toward the full ~300–370 set continues incrementally. Source:
  [v1.7.0](plans/v1.7.0-forge-plan.md) G1; [v2.0.0 plan](plans/v2.0.0-master-clock-plan.md)
  E. Target: **v1.7.x → v2.0+**. Files: `crates/rustynes-mappers/src/sprintN.rs`.
- `[ ]` **Zero-library mappers (no freely-available ROM)** — families 28, 29, 31,
  39, 81, 174, 179 have no freely-available dump, so they have no committed
  screenshots (register-decode unit-tested only). Source: the standing
  mapper-ROM-coverage policy. Target: **backfill via homebrew if available / TBD**.
  Files: `tests/roms/external/`, `screenshots/`.
- `[ ]` **`m176` Waixing FS005 detection follow-up** — three `.WXN` Chinese dumps
  are currently misdetected as m30 (UNROM-512); they need proper m176 board
  support + re-staging. Not an m30 bug. Source: the blank-boot-fixes memory note.
  Target: **follow-up**. Files: `crates/rustynes-mappers`, frontend `game_db`.
- `[ ]` **Broken-boot residuals (blank/incomplete render)** — the v1.6.0 E-mapper
  coverage pass documented broken-boot cases (e.g. around m50/51/205/245/290 +
  m244/250 + some Vs.System multicart/menu titles). The m30/m80/m185 blank-boot
  fixes landed; remaining broken-boots + the multicart/Vs-menu `capture_override`
  boot-smoke polish are open. Source:
  `docs/testing/v1.6.0-e-mapper-coverage-2026-06-18.md`; the blank-boot-fixes memory
  note; the coverage-harness reuse note. Target: **v1.7.x / follow-up**. Files:
  `crates/rustynes-test-harness/tests/external_coverage.rs`, `screenshots/`.
- `[ ]` **`m301` / `m348` UNIF board-map entries** — UNIF board names that still
  need wiring into the loader's board map (the UNIF loader shipped in v1.6.0;
  m301 A7-outer-bank was patched, the board-map breadth continues). Source:
  v1.6.0 fix train; [v2.0.0 plan](plans/v2.0.0-master-clock-plan.md) E (UNIF).
  Target: **v1.7.x → v2.0+**. Files: `crates/rustynes-mappers` UNIF board map.
- `[x]` **Snapshot re-bless after blank-boot fixes** — *(done; v1.8.9 reconcile: commit
  `c286e63` re-blessed the `external_coverage` snapshots after the m30/m80/m185 fixes. Any
  FUTURE broken-boot fixes in beta.6 will need their own re-bless — that residual stays `[M]`.)*
  Source: the blank-boot-fixes memory note.
- `[x]` **`.zip`/`.7z`/`.fds` coverage-harness support (#59)** — *(shipped; v1.8.9
  reconcile: `external_coverage.rs` discovers `.zip`/`.7z`/`.fds` and `tests/common/mod.rs`
  `load_nes()` unwraps archives + routes FDS through `Nes::from_disk()`.)* mirror the
  frontend load dispatch so it can screenshot archived/FDS ROMs. Source: v1.7.0
  beta.5 carryover (#59). Files:
  `crates/rustynes-test-harness/tests/external_coverage.rs` +
  `crates/rustynes-test-harness/tests/common/mod.rs`.
- `[ ]` **Full ~300-mapper set + 100% TASVideos compatibility** — the original
  ambitious v1.0.0 bar, redefined down to "production-quality + hardware-accurate"
  and pursued incrementally ever since. Source:
  [v1.0.0 synthesis](plans/v1.0.0-synthesis-plan.md); [ROADMAP](ROADMAP.md).
  Target: **long-tail / no fixed version**.

---

## 8. Reach / polish (v1.7.0 H4/H5/H6) — not yet on `main`

- `[x]` **Per-game `<rom>.json` config overrides + DIP editor + lag counter
  (H4)** — *(shipped; v1.8.9 reconcile: `crates/rustynes-frontend/src/per_game.rs` (the
  `<rom>.json` overlay, region/mapper/submapper/mirroring/DIP, data-dir overlay wins over the
  sibling); lag counter in `emu.rs` + `ui_shell.rs`. Determinism firewall: overrides flow
  through the v1.2.0 game-DB path; the core + harness never read files.)* Source:
  [v1.7.0](plans/v1.7.0-forge-plan.md) H4. Files: `crates/rustynes-frontend/src/per_game.rs`.
- `[x]` **i18n framework (H5)** — *(shipped; v1.8.9 reconcile:
  `crates/rustynes-frontend/src/i18n.rs` — compile-time string catalogs, `tr(Key::*)` +
  `CURRENT_LOCALE` atomic, English (byte-identical default/fallback) + Spanish; wasm-safe.
  Further per-panel string conversion can continue per the established pattern.)* Source:
  [v1.7.0](plans/v1.7.0-forge-plan.md) H5. Files: `crates/rustynes-frontend/src/i18n.rs`.
- `[x]` **Web / wasm parity (H6)** — *(shipped; v1.8.9 reconcile: File System Access
  API + `?settings=` in `wasm_io.rs` + `wasm_share.rs`, Gamepad API in `wasm_gamepad.rs`,
  PWA/offline via `crates/rustynes-frontend/web/manifest.webmanifest` +
  `crates/rustynes-frontend/web/sw.js`.)* File System Access API,
  Gamepad API, PWA/offline, base64 `?settings=` share-links. Source:
  [v1.7.0](plans/v1.7.0-forge-plan.md) H6. Files: `crates/rustynes-frontend` (wasm) + `web/`.

---

## 9. Mobile (v1.8.0 Android / v1.9.0 iOS) — entirely future

Both mobile plans are locked but unstarted (no mobile crates on `main`). Each
ships a focused MVP and defers the same heavyweight subsystems to a follow-up
mobile point release.

- `[ ]` **v1.8.0 "Android" MVP** — a hybrid + focused-MVP Android frontend.
  Source: [v1.8.0 plan](plans/v1.8.0-android-plan.md). Target: **v1.8.0**.
- `[ ]` **v1.9.0 iOS / iPadOS MVP** — SwiftUI + Metal over a shared
  `rustynes-mobile` bridge; cross-device `.rns` + battery-SRAM portability is
  in-scope. Source: [v1.9.0 plan](plans/v1.9.0-ios-plan.md). Target: **v1.9.0**.
- `[ ]` **Mobile Lua scripting** — deferred on both platforms (mlua/ NDK + arm64
  cross-compile work; gated behind a later increment / "Developer" toggle).
  Source: [v1.8.0](plans/v1.8.0-android-plan.md), [v1.9.0](plans/v1.9.0-ios-plan.md).
  Target: **v1.8.x / v1.9.x**.
- `[ ]` **Mobile RetroAchievements** — deferred on both (needs a Compose / SwiftUI
  OAuth login UI + keychain token + privacy disclosure over the cross-compiled
  rcheevos). Source: [v1.8.0](plans/v1.8.0-android-plan.md),
  [v1.9.0](plans/v1.9.0-ios-plan.md). Target: **v1.8.x / v1.9.x**.
- `[ ]` **Mobile netplay** — deferred on both; the transport is the blocker (mobile
  NAT/CGNAT + iOS background limits) → local Wi-Fi / GameKit / TURN later; ties to
  the desktop TURN carryover in §3. Source:
  [v1.8.0](plans/v1.8.0-android-plan.md), [v1.9.0](plans/v1.9.0-ios-plan.md).
  Target: **v1.8.x / v1.9.x**.
- `[ ]` **Mobile egui debugger surface** — kept only as an optional sideload
  power-user overlay (Android) / a future hybrid embed (iOS), not a first-class
  mobile surface. Source: [v1.8.0](plans/v1.8.0-android-plan.md),
  [v1.9.0](plans/v1.9.0-ios-plan.md). Target: **v1.8.x / v1.9.x**.
- `[ ]` **iCloud / cross-device save-state sync** — a future note in the iOS plan
  (the format is platform-independent; the sync layer is not built). Source:
  [v1.9.0 plan](plans/v1.9.0-ios-plan.md). Target: **TBD**.
- `[~]` **Host-localizable bridge warnings (warning-code enum)** — the
  `rustynes-mobile` bridge previously handed the host a pre-composed *English*
  advisory string for the pre-v2.0.0 movie/epoch notice (`movie_play` →
  `drain_warnings`). **Bridge + Android DONE (v2.0.3 + v2.0.4):** v2.0.3 added the
  stable `HostWarning` code/enum + `drain_warning_codes()` (with `drain_warnings()`
  preserved verbatim so existing hosts are unbroken); v2.0.4 wired the **Android**
  host to drain the codes and resolve each through a device-locale string resource
  (`host_warning_pre_timebase_movie`, en + es) after a `.rnm` load. **Remaining:** the
  **iOS** half (`Localizable`) — v2.0.5–v2.0.8 iOS train — and, optionally, migrating
  the desktop/wasm frontends off their own English literals onto the same code (they
  stay in parity today because `HostWarning::message()` is byte-identical). Files:
  `crates/rustynes-mobile/src/lib.rs`, `android/app/src/main/.../MainActivity.kt`,
  `android/app/src/main/res/values{,-es}/strings.xml`. Target: **iOS half v2.0.5–v2.0.8**.

---

## 10. Maintainer-manual verifies (CI cannot self-certify)

Consolidated cross-reference of every item that needs a human, a device, a live
account, or a hosted deploy (all also listed under their theme above).

- `[M]` **F1 — on-device touch UX** — the web touch-control layer can't be
  exercised headlessly; verify on a real touch device. Standing since
  [v1.2.0](plans/v1.2.0-curator-plan.md). Target: **maintainer-manual**.
- `[M]` **F3 — live-netplay host/TURN connectivity matrix** — see §3; flip
  `docs/netplay-webrtc.md` §4 to "Verified" afterward. Standing since
  [v1.2.0](plans/v1.2.0-curator-plan.md). Target: **maintainer-manual**.
- `[M]` **Browser RA auth-proxy deploy + live-account verify** — §4 (ADR 0015).
  Target: **maintainer-manual**.
- `[M]` **A/V recording, HD-audio, shader/NTSC visual output** — §5; no codec /
  audio / GPU-validation path in CI. Target: **maintainer-manual**.
- `[M]` **GPU-timing crash-fix verify** — the v1.5.0 `TIMESTAMP_QUERY_INSIDE_ENCODERS`
  startup-crash fix can't be exercised on headless CI; re-test the release binary.
  Source: v1.5.0 release notes. Target: **maintainer-manual**.
- `[M]` **egui render / pointer-event verify** — v1.6.0 release carryover; headless
  CI can't exercise GPU render or egui pointer events. Target: **maintainer-manual**.
- `[M]` **Snapshot re-bless after blank-boot fixes** — §7. Target:
  **maintainer-manual**.

---

## 11. CI / tooling follow-ups (proposed, not yet implemented)

- `[ ]` **`cargo-hack` mutually-exclusive feature clippy in CI** — the
  `scripting` / `scripting,hd-pack` / `retroachievements` (and the new
  `script-ipc` / `browser-cheevos`) clippy combos run **only locally / in the
  pre-commit hook**; promoting a feature-powerset clippy into the CI lint job
  closes a real coverage gap (`--fix` can strip cfg-gated code another feature
  needs). Source: the CI-optimization memory note (PR #120 proposals). Target:
  **TBD**. Files: `.github/workflows/`.
- `[ ]` **Free arm64 CI leg** — `ubuntu-24.04-arm` is free on public repos and runs
  in parallel. Source: CI-optimization note. Target: **TBD**.
- `[ ]` **`dorny/paths-filter` per-job skips** — needs a `ci-success` aggregator
  job. A doc-only paths-filter gate already landed (#124); broader per-job skips
  remain proposed. Source: CI-optimization note. Target: **TBD**.
- `[ ]` **`merge_group` + PR-Ubuntu-only matrix** — the highest runner-minute
  saver but higher risk; maintainer decision pending. Source: CI-optimization note.
  Target: **TBD (maintainer decision)**.
- `[ ]` **`cargo-nextest` adoption** — ~1.3–1.5× test speedup but needs a separate
  `cargo test --doc` step and no retries. Source: CI-optimization note. Target:
  **TBD**.
- `[x]` **`full` native feature alias (#54)** — *(shipped; v1.8.9 reconcile: the `full`
  feature in `crates/rustynes-frontend/Cargo.toml` + the `full-run`/`full-build` aliases in
  `.cargo/config.toml`.)* an umbrella feature for the maximal native build. Source: v1.7.0
  beta.5 carryover. Files: `crates/rustynes-frontend/Cargo.toml`, `.cargo/config.toml`.
- `[x]` **Re-sync the vendored `100thCoin/AccuracyCoin` test-ROM oracle** — DONE in
  the **v2.0.1 AccuracyCoin re-sync PR**. Re-synced to upstream `main` commit
  `71f57fb` (2026-06-26): re-extracted `SOURCE_CATALOG.tsv` (**144 -> 146 rows /
  139 -> 141 assigned tests**), replaced the runtime `.nes` with the upstream build
  (ca65 toolchain present but the upstream ships a prebuilt ROM), bumped the
  `catalog_has_exactly_146_entries` + `entry_by_index_is_zero_based` drift-guards +
  the rustdoc, corrected the stale `README.md` extraction-recipe pointer (now an
  inline recipe: walk `Suite_*`/`table` blocks, resolve each `result_symbol` to its
  `result_X = $ADDR`), and re-verified the pass rate. The two existing-test upstream
  fixes (`"$2002 Flag Timing"` page-boundary, `"Sprites On Scanline 0"` error code)
  folded in cleanly with no change to RustyNES's result. The two new PPU tests were
  added: `"ALE + Read"` (`$0491`) and `"Hybrid Addresses"` (`$0492`). **Outcome: the
  v2.0.0 core PASSES 139 of the 141 assigned tests — it FAILS the two new ones** (the
  8 pre-existing PPU-Misc tests still pass, so nothing regressed); honest headline is
  now **139/141 (98.58%)**, down from 139/139 only because the denominator grew.
  Files: `tests/roms/AccuracyCoin/`, `tests/roms/accuracycoin/`,
  `crates/rustynes-test-harness/src/accuracy_coin_catalog.rs`.

- `[ ]` **Pass the two new AccuracyCoin PPU tests — `"ALE + Read"` + `"Hybrid Addresses"`** —
  the accuracy gap opened by the v2.0.1 re-sync above. Both hinge on the PPU **octal
  latch** (the 74LS373 that multiplexes PA0-7 with the CHR data bus via the ALE
  signal; `AccuracyCoin.asm:2541-2614` is the authoritative cycle-level spec, nesdev
  `PPU_rendering.xhtml:65,116,195-201`): a mid-render `LDA $2007` leaves a **stale
  octal-latch value** that corrupts the next background pattern fetch address
  (`{new PAR high 6}:{stale low 8}`), and a mid-fetch `STA $2006` composes a **hybrid
  address** (`{new v high 6}:{octal-latch low 8}`, e.g. `$2F19`). RustyNES's
  single-step fetch model already has the `$2007`-during-render machinery
  (`render_data_bus` + `ppudata_sm_countdown`, passes `$2007 Stress`) but **no
  persistent PPU-bus-address register** and **no `$2006` render-time hook** — so the
  fix needs an `octal_latch`/`bus_addr` pair fed from every fetch (snapshot v5) plus
  the two register-path hooks. Reference: **Mesen2** models both via a single
  `_ppuBusAddress` (+ 3-dot-delayed `v`, bus not re-synced during render); **ares**
  models only the `$2007` side (`io.busAddress`); **higan** blocks `$2007` during
  render. **ATTEMPTED in v2.0.1 and deferred — see [ADR 0030](../docs/adr/0030-accuracycoin-ale-read-hybrid-addresses-octal-latch.md).**
  The bounded attempt added the `octal_latch` + one-shot hybrid-fetch hook and confirmed
  the tracking is inert (still 139/141), but RustyNES's single-step fetch model does not
  hold the per-cycle multiplexed-bus low byte the tests probe, so the corruption lands on
  the wrong tile; the attempt was reverted in full (no format churn). Reproducing it needs
  either a **2-cycle-ALE fetch refactor** or **Mesen2's persistent-`_ppuBusAddress` +
  3-dot-delayed-`v` model** — both invasive, high-regression-risk fetch-path rewrites,
  deferred to a dedicated accuracy session (land Hybrid Addresses first via the Mesen2
  model, then ALE+Read). Target: a future accuracy session. Files:
  `crates/rustynes-ppu/src/{ppu.rs,snapshot.rs}`.

---

## 12. Misc / smaller deferrals

- `[ ]` **NSF waveform visualizer depth** — an NSF waveform *scope* shipped in
  v1.5.0 C; broader eye-candy visualization over the NSF player was noted as a
  lower-priority deferral. Source: [v1.3.0](plans/v1.3.0-bedrock-plan.md). Target:
  **TBD**.
- `[x]` **Kid Icarus FDS side-B post-registration re-entry** — *(resolved; v1.8.9
  reconcile: closed by the v1.6.0 "Studio" FDS-proper work — the timed disk-head position
  model + `$4032` auto-insert + per-game CRC quirk table close the side-B post-registration
  replay; see CHANGELOG `[1.6.0]`.)* Source: [v1.3.0](plans/v1.3.0-bedrock-plan.md).
- `[x]` **ROM pre-warming** — evaluated and effectively moot: the v1.0.0 core's
  run-ahead / display-sync pacing supersede the parent's pre-warming idea. Source:
  [v1.0.0 synthesis](plans/v1.0.0-synthesis-plan.md). Status: **resolved (no
  action)**.
- `[x]` **Custom window chrome** — rejected as a poor fit (RustyNES keeps native
  OS decorations). Source: [v1.3.0](plans/v1.3.0-bedrock-plan.md). Status:
  **rejected (no action)**.
- `[x]` **Niche peripherals (mic / Hyper Shot / Barcode / R.O.B. / OEKA)** —
  by-design avoid; the niche peripherals worth doing (Family Trainer / Subor
  keyboard / Konami+Bandai Hyper Shot aliases) already shipped in v1.3.0. Source:
  [v1.4.0](plans/v1.4.0-fidelity-plan.md). Status: **by-design (no action)**.

---

## Provenance

Sources reconciled: all `to-dos/plans/*.md` per-version plans, `plans/research/`,
and `plans/engine-lineage/`; `CHANGELOG.md` `[Unreleased]`; `to-dos/ROADMAP.md`;
`docs/STATUS.md`; `docs/adr/0002`, `0003`, `0012`, `0015`; `docs/netplay-webrtc.md`
§4; `docs/cheevos-browser.md`; the RustyNES project memory bank
(`MEMORY.md` + the per-version + policy notes); and a grep reconciliation of
`crates/` feature flags, modules, and mapper tiers on `main` @ `de682d8`.
Excluded: everything already shipped (VRC7/OPLL audio per ADR 0006; the v1.7.0
beta.1–4 A/B/C/D/E/F/G workstreams; Game Genie encoder UI / movie `.srt` / `.tbl`
per the maintainer's instruction; `cpu_interrupts_v2` 5/5 strict since v1.3.0;
the m30/m80/m185 blank-boot fixes).
