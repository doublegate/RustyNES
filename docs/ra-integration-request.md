# RetroAchievements hardcore-compliance request (draft)

A ready-to-send application for RetroAchievements to validate RustyNES as a
hardcore-compliant client. The maintainer sends it; nothing here has been sent.
Refreshed for v3.4.0 (`T-RA-HARDCORE`); the previous draft described v1.8.8 and
a dual MIT / Apache-2.0 license, both out of date. Fill in the bracketed bits
(`[...]`) before sending.

## How to apply (researched 2026-10-16)

What RA documents:

1. **The requirements** are RA's [Hardcore Compliance
   Requirements](https://docs.retroachievements.org/general/hardcore-compliance-requirements.html)
   (sections A-H; section G lists the automatic-rejection criteria). The page
   refers to "applying for compliance" but gives no address or form.
2. **The contact** is named in the rcheevos [`rc_client` integration
   guide](https://github.com/RetroAchievements/rcheevos/wiki/rc_client-integration),
   section "User-Agent Header": if a User-Agent is not recognized, "the server
   will turn hardcore unlock requests into softcore unlock requests. Please
   [contact RAdmin](https://retroachievements.org/messages/create?to=RAdmin) to
   have your client validated for hardcore support." That link opens a private
   message to the `RAdmin` account on retroachievements.org; it needs a logged-in
   RA account. **This is the route to use.**
3. **Not the forum.** A forum post returned HTTP 403 before (and RA's forum
   pages return 403 to unauthenticated fetches); the team's own docs do not
   name the forum, Discord or GitHub issues as the application channel. A
   short RAdmin message linking this document and the repository is the
   documented path. A Discord or rcheevos-issue contact is fine for technical
   questions, not as the application.
4. **Eligibility** (section D): the emulator must have been publicly released
   for at least six months. RustyNES's first public GitHub release is v0.1.0,
   2025-12-19, so it qualifies from 2026-06-19.
5. **After approval** RA lists the client on its
   [Downloads page](https://retroachievements.org/downloads) and may re-evaluate
   it periodically.

Before sending, the maintainer must:

- [ ] publish a **privacy policy** page (section F; a placeholder or a
      contradiction is an automatic rejection). RustyNES collects no telemetry;
      the policy should say so and state what is stored locally (the RA username
      and login token in the config file, never the password; per-game progress
      files; save states) and what leaves the device (RA API calls only; on iOS,
      the optional CloudKit save-state sync goes to the user's own iCloud).
      `[URL of the privacy policy]`
- [ ] point at the **license listing** (section F): `NOTICE` and
      `docs/originality-and-provenance.md` in the repository; the project is
      GPL-3.0-or-later and ships source, so the copyleft obligations are met.
      There is no monetization, so no features matrix is needed.
- [ ] confirm the User-Agent history below is complete.
- [ ] confirm the release the message names is the current one
      (`docs/STATUS.md`, "Current release").

---

**To:** RAdmin (<https://retroachievements.org/messages/create?to=RAdmin>)

**Subject:** Hardcore validation request: RustyNES (NES, rc_client)

Hello,

I would like RustyNES validated as a hardcore-compliant client. It is an
open-source, cycle-accurate Nintendo Entertainment System emulator written in
Rust that integrates `rc_client` directly, and I have audited it against the
Hardcore Compliance Requirements; the audit, with the code location and the test
for every rule, is linked below.

## Project

- **Name / version:** RustyNES v3.4.0 `[confirm the current release]`
- **Source:** <https://github.com/doublegate/RustyNES>, **GPL-3.0-or-later**
  (license listing: `NOTICE`, `docs/originality-and-provenance.md`). No
  monetization, no paid features, no telemetry. Privacy policy: `[URL]`.
- **First public release:** v0.1.0, 2025-12-19.
- **Platforms with RetroAchievements:** Linux, macOS and Windows (the native
  desktop build with the `retroachievements` feature; the `full` build includes
  it), Android and iOS (a shared Rust bridge). The WebAssembly build is
  casual-only by construction (it cannot enable hardcore) and runs behind a
  separate auth proxy; please treat it as out of scope for hardcore.
- **Libretro:** a separate RustyNES libretro core exists; under RetroArch,
  RetroArch's own integration handles achievements, so this request is about
  the standalone frontend and the mobile apps.

## Integration

- **rcheevos 12.5.0**, vendored, `rc_client` through a hand-written FFI with
  struct-layout guards. Console `RC_CONSOLE_NINTENDO`; hashing by rcheevos
  (`rc_client_begin_identify_and_load_game` with the raw image).
- **Memory map** (the read callback, side-effect-free): RA `0x0000-0x07FF` ->
  CPU RAM `$0000-$07FF`; RA `0x0800-0x27FF` -> cartridge RAM `$6000-$7FFF`;
  anything else unmapped. Background memory reads are off
  (`rc_client_set_allow_background_memory_reads(0)`), so address validation
  runs on the emulation thread against the live machine.
- **User-Agent:** `RustyNES/<version> (<OS> <arch>) rcheevos/<version>`, for
  example `RustyNES/3.4.0 (Linux x86_64) rcheevos/12.5.0`: your documented
  `Name/version (system) extension` shape, with a numeric incrementing version.
  **History, for disclosure (section C):** RustyNES has always identified as
  `RustyNES/<version>`; until v3.4.0 the string was
  `RustyNES/<version> rcheevos/<version>`, without the system clause. It has
  never used another emulator's User-Agent. The browser build's proxy sends the
  same product token.
- **Offline unlocks:** an unlock whose request fails for lack of a network is
  reported to `rc_client` as retryable, so `rc_client` keeps it queued and
  re-sends it when the connection returns (Disconnected / Reconnected are shown
  to the player). The queue is `rc_client`'s, in memory: an unlock still pending
  when the program is closed is lost. If you require the queue to survive a
  restart, please say how you would like it stored (it holds the API token and
  a signed request), and I will add it.
- **Hardcore** is on by default on the desktop (`[confirm the mobile apps' default]`); the achievements panel switches
  it with one click; a "Hardcore mode" / "Casual mode" toast appears when a game
  is identified, and the desktop status bar keeps a HARDCORE marker.

## Hardcore enforcement

One table (`crates/rustynes-ra/src/hardcore.rs`) holds every decision with the
clause of your requirements it rests on; the desktop and the mobile bridge both
enforce it, each block at the point of action, and each pinned by a test that
was shown to fail when the block is removed (a value mutation). Full detail:
`docs/frontend.md`, section "Hardcore mode".

| rule (your section B / G) | desktop: where refused | mobile bridge | test |
| --- | --- | --- | --- |
| no save-state loading | every load route asks `session_policy::refusal` | `load_state` -> `HardcoreBlocked` | `session_policy::tests::hardcore_refuses_a_load_only`; `hardcore_blocks_load_state_only` |
| cheats disabled (Game Genie included) | Game Genie list emptied and raw RAM cheats skipped on every frame | `cheat_add_genie` refused; codes removed each tick | `emu::tests::hardcore_runs_the_frame_without_game_genie_codes`, `hardcore_applies_no_raw_ram_cheat`; `hardcore_refuses_cheats_scripts_playback_and_memory_tools` |
| no rewind | refused where the rewind runs | (no rewind on mobile) | `emu::tests::hardcore_refuses_a_held_rewind` |
| no slowdown | the pacer runs any speed below 100% at 100%; the speed menu refuses it | host-side pacing (Kotlin / Swift) | `emu::tests::hardcore_paces_slowdown_at_full_speed_and_keeps_fast_forward` |
| no frame advance | refused | host-side | `app::tests::every_hardcore_start_site_asks_the_one_rule` |
| no memory editors / viewers | memory viewer, RAM search and pokes refused | `poke_ram` no-op; memory reads return nothing | `debugger::tests::hardcore_closes_the_debugger_memory_cheat_and_tas_windows_first`; `emu::tests::debug_poke_is_gated_by_writes_locked` |
| no debuggers | every inspector window closes | debugger reads return nothing | same debugger test; the mobile test above |
| no scripting | a script load is refused and a running script stopped | `load_script` refused; unloaded each tick | `app::tests::every_hardcore_start_site_asks_the_one_rule`; `hardcore_tears_down_running_features_on_the_next_frame` |
| no TAS / recorded-input playback | movie playback, movie import, TAStudio, BasicBot refused; a playing movie stops | `movie_play` refused; playback stops each tick | `emu::tests::hardcore_stops_movie_playback_and_keeps_a_recording` |
| a resumed session drops to casual | saved achievement progress is never restored in hardcore | same (shared session) | `hardcore_never_restores_saved_progress` |
| casual -> hardcore needs a full reset | refused while a movie or netplay runs; otherwise a power cycle, then `rc_client_reset` | same | `app::tests::switching_hardcore_on_power_cycles_a_running_game`; `switching_hardcore_on_power_cycles_and_waits_for_a_recording` |
| leaderboards and rich presence cannot be disabled | there is no switch | none | - |

Allowed in hardcore, following your text: creating save states, recording
movies, fast forward, reset and power cycle, and pausing (throttled by
`rc_client_can_pause`). Run-ahead and netplay are not mentioned in your
requirements and are allowed; please tell me if you want either restricted.

Known differences I would rather state than have you find:

- On Android, the app's automatic resume of its on-background save state is
  refused in hardcore (it is a state load), so the game starts from power-on;
  your wording asks that a resumed session drop to casual instead.
- Speed controls on Android and iOS live in the native app layers; the Rust
  bridge cannot enforce them, and they are `[audited / not yet audited]`.
- No hardcore session has yet been played against your production server from
  this version; the offline-unlock behaviour was verified against a scripted
  fake server that speaks your API's JSON.

I am happy to make any change you ask for. Thank you for rcheevos and for
considering RustyNES.

— `[name]` (RA username `[username]`)

---

## Notes for the maintainer

- The message is long on purpose: section G's criteria are the ones RA checks
  first, and the table answers each with a file and a test.
- If RA asks for a build to test, point them at the latest GitHub release's
  desktop binaries; they are built with the `full` feature set, which includes
  `retroachievements` `[confirm against release.yml before sending]`.
- Keep this file current: it is the record of what was claimed to RA.
