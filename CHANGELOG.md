# Changelog

This is the concise, readable summary of notable changes to RustyNES — a few
tight highlights per release. For the full per-version detail (engineering
narrative, engine lineage, ADR references, PR trains, and technical rationale),
see [CHANGELOG-FULL.md](CHANGELOG-FULL.md). The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and the project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

RustyNES's cycle-accurate emulation core arrived in v1.0.0; the `v0.9.x` rows are
the documentary lineage of how that core was built (not standalone user
releases), and `v0.1.0`–`v0.8.6` are the original pre-1.0 engine that the
cycle-accurate core later replaced.

## [Unreleased]

### Added

- Antigravity PR reviewer (`.github/workflows/antigravity-review.yml` +
  `scripts/agy-review.sh`): an automated first-pass code review on a self-hosted
  runner, driven by the `agy` CLI's OAuth session (Google AI Ultra, no metered
  API key). Runs on PR open/reopen and on an `/agy-review` comment from a
  contributor with write access, and replaces its own prior comment each run.
  Review priorities live in `.github/agy-review.md`. CI-only — no crate, no
  shipped artifact, and no emulation-core change.

### Security

- The reviewer executes on the maintainer's own hardware with a token in scope,
  so its trigger and execution paths are gated accordingly: fork PRs cannot
  schedule the job, the automation scripts are checked out from the default
  branch rather than the PR head (a PR cannot rewrite its own reviewer), `agy`
  is launched with `GH_TOKEN`/`GITHUB_TOKEN` removed from its environment, the
  `script(1)` fallback quotes its argv with `printf %q` instead of interpolating
  env-settable flags into a shell string, the conversation-database fallback is
  removed outright (agy's store is shared per-user, so it could copy an unrelated
  session into a public comment, and agy exposes no per-invocation store to scope
  it to), and prior-comment cleanup is restricted to comments authored by the
  workflow's own bot. The trigger gate — not `agy --sandbox`, which upstream
  reports can be auto-approved away — is the trust boundary, and is documented as
  such at the invocation site.

## [2.2.2] - 2026-07-21 - "Conduit" (libretro buildbot 10/10 + CI supply-chain hardening + single-source toolchain)

A **build, distribution, and CI-integrity patch**. It carries RustyNES onto
RetroArch's own buildbot — the recipe now builds **all ten platform jobs**,
after a three-round diagnosis against a third-party pipeline we cannot push to
or re-run — hardens the GitHub Actions supply chain, and collapses the
toolchain to a single pinned source of truth with no `nightly` on any build
path.

**Zero emulation-core changes.** No file under `crates/rustynes-{cpu,ppu,apu,
mappers,core}` is touched, so the deterministic `#![no_std]` chip stack,
save-state / TAS / netplay-replay formats, and every golden vector are
untouched by construction: **AccuracyCoin holds 141/141 (100.00%)**, nestest
stays 0-diff, and `blargg_apu_2005` / `pal_apu_tests` (10/10) /
`visual_regression` / the 60-ROM commercial oracle are all unchanged from
v2.2.1.

One behavioural improvement does reach a shipped artifact: the libretro **tvOS**
core is now built with `panic = "abort"` like every other platform, rather than
the `panic = "unwind"` its previous `-Zbuild-std` path forced.

### Added

- Libretro buildbot CI recipe (`.gitlab-ci.yml`, issue #311) covering Windows
  x64, Linux x64, macOS x64/arm64, Android (4 ABIs), iOS arm64, and tvOS
  arm64 — the missing piece to get RustyNES onto RetroArch's built-in core
  downloader (the repo was already integrated with the legacy
  `libretro-super` scripts). Paired with a `[workspace] default-members`
  fix so the templates' unscoped `cargo build --release --target <triple>`
  builds only `crates/rustynes-libretro`, and a `[lib] name = "rustynes"`
  override resolving a compiled-artifact naming collision with the shared
  CI templates' fixed `${CORENAME}_libretro` convention.
- Libretro core feature completion: native `RETRO_ENVIRONMENT_SET_MEMORY_MAPS`
  registration (the memory-descriptor path RetroAchievements' `rcheevos`
  prefers, alongside the existing legacy pointer API); an FDS load-path fix
  (`.fds` content is now correctly routed to `Nes::from_disk` with a
  `disksys.rom` lookup in the frontend's system directory — previously
  broken despite `valid_extensions` advertising it) plus a full disk-control
  interface for FDS multi-side swapping via RetroArch's Quick Menu; native
  Game Genie cheat support (`on_cheat_set`/`on_cheat_reset`); and a
  `get_fastforwarding`-gated audio-push skip during RetroArch's
  fast-forward/rollback-netplay catch-up path.

### Security

- **`persist-credentials: false` on all 19 CI checkouts, plus a fail-closed
  release-tag check and a pinned toolchain action (closes #318).**
  `actions/checkout` defaults to writing the workflow `GITHUB_TOKEN` into
  `.git/config`, where any code the job then executes from the checkout —
  Cargo build scripts, proc macros, test binaries, Gradle build scripts,
  `scripts/*.sh`, the MkDocs build — can read it. On a pull request that tree
  is by definition unreviewed code, and nearly every CI job compiles or runs
  it (the exceptions being the `audit` / `deny` jobs, which install prebuilt
  binaries and only parse `Cargo.lock`).
  Audited rather than applied blanket: `.github/actions/rust-setup` performs
  no checkout of its own, so call-site hardening is complete coverage; and
  **no job was found to need the checkout credential** — nothing pushes
  commits, tags, or branches, there are no submodules, GitHub Pages uses the
  OIDC flow (not a `gh-pages` push), and `gh release create` /
  `softprops/action-gh-release` authenticate by API token while
  `fastlane match` clones a different remote with its own `MATCH_GIT_*`
  credentials. Highest-exposure site was not a `ci.yml` job but `web.yml`'s
  `build`, whose workflow-level `permissions:` grant `pages: write` +
  `id-token: write` and which is PR-reachable while running `trunk`,
  `cargo doc`, `pip install`, and `mkdocs build`.
  Two related hardening items landed alongside it:
  - `release-auto.yml`'s tag-existence check was
    `git ls-remote --exit-code --tags origin "refs/tags/$tag" >/dev/null 2>&1`,
    which collapsed *tag present*, *tag absent*, and *lookup failed* into a
    two-way answer, reading any non-zero exit as "absent" — so a transient
    network or auth blip pushed an already-released version down the
    `should_release=true` path. It is now a `gh api` call against
    `git/matching-refs/tags/<tag>`, chosen over `git/ref/tags/<tag>` because it
    answers "absent" with HTTP 200 and an empty array rather than a 404, so a
    genuine miss can never be confused with an error and no error-body parsing
    is needed. That endpoint matches by prefix, so the exact ref is compared in
    `jq` — verified necessary, not theoretical: `v2.2` prefix-matches two real
    tags while exact-matching none. Every failure path now aborts the job under
    `set -euo pipefail` instead of resolving to a release decision — including
    the one shape that slipped through review-round one: an explicit
    `type != "array"` guard, because a body of `{}` makes `.[]` iterate zero
    object values, so the filter returns `0` and is indistinguishable from a
    genuine "tag absent", which takes the *release* path. Removing
    the last Git operation is also what let that job's checkout — the only one
    that had needed the credential — join the sweep.
  - `.github/actions/rust-setup` pinned `dtolnay/rust-toolchain` from `@master`
    to commit `e97e2d8c` (`# v1`). `@master` is a **branch** that advances on
    every upstream commit, unlike the `@vN` tags used everywhere else, and this
    composite feeds 12 of the 19 checkouts — including `release.yml`
    (`contents: write`) and `web.yml` (`pages: write` + `id-token: write`) —
    while being the action that installs the compiler. The `# v1` trailing
    comment is what Dependabot's already-enabled `github-actions` ecosystem
    reads to keep the pin current, so this does not trade a supply-chain risk
    for a stale-action one.
  Purely additive CI configuration — no source, build output, or emulation
  behavior changes.

### Changed

- **One toolchain everywhere: `rust-toolchain.toml` is now the single source
  of truth for CI.** `.github/actions/rust-setup` parses the `channel` from
  that file and installs it, failing closed if it cannot be parsed, so there
  is no longer a `toolchain:` version literal anywhere in `.github/` — a
  toolchain bump is a one-line edit. Previously the composite defaulted to
  `stable` and 5 of its 12 call sites overrode that with an explicit
  `1.96.0`. That was misleading rather than broken: `rust-toolchain.toml` is
  a directory override and outranks the `rustup default` the action
  performs, so **every job was already compiling on 1.96.0** — the `stable`
  default merely downloaded a second toolchain nothing used, and made the
  workflows read as though they tested against latest stable, which they
  never did. With the libretro tvOS job also moved onto the pin (see Fixed),
  every build, test, lint, docs, release, and packaging path across GitHub
  Actions, the libretro buildbot, and local builds now runs on the same
  pinned 1.96.0. Nightly survives in exactly two places, neither of them a
  gate: `cargo fuzz`, which requires it for its sanitizer flags, and the
  dormant `rustynes-monetization` crate's standalone `uniffi-bindgen`
  helper.
- **Dependency version-bump consolidation (closes Dependabot #313–#315).**
  Rolled all three open Dependabot PRs into one reviewed change plus a
  full `cargo update --workspace` sweep of the rest of the tree, all with
  **no source changes** and the deterministic `#![no_std]` core untouched
  (AccuracyCoin stays **141/141**): CI **actions/setup-python v6 → v7**
  (MkDocs step), **lz4_flex 0.13 → 0.14** (save-state/movie compression;
  `default-features = false` + `safe-encode`/`safe-decode` retained, plus
  an explicit `alloc` feature request — 0.14 split `alloc`-vs-`std` no_std
  support, and without it the `rustynes-core` no_std cross-compile job
  fails to compile `compress_prepend_size`/`decompress_size_prepended`),
  the `production-dependencies` group (**bitflags 2.13.0 → 2.13.1**,
  **bytemuck 1.25.1 → 1.25.2**, **cc 1.2.67 → 1.3.0**, **clap 4.6.1 →
  4.6.2**, **futures-core/-macro/-sink/-task/-util 0.3.32 → 0.3.33**,
  **serde_json 1.0.150 → 1.0.151**), and a workspace-wide `cargo update`
  picking up **tokio 1.52.3 → 1.53.1** (which also drops its transitive
  `windows-sys`/`windows_*` 0.53.x dependency set entirely) and
  **clap/clap_derive → 4.6.3**. Surveyed the remainder of the tree via
  `cargo outdated --workspace` and confirmed nothing else is actionable:
  `getrandom` 0.2 (wasm32) is pinned transitively by `ring` upstream, and
  `wgpu`/`naga` 30.0.0 stay out because `egui-wgpu` 0.35.0 (the newest
  egui release) still requires `wgpu = "^29.0"` — bumping wgpu alone
  would split the tree across two incompatible majors, so the desktop
  stack stays in its existing egui 0.35 / wgpu 29 / winit 0.30.13 /
  accesskit 0.24.1 tier until egui itself moves. GitHub Actions were all
  already pinned to their current major tag and float to the latest
  point release automatically. Android/iOS Gradle/Swift dependency
  versions are intentionally out of scope here (they are their own
  separately-verified trains, per project convention — see the v1.8.8
  "Atlas" and iOS dep-refresh history). Verified with `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -D warnings` (+ every feature
  combo), and `cargo test --workspace`.

### Fixed

- **Libretro buildbot pipeline: 1 of 10 jobs green → all 10 building.** The
  first run of the v2.2.1 `.gitlab-ci.yml` recipe on libretro's GitLab
  buildbot ([pipeline #91899](https://git.libretro.com/libretro/RustyNES/-/pipelines/91899))
  passed only `libretro-build-linux-x64`. Three independent, previously
  invisible defects — all on our side, none in libretro's build images:
  - **Missing cross-compile targets (8 jobs).** `rust-toolchain.toml` pins
    `channel = "1.96.0"`, so rustup installs a *fresh* 1.96.0 toolchain in
    the build image carrying only the host std plus the two targets that
    file declares — bypassing the image's own default toolchain, on which
    every libretro cross target is pre-provisioned. Every non-host job died
    with `E0463: can't find crate for core`; Linux x64 survived only because
    its target *is* the host triple. Each job now runs
    `rustup target add ${RUST_TARGET}` into the pinned toolchain. Not solved
    by extending `rust-toolchain.toml`'s `targets = [...]`, which would make
    every contributor and every GitHub Actions job download ~8 extra
    `rust-std` components on each toolchain install.
  - **Upstream `rust-libretro` MinGW ABI bug (Windows).** Masked behind the
    target failure and never previously reached. `rust-libretro 0.3.2` casts
    a keycode to `i32` under `cfg(target_family = "windows")`, but C enum
    signedness follows the *ABI*, not the OS family: only the **MSVC** ABI
    gives plain enums `int`. Under **MinGW** — which is what the buildbot's
    `x86_64-pc-windows-gnu` job uses — bindgen emits `retro_key(c_uint)`, so
    the crate fails with `E0308`. Upstream has had no commit since 2023-02
    and 0.3.2 is its newest release, so `.cargo/config.toml` now points
    bindgen's clang at the matching MSVC triple for **both** MinGW targets
    — `x86_64-pc-windows-gnu` (the buildbot) and `i686-pc-windows-gnu`
    (32-bit Windows via this crate's `Makefile`, the legacy libretro-super
    path), which was verified to fail identically. Surgical: the
    generated bindings differ by 28 lines, all enum signedness, with no
    struct layout, signature, or type size affected.
  - **tvOS `panic_abort` + MSRV (tvOS).** Its template overrides `script`
    with `cargo +nightly build -Zbuild-std`, which bypasses our channel pin
    onto the image's stale 1.94.0-nightly — below the workspace's
    `rust-version = "1.96"`, failing cargo's MSRV gate before compiling
    anything. Refreshing the nightly channel exposed a second issue: bare
    `-Zbuild-std` does not build `panic_abort`, which
    `[profile.release] panic = "abort"` requires. Both are handled in the
    job (the template hardcodes the flag, so neither is fixable by argument).
  - **New `libretro-cross` CI job** (`.github/workflows/ci.yml`) cross-checks
    `rustynes-libretro` against the buildbot ABI families a Linux runner
    can model faithfully — MinGW-Windows and Android/NDK. The Apple
    families are excluded on purpose: bindgen needs a real per-target
    sysroot, there is no Apple SDK on a Linux runner, and feeding it host
    glibc headers would generate Apple bindings from Linux headers — a
    lookalike rather than a rehearsal. There was previously
    *zero* libretro coverage in GitHub Actions, which is why all three
    defects reached a third-party buildbot we cannot push to or re-run.
- **Libretro buildbot: the last failing job (tvOS) now builds on the pinned
  stable toolchain, dropping three workarounds.** The follow-up run
  ([pipeline #91954](https://git.libretro.com/libretro/RustyNES/-/pipelines/91954))
  took the recipe from 1/10 to 9/10, leaving only
  `libretro-build-tvos-arm64`. Its upstream template overrides `script` with
  `cargo +nightly build -Zbuild-std`, which dates from when
  `aarch64-apple-tvos` was a tier-3 target with no distributed `rust-std`.
  The target has since been promoted and rustup now ships a complete
  prebuilt std for it — `panic_abort` included — so the job now uses the
  shared Apple build script (via `!reference`) on the same pinned 1.96.0 as
  every other job. That removes all three workarounds the `+nightly` path
  had forced, rather than adding a fourth: the nightly-channel reinstall
  (needed because `+nightly` outranks the channel pin and the image's
  1.94.0-nightly is below the workspace `rust-version = "1.96"`); the
  `CARGO_PROFILE_RELEASE_PANIC=unwind` override (needed because bare
  `-Zbuild-std` omits the `panic_abort` that `[profile.release] panic =
  "abort"` requires, and the hardcoded flag cannot be overridden by
  `CARGO_UNSTABLE_BUILD_STD`); and a clearing of the image-injected
  `-Car=<path>,Clink-arg=...`, whose long-deprecated `-C ar` became a **hard
  error in Rust 1.97** (bisected: 1.96.1 warns, 1.97.1 errors) and so broke
  the refreshed nightly. tvOS now honours `panic = "abort"` exactly like
  every other platform. All four Apple jobs still receive the `-C ar` flag
  and are green only because 1.96.0 treats it as a warning, so
  `rust-toolchain.toml` carries a warning for whoever bumps that pin to
  1.97+.

## [2.2.1] - 2026-07-15 - Housekeeping patch (dev-tooling archival + dependency consolidation + FDS test corpus)

Zero accuracy, feature, or core changes — the deterministic `#![no_std]` chip
stack, save-state / TAS / netplay-replay formats, and every golden vector are
untouched. AccuracyCoin holds **141/141 (100.00%)**, unchanged from v2.2.0.

### Added

- **Game Genie re-key research tooling archived (PR #304).** Preserved the six
  intermediate research / verification scripts behind the header-robust Game Genie
  code re-key (which shipped in #262) beside the generator in `scripts/gg/`:
  `crc_combine.py` (a pure-Python `zlib.crc32_combine` implementation,
  self-tested against 2000 random synthetic trials, underpinning the
  `rom_crc32 == crc32_combine(prgCRC, chrCRC, chrLen)` identity),
  `alias_resolve.py` (long-tail title-alias CRC resolution), `coverage.py` /
  `coverage2.py` (name-join coverage accounting), `inspect.py`, and `verify.py`
  (which now proves the combine identity over every standard nes20db cart dump).
  Dev / research tooling only — paths resolve repo-relative, and it touches no
  crate and does not affect the build or the deterministic core.

- **`TakuikaNinja` FDS hardware-verification probes wired in (gated,
  gitignored).** Added `crates/rustynes-test-harness/tests/fds_takuikaninja.rs`
  with four `RUSTYNES_FDS_BIOS`-gated smoke tests against
  `FDS-Mirroring-Test`, `FDS-4023-Test`, `FDS-Audio-Registers`, and
  `FDS-4030D1-Addr` — real hardware-verified probes of `$4023`/mirroring/audio
  register behavior and the FDS DRAM-refresh-watchdog IRQ. None of the four
  carries an explicit permissive license, so they're staged gitignored under
  `tests/roms/external/fds-takuikaninja/` (fetched from the author's GitHub
  releases) rather than committed, mirroring the existing commercial-ROM
  convention; every test skips cleanly when the BIOS or a probe disk is
  absent, keeping CI clean by default. The underlying `$4023` and mirroring
  behaviors these probes exercise are already implemented and unit-tested
  independently in `crates/rustynes-mappers/src/fds.rs` — this is regression
  insurance against a second, hardware-verified oracle, not a fix for a gap.
  The `$4030.D1` DRAM-watchdog probe tracks a known, honest residual (not yet
  modeled by RustyNES or, per upstream, by most current FDS emulators) —
  see `docs/accuracy-ledger.md`.

### Changed

- **Dependency consolidation (PR #305 — closes Dependabot #298–#303).** Rolled
  all six open Dependabot bumps into one reviewed change, each verified against
  the code and gates; all landed with **no source changes** and the deterministic
  `#![no_std]` core untouched (AccuracyCoin stays **141/141**): **pollster
  0.4 → 1.0** (frontend / android / iOS `block_on` for wgpu/Metal init), **wide
  0.7 → 1.5** (the desktop `u32x8` SIMD blitter — the SIMD-vs-scalar byte-identity
  gate still passes), **tungstenite + tokio-tungstenite 0.29 → 0.30** (netplay +
  mobile signaling; `default-features = false` + `handshake` +
  `rustls-tls-webpki-roots` retained for clean pure-rustls cross-compiles),
  **bytemuck 1.25.0 → 1.25.1** and **cc 1.2.66 → 1.2.67** (patch), and CI
  **actions/setup-python v5 → v6** (MkDocs step). Verified with `cargo check` /
  `clippy -D warnings` / `cargo deny` / the `wide` byte-identity test / the full
  netplay tungstenite-0.30 suite.

## [2.2.0] - 2026-07-12 - "Capstone" (milestone cut — netplay matchmaking/lobby + FDS medium model + peripherals + quality/security pass)

### Added

- **Netplay lobby + matchmaking (v2.2.0 "Capstone", B5).** The pure signaling protocol (`crates/rustynes-netplay/src/signaling.rs`) grows a **browse-and-join** lobby directory and a matchmaking path atop the existing room-code / TURN stack. New `SignalMessage` variants — `ListRooms { rom_hash }` → `RoomList { rooms: Vec<RoomInfo> }` (the open, joinable, optionally game-filtered rooms; each `RoomInfo` carries the code / player count / capacity / `rom_hash` and *no* SDP/ICE/identity), and `QuickMatch { rom_hash, max_players }` → `Matched { room, slot, max_players }` (server-side "quick play": join any open room for the ROM via the shared `add_to_room` primitive, or create a fresh one with a deterministic `QM-NNNNNN` code). The `room-list` JSON array is parsed by a brace-depth walk bounded at `MAX_ROOM_LIST` (256) so an oversized frame cannot force an unbounded allocation. Determinism/rollback contract untouched — this is signaling only.
- **Delayed-stream spectators (v2.2.0 "Capstone", B5).** `SpectatorConfig.delay_frames` (clamped to `MAX_DELAY_FRAMES` = 512 ≈ 8.5 s) layers an intentional broadcast / anti-spoiler / jitter-smoothing hold atop the natural spectator lag: frame `f` is revealed only once frame `f + delay_frames` is confirmed (`reveal_horizon()`). Purely a *presentation* delay — frames are still produced byte-identically and in order, and the spectator still sends nothing — so it cannot perturb the match. Wired to a configurable `NetplayUi::spectator_delay_frames` (default 0).
- **Hardened desync surface (v2.2.0 "Capstone", B5).** `DesyncDiagnostics` gains a single graded `DesyncStatus` { `InSync` / `Suspect` / `Desynced` } verdict with a hysteresis threshold (`DEFAULT_DESYNC_THRESHOLD` = 3 consecutive mismatches ≈ 1.5 s at the 30-frame checksum interval) so a lone reordered / late peer checksum no longer flashes a false desync banner, plus a sticky peak-run rule so a confirmed (unrecoverable) desync never silently downgrades. Still pure telemetry over the `NetMessage::Checksum` digests the session already exchanges.
- **Peer-liveness RTT timeouts (v2.2.0 "Capstone", B5).** A graded `PeerLink` { `Live` / `Interrupted` / `TimedOut` } for an already-synced `NetplayConnection`, driven by `last_recv` against `peer_interrupt_timeout` (2 s) / `peer_disconnect_timeout` (5 s), plus a terminal `DisconnectReason::PeerTimeout`. Deliberately far above Mesen's trigger-happy ~150 ms (documented on `PeerLink`): a single lost 1 Hz `Quality` ping or a routine Wi-Fi/LTE retransmit spike never trips it, matching the multi-second grace windows GGPO/Parsec use. Both thresholds are builder-configurable (`with_peer_timeouts`).
- **Fuzz-target expansion (v2.2.0 "Capstone", quality).** `fuzz/` grows from 3 to 8 cargo-fuzz targets covering the remaining untrusted-input boundaries: `ppu_reg_io` (`Ppu::cpu_{read,write}_register` over a minimal `PpuBus`), `apu_reg_io` (`Apu::write_register` / `read_status`), `netplay_message` (the highest-value target — `NetMessage::from_bytes` binary UDP + `SignalMessage::parse` JSON signaling/lobby, both ingesting bytes straight off the wire), `save_state` (`parse_header` + `Nes::extract_thumbnail` + `restore_quiet`), and `movie` (`Movie::deserialize`). Each builds under nightly cargo-fuzz and runs clean for tens of thousands of iterations. `fuzz/README.md` documents the targets + the LeakSanitizer-under-sandbox note.
- **Read-only ROM Info browser (v2.2.0 "Capstone").** A new **Tools → ROM Info** panel (`crates/rustynes-frontend/src/debugger/rom_info_panel.rs`) surfaces, for the loaded ROM, the two dump-identity CRC32 keys (the header-excluded game-DB key + the full-file **No-Intro** key), the SHA-256, the effective per-game database entry (title / mapper / region / mirroring / submapper), and the decoded cartridge header read straight off the running `Nes` (mapper id, region, PRG-ROM / CHR-ROM sizes). Read-only (`&Nes`) — never mutates the emulator or the DB overlay; the deterministic core never consults it. Honest about surfacing only the vendored per-game DB + the header (no bootgod / nescartdb table is vendored).
- **MkDocs handbook deepening (v2.2.0 "Capstone", quality).** Four new Material-for-MkDocs handbook pages — `docs/expansion-audio.md`, `docs/pal-region.md`, `docs/crt-composite.md`, `docs/creator-tools.md` — curated entry points for the newer subsystems, cross-linked to the authoritative `apu-2a03.md` / `ppu-2c02.md` / `frontend.md` specs, with matching `mkdocs.yml` nav entries.
- **FDS medium model completion — CRC-16 / gap / continuous head-seek (v2.2.0
  "Capstone", F4.3).** The Famicom Disk System RAM adapter
  (`crates/rustynes-mappers/src/fds.rs`) completes the disk **medium** model. The
  disk is a synthesized byte-stream wire image — lead-in / inter-block gaps, a
  `$80` start mark, the block bytes, and a **CRC-16/KERMIT** per block — and each
  BIOS-written block now **re-emits a fresh per-block CRC-16** over its updated
  payload (`resynth_block_crc`), modelling the RP2C33 controller's continuous CRC
  generator so the medium stays self-consistent after a write. A new **continuous
  analog head-seek / velocity model** (opt-in, default-OFF —
  `Fds::set_analog_head_seek`) replaces the flat fixed `HEAD_RESEEK_CYCLES`
  motor-restart not-ready window with a belt-driven, distance-proportional seek
  time (`HEAD_SEEK_BYTES_PER_CYCLE` velocity + `HEAD_SEEK_SETTLE_CYCLES` settle,
  clamped to a cold spin-up), sized from the head-travel distance captured at
  motor-off. A **BIOS-free synthetic write-verify oracle**
  (`Fds::medium_write_verify`) walks the wire image and asserts every block's
  CRC-16 and gap/mark framing round-trips — the CI-verifiable half of the medium
  model; the real-BIOS write-CRC path needs a copyright `disksys.rom` and is
  exercised only from a gitignored local dump (`docs/accuracy-ledger.md` records
  the CI-verifiable-vs-local-only split). **Additive and deterministic**: with
  the head-seek model off (the default) a non-writing `.fds` run is
  **byte-identical** to prior releases; the new state round-trips an additive
  **v4** FDS save-state tail (v1/v2/v3 blobs load with the model disabled).
  AccuracyCoin has no FDS ROM, so **141/141 (100%)** is unaffected.
- **Famicom microphone + Zapper light-timing hardening (v2.2.0 "Capstone"
  peripherals).** The Famicom built-in controller-2 **microphone** is modelled on
  **`$4016` bit 2** (`Nes::set_microphone` / `Bus::set_microphone`), wired through
  the frontend input path (hold-to-talk `M` key → `FrameInputs.microphone` →
  latch), for games such as *The Legend of Zelda* (Pols Voice) and *Kid Icarus*.
  It is a `$4016`-only signal (never touches `$4017`). The **Zapper** photodiode
  now integrates a **3×3 aperture** (field-of-view) around the aim point,
  asserting light only when ≥2 pixels cross the luma threshold
  (`ZAPPER_APERTURE_*`) — hardening detection against sub-pixel aim error and PPU
  edge noise vs the prior single-pixel sample, while staying a deterministic pure
  function of the presented framebuffer (no save-state change). Both are additive
  and **default-off**: the mic released leaves the `$4016` read byte-identical,
  and the standard controller / Four Score path is unchanged. (The full Family
  BASIC `9×8` keyboard matrix was already modelled; its frontend mapping is
  unchanged.)

### Changed

- **Movie (`.rnm`) deserializer hardening (v2.2.0 "Capstone", quality).** The new `movie` fuzz target surfaced two OOM DoS paths in `Movie::deserialize` (`crates/rustynes-core/src/movie.rs`), both now fixed **byte-identically for valid input**: (1) the untrusted 4-byte `frame_count` was passed straight to `Vec::with_capacity`, so a 49-byte header could claim a multi-gigabyte reservation — now capped at `remaining_bytes / width` (== `frame_count` for any real file); (2) a `bytes_per_frame` of 0 made each `r.take(0)` consume no input, so the frame loop pushed `frame_count` empty records out of a finite file — now rejected up front (a real movie always writes the fixed `BYTES_PER_FRAME` ≥ 1). Regression test `deserialize_hostile_frame_count_does_not_oom` added; the existing 44 movie tests (incl. the determinism round-trip) stay green.

## [2.1.10] - 2026-07-12 - "Fathom" (creator tools and web parity — TAStudio greenzone + Lua API breadth + browser-RA auth-proxy deploy stack + Vs. DualSystem libretro presentation — "Loom")

### Added

- **Vs. `DualSystem` presentation in the libretro core (v2.1.10 "Web Parity").**
  The libretro core (`crates/rustynes-libretro`) now presents Vs. `DualSystem`
  arcade cabinets (Balloon Fight / Wrecking Crew / Tennis / Baseball), reaching
  parity with the desktop frontend. It detects them with the same `Emu::from_rom`
  (NES 2.0 header Vs. type OR the SHA-keyed `vs_db`), steps **both** cross-wired
  consoles each `retro_run`, and composes their two 256×240 framebuffers into a
  single **512×240** XRGB8888 side-by-side image (MAIN left, SUB right) — presented
  within a 512-wide `max_width` geometry so RetroArch draws the variable width with
  no geometry renegotiation. Libretro ports 0/1 → MAIN P1/P2, 2/3 → SUB P1/P2; only
  MAIN audio plays; save states use `VsDualSystem::snapshot`/`restore`; the RA/cheat
  memory maps expose the MAIN console. Previously a `DualSystem` dump booted a single
  console that hangs on its absent partner. The deterministic `no_std` core is
  untouched and byte-identical — this is a parallel present/serialize branch in the
  FFI wrapper. **Code-complete + builds** (`cargo build -p rustynes-libretro`);
  a live RetroArch run with a real cabinet dump is the maintainer's manual check.
  Docs: `docs/libretro/advanced_features.md`, `docs/frontend.md`.
- **Browser RetroAchievements auth-proxy deploy stack (v2.1.10 "Web Parity", ADR
  0015).** The browser-RA marshalling (`ra_glue.js` + `wasm_cheevos.rs`) has shipped
  since v1.7.0; this lands the remaining ADR 0015 carryover's **deployable** half —
  the casual-only auth proxy that injects RA's identity `User-Agent` server-side
  (browsers forbid scripts from setting it). `deploy/` gains a first-class
  `ra-proxy` compose service (`deploy/Dockerfile.raproxy`, running the stdlib-only
  reference stub) behind the shared Caddy TLS proxy at `https://<DOMAIN>/ra/*`,
  configured **purely from env** (`RA_USER_AGENT` / `RA_ALLOWED_ORIGINS` /
  `RA_UPSTREAM` / `RA_ENFORCE_CASUAL`) — the proxy holds no RA secret. The stub
  grew env-var configuration so one script serves both local dev and the container.
  Marshalling-contract tests added to `wasm_cheevos.rs` (ACHIEVEMENT_TRIGGERED
  filtering + malformed-payload tolerance + the not-configured caveat).
  **Code-complete + compose/config validated**; standing the stack on a live host,
  the RA-team `User-Agent` coordination, and a real browser RA login + casual unlock
  are the un-CI-able acceptance gate (runbook: `deploy/README.md`,
  `docs/cheevos-browser.md`).
- **Creator tools: TAStudio depth + Lua API breadth (v2.1.10 "Creator Tools &
  Web Parity", B8 + B9).**
  - **Force-greenzone (B8).** A new "Force GZ" toggle in the TAStudio piano-roll
    header guarantees a cached save-state at *every* frame in a bounded range
    (up to `MAX_FORCED_GREENZONE_FRAMES` = 10,800 ≈ 3 min at 60 fps), so
    scrubbing / rewinding anywhere inside it is instant — versus the normal
    density-tiered keyframe skeleton. Forced frames are pinned as non-evictable
    anchors and captured as the editor seeks / records across them; shrinking or
    clearing the range releases only the anchors force-greenzone itself added
    (marker / branch-point anchors are untouched). Documented memory budget; a
    pure caching optimisation, so a seek into the forced range stays
    bit-identical to a linear replay (the determinism / TAS contract is
    unchanged). *(Named markers and branch save-slots already shipped in v1.6.0;
    this deepens the greenzone half.)*
  - **Lua HUD: `emu.drawLine` (B9).** The fourth overlay primitive alongside
    `drawText` / `drawRect` / `drawPixel` — a straight segment for graphs, watch
    plots, and hitbox visualisers. Pure overlay (never write-gated); full mlua +
    piccolo parity.
  - **Lua memory: palette + CHR domains (B9).** `memory:read_palette(idx)`
    (`$3F00-$3F1F`, 6-bit index) and `memory:read_chr(addr)` (`$0000-$1FFF`,
    mapper-banked), both via the side-effect-free debug-peek path — the
    `*Debug` (no open-bus / no read-buffer-advance / no mapper side-effect)
    variant by construction on this observational engine.
  - **Lua lifecycle events (B9).** `reset`, `spriteZeroHit`, and `codeBreak`
    join the `emu.addEventCallback` surface (host-fired: `reset` on
    soft-reset / power-cycle, `spriteZeroHit` once per frame the PPU sprite-0
    hit flag was set — sampled non-destructively via `peek($2002)` — and
    `codeBreak` on a debugger breakpoint). Observational (no live `Nes`).
  - **piccolo (wasm) parity uplift (B9).** The experimental pure-Rust backend
    gains `emu.drawLine`, the read-parity `memory` table (CPU / PPU / palette /
    CHR / OAM reads served from an extended per-frame snapshot; `poke` keeps the
    gated + deferred contract), and an `addEventCallback` no-op so portable
    scripts don't error on wasm — closing most of the read + HUD gap (ADR 0012
    carve-out now only the per-access / per-interrupt replay callbacks and the
    host-fired events).
  - **Example script library.** Three well-commented additions —
    `hud_graph.lua` (drawLine value graph), `palette_viewer.lua`
    (`read_palette` / `read_chr` inspector), and `lifecycle_events.lua` (every
    `addEventCallback` event) — all compile-time embedded and exercised by the
    `bundled_example_scripts_load_and_run` test.
  - Determinism preserved throughout: reads are debug-peeks, writes stay
    gated / deferred exactly as before, and the deterministic `#![no_std]` core
    is untouched — **AccuracyCoin holds 141/141 (100%)** and save-state / TAS
    replay stays byte-identical.

### Deferred (documented)

- **Vs. `DualSystem` on the wasm desktop-style present (v2.1.10 "Web Parity").**
  The CPU compositor (`Gfx::compose_dual_into`) and the core (`Emu::Dual`) are
  already cross-platform, but the wasm ROM-load detection + un-gating the GPU
  present branch (`Gfx::render_dual`, currently `cfg(not(wasm))`) remain deferred.
  The libretro dual present (see Added) ships now; mobile stays deferred. See
  `docs/frontend.md`.

## [2.1.9] - 2026-07-12 - "Fathom" (presentation and signal — marquee CRT shader stack (CRT-Royale / guest-advanced / Sony Megatron) + raw NTSC composite signal-decode path + GIF/WAV capture + generated-palette editor — "Aperture")

### Added

- **Marquee CRT shader stack + raw NTSC composite signal (v2.1.9 "Presentation
  & Signal").** A presentation/display cut, all opt-in and **default
  byte-identical** (the shipped presentation is untouched, so `visual_regression`
  stays byte-identical and AccuracyCoin holds **141/141**).
  - **Raw composite core (P4).** A new `rustynes-ppu::raw_signal` module that
    keeps the 2C02 composite waveform *un-decoded*: for every `(index, emphasis)`
    pair it emits the twelve per-subcarrier-phase voltages the chip actually
    generates, so a decoder can reproduce signal-domain artifacts a per-colour
    palette cannot — composite colour bleed, dot crawl, and the waterfall/dither
    transparency tricks. Follows the canonical Bisqwit `nes_ntsc` / Mesen2 "raw
    palette" model; `generate_raw_signal_lut()` yields the full 512×12 table a
    host uploads as a signal texture. No transcendental in the path, so it is
    `f32` byte-identical across x86 / aarch64 / wasm / `thumbv7em` (a `no_std`
    `GOLDEN_SIGNAL` cross-target lock guards it). Additive + default-OFF.
  - **CRT shader stack (B6).** Three single-pass WGSL ports of the reference
    libretro *slang* CRT presets, added as **new WGSL files** in
    `rustynes-gfx-shaders` behind a `CrtStackShader` registry: **CRT-Royale**
    (luminance-scaled Gaussian beam, selectable aperture/slot/shadow mask,
    gamma-correct scanlines, curvature), **CRT Guest Advanced / guest-dr-venom**
    (power-shaped beam, halation glow, mask, curvature), and **Sony Megatron**
    (per-subpixel phosphor lighting with an HDR headroom + SDR Reinhard
    fallback). All four new shaders — the three CRT plus the P4 signal-decode
    pass — are gate-validated as real, compilable WGSL by the same **naga**
    front-end + validator wgpu runs at pipeline creation.
  - **Composable-stack UI wiring + per-game presets.** The CRT trio and the raw
    signal-decode pass are selectable from **Settings → Shaders** as first-class
    `ShaderStack` passes with their `#pragma parameter` sliders (mask type,
    scanline weight, curvature, beam, glow, HDR headroom), plus per-game shader
    presets (auto-applied on ROM load, off by default).
  - **Raw NTSC signal-decode pass (P4 shader).** `signal_decode.wgsl`
    reconstructs the 2C02's actual two-level chroma square wave from the
    palette-index framebuffer (matching `raw_signal.rs` byte-for-byte) and
    demodulates it with a windowed quadrature filter — decoding the true signal
    rather than re-encoding already-decoded RGB.
  - **Capture: GIF / animated capture + WAV audio export.** The `av-record`
    feature gains GIF export (palette-quantized, frame-decimated) and standalone
    WAV audio export alongside the existing PNG-sequence / raw A/V capture —
    driven from a read-only framebuffer/audio tap so capture never perturbs the
    deterministic emulation timeline.
  - **Palette editor — live generated-palette preview.** The existing
    generated-NTSC palette editor (Settings → Video → Generated NTSC palette:
    saturation / hue / contrast / brightness / gamma sliders feeding
    `palette_gen.rs`, plus the 64-swatch editor) gains a live 16×4 swatch preview
    of the *generated* base that regenerates from the current params as you drag
    the sliders — so the look can be dialled in before enabling it. Purely
    visual; the shipped palette is unchanged until explicitly enabled.

## [2.1.8] - 2026-07-12 - "Fathom" (performance — default-off specialized fast PPU dot-loop (differential-tested byte-identical, +12% rendering-heavy) + SIMD software blitter + wasm size/startup pass; "Tempo")

### Performance

- **Specialized visible-scanline fast dot path (v2.1.8 "Performance" A1;
  default-OFF, opt-in).** Profiling a representative mixed workload
  (`perf`, the PGO training corpus) shows `Ppu::tick` is the emulator's single
  hottest function — **~46% of frame self-time** — and the overwhelming majority
  of its 89,342 per-frame invocations are visible-scanline background-render dots
  whose surrounding event/bookkeeping branches are all statically dead. A new
  **runtime knob** (`Nes::set_fast_dotloop`, default **false**) dispatches those
  "clean" dots — a visible scanline, dots `1..=256`, rendering stably enabled,
  and no sub-dot disturbance in flight (no `$2006` copy-V or PPUMASK write-delay
  pending, no PPUDATA state machine running, no armed/pending OAM-corruption,
  warm scanline-classification cache) — to `Ppu::tick_visible_render_fast`, a
  straight-line handler that runs the **identical** helper sequence with the
  dead branches pruned. Any disturbance drops instantly back to the exact
  per-dot path.
  - **Byte-identical (proven, not assumed).** The default (`false`) is
    byte-identical to a build without the field. With the knob ON, a new
    differential test (`fast_dotloop_diff`) runs a corpus (`nestest`,
    `flowing_palette`, `oam_stress`, `AccuracyCoin`, the Holy Mapperel MMC1/MMC3
    boards, and a mid-frame raster demo) through BOTH paths and asserts
    bit-for-bit identical framebuffer + palette-index framebuffer + audio + CPU
    cycles + full core snapshot, every frame — including under the opt-in
    `Rp2c02G` die revision (v2.1.7 #280), whose `$2003`-write-during-render
    OAM-corruption is one of the disturbances that forces the exact path.
    AccuracyCoin holds **141/141**, nestest 0-diff, the `visual_regression`
    golden set, and the APU oracle all stay byte-identical.
  - **Measured (interleaved per-frame A/B, drift-robust):** rendering-enabled
    content (`nestest`, a rendered menu) is **~+12.3% faster per frame**
    (4.54 → 3.98 ms, stable across rounds), well above the project's
    >3%-Criterion adoption bar; rendering-**disabled** content (`flowing_palette`,
    which shows all 64 colours via the rendering-off backdrop-override trick, so
    the fast path never applies) is **neutral** (~+0.3%, the reordered
    short-circuit guard costs ~nothing). See `docs/performance.md`.
  - **Architectural note:** a *whole-scanline batch* (the Mesen2/tetanes-style
    straight-line renderer) is **precluded** by the v2.0.0 "Timebase" lockstep
    every-cycle-bus-access scheduler — `run_ppu_to` advances the PPU ≤3 dots per
    CPU cycle and the CPU observes A12/NMI/sprite-0/`$2002` at 3-dot granularity,
    so the PPU is never invited to run a scanline uninterrupted. This is a
    per-dot specialization, not a dot-batch. **Shipped default-OFF** (the
    shipped build is unchanged/byte-identical); recommended for promotion to
    default after maintainer review + a clean-host Criterion confirmation.
- **Vectorized software palette-index -> RGBA blitter + wasm size/startup pass
  (v2.1.8 "Performance", A2 + A4).** A new frontend-only `gfx_blit` module
  (`crates/rustynes-frontend/src/gfx_blit.rs`) that converts the PPU's
  palette-index framebuffer (`&[u16]`, `(emphasis << 6) | colour`) to RGBA8
  through the exact 512-entry LUT the core emits with, so its output is
  **byte-identical** to `Ppu::framebuffer` by construction. Three interchangeable
  paths — a scalar reference, a tight scalar-`u32`, and portable SIMD (`wide::u32x8`
  on desktop / `core::arch::wasm32` `v128` under `+simd128` on wasm, with a scalar
  fallback for non-SIMD wasm) — all validated byte-for-byte equal by the
  `simd_equals_scalar_byte_identical` unit test over a full-frame corpus that sweeps the whole
  `0..512` domain, and profiled by a Criterion bench (`benches/gfx_blit.rs`). The
  conversion is a memory-bound LUT gather, so per the measured bench the SIMD path
  is within noise of scalar (documented honestly in `docs/performance.md`); the
  module is a reusable, oracle-checked utility (the shipped on-screen frame path
  stays GPU-resident and does not route through it). Determinism-neutral: the core
  and its golden vectors are untouched — AccuracyCoin **141/141**, `visual_regression`
  byte-identical. Web build (A4): the release wasm artifact now runs `wasm-opt -O4`
  (SIMD + bulk-memory features preserved) via `data-wasm-opt` in `web/index.html`,
  with streaming instantiation documented; the real `trunk build --release`
  bundle measures **3.99 MiB gzip** — 1.01 MiB of headroom under the 5 MiB budget
  (`scripts/wasm_size_budget.sh`). `wide` is a native-only dependency, so it
  never enters the wasm bundle and the `#![no_std]` chip stack stays
  dependency-light.

## [2.1.7] - 2026-07-12 - "Fathom" (hardware revisions & DMA frontier — opt-in PPU/2A03 die-revision + power-on RAM/palette model + honest DMA "unexpected read" residual ADR 0033; "Stepping")

### Added

- **PPU die-revision + power-on hardware model (v2.1.7 "Hardware Revisions &
  DMA Frontier", P5 — PPU side).** A selectable 2C02 die revision and power-on
  state model, every knob **opt-in and default-off** so the deterministic core
  stays **byte-identical** at the default (AccuracyCoin **141/141 (100%)**,
  nestest 0-diff, `visual_regression` / `pal_apu_tests` 10/10 and save-state
  round-trip all unchanged). Four additive pieces:
  - **`PpuRevision` enum** (`rustynes_core::PpuRevision`; default `Rp2c02H`,
    opt-in `Rp2c02G`) gating the one revision-dependent quirk RustyNES models.
    Config re-applied on load like `region` — not serialized.
  - **OAMADDR (`$2003`) write-during-render OAM corruption**, modeled only on
    the opt-in `Rp2c02G` die: a `$2003` write while rendering is active copies
    OAM row 0 over the row `(value>>3)&0x1F` (reusing the existing `CorruptOAM`
    row-copy, committed on the next rendered dot). The *Huge Insect* glitch. The
    default revision never arms it. The corruption state it can arm
    (`oam_corruption_pending`/`_index`) already round-trips via the v6 PPU
    snapshot tail, so **no snapshot-format change** is needed. Documented as an
    honest opt-in approximation (the exact per-revision/per-title byte output is
    not independently oracle-verified) in `docs/accuracy-ledger.md`.
  - **Power-up palette-RAM model** (`rustynes_core::PaletteInit`; default
    `Zeroed`, opt-in `Blargg`) loading the canonical blargg power-up dump (6-bit
    masked) for software that samples uninitialized palette RAM. Writes only
    `palette_ram` (already serialized), so no snapshot change. Default keeps the
    established all-zero power-up palette.
  - **Power-on work-RAM model** (`rustynes_core::PowerOnConfig` / `PowerOnRam`:
    `Zeroed` default / `Seeded(u64)` / `Filled(u8)`) via the new
    `Nes::from_rom_with_power_on_config`, for titles that read uninitialized RAM
    (*Final Fantasy* RNG seed, *River City Ransom*, *Cybernoid*). Every fill is
    **deterministic** (no wall-clock / OS RNG), stored on the bus so
    `power_cycle == fresh boot`. `from_rom_with_power_on_seed` now routes through
    `PowerOnRam::Seeded`; the default all-zero path is unchanged.
  - Exposed through additive `Nes` setters (`set_ppu_revision`,
    `set_power_up_palette`, `set_power_on_ram`) mirroring the v2.1.4 OAM-decay
    knob shape, and default-off `[emulation]` config keys
    (`ppu_oamaddr_corruption`, `blargg_power_up_palette`,
    `randomize_power_on_ram`, `power_on_ram_seed`) pushed into the core on ROM
    load / power-cycle / startup. `docs/ppu-2c02.md` documents each; the
    `#![no_std]` chip stack stays clean.
- **2A03 die-revision config + the DMA "unexpected read" frontier (v2.1.7
  "Hardware Revisions & DMA Frontier").** New additive
  `Cpu2A03Revision { Rp2A03G (default), Rp2A03H }` config
  (`Nes::set_cpu_2a03_revision`) gating the DMA unit's "unexpected DMA" extra
  parked-address re-read on a DMC-halt-overlaps-OAM-halt cycle. **Modeled +
  verified:** the existing DMC↔OAM collision (get/put), OAM alignment, aborted
  DMC-DMA, and the `$2007`/`$4015`/`$4016`/`$4017` DMC-glitch register-readout
  corruption all stay green on the default — the five `dmc_dma_during_read4`
  ROMs, both `sprdma_and_dmc_dma` variants, and `dma_timing_pin` all `Pass`.
  **Documented residual (honesty gate, ADR 0033):** the RP2A03G-vs-RP2A03H die
  revision is modeled by **no** public reference emulator (Mesen2 / ares /
  BizHawk / TriCNES / fceux / nestopia / GeraNES / higan) and verified by **no**
  test ROM; on this engine the revision gate fires but is a **documented no-op
  on every committed oracle** (the parked address during a DMC+OAM overlap is
  always the post-`$4014` instruction fetch, never a side-effect register), so
  `Rp2A03H` is byte-identical to `Rp2A03G` today — the difference is a
  mechanism-level model, not an observable divergence, and its direction is an
  unverified hypothesis recorded not faked. The revision is a config re-applied
  on load, **not** part of the save-state; the default (`Rp2A03G`) stays
  byte-identical (AccuracyCoin **141/141**, nestest 0-diff, save-state
  round-trip byte-identical). No `dmc_dma_during_read4` sub-test is made to fail
  or newly `#[ignore]`'d. See ADR 0033 + `docs/scheduler.md` §"Unexpected DMA".

## [2.1.6] - 2026-07-11 - "Fathom" (expansion audio — decibel oracle + hardware/Mesen2 channel-level calibration + Namco 163 12 dB fix + mix UI/scopes; "Timbre")

### Added

- **Marquee CRT shader stack + raw NTSC composite signal (v2.1.9 "Presentation
  & Signal").** A presentation/display cut, all opt-in and **default
  byte-identical** (the shipped presentation is untouched, so `visual_regression`
  stays byte-identical and AccuracyCoin holds **141/141**).
  - **Raw composite core (P4).** A new `rustynes-ppu::raw_signal` module that
    keeps the 2C02 composite waveform *un-decoded*: for every `(index, emphasis)`
    pair it emits the twelve per-subcarrier-phase voltages the chip actually
    generates, so a decoder can reproduce signal-domain artifacts a per-colour
    palette cannot — composite colour bleed, dot crawl, and the waterfall/dither
    transparency tricks. Follows the canonical Bisqwit `nes_ntsc` / Mesen2 "raw
    palette" model; `generate_raw_signal_lut()` yields the full 512×12 table a
    host uploads as a signal texture. No transcendental in the path, so it is
    `f32` byte-identical across x86 / aarch64 / wasm / `thumbv7em` (a `no_std`
    `GOLDEN_SIGNAL` cross-target lock guards it). Additive + default-OFF: the
    core, the default framebuffer, and AccuracyCoin are unaffected.
  - **CRT shader stack (B6).** Three single-pass WGSL ports of the reference
    libretro *slang* CRT presets, added as **new WGSL files** in
    `rustynes-gfx-shaders` behind a `CrtStackShader` registry: **CRT-Royale**
    (luminance-scaled Gaussian beam, selectable aperture/slot/shadow mask,
    gamma-correct scanlines, curvature), **CRT Guest Advanced / guest-dr-venom**
    (power-shaped beam, halation glow, mask, curvature), and **Sony Megatron**
    (per-subpixel phosphor lighting with an HDR headroom + SDR Reinhard
    fallback). All four new shaders — the three CRT plus the P4 signal-decode
    pass — are gate-validated as real, compilable WGSL by the same **naga**
    front-end + validator wgpu runs at pipeline creation.
  - **Raw NTSC signal-decode pass (P4 shader).** `signal_decode.wgsl`
    reconstructs the 2C02's actual two-level chroma square wave from the
    palette-index framebuffer (matching `raw_signal.rs` byte-for-byte) and
    demodulates it with a windowed quadrature filter — decoding the true signal
    rather than re-encoding already-decoded RGB.
  - Display suite: the CRT shaders expose curvature / mask-type / scanline-weight
    uniforms and build on the existing 8:7 PAR correction, overscan crop, and
    hqNx/xBRZ integer-style scaler foundations already in the tree.

- **Expansion-audio mix UI + per-channel visualization (v2.1.6 "Expansion
  Audio").** A dedicated **Audio Mixer** tool panel (Tools → Audio Mixer)
  unifying per-source mix balance with live per-channel visualization for any
  ROM — cartridge audio, not just `.nsf` tunes. Per-source gain sliders
  (`0.0`–`2.0`) + mute toggles for the five base 2A03 channels (pulse 1/2,
  triangle, noise, DMC) and the on-cart **expansion** channel, which is enabled
  and labelled with the detected chip family (VRC6 / VRC7 (OPLL) / MMC5 / Namco
  163 / Sunsoft 5B / FDS). Sensible **presets** — `Authentic (HVC-001)` (unity),
  a Mesen-style `Balanced` rebalance (tames a hot expansion chip vs the 2A03),
  and `Expansion boost` — plus a reset-to-unity. Per-channel **oscilloscope**
  traces and peak **VU meters** (master + all six sources), including a new
  read-only expansion-audio display tap (`ApuDebugView::external` /
  `Apu::external_out()`). The NSF player panel gains the same expansion scope/VU,
  and the scope/VU primitives are factored into a shared module reused by both.
  **The mix is a frontend re-weight, not a synthesis change**: it drives the
  existing determinism-safe `channel_gain` / `channel_mask` core overlay, which
  is byte-identical at unity and is never serialized into the save state — so a
  save-state / TAS / netplay replay stays byte-identical regardless of the
  slider positions, and the visualization samples a read-only copy that never
  feeds back into synthesis. AccuracyCoin holds **141/141 (100%)**.
- **Expansion-audio decibel oracle (v2.1.6 "Expansion Audio").** Upgraded `crates/rustynes-test-harness/tests/audio_expansion.rs` from pure `insta` snapshots into a real accuracy oracle: each bbbradsmith `db_*` comparison ROM now has a machine-verifiable level criterion. The new `level_db_*` tests measure the peak amplitude of the reference-2A03-square and expansion-square segments in the rendered waveform (`common::capture_frame_peaks` over deterministic frame windows) and **assert** the expansion/reference ratio against the Mesen2 / hardware target — APU triangle ÷ square ≈0.524, VRC6 ≈1.506, MMC5 ≈1.000, N163 1-channel ≈6.02. The 19 `insta` snapshots are retained as byte-exact regression guards.
- VRC7 instrument-ROM verification: `vrc7_all_15_melodic_patches_match_nuke_ykt_canonical` pins all 15 melodic patches (+ 3 rhythm) to the canonical Nuke.YKT dump (byte-identical across fceux / Mesen2 / nestopia) — the real `patch_vrc7` criterion. Sunsoft 5B log-DAC step-law and Namco 163 long-period (256-sample) wavetable unit tests added.
- **Vs. `DualSystem` presentation in the libretro core (v2.1.10 "Web Parity").**
  The libretro core (`crates/rustynes-libretro`) now presents Vs. `DualSystem`
  arcade cabinets (Balloon Fight / Wrecking Crew / Tennis / Baseball), reaching
  parity with the desktop frontend. It detects them with the same `Emu::from_rom`
  (NES 2.0 header Vs. type OR the SHA-keyed `vs_db`), steps **both** cross-wired
  consoles each `retro_run`, and composes their two 256×240 framebuffers into a
  single **512×240** XRGB8888 side-by-side image (MAIN left, SUB right) — presented
  within a 512-wide `max_width` geometry so RetroArch draws the variable width with
  no geometry renegotiation. Libretro ports 0/1 → MAIN P1/P2, 2/3 → SUB P1/P2; only
  MAIN audio plays; save states use `VsDualSystem::snapshot`/`restore`; the RA/cheat
  memory maps expose the MAIN console. Previously a `DualSystem` dump booted a single
  console that hangs on its absent partner. The deterministic `no_std` core is
  untouched and byte-identical — this is a parallel present/serialize branch in the
  FFI wrapper. **Code-complete + builds** (`cargo build -p rustynes-libretro`);
  a live RetroArch run with a real cabinet dump is the maintainer's manual check.
  Docs: `docs/libretro/advanced_features.md`, `docs/frontend.md`.
- **Browser RetroAchievements auth-proxy deploy stack (v2.1.10 "Web Parity", ADR
  0015).** The browser-RA marshalling (`ra_glue.js` + `wasm_cheevos.rs`) has shipped
  since v1.7.0; this lands the remaining ADR 0015 carryover's **deployable** half —
  the casual-only auth proxy that injects RA's identity `User-Agent` server-side
  (browsers forbid scripts from setting it). `deploy/` gains a first-class
  `ra-proxy` compose service (`deploy/Dockerfile.raproxy`, running the stdlib-only
  reference stub) behind the shared Caddy TLS proxy at `https://<DOMAIN>/ra/*`,
  configured **purely from env** (`RA_USER_AGENT` / `RA_ALLOWED_ORIGINS` /
  `RA_UPSTREAM` / `RA_ENFORCE_CASUAL`) — the proxy holds no RA secret. The stub
  grew env-var configuration so one script serves both local dev and the container.
  Marshalling-contract tests added to `wasm_cheevos.rs` (ACHIEVEMENT_TRIGGERED
  filtering + malformed-payload tolerance + the not-configured caveat).
  **Code-complete + compose/config validated**; standing the stack on a live host,
  the RA-team `User-Agent` coordination, and a real browser RA login + casual unlock
  are the un-CI-able acceptance gate (runbook: `deploy/README.md`,
  `docs/cheevos-browser.md`).

### Changed

- **Expansion-audio channel levels calibrated to the hardware / Mesen2 db_* levels.** VRC6 square `256 → 979` (`VRC6_MIX_SCALE`, ≈0.39× → ≈1.51× the 2A03 pulse), MMC5 pulse/PCM `256/16 → 650/40` (≈0.39× → ≈1.0×, "equivalent to the APU" per hardware), and **Namco 163** `64 → 261` (`NAMCO163_MIX_SCALE`, ≈1.48× → ≈6.02× for 1-channel mode — no reference emulator attenuates N163; ours was ~12 dB too quiet). The N163 fix is bit-shared with the NSF playback path. **Base 2A03 NTSC output stays byte-identical** — expansion audio is a separate additive `mix_audio` term (0 for non-expansion mappers), so AccuracyCoin (141/141), `blargg_apu_2005`, `nestest`, and `visual_regression` are unchanged; only the three `db_vrc6a/b`/`db_mmc5` expansion snapshots were re-blessed (audio hash only, provably more accurate).

### Deferred (documented)

- **Vs. `DualSystem` on the wasm desktop-style present (v2.1.10 "Web Parity").**
  The CPU compositor (`Gfx::compose_dual_into`) and the core (`Emu::Dual`) are
  already cross-platform, but the wasm ROM-load detection + un-gating the GPU
  present branch (`Gfx::render_dual`, currently `cfg(not(wasm))`) are deferred to
  the v2.1.8/v2.1.9 gfx/composite rebase to avoid colliding with that concurrently
  rewritten present path. The libretro dual present ships now (see Added); mobile
  stays deferred. See `docs/frontend.md`.
- **Sunsoft 5B absolute level** and **VRC7 FM level** are honest documented gaps (`docs/accuracy-ledger.md` §Expansion-audio levels): the 5B log-DAC *shape* is hardware-exact but its full vol-15 / 3-simultaneous-tone range overflows the `i16` `mix_audio` contract (needs a wider mix path); the VRC7 OPLL FM synth + patch ROM are correct, but the pseudo-sine absolute level is patch-dependent and has no clean square-vs-square oracle. Both stay snapshot-guarded.

## [2.1.5] - 2026-07-11 - "Fathom" (regression net & residual — Holy Mapperel mapper regression net + PAL APU frame-counter 10/10 + real TURN NAT-retransmit production fix + fat-LTO A/B validation + MMC3 F5.0 A12-phase study; "Vernier")

### Added

- **Mapper bank-reachability + IRQ regression net (v2.1.5 "Regression Net &
  Residual").** Wired the tepples **Holy Mapperel** cartridge-PCB-assembly test
  ROMs into CI as a dedicated mapper regression net
  (`crates/rustynes-test-harness/tests/holy_mapperel.rs`, gated on the default
  `--features test-roms`). Holy Mapperel detects which mapper it is running on
  purely from the console's mirroring + bank-switching response (no header
  trust), sizes PRG/CHR ROM/RAM, proves every PRG/CHR bank is reachable, and
  exercises WRAM + the MMC3/FME-7 interval-timer IRQ — coverage the
  `AccuracyCoin` and blargg CPU/PPU corpora barely touch and the gitignored
  60-ROM commercial oracle can't provide in CI. Because Holy Mapperel reports
  its verdict visually (no blargg `$6000` status protocol), each of the 17
  committed zlib-licensed ROMs is driven to its settled result screen and pinned
  by an `insta` framebuffer-hash snapshot (the same determinism-backed technique
  `visual_regression` uses), with two structural guards — *settled* (byte-stable
  across a late frame window, so a Morse-code hard-crash never green-lights) and
  *non-blank* — running first so a hard fault surfaces with a ROM-named message.
  The suite is data-driven over the committed ROM directory, so a newly-added
  ROM auto-enrolls (new snapshot line + a forced `UNVERIFIED` classification).
  15 of 17 ROMs detect the correct mapper and reach every bank with detailed
  code `0000`; the two MMC1 (`M1_*`) and two FME-7 (`M69_*`) ROMs surface a
  documented, honestly-pinned **WRAM-protection residual** whose cause differs
  per mapper. MMC1 genuinely does not model its software WRAM write-protect:
  `mmc1.rs` accesses `$6000-$7FFF` `prg_ram` unconditionally and ignores the
  `$E000`/`$A000` bit-4 disable, so the driver flags `1000` (SJROM, `$E000`
  layer) / `5000` (SNROM, both layers) — a widely-shared simplification (Holy
  Mapperel's own README notes FCEUX / PowerPak omit it, and modelling MMC1
  RAM-disable is a known game-compat hazard). FME-7 is *not* an
  always-enabled-WRAM case: it **does** model the command-`$8` RAM-enable
  (bit 7) / RAM-select (bit 6) bits — `sprint3.rs` maps PRG-RAM only when both
  are set and PRG-ROM when RAM is deselected — so its `1000` is a narrower gap:
  the "RAM selected but disabled" state (bit 6 = 1, bit 7 = 0) should read back
  as **open bus**, but RustyNES falls through to the last PRG-ROM bank; the
  driver's third "read open bus" sub-check reads the last-bank tag (`1`, below
  its `>= 3` open-bus threshold) and flags the WRAM-enable nibble. The FME-7
  IRQ nibble is `0` (the interval-timer IRQ works), and neither case is a
  bank-reachability defect (every bank is reachable). The net is purely
  additive: it changes no core behavior, so `AccuracyCoin` (141/141) and the
  commercial byte-identity oracle stay unchanged. ROM license provenance
  (zlib, Damian Yerrick) is recorded in `tests/roms/LICENSES.md`; the residuals
  are recorded in `docs/accuracy-ledger.md`.
- **MMC3 R1/R2 residual A12-phase instrumentation study (v2.1.5 F5.0, ADR
  0002).** A purely-observational, default-off probe feature
  (`mmc3-a12-phase-probe`, in `rustynes-mappers` + `rustynes-core` +
  `rustynes-test-harness`) plus a reproducible study fixture
  (`crates/rustynes-test-harness/tests/mmc3_r1r2_phase_probe.rs`) that answers,
  with *fresh direct instrumentation*, the one avenue ADR 0002's F5.0 closure
  left open: on the four `#[ignore]`'d MMC3 IRQ residuals, does any *qualifying*
  (`gap >= 3`) A12 rising edge that clocks the IRQ counter ever land in the
  post-access (M2-high, φ2) half of a host CPU cycle — the sub-cycle window an
  ares-style M2-half-cycle low-time filter would treat differently from the
  integer `gap >= 3` model? The feature seeds the real M2-phase into the mapper
  `sub_dot` on the live one-clock scheduler and *only counts* qualifying rises
  by half (no assertion deferral), so the emulated timeline is byte-identical to
  the default build; the tallies are surfaced via `MapperDebugInfo.extra`. The
  study **refines** the F5.0 finding: the two `scanline_timing` residuals
  (`mmc3_test_2/4` #3, `mmc3_test_v1/4` #3) have zero post-access IRQ-clocking
  rises — directly confirming Session B's (2026-07-02) indirect byte-identity
  result — but the two "reload/set-IRQ-every-clock" residuals (`mmc3_test_v1/5`
  #2, `mmc3_test_v1/6` #2), which Session B never tested, have **4** post-access
  IRQ-clocking rises each (and *every* qualifying rise post-access). So the
  "no post-access rise" premise is ROM-specific, not a structural NTSC-MMC3
  property. Separately, engaging the existing default-off `mmc3-m2-phase-irq`
  rising-edge deferral lever on `/5` and `/6` leaves their failure status
  byte-identical — it is non-curative. **No production, scheduler, or MMC3
  default behavior changed; AccuracyCoin stays 141/141** and all four residuals
  stay `#[ignore]`'d. The ares-style M2-edge low-time *filter* remains the one
  genuinely-untested axis-B lever; ADR 0002 records it as an axis-B candidate
  deferred to a maintainer decision (see the 2026-07-11 F5.0 decision update)
  and `docs/accuracy-ledger.md` is updated with the refined disposition.
- **PAL APU frame-counter step positions + screen-reading oracle (v2.1.5
  "Regression Net & Residual").** Modeled the PAL (2A07) APU frame-counter
  sequencer step positions and wired blargg's freely-redistributable
  **`pal_apu_tests`** corpus (10 sub-ROMs, PAL-calibrated) into CI as the first
  PAL-region APU oracle (`crates/rustynes-test-harness/tests/pal_apu_tests.rs`,
  gated on the default `--features test-roms`), forcing PAL region via a
  throwaway-header stamp. In wiring it, this **corrects a false oracle**: the
  prior revision drove these 2005-era ROMs through the `$6000` WRAM status
  runner and asserted `status == 0` — but they are plain NROM with *no
  PRG-RAM*, so `$6000` reads `0` forever and the check passed vacuously,
  claiming "all ten PASS" while validating nothing (the blargg `$DE $B0 $61`
  completion magic never even appears). The suite now reads the ROMs' real
  **on-screen** verdict (`APU <title>` then `PASSED` / `FAILED: #<n>`) decoded
  from the nametable by the new `run_nes_screen` harness runner, which
  early-returns the instant the verdict renders (5-18 frames) and treats a
  never-settling screen as a hard failure, never a pass.
  - **PAL frame counter (`crates/rustynes-apu/src/frame_counter.rs`).** The
    2A03 (NTSC) and 2A07 (PAL) share the same six-step sequencer but divide the
    CPU clock differently, so the identical quarter/half/IRQ events land at
    different CPU-cycle counts. `FrameCounter` now carries a `pal` selector,
    derived from the console `Region` by `Apu::new` (true only for
    `Region::Pal`; NTSC and Dendy keep the NTSC positions). PAL 4-step
    (mode 0) clocks at 8313 / 16627 / 24939 / 33252 / 33253 / 33254; PAL 5-step
    (mode 1) at 8313 / 16627 / 24939 / 41565 / 41566 (Mesen2 `stepCyclesPal`).
    The mode-0 terminal three cycles replicate the NTSC IRQ-flag-visibility /
    `irq_line_active` split verbatim at the PAL positions.
  - **Result: 10 of 10 pass** (was a vacuous 10/10, honestly 3/10 pre-model) —
    the three region-independent checks (`01.len_ctr`, `02.len_table`,
    `03.irq_flag`); the five PAL frame-counter-timing checks
    (`04.clock_jitter`, `05`/`06.len_timing_mode0`/`1`, `07.irq_flag_timing`,
    `08.irq_timing`) that flipped to PASS with the PAL step positions; and
    `10.len_halt_timing` / `11.len_reload_timing` closed by the length
    halt/reload ordering fix below.
  - **NTSC byte-identity preserved (sacred).** The step-position change is
    strictly region-gated: the NTSC/Dendy step tables are unchanged and the
    power-on / snapshot-restore default is NTSC (the `pal` selector is
    *derived*, not persisted — the APU snapshot format is untouched, and
    `Apu::restore` re-derives it from the restored region). The halt/reload
    ordering change is region-agnostic but byte-identical on NTSC by
    construction (see below). Verified byte-identical: AccuracyCoin 141/141
    (100.00%), `apu_test` 8/8, NTSC `blargg_apu_2005` 11/11, `f2_accuracy_audit`
    6/6, `apu_mixer` / `volume_tests` / `visual_regression` unchanged, `nestest`
    0-diff.
  - **Length halt/reload write-ordering fix (`crates/rustynes-apu/src/length.rs`).**
    Closes `10.len_halt_timing` (was `FAILED: #3`) and `11.len_reload_timing`
    (was `FAILED: #4`). The 2A03 applies a length-counter **halt** change and a
    length **reload** one step *behind* the frame sequencer's half-frame length
    clock: a halt write on the clock cycle governs the *next* clock (not this
    one), and a reload on the clock cycle is dropped if the counter was clocked
    from a non-zero value. `LengthCounter` now defers both — `set_halt` latches
    `new_halt`, `load` latches `reload_val` + a `previous_count` snapshot — and
    `LengthCounter::reload` (called on all four length channels once per CPU
    cycle in `Apu::tick_with_external`, **after** the half-frame clock and
    **before** the mixer sample) promotes the halt and applies the reload only
    when the post-clock count still equals the snapshot. Mirrors `TetaNES`
    `LengthCounter::reload` and Mesen2's `_newHaltValue` + reload-request.
    Because the reload settles in-cycle on the common non-coincident write and
    halt does not affect channel output directly, the change is byte-identical
    on NTSC — it alters only the exact write-on-the-clock-cycle coincidence the
    ROMs probe. The APU snapshot layout is unchanged (the deferral scratch
    fields are not serialized; `read_length` seeds `new_halt = halt`).
  - ROM provenance (blargg, public domain) is in `tests/roms/LICENSES.md`; docs
    updated in `docs/apu-2a03.md`, `docs/accuracy-ledger.md`, `docs/STATUS.md`,
    `docs/testing-strategy.md`.

### Changed

- **fat-LTO release profile — measured, documented, and validated (v2.1.5
  build-optimization pass).** `[profile.release]` already shipped
  `lto = "fat"` + `codegen-units = 1` (since the v1.0.0 engine transplant), but
  the choice had never been backed by an in-repo A/B and `docs/performance.md`
  even mis-stated the profile as `lto = "thin"` in two places. Ran the
  measure-first A/B (`lto = "fat"` vs `lto = "thin"`, everything else held; same
  host, back-to-back Criterion, `taskset`-pinned): fat is **+8.4%**
  (`nes_run_frame_nestest`) / **+20.8%** (`nes_run_frame_flowing_palette` and
  `ppu_tick_one_frame`) faster on every cross-crate path, and within noise
  (+0.3%) on the single-crate `cpu_throughput` control — the signature of a
  cross-crate-inlining win. Byte-identity was **verified, not assumed**: both
  profiles rebuilt in release mode pass the golden oracle byte-for-byte
  (AccuracyCoin 141/141, `nestest` golden-log 0-diff, `visual_regression`,
  `apu_mixer`/volume audio). **No default-build change** — this documents and
  retroactively justifies the existing fat-LTO default (well above the standing
  > 3% + byte-identical bar) and corrects the stale profile text. Also documents
  the opt-in `release-native` (`target-cpu=native`) and `x86-64-v3` host-tuned
  build variants, and refreshes the `pgo.yml` determinism-oracle comments from
  the stale `AccuracyCoin 139/139` to the shipped `141/141`. Detail:
  `docs/performance.md` § "fat-LTO vs thin-LTO release-profile A/B".

### Fixed

- **Netplay: the native TURN client now retransmits (RFC 5389 §7.2.1) — a real
  production bug where symmetric-NAT relay fallback aborted on a single dropped
  UDP datagram.** The native TURN client
  (`crates/rustynes-netplay/src/relay.rs`) sent each `Allocate` /
  `CreatePermission` request exactly once and waited for the reply; a single
  dropped UDP datagram (request *or* response) hard-failed the whole NAT
  traversal with `NatPhase::Failed("TURN allocate failed: …")` — so real
  symmetric-NAT netplay over any lossy internet path was equally fragile, not
  only the CI loopback test. On loopback this is rare but real — a loaded shared
  CI runner (observed on `windows-latest`) can drop a `127.0.0.1` datagram (a
  receive-buffer overflow, or — during the peer's socket-startup window — an ICMP
  "Port Unreachable" that a subsequent `recv_from` surfaces as a transient
  `ConnectionReset`), which intermittently red-lit
  `nat_connect_loopback_relay_then_session_digests_agree` on `main` and, in turn,
  blocked `release-auto`. The client now retransmits the request every 250 ms
  (`RTO`) until the caller's overall timeout, guided by RFC 5389 §7.2.1 (a fixed
  250 ms RTO here, not the RFC default 500 ms + exponential backoff), recovering
  transparently from a dropped datagram (STUN/TURN requests are idempotent, so a
  duplicated request is answered again and any late duplicate response is
  discarded by the transaction-id filter). This is a real robustness fix for
  production symmetric-NAT fallback over lossy paths, not just a test workaround;
  the session-digest-agreement assertion (the determinism contract) is unchanged.
  The receive loop treats a read-timeout expiry (`WouldBlock` on Unix, `TimedOut`
  on Windows) **and** a transient `ConnectionReset` / `ConnectionRefused` as
  "retransmit and retry" rather than a hard failure.

## [2.1.4] - 2026-07-11 - "Fathom" (accuracy hardening — opt-in OAM decay + BestEffort boot-smoke sweep + MMC3-clone A12/IRQ timing oracle; "Caliper")

### Added

- **Optional OAM decay (accuracy, default-OFF).** The 2C02's Object Attribute
  Memory is dynamic RAM: sprite evaluation implicitly refreshes it every rendered
  scanline, but with rendering disabled long enough the un-refreshed rows lose
  charge and decay to a fixed garbage pattern. RustyNES now models this exactly
  like Mesen2 (`ReadSpriteRam`/`WriteSpriteRam`, 3000-CPU-cycle refresh window per
  8-byte row): every OAM read (`$2004` **and** the sprite-evaluation reads) and
  write refreshes the row's timestamp, and a row un-touched past the window decays
  on the next read to `((sprAddr & 3) == 2) ? (sprAddr & 0xE3) : sprAddr`. It is
  **off by default** — with the default the framebuffer/audio/replay output and
  the AccuracyCoin / commercial / visual regression suites are **byte-identical**
  to a decay-free build. NTSC/Dendy only (PAL's refresh cadence masks decay).
  Deterministic when on (driven off the PPU's monotonic dot counter, never
  wall-clock/OS-RNG). Enable via **Settings → Emulation → "OAM decay (accuracy)"**,
  the `[emulation] oam_decay` config bool, or `Nes::set_oam_decay(true)`. The
  per-row decay state round-trips the save-state via an additive
  `PPU_SNAPSHOT_VERSION` v7 tail (stored as a relative age so a run-ahead / netplay
  `snapshot`→`restore` stays byte-identical); pre-v7 `.rns` blobs still load.

- **CI boot-smoke sweep of every `BestEffort` mapper family (Fathom F3.1).** A
  new test-harness suite
  (`crates/rustynes-test-harness/tests/v21_best_effort_sweep.rs`, `--features
  test-roms`) exercises the full parse → construct → dispatch → run-loop
  integration for **all 26** `BestEffort` (Tier-2) mapper families — the
  reference-ported long-tail boards that lack a cleanly-booting redistributable
  ROM dump and so can never be honestly oracle-gated. The target set is derived
  live from the `rustynes-mappers::mapper_tier` classifier (the single source of
  truth), so any future family promoted into or out of `BestEffort` is swept —
  or dropped — automatically with no edit to the test. Each family is built into
  a synthetic minimal iNES / NES 2.0 image (256 KiB PRG spin loop + CHR-RAM;
  NES 2.0 headers with the byte-8 mapper-MSB for the 17 high-id boards `> 255`)
  and run for ~60 headless, deterministic frames, asserting no panic, an exact
  mapper-id header round-trip, and a well-formed 256×240 RGBA framebuffer. Any
  panic in a `BestEffort` register decode, bank wiring, or per-tick hook is now
  caught in CI instead of only when a user loads a real cart. This is a **pure
  safety net**: it promotes nothing, adds no accuracy/oracle claim (accuracy
  stays defined by the Core/Curated gate), and leaves runtime behaviour and the
  deterministic `#![no_std]` core byte-identical. The two NTDEC boards 81 / 174
  correctly reject a CHR-RAM header with a typed `RomError` (not a panic) and are
  handed CHR-ROM geometry; no real panics were found in the sweep. See
  `docs/mappers.md` ("Mapper accuracy tiering") and `docs/adr/0011-mapper-tiering.md`.
- **Shared MMC3-clone A12/IRQ timing oracle (Fathom F3.3).** A new chip-level
  test suite (`crates/rustynes-test-harness/tests/mmc3_clone_a12.rs`,
  deterministic, headless, no ROM files — runs in the default `cargo test`)
  proves the reusable `Mmc3Clone` core reproduces MMC3's A12-clocked
  scanline-counter IRQ timing for all **eleven** `Mmc3CloneMapper` boards
  (mappers 44, 49, 52, 115, 134, 189, 205, 238, 245, 348, 366). Because every
  board routes its `$8000`-`$FFFF` register space — including the IRQ ports
  `$C000`/`$C001`/`$E000`/`$E001` — into the same shared counter, the scanline
  IRQ is board-independent by construction; the oracle exercises each board's
  own register decode to confirm the ports reach that counter. The centerpiece
  drives every clone board and a reference plain `Mmc3` (Sharp / rev A) through
  the identical canonical rendering-scanline A12 edge sequence and asserts the
  clone reproduces the reference's per-scanline IRQ-assert bitmap
  **bit-for-bit**: the IRQ first asserts on rising edge `latch + 1` (the initial
  `$C001` reload consumes edge 0, then `latch` decrements reach zero) and
  re-asserts every `latch + 1` scanlines once acknowledged. The suite also pins
  the `$E001`/`$E000` enable/acknowledge gate, the `$C001` reload periodicity,
  and the A12 rising-**edge filter** (holding A12 high across consecutive reads
  clocks the counter exactly once — no double-clock). The reference `Mmc3` *is*
  the oracle, so any clone whose shared core drifted from MMC3's scanline timing
  would fail. This is **additive test evidence** deepening the cluster's
  existing `Curated` classification — it promotes nothing, moves no tier, and
  leaves the deterministic `#![no_std]` core byte-identical (no mapper source
  changed: the clone core already matches MMC3 timing). See `docs/mappers.md`
  ("MMC3-clone A12/IRQ timing oracle").

## [2.1.3] - 2026-07-11 - "Fathom" (quality-of-life — APU filter-model audio fix + Game Genie code nomination/database + universal header-robust matching + MkDocs docs handbook; "Codex")

### Added

- **Game Genie matching is now header-insensitive for all ~520 games.** The bulk
  catalog is keyed by the full-file No-Intro CRC, which only matches a dump whose
  16-byte iNES header is byte-identical to No-Intro's — so a **re-headered** dump
  (common) missed. A new third catalog (`genie_database_headerless.tsv`, ~16.5k
  rows / 521 games) carries the same libretro codes **re-keyed to the
  header-excluded `rom_crc32`** (via the NES 2.0 database's content CRCs, joined
  by game name with a manual alias table for the long-tail titles), so a game now
  resolves from PRG + CHR content regardless of its header. Previously only 6
  curated classics had a header-excluded key. The re-key is regenerated by
  `scripts/gg/gen_headerless_genie_db.py` (the NES 2.0 DB is a build-time input,
  never committed). All three catalogs ship on every target including wasm
  (together ~370 KiB gzip, inside the 5 MiB budget). Frontend-only; the
  deterministic core is untouched.
- **APU audio filter-model selector** (fixes the "thin / missing bass channel"
  sound). RustyNES applies the authentic **NES front-loader** analog filter — a
  90 Hz + an aggressive **440 Hz high-pass** + a 14 kHz low-pass — which is
  byte-correct (identical to ares/tetanes; verified by the APU golden vectors)
  but rolls off the bass/triangle register hard, reading as a missing channel.
  Mesen2 / FCEUX / Nestopia omit that high-pass, which is why they sound fuller.
  You can now pick the model in **Settings → Audio → Filter model**
  (`[audio] filter_model`): **`nes`** (default, authentic — byte-identical to
  earlier builds), **`famicom`** (a single ~37 Hz high-pass — the nesdev Famicom
  spec, fuller low end), or **`clean`** (a ~10 Hz DC-block only — fullest, the
  Mesen2-like character). Core: `Apu::set_filter_model` / `Nes::set_apu_filter_model`.
  Tonal only — channel content, determinism, save-states, and the audio oracle are
  unchanged on the default. The DRC resampler + band-limited BLEP synthesis were
  audited and found correct (they match Mesen2's approach); no change needed there.
- **Game Genie per-game code nomination + a bulk code database**. The Cheats
  panel now suggests the known Game Genie codes for the loaded game — a
  category-grouped "Known codes" pick-list, each row feeding the same validated
  `GenieCode::new` + persistence path as a hand-typed code — instead of only
  decoding codes you enter (previously it showed "No Game Genie cheats. Enter a
  6- or 8-character code above." for essentially every commercial ROM). A new
  bulk catalog (`genie_database_full.tsv`, **~10,800 codes across ~520 USA/World
  games**) is ingested from the openly-licensed libretro-database Game Genie
  files and keyed to every known dump's CRC32 via the No-Intro NES DAT. To match
  whatever dump "flavor" a user has, a ROM is now recognized on **two** CRC32
  keys: the header-excluded `rom_crc32` (the curated starter catalog) and the
  full-file No-Intro `rom_crc32_full` (the bulk catalog), unioned + de-duplicated.
  Frontend-only (the deterministic core is untouched; codes re-validate at load).
  The bulk catalog ships on every target including the wasm browser demo — at
  ~777 KB raw it gzips to ~128 KiB, well inside the wasm bundle's 5 MiB budget —
  so the browser build carries the full game coverage too.
- **Material for MkDocs documentation site** at `/docs/` on GitHub Pages
  (<https://doublegate.github.io/RustyNES/docs/>). The existing Pages deployment
  now serves three sections from one artifact: the playable wasm demo at the
  site root (`/`), the workspace rustdoc at `/api/`, and this new
  Material-themed handbook at `/docs/`. The handbook renders the existing `docs/`
  subsystem specs and user guide directly (no duplicated content — `docs_dir`
  points at the source-of-truth tree) with a curated, grouped navigation
  (Overview, Emulation Core, Frontend & Features, Testing & Accuracy, Platforms,
  User Guide), a light/dark palette toggle, instant navigation, search, and
  copy-to-clipboard code blocks. Per-page **social preview cards** (the `social`
  plugin) render an Open Graph / Twitter image for each page so shared `/docs/`
  links unfurl richly, and the `privacy` plugin self-hosts the theme's web-fonts
  into the build for a network-free, GDPR-clean served site.
  `.github/workflows/web.yml` gains a Python + `mkdocs-material[imaging]` build
  step (with the Cairo/Pango system libraries the card renderer needs) that emits
  the handbook into `_site/docs/` alongside the demo and rustdoc copies, and now
  also triggers on `docs/**` / `mkdocs.yml` changes.

## [2.1.2] - 2026-07-11 - "Fathom" (display-fidelity — generated NTSC palette + composite-shader ladder + Vs. `DualSystem` second screen + NSF non-60 Hz/NSFe; "Prism")

### Added

- **Vs. `DualSystem` second-screen presentation** (Fathom F2.1, desktop). A loaded
  Vs. `DualSystem` cabinet (Balloon Fight, Wrecking Crew, Tennis, Baseball) now
  runs **both** cross-wired consoles and presents them together — side-by-side
  (512x240, default) or stacked (256x480), selectable via `[graphics]
  dual_screen_layout`. P1/P2 drive the main console, P3/P4 the sub; coin-insert
  (F10) and the main console's audio are wired. The core dual engine already
  existed (`VsDualSystem` / `Emu::Dual`); this adds the frontend path — an
  additive `EmuCore::dual` field, a `produce_dual_frame` step, a composed
  two-screen blit (`Gfx::render_dual`), and Vs.-DB DIP/RGB-palette applied to both
  consoles — so the single-console path stays byte-identical. The advanced
  single-`Nes` features (run-ahead, rewind, netplay, TAS, dual save-state) are
  **scoped out in dual mode** (ADR 0032); the debugger/HD are unavailable there.
  Real-cabinet boot remains fixture-limited (the circulating dumps are the MAME
  maincpu half only). Desktop only for now; wasm/mobile deferred.
- **NTSC composite-shader ladder completed** (Fathom F2.2). The three-rung
  display-only ladder — simplified blur (`Ntsc`) → LMP88959 composite
  (`Lmp88959`) → Bisqwit per-dot (`CompositeRt`) — is verified end-to-end, and
  **live emulator-synced dot-crawl is now wired to LMP88959** as well as Bisqwit:
  the NES 3-frame colour phase (`ntsc_phase()`) advances the LMP base subcarrier
  phase (`video_phase / 3` turn) on top of the user's static offset. The live
  phase is decoupled from the (heavier) palette-index snapshot, so an LMP-only
  stack gets crawl without the index upload. All passes stay display-only —
  `visual_regression` is byte-identical with any filter active. Documented the
  legacy-vs-stack precedence and the palette↔pass split (the generated/custom
  palette feeds the RGBA passes but not the index-based Bisqwit pass); no
  separable-kernel rung is added (LMP covers that tier). See `docs/frontend.md`.
- **Generated NTSC palette** (Fathom F1.4). A new in-core synthesizer
  (`rustynes_ppu::generate_base_palette`) produces the 64-entry base palette from
  a model of the 2C02's composite-video output (the Bisqwit / ares YIQ
  integration: two-level chroma square wave over 12 subcarrier phases →
  demodulate → FCC YIQ→RGB with gamma), tunable via saturation / hue / contrast /
  brightness / gamma. Every transcendental routes through `libm`, so the output is
  **byte-identical across all targets** (x86 / aarch64 / wasm / `thumbv7em`) and
  locked by a committed golden. It feeds the existing `set_custom_palette` /
  emphasis-LUT path (no new emphasis model) and is **off by default** — the
  shipped build keeps the hand-authored palette and is byte-identical; enable and
  tune it under Settings → Palette → "Generated NTSC". Presentation-only; the
  deterministic core and AccuracyCoin (141/141) are unaffected.
- **NSF non-60 Hz playback + NSFe support** (Fathom F4.1/F4.2). The NSF player now
  parses the header **play-speed divider** (`$6E-$6F` NTSC / `$78-$79` PAL, µs per
  `play`) and drives non-standard rates correctly: a PAL 50 Hz tune — or any custom
  divider — on the NTSC console runs `play` from a mapper **cycle-timer IRQ** (the
  driver disables the APU frame-counter IRQ once in `init`, then arms a
  level-triggered, `$5FF1`-acked timer that fires every `period` CPU cycles). The
  standard 60 Hz path is unchanged and **byte-identical** (vblank-NMI). The extended
  chunked **`NSFE`** container is now parsed as well (INFO / DATA / BANK / auth
  chunks; rate derived from the region flag), routed through the same
  `Nes::from_nsf` path and frontend file detection. Covered by new `nsf` unit tests
  plus a core integration test asserting the timer IRQ drives `play` at a sub-60 Hz
  rate. Determinism / AccuracyCoin unaffected (NSF is not on the oracle path).

## [2.1.1] - 2026-07-10 - "Fathom" (patch — Wizards & Warriors freeze fixed at the root: game-DB mirroring override + a run-ahead PPU-snapshot gap)

### Fixed

- **Wizards & Warriors (and ~1900 other games) no longer freeze at level load —
  the actual root cause.** The per-game database (`game_database.txt`, vendored
  from TetaNES) force-applied its `mirroring` column to *every* matched ROM,
  including mappers that control their own nametable mirroring at runtime.
  Wizards & Warriors is AxROM (mapper 7), which flips single-screen A↔B mid-frame
  to draw its status bar; the DB's spurious `Horizontal` pinned the mirroring,
  blanked the bottom half of the screen, killed the sprite-0 split, and hung the
  game (on desktop **and** WASM; a headless core, which never consults the DB, was
  always unaffected). The game-database mirroring override is now honored **only**
  for hardwired-mirroring boards (NROM/UxROM/CNROM/GxROM) via the new
  `Mapper::has_hardwired_mirroring()` capability (default `false` — the safe
  direction, so a mapper that controls its own mirroring can never be corrupted),
  gated in `App::apply_game_db` and the per-game overlay through
  `Nes::mapper_has_hardwired_mirroring()`. This protects **1914** mapper-controlled
  database rows from the same class of corruption. Regression-tested
  (`hardwired_mirroring_gate_matches_board_type`) and verified **byte-identical**
  to a clean headless replay through the real game-DB path. See ADR 0031.
- **Run-ahead PPU save-state gap hardened** (`PPU_SNAPSHOT_VERSION` 5 → 6). Run-ahead's
  per-frame `snapshot`/`restore` round-trip did not serialize some PPU render
  state — the per-sprite shifter-halt state (`spr_halted`), the 1-dot-delayed
  rendering gate (`prev_rendering_enabled` / `rendering_enabled_delayed`), and the
  OAM-row-corruption arming state — so a snapshot/restore could drift them. This is
  a genuine save-state-completeness fix that also hardens netplay rollback and
  manual save/load. **Note:** this was originally believed to be the Wizards &
  Warriors freeze cause; deeper full-core-state diffing later proved run-ahead was
  byte-identical and the freeze was the game-DB mirroring override above — this
  change remains a valid correctness improvement on its own. The additive v6 tail
  keeps pre-v6 `.rns` states loadable (upconverting to power-on defaults) — not an
  ADR-0028 epoch break.
- Regression tests: `hardwired_mirroring_gate_matches_board_type` (mirroring gate)
  and the GitHub-safe `ww_runahead_matches_plain_across_a_mid_frame_split` (skips
  cleanly when the commercial dump is absent). The core / accuracy path is
  unchanged — AccuracyCoin stays **141/141**, no oracle moves, determinism holds.
- Version: workspace `2.1.0 → 2.1.1`.

## [2.1.0] - 2026-07-09 - "Fathom" (accuracy remediation — PPU display quirks, mapper completion, MMC3 residual closed)

- The **accuracy-remediation** release — a core/desktop cut that lands **ahead of**
  the joint mobile store launch (which moved from v2.1.0 to **v2.2.0**, so the
  Android + iOS apps ship on this improved core). AccuracyCoin stays **141/141**,
  nestest 0-diff, the `#![no_std]` chip stack untouched; the deterministic core is
  unchanged except the display-only PPU fix below. No save-state/format bump.
- **PPU palette backdrop-override (F1.1).** When rendering is disabled and the VRAM
  address `v` points into palette space (`$3F00-$3FFF`), the PPU now outputs the
  color at `v & 0x1F` instead of the universal backdrop — the documented 2C02
  display behavior, **byte-exact with TriCNES** (`Emulator.cs`). This makes the
  `full_palette` / `flowing_palette` demos render correctly (all 64 colors) and is
  a display-only change (palette RAM is never mutated). Nine snapshots re-blessed —
  the 2 palette demos + 7 commercial games (Micro Machines-style palette tricks) —
  all converging RustyNES **with** its TriCNES oracle; `external_real_games` 60/60
  stays byte-identical.
- **PPU OAM + open-bus audits (F1.2 / F1.3).** The OAMADDR-forced-to-0 (dots
  257-320), `$2004` `$E3` attribute mask, and open-bus refresh map were audited
  against the Blargg `ppu_open_bus` table + AccuracyCoin and found already correct;
  each is now locked by a fast unit regression test. The `OAMADDR & 0xF8`
  render-start copy stays unmodeled by design — Mesen2, ares, and TriCNES all omit
  this revision-dependent corner.
- **Mapper completion (F3): 86 families promoted BestEffort → Curated** with a
  commercial-ROM boot-snapshot oracle (57 already-staged + 29 sourced from GoodNES
  v3.23b). The tier split is now **51 Core + 95 Curated + 26 BestEffort = 172**,
  taking oracle-gated coverage from **60 → 146** of 172 families. The 26 still
  BestEffort have no cleanly-booting dump (16 NES 2.0 high-id boards + 8 with no
  matching cart + 2 whose only dump jams at boot) and stay register-decode +
  save-state unit-tested only.
- **MMC3 R1/R2 scanline-IRQ residual CLOSED (ADR 0002 F5.0).** The instrumentation-
  first review confirmed the residual is a differential 1-dot deficit that is
  structurally unreachable on the one-clock batched-catch-up model (21+ falsified
  levers; zero production-ROM impact), so it is now closed by-design-permanent, not
  deferred. All **20** `#[ignore]`'d tests are catalogued with dispositions in the
  new `docs/accuracy-ledger.md` — none is an accuracy gap.
- **Doc reconciliation (F0).** `docs/mappers.md` + `docs/compatibility.md` corrected
  (MMC5 vertical split-screen + audio and the Vs. `DualSystem` core are implemented,
  not deferred); new `docs/accuracy-ledger.md` maps every approximation to its
  disposition (remediated / no-stricter-oracle / deferred / out-of-scope).
- Version bump: workspace `2.0.8 → 2.1.0`. Mobile `MARKETING_VERSION`s are unchanged
  (the apps re-release at v2.2.0).

## [2.0.8] - 2026-07-09 - "Harbor" (iOS release candidate — "Harborlight")

- The **iOS release candidate** and the final release of the iOS finalization window
  (v2.0.5–v2.0.8), on the byte-identical v2.0.0 "Timebase" core: **AccuracyCoin
  141/141**, nestest 0-diff, the `#![no_std]` chip stack untouched. Host / iOS-only.
- **App Store Connect listing metadata staged** (files only, no upload):
  `fastlane/metadata/ios/{en-US,es-ES}/` — name, subtitle, promotional text,
  keywords, description, release notes, support / marketing URLs, plus a copyright
  line — mirroring the Android `fastlane/metadata/android/` tree, namespaced under
  `ios/` so `deliver` (iOS) and `supply` (Android) never collide.
- **Dormant App Store `release` lane** added to `fastlane/Fastfile`: it stages the
  build + listing and **does not submit** (`submit_for_review: false`,
  `automatic_release: false`). It is **not** wired into CI — the interim iOS channel
  stays **TestFlight** (the `beta` lane) until the v2.1.0 joint launch, when a
  maintainer runs it with signing provisioned.
- **App-Review §4.7 self-audit** recorded (no bundled / downloadable ROMs, no in-app
  ROM links, no Nintendo branding, in-app ownership notice, searchable library,
  4+ age rating) in `docs/ios-v2.0.8-readiness.md`.
- **Release-automation fix:** the `release-auto` workflow's global `concurrency`
  group let GitHub cancel an older *pending* release run when a newer one queued
  behind the (slow) binary build — which silently skipped a middle version during a
  rapid train (v2.0.6 was dropped between v2.0.5 and v2.0.7; both have since been
  published manually). The group is now keyed per-commit, so distinct versions
  release independently and none is ever superseded.
- Version bump: workspace `2.0.7 → 2.0.8`; iOS `MARKETING_VERSION → 2.0.8`.
- Still **TestFlight-only**; the App Store + AltStore PAL launch is the future
  **v2.1.0**. Screenshots, real signing, the listing upload, and the App-Review
  submission are the maintainer / v2.0.9 / v2.1.0 closeout.

## [2.0.7] - 2026-07-09 - "Harbor" (iOS polish + App Store submission floor — "Trim")

- The third iOS finalization release (the v2.0.5–v2.0.8 window), on the
  byte-identical v2.0.0 "Timebase" core: **AccuracyCoin 141/141**, nestest 0-diff,
  the `#![no_std]` chip stack untouched. Host / iOS-only.
- **App Store submission floor wired.** Apple mandates the **iOS 26 SDK / Xcode 26**
  for every App Store Connect upload from **2026-04-28**; the tag-gated iOS CI now
  selects the newest Xcode 26.x on the runner (falling back with a warning on older
  images, so the xcframework build still runs). This pins the **build SDK**, separate
  from the minimum OS.
- **Deployment target reconciled `iOS 15.0 → 17.0`.** The SwiftUI shell already uses
  `NavigationStack` (iOS 16) and `.topBarTrailing` (iOS 17, unguarded, 12+ sites), so
  the prior 15.0 declaration was never actually buildable; 17.0 matches the real API
  floor. (Product note: this is the minimum OS; guard those APIs to target lower.)
- **Privacy manifest re-audited** against the v2.0.6 crash reporter: it collects no
  new data type and adds no new required-reason API (UserDefaults is already
  declared; local-only, backup-excluded, off by default), so `PrivacyInfo.xcprivacy`
  needs no change — documented in-manifest.
- Performance / energy review notes (Metal / ProMotion, app thinning) captured for
  the on-device pass. Version bump: workspace `2.0.6 → 2.0.7`; iOS
  `MARKETING_VERSION → 2.0.7`.
- TestFlight-only; App Store + AltStore PAL deferred to v2.1.0. On-device profiling +
  the Xcode-26 archive are flagged for the v2.0.9 readiness pass.

## [2.0.6] - 2026-07-09 - "Harbor" (iOS feature parity — "Parity")

- The second iOS finalization release (the v2.0.5–v2.0.8 window), on the
  byte-identical v2.0.0 "Timebase" core: **AccuracyCoin 141/141**, nestest 0-diff,
  the `#![no_std]` chip stack untouched. Host / iOS-only — no accuracy / save-state /
  determinism number moves.
- **New opt-in crash-reporting surface** (privacy-first, **off by default**) — the
  iOS analogue of the Android v1.8.8 `CrashReporter`, closing the v1.9.9 readiness
  gap. Enabled from **Settings → Diagnostics**, an uncaught-`NSException` handler
  writes **local** crash logs (viewable + copyable in-app; **nothing is uploaded**,
  so the "Data Not Collected" privacy label is unchanged). The handler re-checks the
  live opt-in at crash time, so opting out stops new logs immediately. EN + ES.
- **Feature-parity re-verification** of the v1.9.x host features against the v2.0.0
  bridge (Game Center, CloudKit save sync, MFi controllers, capture / PiP,
  accessibility) — all route through the unchanged bridge surface; recorded in
  `docs/ios-v2.0.6-readiness.md`.
- Version bump: workspace `2.0.5 → 2.0.6`; iOS `MARKETING_VERSION → 2.0.6`.
- TestFlight-only; the App Store + AltStore PAL launch stays deferred to v2.1.0.
  On-device crash-capture verification is flagged for the v2.0.9 readiness pass.

## [2.0.5] - 2026-07-09 - "Harbor" (iOS re-port onto Timebase — "Landfall")

- Opens the iOS finalization window (v2.0.5–v2.0.8) of the v2.0.x "Harbor" train:
  the iOS/iPadOS app is re-ported onto the v2.0.0 "Timebase" core — the iOS
  analogue of the Android v2.0.1 re-port. Host/iOS-only; the emulation core is
  unchanged and byte-identical to v2.0.4 (AccuracyCoin 141/141, nestest 0-diff).
- The iOS host now localizes bridge warnings (device-locale strings, EN + ES) for
  the pre-Timebase movie notice: loading a pre-v2.0.0 `.rnm` still replays its
  input, but surfaces a non-blocking notice that byte-exact framebuffer/audio
  reproduction is not guaranteed across the ADR-0028 timebase change — the iOS
  analogue of the Android v2.0.4 warning, verbatim wording and shared ES copy.
- The UniFFI-Swift binding surface is re-confirmed against the v2.0.0 bridge
  (`drainWarningCodes` / `HostWarning.preTimebaseMovie`); the iOS
  `MARKETING_VERSION` is realigned from the frozen v1.9.x default to `2.0.5`.
- TestFlight-only; the App Store + AltStore PAL launch stays deferred to the
  v2.1.0 joint milestone. On-device re-port verification (save-state migration +
  the AccuracyCoin / SMB / Zelda determinism smoke on Apple silicon) is flagged
  for the v2.0.9 dual-app readiness pass.

## [2.0.4] - 2026-07-08 - "Harbor" (Android release candidate — "Slipway")

- Android release-candidate milestone; the emulation core is unchanged and
  byte-identical to v2.0.3 (AccuracyCoin 141/141, nestest 0-diff) — a
  host/Android-only cut.
- The Android host now localizes bridge warnings (device-locale strings, EN + ES)
  for the pre-Timebase movie notice, completing the v2.0.2–v2.0.4 carryover.
- Version-controlled Fastlane / Play Console listing metadata (EN-US, ES-ES)
  staged for a maintainer upload; release signing wired with a graceful
  debug-signing fallback; debug-only StrictMode diagnostics.
- No store submission yet (that is the future v2.1.0 joint launch); the `foss`
  flavor stays behaviour-identical.

## [2.0.3] - 2026-07-08 - "Harbor" (2-cycle-ALE promoted to default — shipped AccuracyCoin 141/141 — "Keel")

- The 2-cycle-ALE octal-latch PPU fetch model is promoted to the shipped default
  (ADR 0030) — **shipped AccuracyCoin is now 141/141 (100%)**; both the "ALE +
  Read" and "Hybrid Addresses" PPU tests now pass on the default build.
- Two commercial titles render more TriCNES-faithfully at a mid-render `$2006`
  scroll write — Super Mario Bros. 3 and Uchuu Keibitai SDF.
- The Android `play` flavor gains its full (still-dormant) monetization surface
  (AppLovin MAX + RevenueCat); the `foss` flavor keeps a no-op twin.
- Netplay rollback-determinism fix (new PPU snapshot v5 tail); headless frame
  cost rises ~10% (still ~4x realtime), accepted for the accuracy gain.

## [2.0.2] - 2026-07-08 - "Harbor" (octal-latch PPU model — AccuracyCoin 141/141 flag-on — "Soundings")

- A new octal-latch multiplexed-bus PPU model (ADR 0030) ships **default-off**:
  flag-on it reaches AccuracyCoin 141/141, while the shipped default stays
  byte-identical to v2.0.1 at its honest 139/141.
- The model faithfully reproduces the NES PPU's pin-multiplexed VRAM bus
  (74LS373-class octal latch), modeling the two corruption events behind the
  "ALE + Read" and "Hybrid Addresses" tests.
- The correct oracle was identified as TriCNES (the AccuracyCoin author's own
  emulator), not Mesen2; promotion to the default is the deliberate v2.0.3 step.

## [2.0.1] - 2026-07-08 - "Harbor" (first Android re-port onto Timebase + AccuracyCoin re-sync + housekeeping — "Mooring")

- First release of the v2.0.x "Harbor" mobile-finalization train: the Android app
  is re-ported onto the v2.0.0 "Timebase" core.
- The AccuracyCoin oracle is re-synced to upstream (146 rows / 141 assigned
  tests); measured honestly at 139/141 — the two new PPU tests are known,
  documented gaps.
- Structural `foss` / `play` Android flavor split scaffolding (ADR 0025): a
  default `foss` flavor with no Google SDKs, no ads, no tracking.
- CI cost optimization (the heavy suite gated to release branches); uniffi
  0.31→0.32 and mlua 0.11→0.12 dependency bumps.

## [2.0.0] - 2026-07-03 - "Timebase" (one-clock master-clock rewrite + Vs. DualSystem)

- The scheduler substrate is rewritten from a five-counter, dot-lockstep model to
  a single canonical cycle counter with every-cycle bus access and a
  split-around-the-access PPU catch-up (ADR 0002 / ADR 0029), now the only path.
- RustyNES's designated breaking release (ADR 0003): the save-state (`.rns`) and
  TAS movie (`.rnm`) format epochs bump (ADR 0028) — a pre-v2.0.0 `.rns` slot now
  fails to load with a clear error instead of silently misreading stale data.
- New core-level Vs. `DualSystem` dual-console support (`Emu::Dual`) for the four
  Vs. arcade cabinet boards — core-and-test-harness-only in this release
  (frontend wiring deferred).
- AccuracyCoin holds 100% (139/139) across all five betas + rc.1; the R1/R2 MMC3
  IRQ-timing residual is by-design-deferred beyond this release with a
  mechanism-level finding recorded in ADR 0002.

## [1.10.0] - 2026-07-01 - "Arcade" (Libretro core + dependency refresh)

- A new native Libretro core (`rustynes-libretro`) integrates RustyNES into
  RetroArch — RetroAchievements, dynamic audio sync, and deterministic
  save-state / rollback.
- The egui GUI stack moves 0.34.3 → 0.35.0 plus an in-constraint transitive
  dependency refresh; the core stays byte-identical and AccuracyCoin holds
  139/139.
- The iOS release workflow no longer fails on every tag push when the signing
  secrets are absent.

## [1.9.9] - 2026-06-26 - "Workshop" (iOS creator / power tools + readiness gate)

- The final iOS TestFlight release before the v2.0.0 core rewrite — it brings the
  desktop creator / power tools to touch and runs a full pre-freeze readiness pass.
- Cheats (a Game Genie editor + raw-RAM poke), a read-only debugger inspector, a
  touch TAStudio piano-roll, foreign movie import (`.fm2` / `.bk2` / …), a
  host-side audio-depth DSP, and symbol-map loading.
- First iOS release to extend the shared bridge (additive forwarding only); the
  core stays byte-identical and AccuracyCoin holds 139/139.

## [1.9.8] - 2026-06-26 - "Horizon" (iOS store-readiness)

- iOS store-readiness: accessibility (VoiceOver, Dynamic Type, high-contrast /
  colorblind palettes), EN / ES i18n, ReplayKit capture, Game Center, and a
  privacy-manifest pass.
- A dormant StoreKit 2 scaffold + `foss` / App-Store seam (activation deferred to
  v2.1.0).
- SwiftUI-shell only; the core stays byte-identical and AccuracyCoin holds
  139/139.

## [1.9.7] - 2026-06-25 - "Relay" (iOS connectivity completion)

- iOS connectivity completion: room-code (CGNAT / TURN) netplay, robust
  GameController hot-plug, and iCloud save-state sync (CloudKit).
- SwiftUI-shell only; the core stays byte-identical and AccuracyCoin holds
  139/139.

## [1.9.6] - 2026-06-25 - "Link" (iOS connectivity & scripting)

- Surfaces the shared bridge's Lua scripting, RetroAchievements, and direct-IP /
  LAN netplay in the iOS SwiftUI shell.
- SwiftUI-shell only; the core stays byte-identical and AccuracyCoin holds
  139/139.

## [1.9.5] - 2026-06-25 - "Curator" (iOS power-user feature port)

- iOS power-user features: TAS `.rnm` movies, custom `.pal` palettes, `.zip`
  ROMs, a per-game overrides DB, HD-pack loading, and iCloud config sync.
- The core stays byte-identical and AccuracyCoin holds 139/139.

## [1.9.4] - 2026-06-25 - "Lens" (iOS Metal renderer + shader stack)

- Completes the iOS wgpu → Metal render path: the full shared shader stack
  (None / Scanlines / CRT / NTSC / Bisqwit) with per-filter controls.
- ProMotion 60–120 Hz pacing, surface-loss / background lifecycle handling, and a
  verified CoreAudio hot path.
- The core stays byte-identical and AccuracyCoin holds 139/139.

## [1.9.3] - 2026-06-25 - "Workshop-lite" (iOS settings, save-state slots, onboarding)

- iOS settings / persistence / onboarding: a sectioned Settings form, four
  save-state slots per ROM, an in-game pill menu, first-run onboarding + About,
  and iPad multitasking polish.
- The core stays byte-identical and AccuracyCoin holds 139/139.

## [1.9.2] - 2026-06-25 - "Input" (iOS multi-touch, controllers, haptics)

- iOS input: a true multi-touch on-screen NES pad (Android-parity render),
  responsive iPhone / iPad sizing, GameController P1–P4 with remapping, and
  optional Core Haptics.
- The core stays byte-identical and AccuracyCoin holds 139/139.

## [1.9.1] - 2026-06-25 - "Patch" (iOS TestFlight cadence + dormant freemium gate)

- An iOS TestFlight build-refresh cadence (a bi-monthly cron to keep external
  testers live) and a dormant freemium-gate scaffold (fully unlocked through the
  entire v1.9.x train).
- The core stays byte-identical and AccuracyCoin holds 139/139.

## [1.9.0] - 2026-06-25 - "Sunrise" (iOS / iPadOS foundation)

- The first iOS / iPadOS release: a native SwiftUI shell over the byte-identical
  Rust core via the shared `rustynes-mobile` UniFFI bridge.
- New `rustynes-ios` shim (Metal rendering + CoreAudio), the SwiftUI app, ROM
  import, save-states / rewind / run-ahead / TAS-playback, and build / ship
  tooling (xcframework + fastlane + CI); ADRs 0026 / 0027.
- Distributed as interim TestFlight (App Store deferred to v2.1.0); the core stays
  byte-identical and AccuracyCoin holds 139/139.

## [1.8.9] - 2026-06-25 - "Backlog" (creator tooling, debugger depth, full HD-pack parity, mappers 168→172)

- Mapper breadth grows 168 → 172 families (NTDEC / TXC / discrete-BMC multicarts)
  plus ~35 more UNIF board aliases.
- Full Mesen2 HD-pack parity (the Zelda texture-mapping bug fixed; every Mesen2
  HD-pack form now implemented).
- New creator tools: a Game Genie database, a BasicBot save-state input search,
  detachable panel windows, TAS re-record counts, A/V codec depth
  (H.264 / H.265 / VP9), a desktop on-screen controls overlay, and an FDS firmware
  manager.
- A dormant mobile monetization core (`rustynes-monetization`) is added and the
  `foss` / `play` flavor split decided (ADR 0025); the core stays byte-identical
  and AccuracyCoin holds 139/139.

## [1.8.8] - 2026-06-20 - "Atlas" (Google Play launch readiness)

- Android Google-Play launch readiness: the toolchain is modernized to the
  Android 16 (API 36) target mandate (AGP 9, Gradle 9, compileSdk 37).
- Adaptive / foldable / TV layouts, a modern-UX pass (edge-to-edge, predictive
  back, splash), Material You dynamic color, and EN / ES i18n.
- A box-art ROM library with scrapers + secure secret storage, a
  performance / startup / app-size pass, and capture / share + platform surfaces
  (screenshots, MP4 clips, PiP, a Quick-Settings tile, a home-screen widget).
- Play Games cloud saves, achievements / leaderboards, and Play Integrity — all
  default-off; the core stays byte-identical and AccuracyCoin holds 139/139.

## [1.8.7] - 2026-06-20 - "Android" (Connectivity completion)

- CGNAT / TURN room-code netplay so phones on cellular (symmetric-NAT) networks
  can play.
- A robust hardware-controller input pipeline (wired USB + Bluetooth, analog
  sticks / HAT, per-port P1–P4, remapping, turbo), a controller-aware UI, and
  Chromecast prep (default-off).
- Sideload-only build; the core stays byte-identical and AccuracyCoin holds
  139/139.

## [1.8.6] - 2026-06-20 - "Android" (Connectivity & scripting)

- Lua scripting, RetroAchievements, and direct-IP / LAN netplay on Android — each
  reusing the desktop engine over the shared bridge (now connectivity-complete,
  so iOS inherits all three).
- An Open / Close ROM toggle plus a Windows CI line-ending fix; the core stays
  byte-identical and AccuracyCoin holds 139/139.

## [1.8.5] - 2026-06-20 - "Android" (Power-user features)

- Custom `.pal` palettes, compressed `.zip` ROMs, the Bisqwit composite NTSC GPU
  filter, TAS `.rnm` movies, a per-game settings DB, and HD-packs on Android.
- The HD-pack subsystem is extracted to the shared `rustynes-hdpack` crate; the
  core stays byte-identical and AccuracyCoin holds 139/139.

## [1.8.4] - 2026-06-20 - "Android" (Native wgpu renderer & shaders)

- The NES picture now draws through wgpu on a `SurfaceView` (Vulkan / GLES)
  instead of a Compose `Bitmap` blit, opt-in behind a setting.
- A shared WGSL shader stack (the new `rustynes-gfx-shaders` crate):
  None / Scanlines / CRT / NTSC with per-filter tuning sliders, plus a cheaper
  native-audio hot path.
- The core stays byte-identical and AccuracyCoin holds 139/139.

## [1.8.3] - 2026-06-20 - "Android" (Controller, casting & polish)

- An authentic NES-004 on-screen controller, cast-gameplay-to-a-TV via the
  Presentation API, per-screen-mode controller size / opacity, a controller size
  slider, and graded haptics.
- First-run onboarding, an About dialog, a Clear Recent action, a Material-3
  Settings sheet, and a four-slot save-state manager.

## [1.8.2] - 2026-06-20 - "Android" (Input & the virtual controller)

- A multi-touch virtual NES controller (simultaneous presses, D-pad diagonals,
  slide-between-buttons) whose art and touch regions resize / remap in lockstep.
- The real RustyNES adaptive app icon plus an icon wordmark refresh, and a
  `PLAY_BUILD` flag so sideload / dev builds stay full-featured.

## [1.8.1] - 2026-06-19 - "Android" (Patch)

- The free-tier demo session is shortened from 10 minutes to 8 minutes.
- Confirmed the debug "Full Unlock" override is absent from the Play (release)
  build (R8 strips the dead branches).

## [1.8.0] - 2026-06-19 - "Android" (Platform Release)

- The first platform (not accuracy) release: a complete, shippable Android app,
  verified on a Samsung Galaxy Z Fold 7.
- A new shared `rustynes-mobile` UniFFI bridge + a `rustynes-android` platform
  crate + a Jetpack Compose app + an Android CI gate (ADR 0024).
- Full on-device emulation: audio, input, save-states / SRAM, a recent-ROMs
  library, video filters (AGSL CRT / scanlines), and a foldable-aware UI.
- Freemium: a free download with a one-time $2.99 "Full Unlock" (a 10-minute
  demo); the emulated output is byte-identical between demo and paid, and the
  pure-Rust core is byte-identical on ARM (AccuracyCoin 139/139).

## [1.7.1] - 2026-06-19

- Fixed a ROM-close GPU abort in release builds and cleaned up pause / unpause
  pacing + audio underruns.
- A Help → Documentation pane overhaul (word-wrap at any scale, a collapsible
  sidebar tree); HD-pack tile substitution now applies in the debugger / tool
  render branch.
- An exhaustive README rewrite for v1.7.0 "Forge".

## [1.7.0] - 2026-06-19 - "Forge" (Feature Release)

- The maximal desktop feature release: an i18n framework (a compile-time string
  catalog + a Settings language picker, ADR 0023) shipping English + Spanish.
- Web / wasm parity: browser Lua, the File System Access API, the Gamepad API,
  PWA / offline, and `?settings=` share-links.
- Audio depth (stereo panning, reverb / crossfeed, an output device picker, a
  20-band EQ, per-context volume), per-game `<rom>.json` config overrides + a DIP
  editor + a lag-frame counter, and browser RetroAchievements completion.
- A new `full` maximal-native-feature build + a `cargo full-run` alias; the core
  stays byte-identical and AccuracyCoin holds 139/139.

## [1.6.0] - 2026-06-18 - "Studio" (Feature Release)

- A shader / filter ecosystem: LMP88959 NTSC / PAL, hqNx / xBRZ upscalers, and a
  constrained RetroArch `.slangp` / `.cgp` preset importer.
- HD-pack HD audio (`<bgm>` / `<sfx>` OGG tracks via the `$4100` register), a
  TAStudio piano-roll, `.fm2` / `.bk2` movies, and a Mesen2-style debugger.
- Mapper breadth grows to ~150 families + UNIF, proper FDS, A/V recording, and
  shaders; the core stays byte-identical and AccuracyCoin holds 139/139.

## [1.5.0] - 2026-06-17 - "Lens" (Feature Release)

- Debugger visualization devtools: an Input Miniatures overlay, a graphical PPU
  event viewer, a PPU scanline-trace viewer + CHR → PNG export, and an HD-pack
  per-pixel inspector.
- Lua API growth, TASVideos-format work, an accessibility pass, and mapper
  breadth 113 → 123 families.
- Browser RetroAchievements scaffolding (ADR 0015); the core stays byte-identical
  and AccuracyCoin holds 139/139.

## [1.4.1] - 2026-06-16

- Four more BestEffort mapper boot / decode fixes (mappers 92, 94, 145, 147)
  surfaced by the boot-smoke-against-real-dumps pass.
- The boot-smoke screenshot corpus is reorganized to mirror the per-mapper tier
  layout; the core stays byte-identical and AccuracyCoin holds 139/139.

## [1.4.0] - 2026-06-16

- "Fidelity" — the compatibility-and-finish release: accuracy polish, a
  per-channel audio mixing UI, and a devtools finish (symbol loading + event
  breakpoints).
- Browser QoL (wasm `.rnm` movies + IndexedDB save-states), a measure-first
  performance pass, and a colorful `rustynes help` TUI + styled `--help`.
- Mapper coverage 101 → 113 families (boot-smoke verified); the core stays
  byte-identical and AccuracyCoin holds 139/139.

## [1.3.0] - 2026-06-16 - "Bedrock" (Feature Release)

- Toolchain modernization: Rust edition 2024, MSRV → 1.96, and the coordinated
  egui 0.34.3 / wgpu 29.0.3 / rfd 0.17.2 / naga 25 dependency tier.
- A frame-pacing fix, a Memory Compare (cheat-hunt) panel, a reorganized menu bar,
  and auto-save-on-change Settings.
- Mapper breadth → 101 families plus Vs. DualSystem header detection, and HD-pack
  `<condition>` gating + `<background>` regions; the core stays byte-identical and
  AccuracyCoin holds 139/139.

## [1.2.0] - 2026-06-15 - "Curator" (Feature Release)

- Library breadth + compatibility + reach: mapper coverage grows 51 → 87 families
  behind a CI-enforced accuracy-tiering honesty gate.
- `.zip` ROM loading + automatic `.ips` / `.ups` / `.bps` soft-patching, a
  per-game database + in-app ROM-Database editor, live NTSC knobs, a composable
  shader stack, and a (default-off) HD-pack loader.
- New peripherals (Family BASIC keyboard, SNES mouse, Arkanoid, a Game Genie DB),
  Lua `onNmi` / `onIrq` / `setInput`, and web touch controls; the SMB3 World 1-1
  flicker is fixed. The core stays byte-identical and AccuracyCoin holds 139/139.

## [1.1.0] - 2026-06-15 - "Scriptable" (Feature Release)

- The flagship Lua scripting engine (sandboxed Lua 5.4, a Mesen2 / FCEUX-style
  `emu` API).
- Visual filters (full NTSC composite + a CRT / scanline pass + `.pal` palettes),
  input & peripherals (Power Pad, turbo / autofire, an input-display overlay), and
  debugger devtools (breakpoints, a cycle trace, an event viewer).
- An NSF / NSFe music player + a 5-band EQ; additive only, so the determinism
  contract and AccuracyCoin 100% hold.

## [1.0.0] - 2026-06-13 - "Cycle-Accurate" (Production Release)

- The first 1.0: RustyNES's emulation core is replaced wholesale with a new
  cycle-accurate, master-clock-precise engine, reaching AccuracyCoin 100.00%
  (139/139) with nestest 0-diff.
- Determinism is a hard contract (bit-identical output), band-limited BLEP audio,
  51 mapper families, Famicom Disk System, and Vs. System / PlayChoice-10 arcade
  support.
- Rollback netplay (2–4 players, native UDP + browser WebRTC), TAS movies, Game
  Genie + raw-RAM cheats, rewind, and opt-in RetroAchievements.
- A polished always-on egui desktop shell, a live in-browser WebAssembly demo, and
  a synthesized documentation set. The `v0.9.x` entries below are the documentary
  lineage of how this core was built.

## [0.9.7] - 2026-06-13 - Optimized Performance (documentary lineage)

- Documentary lineage of the cycle-accurate core (not a standalone user release):
  display-sync pacing modes, run-ahead, dynamic rate control, a dedicated
  emulation thread, browser AudioWorklet, and byte-identical core
  micro-optimizations.

## [0.9.6] - 2026-06-13 - Platform Expansion + RetroAchievements (documentary lineage)

- Documentary lineage: RetroAchievements (rcheevos), Vs. System / PlayChoice-10
  RGB support, mappers 38 → 51, and N-peer netplay (UDP + a browser WebRTC mesh),
  plus real-BIOS FDS boot and real two-instance rollback fixes.

## [0.9.5] - 2026-06-13 - Netplay (documentary lineage)

- Documentary lineage: GGPO-style rollback netplay (up to 4 players, a mesh
  transport) built on the determinism contract, plus STUN / hole-punch and Vs.
  System RGB-PPU groundwork.

## [0.9.4] - 2026-06-13 - Coverage + Input + FDS (documentary lineage)

- Documentary lineage: mappers 25 → 38, expansion input devices (the Arkanoid
  Vaus paddle, the Zapper light gun), and full Famicom Disk System support (RAM
  adaptor, per-cycle timer IRQ, writable disks, 2C33 wavetable audio).

## [0.9.3] - 2026-06-13 - Master-Clock Scheduler -> 100% Accuracy (documentary lineage)

- Documentary lineage: the master-clock-precise scheduler became the only path
  and AccuracyCoin reached 100.00% (139/139), with region-exact CPU:PPU ratios
  (3:1 NTSC / Dendy, 3.2:1 PAL).

## [0.9.2] - 2026-06-13 - Accuracy Hardening + Frontend Features (documentary lineage)

- Documentary lineage: a nesdev accuracy-hardening pass, Game Genie + raw-RAM
  cheats, Four Score support, config-driven gamepad rebinding, and browser
  save-state / movie persistence.

## [0.9.1] - 2026-06-13 - Expansion Audio + Web + TAS (documentary lineage)

- Documentary lineage: VRC7 OPLL FM audio (completing the expansion-audio
  family), the WebAssembly target, and the `.rnm` TAS movie format
  (record / playback / branching).

## [0.9.0] - 2026-06-13 - Cycle-Accurate Core Engine + Frontend MVP (documentary lineage)

- Documentary lineage baseline: the new master-clock-precise, lockstep-scheduled
  core (the Bus owns all mutable state; a one-directional dependency graph),
  band-limited audio, 15 mappers, an egui frontend MVP with rewind + a read-only
  debugger overlay, and the six-layer testing strategy.

## [0.8.6] - 2025-12-29 - Sub-Cycle Accuracy Improvements

- DMC DMA cycle stealing, NES open-bus behavior, and per-CPU-cycle mapper
  clocking; 522+ tests, a 100% Blargg pass rate.

## [0.8.5] - 2025-12-29 - Cycle-Accurate CPU/PPU Synchronization

- True cycle-accurate CPU / PPU synchronization via a `CpuBus` `on_cpu_cycle()`
  callback plus a cycle-by-cycle `cpu.tick()`; VBlank timing tests now pass with
  zero-cycle accuracy.

## [0.8.4] - 2025-12-28 - CPU/PPU Timing & Version Consistency

- The PPU is stepped before the CPU cycle for accurate `$2002` reads at the
  VBlank boundary, plus version-string and doctest fixes.

## [0.8.3] - 2025-12-28 - Critical Rendering Bug Fix

- Fixed a framebuffer showing "4 faint postage-stamp copies" by converting NES
  palette indices to RGB via the lookup table before display.

## [0.8.2] - 2025-12-28 - M10-S1 UI/UX Improvements

- Desktop GUI polish: Light / Dark / System themes, a status bar, a tabbed
  settings dialog, keyboard shortcuts, and modal dialogs.

## [0.8.1] - 2025-12-28 - M9 Known Issues Resolution (85% Complete)

- Audio improvements (two-stage decimation via rubato, A/V sync), PPU edge cases
  (sprite overflow, palette-RAM mirroring), and hot-path `#[inline]` hints.

## [0.8.0] - 2025-12-28 - Rust 2024 Edition & Dependency Modernization

- Rust 2024 Edition across all crates (MSRV 1.88), eframe / egui 0.33, cpal 0.16,
  and new rubato 0.16 high-quality resampling; no user-facing breaking changes.

## [0.7.1] - 2025-12-27 - Desktop GUI Framework Migration

- Migrated the desktop frontend from Iced + wgpu to eframe + egui, adding
  CPU / PPU / APU / memory debug windows and a settings dialog.

## [0.7.0] - 2025-12-21 - "Perfect Accuracy" (Milestone 8: Test ROM Validation Complete)

- A 100% Blargg test-ROM pass rate (CPU 22/22, PPU 25/25, APU 15/15, Mappers
  28/28 — 90 total), via a cycle-accurate CPU `tick()` state machine, PPU
  open-bus emulation, and CHR-RAM support.

## [0.6.0] - 2025-12-20 - "Accuracy Improvements" (Milestone 7: Complete + M8 Progress)

- Timing refinements across CPU / PPU / APU / bus (APU frame-counter precision, a
  hardware-accurate mixer, 513/514-cycle OAM DMA); Blargg CPU tests up to 90%.

## [0.5.0] - 2025-12-19 - "Phase 1 Complete" (Milestone 6: Desktop GUI)

- Phase 1 MVP complete: the `rustynes-desktop` app — a fully playable NES
  emulator (egui / wgpu, 60 FPS, cpal audio, keyboard + gamepad, config
  persistence), delivered ahead of schedule; 400+ tests.

## [0.4.0] - 2025-12-19 - "All Systems Go" (Milestone 5: Integration Complete)

- The `rustynes-core` integration layer connecting CPU / PPU / APU / mappers: a
  hardware-accurate bus, cycle-accurate OAM DMA, a console coordinator, and a
  save-state framework; 398 tests.

## [0.3.0] - 2025-12-19 - "Mapping the Path Forward" (Milestone 4: Mappers Complete)

- A trait-based mapper framework with the 5 key mappers (NROM, MMC1, UxROM,
  CNROM, MMC3) for 77.7% game coverage, full iNES + NES 2.0 parsing, and MMC3
  scanline IRQ.

## [0.2.0] - 2025-12-19 - "The Sound of Innovation" (Milestone 3: APU Complete)

- A complete, hardware-accurate 2A03 APU: all 5 channels, a non-linear mixer, a
  configurable resampler, and a DMC DMA interface; 150 tests.

## [0.1.0] - 2025-12-19 - "Precise. Pure. Powerful." (First Official Release)

- The first release: a cycle-accurate 6502 CPU (all 256 opcodes, a 100% nestest
  golden-log match) and a dot-level 2C02 PPU (97.8% pass rate); 144 tests.
