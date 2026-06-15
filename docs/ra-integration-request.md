# RetroAchievements emulator-integration request (draft)

A ready-to-send message for the RetroAchievements team (their Discord developer
/ emulator-integration channels, or a GitHub issue on `RetroAchievements/rcheevos`).
Fill in the bracketed bits (`[...]`) before sending.

---

**Subject:** Emulator integration request — RustyNES (NES, rcheevos/rc_client)

Hi RetroAchievements team,

I'd like to request integration / client allowlisting for **RustyNES**, an
open-source, cycle-accurate Nintendo Entertainment System emulator written in
Rust. It already integrates `rc_client` and enforces the hardcore rules; I'm
hoping to get it recognized server-side so the "unknown emulator" notice clears
and hardcore unlocks count.

## Project

- Name / version: **RustyNES v1.0.0**
- Source (open-source, dual **MIT OR Apache-2.0**): `[https://github.com/<you>/RustyNES]`
- Live web build (wasm): <https://doublegate.github.io/RustyNES/>
- Platforms: Linux / macOS / Windows (native) + WebAssembly. RA is a native-only,
  opt-in build feature today (the C `rcheevos` lib isn't compiled into the wasm
  build).
- Accuracy: passes the standard NES test-ROM suites (blargg, MMC3, AccuracyCoin
  100%); the emulator core is deterministic (used for TAS movies + rollback netplay).

## Integration

- **`rc_client`** via the vendored MIT **rcheevos** (commit `9ade739`), with a
  hand-written `extern "C"` FFI (no bindgen). An ABI guard (`sizeof` accessors +
  Rust `size_of` tests) catches struct-layout drift on a vendor bump.
- Console: **`RC_CONSOLE_NES`**. Hashing is rcheevos's own `rc_hash`
  (`begin_identify_and_load_game` is handed the raw iNES bytes; rcheevos strips
  the 16-byte header and MD5s the remainder).
- **Memory map** (the `read_memory` callback, side-effect-free):
  - RA `0x0000–0x07FF` → NES system RAM `$0000–$07FF` (2 KiB)
  - RA `0x0800–0x27FF` → cartridge work/save RAM `$6000–$7FFF` (8 KiB WRAM window)
  - anything else → unmapped (reports 0 bytes)

  Reads go through a side-effect-free bus peek (no I/O side effects, no open-bus
  mutation), and the rc_client is single-threaded (HTTP runs on a worker thread;
  completions are dispatched back on the main thread).
- **User-Agent**: requests are sent as `RustyNES/<version> rcheevos`.

**Hardcore enforcement** (this is the part you care about most — happy to walk
through the code). In hardcore mode RustyNES disables every state-manipulation
path:

- save-state **loading** (saving is allowed; loading is blocked)
- **rewind**
- **cheats** — both Game Genie codes and raw RAM pokes
- **frame-advance / slow-motion**
- the **debugger memory view / RAM editing**

Soft **reset** and **power-cycle** remain allowed (and call `rc_client_reset`).
The gates are centralized in a single predicate, so they're easy to audit; the
relevant files are `crates/rustynes-cheevos/` (the safe `rc_client` wrapper) and the
frontend's RA session + hardcore-gating predicate.

## What I'm asking for

- Allowlisting the `RustyNES` client so the unknown-emulator notice clears and
  hardcore unlocks are credited.
- Any additional requirements you have (a specific rcheevos version to track, a
  test checklist, naming conventions, a code review of the hardcore gates, etc.)
  — just point me at them and I'll get them done.

The codebase is open, so you're welcome to audit the integration + the hardcore
enforcement directly. Thanks for maintaining rcheevos and for considering
RustyNES.

— [your name / RA username]

---

## Notes for sending

- **Best channel:** the RetroAchievements Discord (developer / emulator-integration
  area) is where the team handles new-emulator onboarding. A GitHub discussion/issue
  on `RetroAchievements/rcheevos` is a reasonable alternative.
- **Have ready:** the repo link, the rcheevos version/commit, and a one-line
  pointer to where hardcore is enforced (`crates/rustynes-cheevos/` + the frontend RA
  session). Offering a short screen-share / code walkthrough of the hardcore gates
  speeds up approval.
- **Before sending:** ship a stable release (v1.0.0) so the version you cite is
  the one they'd test, and confirm your public repo URL.
