# Regression-Baseline Audit

**Audit date.** 2026-05-17.
**Audit author.** Generated via the regression-detection infrastructure
landing (post-FSM-recovery work).
**Audit cause.** Six earlier commits had landed 21 freely-redistributable
test ROMs under `tests/roms/` (commit `9dc45fb`) without wiring them
into any cargo test target. The FSM-recovery escapade (commit `834be9e`
fix, May 17 2026) demonstrated that **dormant test assets do nothing
to prevent regressions** — the missing harness against committed ROMs
was a structural gap surfaced by the user-visible crash on commercial
games. This audit documents the harnesses that now close that gap and
the baselines they enforce.

For the bug-pattern context that motivates this entire infrastructure,
see
`~/.claude/projects/-home-parobek-Code-RustyNES-v2/memory/feedback_emulator_fsm_mid_cycle_clobber.md`
and ADR-0002 §"Empirical refinement (2026-05-14)".

---

## Inventory — harnesses and what they cover

### `crates/rustynes-test-harness/tests/audio_tests.rs`

**ROMs covered (19, all under `tests/roms/audio-tests/`).** Source:
[bbbradsmith/nes-audio-tests](https://github.com/bbbradsmith/nes-audio-tests),
freely-redistributable.

**Pattern.** `insta::assert_snapshot!` of a one-line text record:
`rom=… frames=… fb_bytes=245760 fb_fnv1a64=… cycles=… audio_samples=… audio_fnv1a64=…`.

**Why full capture (fb + cycles + audio).** Investigation during
harness development found that every audio-test ROM holds a **uniform
palette frame** for the entire test (either `$0D` black, hash
`1719dca5cef7a325`, or `$00` gray, hash `89ee4c476c97a325`). The
upstream `swap.s` boot sequence intentionally suppresses video
rendering — these are **audio-only** tests. Framebuffer hash alone is
a weak sentinel (16 of 19 ROMs would share the same hash; only 3
would surface as distinct in the snapshot). Adding the **audio
waveform FNV-1a hash** of the drained `f32` samples + the **CPU cycle
count** at the deadline frame gives the harness real regression
signal: a change to the APU mixer, mapper-audio extension, or
CPU/PPU timing surfaces as an audio-hash or cycle-count mismatch
even when the framebuffer is unchanged.

**Frame counts.** `STABILIZED_FRAME = 600` (10 s @ NTSC 60 Hz) for
hotswap ROMs — past the upstream "half-second buzz + 256-frame delay
+ second buzz" sequence. `STABILIZED_FRAME_NROM = 180` (3 s) for the
bare-APU ROMs without the hotswap dance (`db_apu`, `tri_silence`,
`dac_square`).

**Baselines (audio_fnv1a64 is the load-bearing field).**

| ROM | Mapper | Frames | fb_fnv1a64 | cycles | audio_samples | audio_fnv1a64 | Visible at baseline |
|-----|--------|--------|------------|--------|---------------|----------------|---------------------|
| `db_apu.nes` | NROM (0) | 180 | `1719dca5cef7a325` | 5330857 | 131335 | `ef7a3f9094aa8453` | Uniform black ($0D palette) |
| `db_vrc6a.nes` | VRC6a (24) | 600 | `1719dca5cef7a325` | 17838738 | 439529 | `e9c3ba6f70ce5006` | Uniform black |
| `db_vrc6b.nes` | VRC6b (26) | 600 | `1719dca5cef7a325` | 17838738 | 439529 | `e9c3ba6f70ce5006` | Uniform black (same audio as VRC6a — same engine, two mappers) |
| `db_vrc7.nes` | VRC7 (85) | 600 | `1719dca5cef7a325` | 17838738 | 439529 | `cd72d179580b9459` | Uniform black |
| `db_n163.nes` | Namco 163 (19) | 600 | `1719dca5cef7a325` | 17838738 | 439529 | `a959298ddb77f74b` | Uniform black |
| `db_5b.nes` | FME-7 / 5B (69) | 600 | `1719dca5cef7a325` | 17838735 | 439529 | `9335bcb91c90ec28` | Uniform black |
| `db_mmc5.nes` | MMC5 (5) | 600 | `1719dca5cef7a325` | 17838733 | 439529 | `79eb3f3a941e35e1` | Uniform black |
| `test_vrc7.nes` | VRC7 (85) | 600 | `89ee4c476c97a325` | 17838738 | 439529 | `38b5d4e010179438` | Uniform gray ($00 palette) |
| `patch_vrc7.nes` | VRC7 (85) | 600 | `1719dca5cef7a325` | 17838736 | 439529 | `a1ce6d2d8a4b5ea3` | Uniform black |
| `clip_vrc7.nes` | VRC7 (85) | 600 | `1719dca5cef7a325` | 17838737 | 439529 | `fe4937243f5f48e4` | Uniform black |
| `noise_vrc7.nes` | VRC7 (85) | 600 | `1719dca5cef7a325` | 17838734 | 439529 | `38167df63766cc76` | Uniform black |
| `test_n163_longwave.nes` | Namco 163 (19) | 600 | `1719dca5cef7a325` | 17838736 | 439529 | `094618cf7678884c` | Uniform black |
| `clip_5b.nes` | FME-7 / 5B (69) | 600 | `1719dca5cef7a325` | 17838736 | 439529 | `3af5facf9b7731da` | Uniform black |
| `noise_5b.nes` | FME-7 / 5B (69) | 600 | `1719dca5cef7a325` | 17838734 | 439529 | `aa6ba8e67a26e6a0` | Uniform black |
| `sweep_5b.nes` | FME-7 / 5B (69) | 600 | `89ee4c476c97a325` | 17838736 | 439529 | `ba1e2d1632ec2b46` | Uniform gray |
| `envelope_5b.nes` | FME-7 / 5B (69) | 600 | `1719dca5cef7a325` | 17838737 | 439529 | `765afef18c925428` | Uniform black |
| `phase_5b.nes` | FME-7 / 5B (69) | 600 | `89ee4c476c97a325` | 17838737 | 439529 | `031ce9cdec5074a5` | Uniform gray |
| `tri_silence.nes` | NROM (0) | 180 | `1719dca5cef7a325` | 5330857 | 131335 | `ae593d801f078809` | Uniform black |
| `dac_square.nes` | NROM (0) | 180 | `1719dca5cef7a325` | 5330856 | 131335 | `672a1c96fc50e29c` | Uniform black |

> Note that `db_vrc6a` and `db_vrc6b` produce **identical audio hashes**
> (`e9c3ba6f70ce5006`). This is expected: they test the same VRC6 audio
> engine on two different pinout variants (Akumajou Densetsu vs. Madara).
> The mapper-decoder paths differ; the audio-mixer output is the same.

**Mapper coverage added by this harness.** VRC6 (24/26, 3 ROMs), VRC7
register surface (85, 5 ROMs — FM deferred per ADR-0004), Namco 163
(19, 2 ROMs), Sunsoft FME-7 / 5B (69, 6 ROMs), MMC5 (5, 1 ROM),
NROM-bundled APU quirks (0, 3 ROMs).

### `crates/rustynes-test-harness/tests/m22.rs`

**ROM covered.** `tests/roms/m22/0-127.nes`. VRC2a (mapper 22)
CHR-banking test by NewRisingSun. Public-domain via
`christopherpow/nes-test-roms`.

**Pattern.** `insta::assert_snapshot!` of fb-only snapshot
(`run_and_hash_with_dump`). The ROM renders a meaningful visual
pattern (CHR-bank index display) at the baseline frame; framebuffer
hash alone is a strong sentinel.

**Frame count.** 240 frames (4 s @ NTSC 60 Hz).

**Baseline.** `rom=m22/0-127.nes frames=240 fb_bytes=245760 fnv1a64=d604007c6cd21329`.

**Visible at baseline.** The ROM's CHR bank-walk progress indicator
shows the digit "016" centered in the lower-left quadrant against a
black background — the ROM is mid-walk through the 128 CHR banks.

### `crates/rustynes-test-harness/tests/mmc1_a12.rs`

**ROM covered.** `tests/roms/mmc1_a12/mmc1_a12.nes`. MMC1 + PPU
A12-transition regression test by tepples. Public-domain via
`christopherpow/nes-test-roms`.

**Pattern.** `insta::assert_snapshot!` of fb-only snapshot.

**Frame count.** 240 frames.

**Baseline.** `rom=mmc1_a12/mmc1_a12.nes frames=240 fb_bytes=245760 fnv1a64=c163e7685c04c78d`.

**Visible at baseline.** Full-screen red-and-blue checkerboard
background overlaid with the text banner:

```
MMC1 WRAM DISABLE SCANLINE
       COUNTER TEST
       C 2010 BREGALAD
USE U\D L\R TO ADJUST DELAY=16
```

The ROM is correctly displaying its instruction screen at the default
delay setting.

### `crates/rustynes-test-harness/tests/external_real_games.rs`

**ROMs covered (60 staged + 6 ignored, all under `tests/roms/external/`).**
Commercial dumps, gitignored — never committed. Snapshots **are**
committed (emulator output, not ROM bytes — no copyright concern), so
a developer with their own ROM dumps gets a working regression net.

**Audit date (commercial-roms section).** 2026-05-17, extending the
3-ROM oracle (commit `3e53802` / `834be9e`) to the full 60-ROM corpus
landed in commits `9dc45fb` + `6b3a818`.

**Pattern.** Multi-line `insta::assert_snapshot!` block with one
checkpoint per line — readable diffs when one of N checkpoints
regresses, the rest stay byte-identical:

```text
rom=mapper-000-NROM/Super Mario Bros.nes
rom_sha256=8af9af55e8a6a8ce0fefbf6d3afb31e4e8c0d7f8c4d3b2a1e0f9d8c7b6a5f4e3
frames=600
fb_bytes=245760
cycles=10741428
audio_samples=240880
audio_fnv1a64=...
checkpoint f120 fb_fnv1a64=...
checkpoint f240 fb_fnv1a64=...
checkpoint f600 fb_fnv1a64=...
```

**Why SHA-256 pinning.** Commercial ROM dumps are user-supplied; a
different region's dump, a header variant, or a corrupted copy would
silently produce a different baseline. The snapshot's `rom_sha256=`
line surfaces the mismatch as a clean `insta` diff (`rom_sha256` field
differs) rather than 8 cascading fb-hash mismatches.

**Why per-line checkpoints.** Most ROMs use the `IdleOnly { frames:
600 }` script (single `checkpoint f600` line); the 3 grandfathered
ROMs (SMB / Excitebike / Kid Icarus) preserve the original START-tap
3-checkpoint pattern (`f120` / `f240` / `f600`). Splitting checkpoints
across multiple snapshot lines means a regression at frame 240 shows
as a 1-line diff, not a cascading change to a multi-field one-liner.

**Per-mapper baseline summary (54 passing + 6 `#[ignore]`'d).**

| Mapper | Pass | Ignored | Notes |
|--------|------|---------|-------|
| 000 NROM | 6 | 0 | SMB / Excitebike / Donkey Kong / Balloon Fight / Ice Climber / Gyromite. SMB + Excitebike use `StartTap`. |
| 001 MMC1 | 7 | 0 | Kid Icarus (StartTap) + LoZ / Metroid / FF / MM2 / CV2 / Ninja Gaiden. |
| 002 UxROM | 4 | 0 | CV / MM / Contra / DuckTales. |
| 003 CNROM | 3 | 0 | Arkanoid / Gradius / Paperboy (Paperboy is mapper 3 in some dumps; this dump correctly decoded as CNROM). |
| 004 MMC3 | 6 | 1 | SMB3 / SMB2 / MM3 / Kirby / Ninja Gaiden 2 / TMNT3 pass. Tiny Toon Adventures 2 ignored — uniform black at f600 (suspect long intro / MMC3 IRQ edge). |
| 005 MMC5 | 3 | 0 | Castlevania III / Bandit Kings / Uchuu Keibitai SDF. |
| 007 AxROM | 4 | 0 | Battletoads / Marble Madness / Cobra Triangle / Solstice. Battletoads is mostly dark at f600 but the unique hash confirms a real fade-in transition, not a uniform frame. |
| 009 MMC2 | 2 | 0 | Punch-Out!! / Mike Tyson's Punch-Out!! |
| 010 MMC4 | 2 | 1 | Famicom Wars / Fire Emblem 1 pass. Fire Emblem Gaiden ignored — uniform gray at f600. |
| 019 N163 | 4 | 0 | Famista '90 / '91 / Final Lap / Mappy Kids. |
| 021 VRC4 | 1 | 0 | Wai Wai World 2. |
| 022 VRC2a | 1 | 0 | TwinBee 3. |
| 023 VRC4 | 3 | 1 | Akumajou Special / Crisis Force / Wai Wai World 1 pass. Ganbare Goemon 2 ignored — uniform gray at f600 (suspect sub-variant decoder mismatch). |
| 024 VRC6a | 1 | 0 | Castlevania III retranslation. |
| 025 VRC4 | 1 | 0 | Ganbare Goemon Gaiden. |
| 026 VRC6b | 0 | 2 | Both Esper Dream 2 and Madara ignored — uniform gray at f600 (shared VRC6b pinout-26 decoder edge). |
| 066 GxROM | 2 | 0 | Doraemon / Thunder & Lightning. |
| 069 FME-7 | 1 | 1 | Batman Return of the Joker passes. Mr. Gimmick ignored — uniform black at f600 (Sunsoft splash + intro likely > 10 s). |
| 075 VRC1 | 2 | 0 | Ganbare Goemon (mapper 75 — the original) / King Kong 2. |
| 085 VRC7 | 1 | 0 | Lagrange Point — VRC7 banking + IRQ + register-surface latching path covered; FM mute baseline locked (ADR-0004). |

Across mappers: **54 passing + 6 `#[ignore]`'d** (43.5 KB of snapshot
text under `crates/rustynes-test-harness/tests/snapshots/external_real_games__*.snap`).

**The 6 `#[ignore]`'d ROMs.** Each is annotated with a `reason =
"…"` on the `#[ignore]` attribute. Common patterns:

- **Uniform-black ($0D palette, hash `1719dca5cef7a325`)**: Tiny Toon
  Adventures 2, Mr. Gimmick. Mr. Gimmick's Sunsoft splash + intro
  animation is famously long; Tiny Toon Adventures 2 has a Konami
  intro + character-presentation sequence. Both likely flip to a real
  frame at frames=1200 or with a START tap.
- **Uniform-gray ($00 palette, hash `89ee4c476c97a325`)**: Fire Emblem
  Gaiden, Ganbare Goemon 2, Esper Dream 2, Madara. Three of the four
  are on VRC4/VRC6 — investigate `rustynes-mappers::vrc24` and `vrc6`
  pinout decoders for the iNES-26 mapping. Fire Emblem Gaiden is
  MMC4 (mapper 10), more likely a slow intro than a mapper bug —
  Famicom Wars and Fire Emblem 1 on the same mapper both render.

**Recommended bisect workflow for ignored ROMs.** Drop `#[ignore]`
locally, run with `RUSTYNES_DUMP_FRAMES=1` and progressively larger
`frames=` overrides to discover the actual title-screen frame, then
re-enable with the discovered baseline. If the ROM still doesn't
boot at frames=3600, the bug is structural — file an issue and keep
the test ignored until the underlying mapper/CPU fix lands.

**VRC7 FM caveat.** Lagrange Point's snapshot locks in the
`Mapper::mix_audio` = 0 silence baseline per ADR-0004. When the FM
synthesizer eventually lands, this baseline is expected to flip and
needs re-capture.

**Mapper coverage added by this harness.** Every supported mapper
(15 of 15) is covered by at least one commercial ROM: NROM (0),
MMC1 (1), UxROM (2), CNROM (3), MMC3 (4), MMC5 (5), AxROM (7),
MMC2 (9), MMC4 (10), Namco 163 (19), VRC2/4 (21/22/23/25),
VRC6 (24/26), GxROM (66), Sunsoft FME-7 (69), VRC1 (75), VRC7 (85).
Regressions in CPU / PPU / scheduler / mapper / audio / DMA timing
that the structural (audio_tests / m22 / mmc1_a12) corpus might
miss surface here against real commercial title screens.

---

## Shared infrastructure

### `crates/rustynes-test-harness/tests/common/mod.rs`

Factored-out helpers used by the three new harnesses:

- `fnv1a64(bytes) -> u64` — canonical FNV-1a-64.
- `rom_path(rel) -> PathBuf` — workspace-rooted `tests/roms/` resolver.
- `write_png(path, fb)` — PNG dump (256×240 RGBA).
- `dump_frame_if_requested(corpus, rom, frame_label, fb)` — opt-in PNG
  dump under `/tmp/rustynes-baseline-screenshots/<corpus>/`, gated on
  `RUSTYNES_DUMP_FRAMES=1`.
- `run_and_hash(rom_rel, frames) -> u64` — fb-only baseline capture.
- `run_and_hash_with_dump(corpus, rom_rel, frames) -> u64` — adds the
  opt-in PNG dump on top of `run_and_hash`.
- `snapshot_line(rom, frames, hash) -> String` — fb-only one-liner
  matching `tests/visual_regression.rs` convention.
- `run_and_capture_full(corpus, rom_rel, frames) -> (fb_hash, cycles,
  samples, audio_hash)` — the full-state regression capture used by
  the audio-tests harness.
- `snapshot_line_full(...) -> String` — full-state one-liner.

The existing harness `crates/rustynes-test-harness/tests/external_real_games.rs`
(the FSM-recovery oracle) is intentionally **NOT touched** — it has
its own inlined `fnv1a64` / `write_png` / `dump_frame` and its own
`BASELINE` table. Per task constraints, it stays frozen.

### Diagnostic dump

Every new harness supports `RUSTYNES_DUMP_FRAMES=1` to emit PNGs at
the baseline frame:

```bash
RUSTYNES_DUMP_FRAMES=1 cargo test -p rustynes-test-harness --features test-roms \
    --test audio_tests --test m22 --test mmc1_a12 \
    -- --test-threads=1 --nocapture
```

PNGs land under `/tmp/rustynes-baseline-screenshots/<corpus>/<rom>_f<N>.png`.
Output is ephemeral (not committed). When a baseline hash mismatch
fires in CI, re-run the failing test locally with the dump enabled and
compare the dumped PNG against the canonical "what should this look
like" reference in this audit document.

---

## Bisect workflow recipes

See `scripts/regression-bisect/README.md` for the full set; the
high-level recipes are:

1. **Real-game regression suspected** — use the FSM-recovery oracle
   (`tests/external_real_games.rs`, requires commercial ROMs at
   `tests/roms/external/`). Bisect with `HARNESS_TEST=external_real_games
   HARNESS_FEATURE=commercial-roms`.

2. **Committed test-ROM regression suspected** — use the appropriate
   new harness. Example for an audio regression:
   `HARNESS_TEST=audio_tests HARNESS_FEATURE=test-roms
   HARNESS_FILTER=audio_db_vrc6a`. No worktree overlay needed since
   ROMs are committed.

3. **Harness file pre-dates the bisect range** — use
   `scripts/regression-bisect/worktree_overlay.sh <commit> --harness
   <path> --feature 'test-roms = []'` to overlay the harness file as
   untracked + patch the Cargo.toml feature flag, then run the bisect
   inside the worktree.

The `bisect_runner.sh` translates exit codes per `git bisect run`
conventions:

- `0` GOOD — build + test both passed.
- `1` BAD — build OK but test failed.
- `125` SKIP — build broken (unreliable; tells bisect to skip this
  commit).

The **shell-redirection gotcha** that bit the FSM-recovery bisect:
`2>&1 > file` is WRONG; `> file 2>&1` is RIGHT. Order matters because
redirections are processed left-to-right.

---

## Validation gates

Confirmed at landing time (2026-05-17):

- `cargo fmt --all --check` — clean.
- `cargo clippy --workspace --all-targets --features test-roms -- -D warnings` — clean.
- `cargo clippy --workspace --all-targets --features test-roms,commercial-roms -- -D warnings` — clean.
- `cargo doc --workspace --no-deps` — clean.
- `cargo test --workspace --features test-roms` — passes (was 510
  strict + 5 ignored before; +21 strict from new harnesses; expected
  531 strict + 5 ignored). `commercial-roms` is OFF by default so this
  count is unchanged by the 60-ROM expansion.
- `cargo test -p rustynes-test-harness --features test-roms,commercial-roms
  --test external_real_games -- --test-threads=1` — **54 passing +
  6 ignored** for the commercial-roms harness alone. Requires staged
  ROMs at `tests/roms/external/` (gitignored).
- `cargo test --workspace --no-default-features` — preserves the
  no_std + alloc invariant; same pass count as before (375 — new
  harnesses are `cfg(feature = "test-roms")` / `cfg(feature =
  "commercial-roms")` and do not affect it).
- AccuracyCoin floor — unchanged. The new harnesses don't touch the
  emulator behavior, only test it.

---

## Recommended next steps

The current baseline set covers the **structural** regression surfaces
the new ROMs are designed for. Tightening directions in priority
order:

1. **Audio-spectrum hashing instead of raw f32 hash.** The current
   audio FNV-1a hash flags any sample-level divergence, which makes it
   *over-sensitive* to harmless changes — adding one cycle of latency
   in the BLEP filter shifts the entire stream and breaks the
   baseline. A windowed FFT magnitude hash (e.g. FNV of the
   power-spectrum bin envelope quantized to ~6 bits per bin) would be
   stable against benign timing shifts while still flagging real
   spectral changes. See Track C3 of the gap-analysis plan for the
   polyphase BLEP work that already landed the FFT regression
   harness; reuse its `rustfft` setup.

2. **Multi-checkpoint hashes within a single run.** The audio-tests
   harness samples one moment in time. For test ROMs that run
   multi-phase sequences (the bbbradsmith corpus does this — see
   `swap.s`), sampling at frames 60 / 180 / 300 / 600 would catch
   regressions that only manifest in the later phases. The
   `external_real_games.rs` harness already does this (frames 120 /
   240 / 600); the new harnesses inherited only the final-frame
   pattern.

3. **CI bisect cron.** Schedule a nightly `git bisect run` against the
   last-known-good `main` commit + the latest commit, with the audio
   + m22 + mmc1_a12 harnesses, so any regression that lands between
   nightly runs is automatically localized. Use the `bisect_runner.sh`
   wrapper.

4. **Wire the holy_mapperel / mmc5 / blargg harnesses through
   `tests/common/mod.rs`.** They currently use inlined helpers. The
   factoring opportunity is real (especially the `rom_path` function,
   duplicated in ~10 test files), but per task constraints those
   files were left untouched. A follow-up sweep that migrates them to
   `common::rom_path` would cut ~50 lines from the harness directory.

5. **`tests/roms/external/` symlink in CI.** The `worktree_overlay.sh`
   script symlinks `tests/roms/external/` into a temp worktree for
   bisect runs. CI doesn't currently have access to that directory
   (it's gitignored), but a CI job with a private artifact store
   could expose it and run `external_real_games.rs` automatically.
   This would catch regressions on real games even when the bisect
   is manual.

6. **VRC7 audio-baseline freeze.** Per ADR-0004, the VRC7 FM
   synthesizer is deferred. When it lands, the 5 `*_vrc7.nes`
   baselines in `audio_tests.rs` will need re-capture. The current
   baselines lock in the **silence** the deferred path emits
   (`Mapper::mix_audio` returns 0); a future "FM landed" PR is
   expected to flip them, and that's the *intended* signal — the
   harness will catch any *unintended* audio change on the
   register-surface path until then.
