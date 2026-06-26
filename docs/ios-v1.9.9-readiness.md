# RustyNES v1.9.9 "Workshop" — iOS readiness gate

This is the readiness record for **v1.9.9 "Workshop"**, the final iOS/iPadOS
TestFlight release before the **v2.0.0 "Timebase"** core rewrite. v1.9.9 freezes the
iOS line for the v2.0.x re-port, so beyond shipping the creator/power-tools it runs a
full pre-freeze readiness pass: host-gate verification, a security audit, a gap
analysis against the train plan + Android, and a completeness-critic sweep. This file
is the authoritative summary; `docs/STATUS.md` remains the per-suite source of truth.

All v1.9.9 work is **additive / off-by-default**. The bridge gains only new forwarding
fns over existing `rustynes-core` APIs (no signature or behaviour change); the
deterministic core and chip crates are untouched; the shipped / native / `no_std` /
wasm core stays **byte-identical** and **AccuracyCoin holds 100% (139/139)**.

## 1. Verification / validation

### Host gate suite (all green)

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `cargo clippy -p rustynes-frontend --features scripting -- -D warnings` | PASS |
| `cargo clippy -p rustynes-frontend --features scripting,hd-pack -- -D warnings` | PASS |
| `cargo clippy -p rustynes-frontend --features retroachievements -- -D warnings` | PASS |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | PASS |
| `cargo build -p rustynes-core --target thumbv7em-none-eabihf --no-default-features` (`no_std`) | PASS |
| `cargo clippy -p rustynes-frontend --target wasm32-unknown-unknown --lib --bins -- -D warnings` | PASS |
| wasm `--no-default-features --features wasm-canvas` clippy | PASS |
| `cargo test --workspace --features test-roms` (AccuracyCoin / blargg / kevtris) | PASS — **139/139** |
| `markdownlint` (pinned v0.39.0, changed docs) | PASS |
| `rustynes-mobile` / `rustynes-ios` unit tests | PASS (24 + 8 + the new transcode test) |

The bridge change is verified additive: `cargo build --workspace` compiles the Android
crate unchanged, and no existing fn signature was modified.

### Feature reachability

Every new bridge fn has a Swift caller; every new view (Cheats, Debugger, TAStudio,
audio-depth controls, foreign-movie import) is reachable from the pill menu / Settings /
movie panel. The audio-depth DSP is bypassable to a bit-identical passthrough (asserted
by `to_bits()` equality tests). The debugger inspector is gated to the FOSS / TestFlight
build and is unreachable on the App-Store channel (ADR 0027).

Swift / iOS build + on-device validation are **not** CI-certifiable on the Linux host and
remain the documented TestFlight carryover (see §4).

## 2. Security audit

**Verdict: shippable — no Critical/High findings, nothing release-blocking.** The new
creator/power-tools surface is well-bounded and memory-safe; credentials are
Keychain-isolated; the importer bounds, FFI `unsafe` invariants, SPSC audio ring, and
TAStudio lock ordering all hold under verification.

| ID | Severity | Item | Disposition |
|---|---|---|---|
| M1 | Medium | Synchronous file I/O on the main thread (ROM / symbol import paths) — a UI-availability nuisance, not a vulnerability | Fixed (moved to `Task.detached`) |
| L1 | Low | Synchronous palette read on main thread | Fixed (moved off-main) |
| I1 | Info | `poke_ram` relied on the core to bound the address | Fixed (bridge-side `addr & 0x1FFF` clamp) |

Verified clean: all importer bounds (64 KiB memory-read cap, 256-row disasm cap, 16 MiB
per-ZIP-member cap with both a size check and a `.take()`), graceful (never-panicking)
parse errors on malformed movie input, the depth-config atomic mailbox (NaN/Inf mapped
to neutral), every `ffi.rs` `unsafe` with a sound `// SAFETY:` invariant, Keychain-backed
RA token + TURN secret (only non-secret config in UserDefaults), no hardcoded secrets, no
ATS bypass, and a `PrivacyInfo.xcprivacy` that still matches the code (creator tools add
no data collection).

## 3. Gap analysis

### Train-plan coverage

The v1.9.0–v1.9.8 workstreams are all present and corroborated by the CHANGELOG and
STATUS. The v1.9.9 "Workshop" workstreams: debugger overlay (read-only, FOSS-gated),
TAStudio piano-roll, foreign movie import (all five formats), Game Genie cheats, raw-RAM
cheats (one-shot), and audio depth are **implemented**; the scoping below is intentional
and acceptable for a TestFlight-readiness release.

### iOS-vs-Android parity

At parity on all emulation, input, library, video, netplay, RA, Lua, cloud,
accessibility, capture, and i18n features. Android-only items that are intentionally N/A
on iOS (QuickTile, home-screen widget / deep-link resume, Play Integrity, Play In-App
Updates, Chromecast) map to platform services the App Store handles differently. Genuine
iOS-applicable deferrals: box-art scraping, an opt-in crash-reporting surface, and a
dedicated external-display output — all post-v2.0.0.

### Creator-tools completeness (intentional scoping)

- Debugger: read-only inspector + single-frame step; no breakpoints / instruction-step /
  register-write (the bridge exposes only observational reads).
- Symbols: host-side `.sym` / `.mlb` / `.nl` parsing; `.dbg` (ca65/cc65 source maps) is a
  carryover.
- Raw-RAM cheats: one-shot poke/peek; Game Genie covers persistent cheating.
- TAStudio: host-side P1 frame table + scripted playback, not an editor over an arbitrary
  loaded movie.
- Audio depth: 5-band EQ (the 20-band desktop EQ is a carryover); no NSF player.

### FDS / NSF (resolved this release)

The shared mobile bridge loads **iNES / NES 2.0 only** — it has no FDS-disk or NSF-player
entry point (true for Android too). The import picker and `Info.plist` previously
advertised `.fds` / `.nsf`, which would fail on selection. v1.9.9 trims the picker +
document types to NES (+ `.zip`) for honesty; FDS + NSF on mobile are a post-v2.0.0
carryover.

## 4. Completeness-critic findings

No merge-blocking defects. Polish items found and dispositioned:

| Item | Disposition |
|---|---|
| TAStudio "Frame" column header + interpolated accessibility label not localized | Fixed (EN/ES keys added) |
| Audio-depth slider labels never localized (`ParamSlider.title` was a plain `String`) | Fixed (`LocalizedStringKey` + EN/ES keys) |
| `importForeignMovie` transcode ran on the main actor | Fixed (transcode moved into the detached task) |
| Hex `TextField`s (Cheats / Debugger) lacked VoiceOver labels | Fixed (`accessibilityLabel` added) |
| TAStudio `.rnm` export captured trailing idle frames | Fixed (stop-at-exhaustion / tail trim) |
| TAStudio "Save" power-cycles silently | Fixed (added a localized caption) |
| No happy-path foreign-movie transcode test at the bridge | Fixed (added an `Ok` transcode test) |

## 5. Carryovers to v2.0.x / v2.1.0

**Forced re-ports by the v2.0.0 save-state-format break (ADR 0002):** `.rns`
save-states + the four slots + CloudKit-synced states, `.rnm` TAS movies / imports, and
cross-platform save + netplay parity must be re-validated on the new core. iOS host
re-port lands v2.0.5–v2.0.8; dual-app readiness v2.0.9.

**Distribution / monetization (v2.1.0, joint with Android):** launch the App-Store
channel plus AltStore PAL, and wire `Entitlements.swift`'s dormant `StoreManager` to
`rustynes-monetization` (StoreKit 2 + RevenueCat + AppLovin MAX, ATT). Dormant /
fully-unlocked through v1.9.x.

**Maintainer-manual (not CI-certifiable):** Apple Developer account + bundle ID, fastlane
match signing + App Store Connect API key, app icon / launch art, and the full on-device
TestFlight verification checklist (ROM import, save / rewind, MFi controller, audio
interruptions across iOS 16/17/18, ProMotion pacing, no crashes on 5–10 ROMs, an accurate
privacy label).

**Deferred features to re-evaluate post-Timebase:** FDS disk + NSF player, 20-band EQ,
`.dbg` source maps, a cheats database, box-art scraping, a home-screen widget, a dedicated
external-display output, and an opt-in crash-reporting surface.
