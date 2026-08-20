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

- **One atomic, durable file write for every path that persists user data.**
  (v2.4.0 item C.) The seven-property write sequence v2.3.9 built for
  `Config::save_to` is extracted into `crate::atomic_write` and adopted everywhere.
  The plan named three call sites; there were **four**, and the fourth is the
  instructive one.

  `save_state.rs` matters most and was named last: **a truncated save state is a
  user's game progress**, a worse loss than a truncated config, and it was still
  using the bare `fs::write` the config path had already been fixed for. It is also
  the path most likely to be written under load — rewind capture, run-ahead and
  netplay rollback all produce save states.

  `per_game.rs` was not in the plan at all, because it *looks* correct: it writes a
  sibling temp file and renames, so a sweep for `fs::write`-onto-a-target clears
  it. It held two of seven. No `fsync`, so the rename could commit a directory
  entry pointing at bytes that never reached the medium; and a **fixed** scratch
  name shared across every process and concurrent call — the exact failure the
  mechanism exists to prevent, reintroduced by the mechanism. A partially-correct
  implementation is harder to spot than an absent one.

  The config path **gains** something it never had: a bounded retry past a
  transient Windows sharing violation. `MoveFileEx` fails if another process has
  the target open, and an antivirus scanner or search indexer reading `config.toml`
  is enough. POSIX has no such constraint, which is why it went unnoticed — and why
  it would have surfaced as a Windows user reporting a save that failed for no
  visible reason. When the attempts are exhausted the error **propagates**.

  **Review then found three more places the module reported success it had not
  earned**, and the shape was the same each time: an error discarded at a call
  site, under a comment explaining the *rest* of the operation. `set_permissions`
  was swallowed — and the mode being applied is the mode the target **already
  had**, so a failure replaces a 0600 file with one at the umask default, *wider
  than what it replaced*, and says nothing. The parent-directory `sync_all` was
  swallowed along with the `File::open` that fed it, so the entire durability
  barrier could be a no-op while the module's own table claimed "yes" for Unix;
  `EIO` — the exact condition the sync exists to detect — was reported as success.
  Both now propagate, the sync excepting only the two errnos that mean *this
  filesystem does not offer the barrier* (`EINVAL`, and `EBADF` on some network
  mounts), since failing a save outright on those mounts is a worse answer than
  proceeding. And the occupied-scratch retry was **one** attempt, on the reasoning
  that the counter cannot repeat a name within a process — true, and beside the
  point, because the collision comes from a *previous* process: a run that crashed
  mid-session orphans one scratch file per save it made, and pid reuse restarts the
  counter at zero, so two orphans defeat one retry.

  A second review round then found a **fourth**, in the fix for the third: on
  exhaustion the last name tried is one that **already existed** — an orphan, or a
  scratch file a colliding instance is actively writing — and the cleanup deleted
  it. A failed save took another process's in-progress data with it. The defect
  predated the loop, which widened it from one chance to eight; the scratch path
  is now `Option`al and assigned only on a successful create.

  **The first test written for that fix did not test it.** It forced a failure by
  writing to a directory, which fails at the *rename* — a branch where the scratch
  file genuinely is ours — so it passed against the defect and the fix alike. Two
  mutations reported NOT CAUGHT, which is the only reason it was noticed; the
  scratch-name source is now injectable so the exhaustion branch is reachable
  without predicting global state.

  A third round found no blocking issues and two worthwhile refinements: `ENOTSUP`
  / `EOPNOTSUPP` joins the excused set, since the list is *ways a filesystem says
  there is no barrier here* and leaving one out fails a save on that mount; and
  the parent-directory resolution loses an allocation. Mutating the second turned
  up a **pre-existing untested property** — the fallback that maps a bare relative
  filename's `Some("")` parent to `.`, without which `File::open("")` returns
  `ENOENT`. Documented as load-bearing since it was written, never tested, and it
  matters *more* now: while the sync was best-effort, losing it meant a durability
  step quietly skipped, but now that it propagates, losing it makes `write_atomic`
  **fail outright** for any relative target.

  Getting the third one under test surfaced something else. Reaching the exhaustion
  branch through `write_atomic` means predicting the process-global `SCRATCH_SEQ`
  and planting a decoy at every name the call will pick — and **that prediction
  races**, because every parallel test calling `write_atomic` consumes sequence
  values. Measured rather than assumed: a serialising mutex over the three tests
  that *peek* at the counter still failed 2 runs in 5, because the tests doing the
  consuming are precisely the ones that never look at it. **The pre-existing
  single-decoy test had been latently flaky since it was written and had simply
  never lost the race.** All three decisions are now named functions driven
  directly, which is the fourth time this release that "extract it so a test can
  reach it" was the actual fix.

  Two mutations forced design changes rather than confirming the design. The retry
  loop's predicate had to become a **parameter**: hard-wired, the exhaustion branch
  is unreachable on Unix, and a mutation making it return `Ok(())` — silently
  reporting a save that never happened — went **uncaught**. And the mode test was
  asserting less than its name claimed, since `opts.mode(0o600)` at creation
  already yields 0600 under any ordinary umask. Two properties are **not**
  observable in-process and the module says so: `fsync` (needs a power loss) and
  creation-mode (a race-window narrowing, where a test can only see the end state).
  Neither should be deleted on the evidence that no test fails.

- **A timeline generation counter, and the telemetry that reads it.** (v2.4.0 item
  B.) v2.3.9 cleared stale debug telemetry on a ROM change and recorded that it
  could not clear it on a save-state load: of the four ways the emulator jumps
  timeline, only one is reachable from a patchable frontend call site — wasm
  load-state restores inside a `spawn_local` task, and rewind happens entirely
  inside the core.

  `Nes` now carries a session-local `timeline_generation`, and `restore_inner`'s
  existing `clear_rewind` parameter already draws exactly the needed distinction,
  so it is reused rather than duplicated. **This departs from the plan's
  enumeration deliberately:** the plan listed netplay rollback as a bump site and
  also stated the mechanism that forbids it — a same-timeline restore must *not*
  bump. Netplay rollback and run-ahead both go through `restore_quiet` precisely
  because they are same-timeline; bumping there would clear a user's telemetry
  sixty times a second, which is worse than the defect being fixed. Both directions
  are pinned by tests.

  **The counter is not serialized**, and that is load-bearing rather than a
  preference: serializing it would put an *old* value back on restore, so loading a
  state saved earlier in the same session could hand a consumer a generation it has
  already seen. The plan asked for an entry in `snapshot_schema_audit.rs`; that file
  audits the four chips, not `Nes`, so the property is pinned by an **executable**
  assertion instead — snapshot at generation N, advance past N, restore, and assert
  it did not come back to N. Simulating serialization makes it fail with exactly
  that diagnostic.

- **A standing release-anchor audit — the drift v2.3.9 corrected by hand cannot
  recur silently.** `crates/rustynes-test-harness/tests/release_anchor_audit.rs`
  pins **15 anchors across 10 documents** against `[workspace.package] version`:
  the README badge and Current Release section, `docs/STATUS.md`, both `AGENTS.md`
  anchors plus its "never claim a later version" guard, `VERSION-PLAN.md` (header
  *and* the `(current)` row of its release table), `to-dos/ROADMAP.md`,
  `SUPPORT.md`, `SECURITY.md`, the root `ROADMAP.md`, `OVERVIEW.md` and
  `ARCHITECTURE.md`.

  Modelled deliberately on `libretro_info_audit.rs`, which exists because the
  libretro `.info` `display_version` drifted from the workspace and advertised the
  wrong licence for eleven days. Same failure, same shape of fix: the manifest is
  the single source of truth and every other statement of the fact is *compared*
  against it rather than maintained beside it. At v2.3.9 those anchors held **six
  different values**, the oldest four releases stale.

  Three assertions beyond the version itself. The CHANGELOG must carry a section
  for the workspace version with a parseable `- <date> - "<Codename>"` tail,
  because `release-auto.yml` reads that exact line twice — for the body fallback
  and for the release title — and it has broken a release before. Any anchor that
  quotes a codename must quote the CHANGELOG's, since a right version beside the
  previous release's codename is the more confusing error: the number looks
  correct, so the sentence around it gets trusted. And `VERSION-PLAN.md`'s table
  must mark exactly one row `(current)` — at v2.3.9 it marked v2.3.5, three
  releases behind its own header.

  **It fails closed.** A marker that matches nothing is a failure, never a pass;
  an audit that finds zero anchors and reports success is indistinguishable from
  one that found them all correct, which is the defect class v2.3.9 was about.
  Proven by mutation rather than asserted: five independent mutations — a drifted
  badge version, a stale codename, an anchor reworded out of existence, a moved
  `(current)` marker, and a renamed CHANGELOG section — each fail the test they
  should and only that test.

### Fixed

- **Two shipped features stop writing empty tables into an untouched config.**
  (v2.4.0 item D.) `graphics.hd_packs` (v1.5.0) and `graphics.shader_presets`
  (v1.2.0) both documented a pre-feature config as "byte-identical". Both were
  byte-identical only until the first save: `#[serde(default)]` is a **load**
  guarantee, and the TOML serializer emits an empty table for an empty collection.
  v2.3.9 corrected the prose and deliberately left the behaviour, because changing
  what a shipped feature writes is a separate decision; this is that decision.

  Both directions are tested, because a one-directional test passes just as happily
  against a field that never persists anything — and the over-eager direction is
  the dangerous one: an `is_empty` returning `true` unconditionally would silently
  discard a user's saved presets on every save, a data-loss bug wearing the shape of
  a tidiness fix. A third test asserts the property once for **the field that does
  not exist yet**, since the defect being fixed is precisely "a field was added and
  the save-side property was not considered".

### Changed

- **The owed upstream libretro sync is filed, and it was smaller than expected.**
  (v2.4.0 item A.) `libretro-super#2074` bumps `display_version` v2.3.5 → v2.3.9 —
  **one line**. Everything else was already correct upstream, including
  `license = "GPLv3+"`. Verified before pushing: the branch file is now
  **byte-identical** to this repository's copy, which is the property
  `libretro_info_audit.rs` exists to make possible.

  `libretro/docs#1180` needed nothing — it is a **pull request** open since
  2026-08-16, `MERGEABLE/CLEAN`, unreviewed, not an issue. The misreading that
  nearly produced a duplicate is recorded because it is reusable: `gh api
  repos/OWNER/REPO/issues/N` **returns pull requests**, since GitHub's issues
  endpoint serves both.

  `AGENTS.md` now carries the cadence rule: **upstream PRs are opened only on
  MINOR/MAJOR releases** (`vX.Y.0` where X or Y changed). Patch releases do not
  sync; the next is **v2.5.0**. A **licence change overrides and syncs
  immediately** — the rule that incident produced in the first place.

- **`to-dos/DEFERRED-AND-CARRYOVER-FEATURES.md` swept entry by entry**, against
  `main` @ `fdfb2c04`. Eleven entries struck, each carrying its evidence inline —
  a file that exists, a workflow line number, a test that says so — rather than a
  bare tick, so a closure can be disagreed with.

  Most of what the sweep found was stale by far more than the five releases it was
  scoped to. **The whole of §6a — the four items (A1-A4) defining the timebase
  rewrite — shipped in v2.0.0 "Timebase" on 2026-07-03**, six weeks and roughly
  twenty releases earlier, and §6's preamble still described AccuracyCoin as
  "100% / 139/139" when it has been an exact **141/141** since v2.0.3. A backlog
  listing the project's designated MAJOR release as pending is not untidy; it is
  misleading about what the emulator is.

  Three entries were closed by something **other than what they proposed**, and
  say so rather than being quietly ticked: the feature-combo clippy gap is closed
  by *enumerating* the combos in CI (eight invocations, including the wasm32 ones)
  rather than by adopting `cargo-hack`, which leaves a real residual — a new
  feature is uncovered until someone adds a line; `merge_group` stays open but its
  companion clause shipped, so the entry is **narrowed** to the merge queue alone;
  and R3 turned out to be a **harness artifact** rather than an emulation
  residual, so A4 is not what fixed it, and conflating the two would inflate what
  the refactor is credited with.

  §7's mapper entries below the v2.3.4 line are **explicitly not swept** — they
  need a ROM corpus to adjudicate, and asserting them from source alone would be
  the over-claim this catalogue exists to avoid.

## [2.3.9] - 2026-08-20 - "Crucible" (what the gates actually cover)

A crucible is where something is tested to destruction rather than inspected, and
that is what this release does to the project's own gates: what they cover, what
they only appear to cover, and where a regression could still reach `main`
unchallenged.

Deliberately not a feature release. The v2.3.x line added five tools in four
releases, and the recurring finding across all of them was never that the
emulation was wrong — it was that **a check reported a pass it had not earned.**

**`rustynes-apu` and `rustynes-core` both change**, so AccuracyCoin is verified
rather than asserted, and re-run after the second round of deletions rather than
only the first: **141/141 (100.00%)** on the authoritative RAM decoder, nestest
0-diff.

### Added

- **A Latency Oracle measurement is remembered per game.** (#410.) Reopening a
  game shows what was measured last time instead of an empty panel, keyed on the
  ROM SHA-256 in `[input]` — the same key shape `graphics.hd_packs` already uses.
  `#[serde(default)]` so an older config loads unchanged, plus
  `skip_serializing_if` so the key stays out of the file until there is something
  to store: a user who never opens the panel carries nothing and their config is
  not rewritten.

  **Remembering is not applying.** Nothing here touches `run_ahead`, and
  restoring never queues a pending apply, so a depth measured in an earlier
  session is still one explicit click from being applied. An **inconclusive**
  result is not remembered at all: a stored "I could not tell" is
  indistinguishable from a stored answer once it has lost the context that
  produced it.

- **The two-acquisition lock race is measured rather than reasoned about.**
  (#409.) The `needs_nes` render arm — taken exactly when a debugger or tool
  panel is open — acquires the emulator lock twice per redraw, and drops it in
  between so composite work does not hold the emulator. If the emulation thread
  takes the lock in that gap, the screen shows frame N while a panel describes
  N+1.

  `Nes::cycle()` is read at both acquisitions and compared: it is cumulative and
  monotonic, and `produce_one_frame` holds the lock across a **whole** frame, so
  any difference at all means at least one complete frame landed in the gap.
  Both a hit count and a denominator are kept — "the race did not fire" and
  "nothing was observed" both read as zero hits, and only the denominator
  separates them.

- **A RAM Atlas address can leave the panel.** (#413.) `Send to RAM Watch`
  exports a classified address into Memory Compare's watch list, carrying the
  verdict **and the lens that produced it** — liveness is relative to the
  observable, and the watch list is exactly where an unqualified "LIVE" would
  outlive the panel that qualified it. `Untested` is spelled out and cites no
  lens, because nothing was observed through one. Every address is exportable,
  including `Inert`: the rule is that a claim carries its evidence, not that
  unverified addresses are unusable.

- **A real produce-to-visible latency series, and the end-to-end figure it makes
  possible.** (#412.) The interim figure was valid only because its lag term is a
  *constant*; the vblank wait and lock contention could not simply be added on,
  because `p95(A) + p95(B)` is not `p95(A + B)`. `PresentBuffer` now stamps each
  frame at publish and the redraw records `stamp.elapsed()` after the present, so
  one sample spans the whole pipeline. The panel reports both figures, gated
  independently, and says which one it is showing.

- **The SAFETY-comment rule is a gate rather than a request.** (#423.)
  `clippy::undocumented_unsafe_blocks` is enabled workspace-wide. All 91 unsafe
  sites already carried a justification; two had it where a human reads correctly
  and a checker cannot — one of them reachable only under the `browser-cheevos`
  wasm feature, so it surfaced only when the gate ran across every gated
  combination. The gate is demonstrated to fail, not assumed to.

### Changed

- **The accuracy battery runs at review time, scoped by path.** (#408.) `setup`
  computed one `full` flag and `test-roms` ran only when it was true, so a
  regular feature PR never ran the battery and an accuracy regression could not
  be caught on the PR that caused it — #403 is the worked example. A second
  `paths-filter` output covers the chip crates, core, `rustynes-gamedb`, the test
  harness and `tests/`, and `test-roms` now runs when it *or* the existing full
  flag is true. `rustynes-gamedb` is included for a non-obvious reason: it
  rewrites the iNES header on load, so it changes what the emulator *is* before a
  cycle runs.

- **Provisioning steps are bounded, not just the jobs.** (#408, #409.) The
  cross-compile gate's `apt-get update && apt-get install` were network fetches
  with no timeout of their own, so a stalled mirror hung until the job timeout
  fired and the run was reported as cancelled rather than as what it was — four
  times during the v2.3.7 cut. Now one bounded, thrice-retried helper, with
  elevation outside `timeout` so a killed fetch cannot orphan `apt-get` holding
  the dpkg lock.

- **The docs-only CI skip is repaired — it had never worked.** (#418.)
  `dorny/paths-filter`'s `predicate-quantifier` defaults to `some`, so `'**'`
  matched everything and all seven `!` exclusions were dead from the day they were
  written. A markdown-only PR logged `Filter code = true`. Fixed with two filter
  steps, because the quantifier is step-level and `accuracy` is a list of
  alternatives that `every` would make unsatisfiable — silently disabling the
  accuracy battery while fixing a different gate.

- **The ARM provisioning installs headers rather than a toolchain.** (#417.) The
  step asked for a whole cross compiler to obtain a header package the comment
  above it had already named, and whose linker it stated was unused. Its timeouts
  were calibrated against an unmeasured claim; the log gave the real number,
  `Fetched 4201 kB in 4min 45s (14.7 kB/s)`.

- **257 lines of dead code removed**, plus 25 `#[allow(dead_code)]` attributes
  that were suppressing nothing. (#422.) Two `mc-r1-*` islands whose cargo
  features exist nowhere in the workspace, and `drain_dma` — called on every CPU
  read, every CPU write and every bus cycle, with an empty body and comments
  claiming the legacy service below it "stays active for the default build".

- **Two `cargo deny` advisory ignores retired.** (#424.) Their own entry said to
  remove them once the resolve moved past quick-xml 0.40, and it had.
  `advisory-not-detected` is a warning, so nothing surfaced it.

### Fixed

- **A freeze from one cartridge kept writing into the next.** (#419.) Both memory
  panels' freezes feed the per-frame raw-cheat overlay, and neither was registered
  with the ROM-transition hook — so a byte frozen in one game went on being written
  into the next at an address that means something else there. The sweep that found
  it now covers every panel under one rule: derived output is discarded,
  user-authored input is kept, and only input that actively **writes** is
  neutralised.

- **The call stack and access counters survived a ROM change.** (#415.) Both are
  reconstructed telemetry that does not rebuild itself, and `uninit_read` is a
  *claim* rather than a tally — carried across a ROM change it reported
  uninitialized-RAM findings about a cartridge no longer loaded.

- **The config file is written atomically and durably.** (#420.) `fs::write`
  truncates then writes, so an interruption left a truncated `config.toml` — every
  keybinding, palette, shader preset and per-game setting. Now a sibling scratch
  file, `fsync`, exclusive creation, mode carried across, symlink resolved
  (including a **broken** link, the freshly-created dotfiles case), and the parent
  directory synced.

- **A failed latency-config save is reported instead of swallowed.** (#411.)

- **Movies record two ports, and now say so.** (#421.) `FrameInput` models P1 and
  P2, so a recording made with the Four Score adapter captures half of what drove
  the run — while the Replay panel printed "Four Score (P1..P4)" at the moment a
  user decides to press Record. Widening the format is a `.rnm` epoch change, so
  this is disclosed at three levels, with the caveat printed directly under the
  claim it qualifies.

### Documentation

- **The release anchors are re-synchronised, and two of them were wrong about
  more than the version number.** Every "current release" claim outside the
  CHANGELOG had drifted, each by a different amount, which is what happens when
  the same fact is written down in eight places: `README.md` and `docs/STATUS.md`
  said v2.3.7, `VERSION-PLAN.md` said v2.3.6 (its release table stopped at
  v2.3.5, still marked `(current)`), `to-dos/ROADMAP.md` said v2.3.3 in one place
  and v2.2.5 in another, `SUPPORT.md` said v2.3.0, and the root `ROADMAP.md` said
  v2.0.4. All now read **v2.3.9**, with v2.3.8 and v2.3.7 demoted into the
  historical trail rather than dropped.

  Two carried a claim worth more than the version:

  - **`SECURITY.md` still offered support for `1.0.x` and marked `< 1.0`
    unsupported** — a policy table describing a project two major versions and
    eleven months behind the tree, on the one document a reporter reads before
    deciding whether a finding is worth sending. Rewritten for the rolling
    patch cadence RustyNES actually ships on, and it now names the two
    boundaries that change what a report *means* rather than merely how old it
    is: v2.0.0 "Timebase" (a pre-v2.0.0 `.rns`/`.rnm` is refused with a clear
    error rather than reinterpreted, so such a parsing report is not
    reproducible against a current build **by design**, ADR 0028) and v2.2.9
    (the GPL-3.0-or-later relicence is a licensing correction, not a SemVer
    break, ADR 0036).
  - **The root `ARCHITECTURE.md` presented the retired dot-lockstep scheduler as
    the current design** — `tick_one_dot`, the `% 3` phase test, and the claim
    that the bus need not re-sync the PPU "because they were already advanced in
    lockstep above". `docs/architecture.md` and `docs/scheduler.md` each carry a
    v2.0.0 banner correcting exactly this and label their historical
    subsections; the root companion had neither, so the one architecture
    document a newcomer opens first was the one describing a scheduler that has
    not existed since 2026-07-03. It now carries the same banner and the same
    labels, and states the part that did **not** change: lockstep was chosen so
    that a mid-instruction PPU event is visible to the rest of the instruction
    without a per-quirk patch, and that consequence survives the mechanism
    moving. This project has been bitten before by prose asserting an intent the
    code does not implement; a spec describing the previous implementation is
    the same failure with a longer fuse.

- **The user guide covers the tools that shipped since it was last touched.**
  `docs/user-guide/analysis-tools.md` documented three tools and the menu had
  five: **Divergence Lens** (v2.3.8) and **Audio Provenance** (v2.3.7) were
  reachable in the UI and absent from the guide, and the menu reference listed
  neither. Both are added, along with v2.3.9's **RAM Atlas → RAM Watch** export
  — including why the lens travels with the address (liveness is relative to
  what was observed, so an unqualified "LIVE" in a watch list is a claim nobody
  can check) and why the cheat, Lua and RetroAchievements exports are
  deliberately still absent (a cheat is a **write**, so it needs a
  locked-session predicate the watch export correctly does without).

## [2.3.8] - 2026-08-20 - "Parallax" (which pixels differ, not just which frame)

Cut from its own boundary commit (#407's merge) rather than from `main`, so its
artifacts contain exactly the Divergence Lens. See the `v2.3.8` tag.

### Added

- **The Divergence Lens — which pixels differ, not just which frame, and why.**
  (#407, the whole of v2.3.8 "Parallax".) Surfaced as a panel under **Tools →
  Analysis**, over a headless `rustynes_probe::divergence` core that is tested
  independently of it. `Probe` could already say whether two
  configurations of the same ROM diverge and at which frame, because a trial
  reduces each frame to one `u64`. That reduction is the right shape for
  *detecting* a difference and the wrong shape for *explaining* one: a hash says
  frame 412 differs and cannot say which pixel, so it has nothing to hand to
  Pixel Provenance, which is where an answer actually lives.

  `divergence::localise` re-runs both configurations to the detected frame,
  keeps the full output instead of its hash, and reports the *shape* of the
  difference — population count, first pixel in raster order, and the inclusive
  bounding box. Count and box separate kinds of bug from each other: one pixel is
  a sprite or a palette entry, 256 in a row is a scanline, tens of thousands is a
  scroll or a mode change.

  It localises on the **index** framebuffer — 256x240 `u16`s of
  `(emphasis << 6) | colour`, the PPU's own per-pixel output before the palette
  lookup — which is half the bytes and at least as sensitive, since the RGBA
  buffer is a pure function of it given the same palette.

  Three answers, and the third is the point: `Identical`, `Differs`, and
  **`Inconclusive`** for an exhausted budget or two trials that cannot be
  compared. The Latency Oracle's precedent applies directly: "I stopped looking"
  must not arrive wearing the same shape as "they agree". The budget is checked
  up front for all four trials, so spending two on detection and then finding the
  localisation pair unaffordable cannot consume the budget that would have
  answered the question.

  Beyond locating a difference, the Lens **explains** it. Trial-scoped
  provenance capture lets a located pixel be handed to the machinery that already
  answers "what wrote this, and from which instruction", so the answer is a cause
  rather than a coordinate — and it closes v2.3.8 item B without bisection. An
  **audio** lens resolves a divergence to the CPU cycle, the cadence at which the
  mix is genuinely computed.

  One defect was found and fixed inside the same work: the Lens left the emulator
  **thirty frames ahead** of where it started. A trial restores the anchor on the
  way in and not on the way out, which is deliberate — it is what lets the Lens
  read the trial's final frame off `nes` directly — but the outermost caller has
  to put the timeline back, and did not.

## [2.3.7] - 2026-08-19 - "Overtone" (the instruction behind every mixed cycle)

An *overtone* is the structure inside a sound that a single pitch reading throws
away, and that is what this release adds: the Audio Scope already showed the
waveform and the Audio Mixer already set the gains, but nothing linked a sample
back to the instruction that caused it. **Audio Provenance** closes that — a
per-register write attribution answering *what wrote this, and from which
instruction*, and a per-CPU-cycle mix trace answering *what were the channels
actually doing*, deliberately shaped as the APU counterpart of Pixel Provenance.

The release's real subject, though, is the trap the feature inherited. Pixel
Provenance shipped **non-functional for four releases** because run-ahead's
rollback cleared its store before any UI could read it, while a comment two lines
above the clear asserted the opposite. Audio Provenance rides the identical
rollback, so the carry landed in the **same change as the feature** rather than
after a bug report. Then the same defect turned up in **three more places** —
every restore in `rustynes-probe` — which meant running the Latency Oracle or the
RAM Atlas silently emptied both provenance panels. The v2.3.6 fix had enumerated
one caller rather than the mechanism, and the test named for the contract could
not see the breach because provenance is deliberately not in the save state.

Two defects were caught by measurement rather than by reading. `apu_throughput`,
built for this release, reshaped the plumbing **three times** on regressions
invisible in the diff; and a fuzz sweep of the save-state parse boundary found
**four** panics in `VRC7`'s OPLL where hand-tracing had found one — the
maximally-hostile fixed payload concealed one of them.

Also fixed: `$4014` and `$4016` were documented as attributed and were not, since
the bus handles them without routing through `Apu::write_register`; the browser
demo applied **no** per-game header corrections; *Rad Racer*'s roadside artifact,
where the PPU spliced a hybrid address from a stale `v`; VRC7 save states dropped
the live FM synthesizer, so rewind garbled the music; and no CI job carried a
timeout, so one hung job silently skipped a release for five hours.

`rustynes-apu` and `rustynes-core` both change, so **AccuracyCoin 141/141
(100.00%, RAM decoder) and nestest 0-diff are VERIFIED, not asserted.**

### Added

- **Audio provenance — point at a moment in the frame and read why it sounds
  like that.** The APU counterpart of pixel provenance, and deliberately the
  same shape: a per-register write attribution answering *what wrote this, and
  from which instruction*, and a per-CPU-cycle mix trace answering *what were
  the channels actually doing*. Surfaced at **Tools → Audio → Audio
  Provenance**. Output-only, runtime-default-off, and not serialized, so the
  deterministic audio contract is unaffected whether it is armed or not.

  Every ingredient but one already shipped — the Audio Scope plots the
  waveforms, the Audio Mixer sets the gains, `Apu::pulse1_out()` and its
  siblings expose live channel outputs, and the Event Viewer already classifies
  `$4000-$4017` writes as `EventKind::ApuWrite`. What existed nowhere is the
  link from a sample back to the instruction that caused it: `EventRec` carries
  `kind / scanline / dot / addr / value` — no PC, no CPU cycle — and is
  scanline-oriented rather than sample-oriented. So the event log is the
  interception *point* this reuses; it is not the record.

  The trace is per **CPU cycle**, the cadence at which the mix is genuinely
  computed, rather than per output sample. `blip` decimates to 44.1 kHz — about
  one sample per 40.6 CPU cycles — and an output sample is a weighted sum of
  transitions across the filter kernel, not a copy of one instant. Recording at
  output rate would mean picking which of those ~40 mixes "is" the sample, which
  the signal chain cannot answer; the panel reports the cycle window and says so
  instead. `MIX_CAP` is sized from **Dendy** (35,464 cycles/frame), not the NTSC
  figure that comes to mind first, and reports `truncated()` rather than
  returning a short buffer that looks complete.

  Register rows carry their **side-band effects**, because naming the right
  instruction and then describing the wrong effect is its own failure: a write
  to `$4003` does not merely set the period, it also loads the length counter,
  resets the duty sequencer and restarts the envelope. Those annotations were
  confirmed against this emulator's own implementation, not from memory.

  **The trap this feature inherited was closed in the same change as the
  feature, not after a bug report.** Pixel provenance shipped non-functional for
  four releases because run-ahead's per-frame rollback cleared the store before
  the frontend released the emulator lock, so the UI could never observe a
  populated record — and a comment asserted the opposite, which is what stopped
  anyone checking. Audio provenance rides the identical rollback, so
  `take_audio_provenance` / `put_audio_provenance` carry the state around
  `restore_quiet` in `RunAhead::finish` from the outset. Save-state loads and
  netplay rollback still clear, unchanged: those are genuine timeline changes,
  and run-ahead's is not. The regression test drives the real produce path at
  `run_ahead = 1` — the default — and is mutation-checked. Both it and its
  control are floored at 20,000 records rather than "non-empty", because the
  APU's reset sequence alone produces eight, so a non-emptiness check would pass
  on a run that emulated nothing.

  Spec: `docs/audio-provenance.md`. `rustynes-apu` gains a `debug-hooks`
  feature, forwarded from the core's.

### Changed

- **Audio provenance costs the shipped default nothing when it is not armed.**
  The feature is compiled into every build (the frontend enables the core's
  `debug-hooks` unconditionally), so "default-off" describes the runtime arm
  rather than the code. Two separate mechanisms were found charging the APU
  hot path while disarmed — the mix record was being built before the arm was
  tested, and the recording body was being inlined into the hot mix path — and
  both are fixed; the disarmed path measures at baseline. The full measurement
  chronology, including a diagnosis that was made, measured and rejected, is in
  `docs/performance.md` §v2.3.7 C2 rather than here.

### Fixed

- **The Latency Oracle and the RAM Atlas no longer empty the provenance
  panels.** Both drive the emulator and then put the timeline back, and
  `Nes::restore_inner` clears the pixel- and audio-provenance stores — correctly
  for a genuine timeline change, wrongly for a restore of the state the user is
  still looking at. `rustynes-probe` had three such restores and none of them
  used the `take_provenance` / `put_provenance` stash that v2.3.6 added for
  exactly this: `Probe::run_uncounted` (once per trial, and a latency
  measurement runs up to 21 of them), `latency::measure_in_place` (the final
  restore, which sits outside every per-trial guard), and the RAM Atlas panel's
  `TimelineGuard`. Both stores are **cumulative** — "which instruction last
  wrote this" can point thousands of frames back, to a palette byte from level
  load or a `$4008` reload from init — so the records were not rebuilt by the
  next frame; they were gone for the session.

  This is the defect class v2.3.6 was written about, found in three more places.
  The v2.3.6 fix was correct at the call site the bug report named and stopped
  there, and `docs/pixel-provenance.md` then described run-ahead as "the one
  caller that needs the exception" — a correct rule with an incomplete
  enumeration under it. Closed by moving the stash into
  `rustynes_probe::TrialGuard`, the guard that already carried rewind capture
  across a trial for the same underlying reason: state that lives outside the
  save state is not carried by a snapshot round trip.

  Pinned by the **full 2x2 matrix** — each of the two stores against each of the
  two probe restores — under four independent mutations, so a fix that put back
  only one store, or guarded only one of the two restores, fails. The fourth cell
  (`measure_in_place` against the *pixel* store) was **missing until review
  caught it**, and it was not a rounding error: the `measure_in_place` mutation
  fails only the audio test, so a final restore that put back the audio stash and
  dropped the pixel one would have passed everything. The claim of "four tests"
  was written before the fourth existed. The existing
  `measure_in_place_restores_the_live_timeline` could not have caught it: it
  compares `nes.snapshot()` before and after, and provenance is deliberately not
  in the snapshot, so it asserted something strictly weaker than the contract it
  is named for. `rustynes-probe` gains a `debug-hooks` passthrough feature,
  without which the guard would have compiled out in precisely the builds that
  need it.

- **VRC7 save states now carry the FM synthesizer, so rewind no longer garbles
  the music.** `Vrc7::save_state` wrote the *shadow* OPLL register bytes and
  never the live synthesizer — not `opll`, not `opll_clock_counter`, not
  `last_opll_sample` — and `load_state` never replayed them either. After a
  rewind, a netplay rollback, or a TAS/save-state restore the FM voice therefore
  resumed from whatever envelope and phase state it happened to be holding.
  Banking, IRQ, mirroring and PRG-RAM had always round-tripped correctly; this
  was audio-only, and only on mapper 85. Recorded as an open frontier in
  `docs/accuracy-ledger.md` since v2.2.3, closed now.

  `rustynes_apu::Opll` gains a `snapshot` / `restore` pair carrying the register
  shadow, the EG and LFO counters, the per-channel patch selection, all 18
  operator slots (phase accumulators, envelope state machines, feedback history)
  and the per-channel outputs. The lookup tables and the chip's patch ROM are
  deliberately not carried — they are constants of construction, and restoring
  them would be restoring a copy of the binary into itself. The chip type rides
  along only as a tag, so a YM2413 blob restored into a VRC7 is rejected rather
  than silently reinterpreting every slot patch against the wrong instrument set.

  The VRC7 mapper section is now **v2**, appending that blob after the VRAM.
  It is additive: a v1 blob still loads and leaves the synthesizer exactly where
  the old build left it, so an old save is no worse than it always was rather
  than newly silent. A build without `mapper-audio` has no synthesizer to
  describe, so it still writes v1 and validates-then-ignores a v2 tail — which
  preserves the cross-feature save portability ADR 0004 promises, and is why the
  version byte is build-dependent rather than unconditionally 2.

  The repair a reader will think of first was rejected on the merits: replaying
  the register shadow through `Opll::write_reg` on load needs no new format, but
  restarts every keyed-on channel's envelope at attack, so every rewind frame
  would produce an audible transient.

  The regression net keys a note, advances 20,000 CPU cycles, saves, and then
  compares 4,000 mixed samples from the source against 4,000 from a **fresh**
  mapper restored from the blob — equal sample for sample. It is
  mutation-checked: making the tail carry a *reset* synthesizer reproduces the
  pre-fix failure exactly. `Opll` is also now registered in
  `snapshot_schema_audit.rs`, the standing field-vs-schema audit, which had
  never been able to see this surface — a save-state surface no audit can see is
  precisely how a gap this size survives for four releases.

  Emulation output is unchanged (nothing on the synthesis path moved), and the
  accuracy contract was verified rather than assumed: AccuracyCoin **141/141**
  via the authoritative RAM decoder, nestest 0-diff.

  Review caught a defect in the fix itself, worth recording because of *why* no
  test could have. The accept check read `version != 1 && version !=
  VRC7_SECTION_VERSION`, and that constant is **1** on a `mapper-audio`-off
  build — so the condition collapsed to "v1 only" there and a no-audio build
  **rejected** every v2 blob, the exact opposite of the portability the constant's
  own doc comment claimed. What a build can *write* and what it must *accept* are
  different sets, and only the first varies by feature; deriving one from the
  other reads as tidy and silently couples them. The check now compares against
  literals.

  The default build takes the other branch and was correct throughout, which is
  why every gate stayed green: CI **linted** the `--no-default-features` shape
  and never **ran** it. `cargo test -p rustynes-mappers --no-default-features` is
  now a CI step, and the new regression test is mutation-checked in both
  configurations — red on no-audio with the old condition, green on the default
  build either way.

  A second review pass found two more, both in the new code and both of a kind
  the tests as written could not see. **A hand-edited save state could crash the
  emulator**: `commit_slot_update` indexes the TLL table as
  `[block_fnum][tl][kl]` with dimensions `[128][64][4]`, and `restore` was
  handing it raw bytes — a `tl` of 255 computes an index of 524,539 into a
  32,768-entry table. Every register field is now masked to its hardware width
  at the parse boundary, which is what those fields physically are. The test that
  proves it is careful about one thing: an all-`0xFF` blob is rejected by the
  envelope-state tag check before any numeric field is read, so the naive hostile
  input passes **by accident** and reports the emulator safe. The interesting
  input is the one that satisfies every explicit check and is still nonsense. A
  later review pass pushed back that the masking did not in fact cover every
  field, and was right: replacing the single fixed payload with a deterministic
  pseudo-random sweep found **three more panics** the fixed one could not,
  including one it actively hid — with every byte `0xFF`, `update_requests` is
  also all-ones, so the slot state was recomputed before the restored values
  could be used. A blob that is maximally hostile in one dimension can be
  harmless in another. The three: `eg_shift` used as a shift amount (`1u32 <<`
  panics at 32), the operator feedback pair summed as two arbitrary `i32`s, and
  `eg_rate_l` indexing a 4-entry table — the last being a field I had explicitly
  traced as safe, using a broken grep whose empty output I read as proof.

  And **`load_state` was not atomic**. The v2 tail introduced a failure that can
  occur *after* the core fields are assigned, which the v1 layout could not, so a
  truncated tail returned `Err` with the banking, IRQ state and 2 KiB of VRAM
  already overwritten — a mapper left in neither its old state nor its new one
  while the caller reported failure and kept running. `Opll::restore` was already
  atomic internally, which is exactly what made it easy to miss: the guarantee
  existed one level down and was silently discarded one level up. It now parses
  into a staged value before the first write. The truncation test asserted only
  on the return value, which is why review found this and the test did not; it
  now asserts the target is byte-identical afterwards.

- **Corrected a stale comment in `security.yml`.** It justified installing
  `cargo-audit` / `cargo-deny` as prebuilt binaries with "the repo pins rustc
  1.96 **but** cargo-audit needs >= 1.88 to compile" — which argues against
  itself, since 1.96 satisfies that. True when written at a 1.86 pin; it
  survived the v1.3.0 bump. Comments only; no behaviour change.

- **The browser demo applied no per-game header corrections.** Every mapper,
  submapper and region fix the vendored game database ships was silently absent
  on the web build — Seicross, which needs submapper 4 to clear its protection
  loop, hung there exactly as it hung on the CLI before v2.3.4.

  The mechanism is the interesting part, because this is the **third** time the
  same correction has been skipped by a load path that does not go through the
  File-menu chokepoint: the CLI (fixed v2.3.4), the mapper-coverage harness
  (fixed v2.3.4), and now the browser. `apply_load_time_header_overrides` has two
  stages — the compiled-in game database, then the per-game `<rom>.json` overlay.
  Only the *second* needs a filesystem, but the whole function was `cfg`-gated
  off wasm on its account, so the first went down with it. **A `cfg` gate
  inherited from the strictest of several stages is a gate on the whole feature,
  and nothing tells you which stages did not need it.**

  The database stage is now its own function, ungated, called from both browser
  ROM entry points (the `wasm-winit` demo's `AppEvent::RomLoaded` and the
  `wasm-canvas` embed's file picker). The overlay stage stays native-only, which
  is correct rather than a remaining gap: a browser has no `<rom>.json` to find.

  Found by the audit v2.3.6 opened after Pixel Provenance — *look for shipped
  features whose core logic is tested and whose frontend wiring is not.* And, as
  with Pixel Provenance, a comment asserted the opposite of the code: the wasm
  path was documented as one that "preprocesses separately" when it preprocessed
  nothing at all. That sentence is corrected in place, quoted, rather than
  quietly deleted.

  Pinned three ways: the browser stage must produce byte-identical output to the
  full native helper when no overlay exists; a premise test proves the correction
  is observable at all, so the agreement test cannot pass vacuously (which is
  precisely the state the browser was in); and a source-text assertion requires
  every wasm ROM entry point to call it — mutation-checked — because those call
  sites live in `cfg`-gated code a native test binary cannot link, so an absent
  call is the one thing behaviour can never catch. That third test was itself
  **vacuous on first writing**, and review caught it: it lives in `app.rs`, so
  `include_str!("app.rs")` pulled in the test's own source — which contains the
  literal it searches for, making the assertion permanently true. It survived a
  mutation check only because the check deleted the *other* file's call. A file
  that reads itself has to exclude the part doing the reading; it now truncates
  at the test module, asserts that the truncation worked, and is
  mutation-checked on both halves.

- **Rad Racer's roadside artifact — the PPU spliced a hybrid address from a
  stale address bus.** A band of stray pixels flickered in the sand to the right
  of the road, tracking the horizon, on 1639 of the 1841 frames of the movie the
  maintainer recorded. It was reported as unfixed after v2.3.0 examined the same
  area and concluded the model was correct.

  Pixel Provenance answered it in one query: the stray pixels reported
  `layer = Backdrop`, `pattern_addr = PATTERN_ADDR_NONE`, `palette = $3F00`.
  They were not sprites or mis-fetched tiles — they were **holes**, pixels for
  which no background tile had been fetched at all.

  When `$2006` is written mid-render the PPU drives a hybrid address: the low
  bits come from the octal latch (a real 74LS373 that holds the previous ALE
  half), the high six from the address bus. `ale_splice` took those high six from
  `self.address_bus` — the value latched at ALE time — rather than recomputing
  them from the **live** `v` at the read dot. The NESdev wiki is explicit that
  the bus is driven every PPU cycle and that the nametable fetch's upper bits
  follow `v`; a game that times a split early therefore reads its tile from an
  address the hardware never presents. Rad Racer times exactly that way, and the
  wiki names the symptom: "a visible glitch at the end of the line".

  The one-line fix splices from the recomputed intended address instead. Note
  what it is **not**: `COPY_V_DELAY` is unchanged at 4. Removing the deferral
  entirely was tried and **fails** AccuracyCoin's `Hybrid Addresses` test —
  `$2006` is applied at the start of a CPU cycle, before its three PPU ticks, so
  an immediate copy lets the next ALE re-drive coherently and hides the bug the
  test looks for. A delay sweep across 1..6 confirms the artifact count is flat
  (1078 / 1072 / 1077 / 1072 / 1080 / 1065), so the residual is the detector's
  floor — real roadside objects — and not remaining signal.

  This touches the core, so the contract is **verified, not asserted**:
  AccuracyCoin **141/141** via the authoritative RAM decoder, nestest 0-diff.
  The bundled AccuracyCoin ROM is also synced to upstream `7dc08e5`, whose own
  source comment was rewritten from "since we are updating `v` this cycle, we
  update the address bus" to "the address bus is updated **every ppu cycle** …
  the upper 6 bits for the nametable fetch are based on the `v` register" —
  independent confirmation of the same reading, from the author of both the test
  ROM and the emulator whose timing this was once calibrated against.

  **One committed visual-regression vector moves with it.**
  `scanline_frame_180` hashes `scanline.nes`, described in its own test as a
  mid-frame scanline-effect demo exercising mid-scanline scroll timing — the
  exact `$2006`-during-render path this changes. Its hash goes
  `39e8052eedc7f4d5` → `7c1cedf0cb725375`, deterministic across independent
  runs, and it is the **only one of nine** vectors to move. The other eight are
  byte-identical, named exactly as the tests are so the entry can be correlated
  with the artifacts: `flowing_palette_frame_60` / `_180` / `_300`,
  `full_palette_frame_60` / `_180`, `ppu_vbl_nmi_basics_frame_60`,
  `instr_test_basics_frame_60` and `nmi_sync_demo_ntsc_frame_180`. That
  distribution is what distinguishes a targeted fix from a broad rendering
  shift.

  Updating a canonical vector is permitted only on an intentional, reviewed
  behaviour change, and this is one — pinned by AccuracyCoin's `Hybrid Addresses`
  test, which covers this precise mechanism, at 141/141. `scanline.nes` has no
  pass/fail protocol, so its hash is a *sentinel* rather than an oracle; it did
  its job by flagging that output changed.

- **CI jobs are bounded, so a hung job can no longer block a release.** No job
  in `ci.yml` carried a `timeout-minutes`, which means every one inherited
  GitHub's **six-hour** default. On the night of the v2.3.6 cut the `lint` job —
  normally four minutes — hung on `main` (2026-08-17 21:11 UTC). Because `main`
  runs deliberately do not cancel each other, the v2.3.6 release commit queued
  behind it and never started; GitHub
  keeps only one pending run per concurrency group, so the commit between them
  was cancelled outright; `Auto Release` fired on *that* cancellation, saw a
  non-success conclusion, and correctly skipped.

  Every PR was green. The release simply never happened, and nothing reported an
  error anywhere — the failure presented as a workflow that had quietly decided
  not to run. Every job now declares an explicit budget with its observed
  duration recorded beside it: a 20-minute floor for jobs that finish in seconds
  (a fixed ratio would put those under a minute, where startup and a cold cache
  trip them for nothing) and roughly 2-3x for the jobs long enough for a ratio to
  mean anything. It was the second hung job that night; the first cost two hours
  on a PR.

  **That fix covered `ci.yml` only, and the gap was found the way the first one
  was — by being blocked.** During this release's own cut, `Clippy Security
  Lints` hung for over two hours in a setup step, on a job whose observed runtime
  is two to three minutes, holding the release PR. `security.yml` had no
  `timeout-minutes` on any of its three jobs, and a sweep found five more
  unbounded workflows: `android.yml`, `ios.yml`, `web.yml`,
  `antigravity-review.yml`, and `release-auto.yml` — the release workflow itself.
  All are now bounded, so the sweep across `.github/workflows/` comes back empty.

  Two details worth keeping. `release-auto.yml`'s `build` job **cannot** carry a
  timeout, because `timeout-minutes` is not valid on a job that uses `uses:`; its
  budget lives on the jobs inside `release.yml`, which already had them. (Review
  challenged this, claiming the restriction was lifted in 2022. It was not —
  checked against the schema with `actionlint`, which reports the key as
  unavailable and lists the seven that are allowed. Adding one is a syntax error,
  not an ignored key.) And
  `antigravity-review.yml` is bounded *harder* than the hosted jobs rather than
  softer, because it runs on the maintainer's own hardware, where a hung run
  holds a real machine instead of a disposable VM.

## [2.3.6] - 2026-08-17 - "Sounding" (measuring, and what a measurement may claim)

A *sounding* is a depth measured with its uncertainty attached, and that is what
every workstream here has in common. Two shipped features are found not to work at
all; two new tools are added that decline to answer rather than guess; and an
optimization campaign is closed on the strength of three measured rejections.

### The release in one line each

- **Pixel Provenance never worked, in any release since v2.3.2** — and two comments
  plus four doc claims asserted the opposite of their own code, which is why nobody
  checked.
- **Duck Hunt could never score.** The Zapper light probe was exactly inverted
  against the protocol the game uses.
- **The Latency Oracle** measures the game's own input lag instead of leaving it to
  a manual frame-advance ritual — and recommends a run-ahead depth without ever
  applying one.
- **The RAM Atlas** classifies all 2 KiB of work RAM, then verifies a candidate by
  perturbing it — the step that separates causation from coincidence.
- **The Tools and Debug menus** are regrouped by task; Tools had reached twenty flat
  entries.
- **APU Workstream D is closed**, on three measured rejections and the mechanism
  that explains them.

### Added

- **Latency Oracle** (`Tools → Analysis`, spec `docs/latency-oracle.md`). Replays
  the current moment twice — once with a probe button held, once with nothing
  pressed — and reports the first frame that differs. That index *is* the game's
  internal lag, because on a deterministic core two replays of identical state can
  differ for exactly one reason.

  It is built to decline rather than guess. `frames` is an `Option`, and `None` and
  `Some(0)` are different answers that are never collapsed: `Some(0)` means the game
  reacted on the next frame, `None` means the probe could not tell. It probes six
  buttons across three observables (framebuffer, then audio, then work RAM) and
  requires agreement; `START` is deliberately excluded, because it pauses many games
  — a reaction to a menu, not to gameplay, and counting it would over-report.

  **It recommends; it never applies.** Run-ahead is linear in the core's frame cost,
  so silently raising it can push a marginal host into dropped frames for a change
  the user never asked for. The depth appears with an explicit Apply button, and a
  test fails if storing a report ever queues a config write on its own.

- **RAM Atlas** (`Tools → Analysis`, spec `docs/ram-atlas.md`). Answers what each
  byte of work RAM is *for*, in two stages with deliberately different confidence.
  Observation classifies every address (untouched / frame tick / rising / falling /
  sparse / volatile) and is **correlation only** — `classify` returns all 2048 labels
  as `Untested`, so observation is structurally incapable of claiming liveness.
  Verification pokes the byte, re-simulates from the same anchor, and compares.

  Liveness is relative to its lens, and every verdict names the one it used: the same
  byte is routinely `Live` through work RAM and `Inert` through the framebuffer.
  `Untested` is a third state, distinct from `Inert`, because "we did not look" and
  "we looked and saw nothing" are different claims. `Inert` is documented as *not*
  meaning unused — a byte the game rewrites from a master copy each frame reads inert
  because the poke is overwritten.

- **`rustynes-probe`**, the deterministic re-simulation engine both tools consume:
  anchor, replay under controlled variation, locate the first divergence. Trials are
  budgeted, and the budget is binding rather than advisory.

- **`rustynes verify <movie.rnm> --rom <rom>`** attestation tests, closing the last
  item of #360.

- **Docs**: `docs/ram-atlas.md`, `docs/latency-oracle.md`, and
  `docs/user-guide/analysis-tools.md`. `docs/pixel-provenance.md` was also added to
  the docs-site nav, having been built but unreachable since v2.3.2.

### Fixed

- **Pixel Provenance now works.** The v2.3.2 "Lucid" marquee returned an empty
  report for effectively every user, from release until now, because of two
  independent defects.

  **Run-ahead erased the record before the UI could read it.** Run-ahead defaults
  to 1, and its per-frame rollback (`RunAhead::finish` → `Nes::restore_quiet`)
  unconditionally cleared both provenance stores. That clear is right for a
  save-state load and for netplay rollback, and wrong here for a reason that has
  nothing to do with the restore: run-ahead's rollback is the *last* thing before
  the frontend releases the emulator lock, so the panel's first opportunity to
  look was always after the wipe. It did not discard a stale timeline; it
  discarded the record for the frame on screen. `finish` now carries both stores
  **around** the restore (`Nes::take_provenance` / `put_provenance` — a move of
  two boxed stores, skipped when neither is armed), keeping exactly the visible
  frame's records. Every other caller still clears, unchanged.

  **Clicking a pixel was never implemented.** The panel offered two coordinate
  spinboxes and no click hit-test, while the docs and release notes said "point
  at"/"pin" a pixel. Clicking the game view now pins that pixel. The NES image is
  a raw wgpu blit rather than an egui widget, so the click is captured in the
  winit handler and converted by a new `gfx::window_to_nes_pixel`, which inverts
  the blit's own letterbox/crop transform — correct at any window size, pixel
  aspect and overscan crop, and `None` on a letterbox bar.

  Also fixed while here: the panel mirrored the core's armed flags in frontend
  state, which desynced permanently the moment a ROM load installed a fresh
  `Nes` (checkbox ticked, core unarmed, no way back but unticking and re-ticking)
  — the core is now the single source of truth; and the panel rendered a cleared
  record as fact, because every field of one reads as a confident "scanline 0,
  dot 0, backdrop, palette `$0000`". It now distinguishes not-armed from
  nothing-recorded-yet from off-screen.

  **Why it went unnoticed:** the core data structures were well unit-tested and
  the frontend wiring was tested by nothing — the same shape as issue #360 in the
  same release train. `runahead.rs` even carried tests pinning the determinism of
  the very code path that destroyed this telemetry. The new regression net drives
  the run-ahead cycle with provenance armed and asserts a record survives, with a
  plain-run control so a failure cannot be misread as a bad assertion, plus three
  tests for the coordinate converter — one round-tripping it against the shader's
  own uniform rather than a third re-derivation of the letterbox.

  Two comments and four documentation claims asserted the opposite of the code
  and are corrected in the same change, including one in `CHANGELOG-FULL.md`'s
  spec (`docs/pixel-provenance.md`) that contradicted itself two sections apart.

  Emulation is untouched: the new core methods are additive and output-only, so
  **AccuracyCoin holds at exactly 141/141** (RAM decoder) with nestest 0-diff —
  verified, not asserted.

- **Duck Hunt is playable: a Zapper shot can finally score.** The gun fired and
  nothing could ever be hit — at any aim point, in any part of a duck.

  Duck Hunt's protocol is "the gun must see **nothing** for one frame, then a
  bright spot in the next". `Bus::sample_zapper_light()` runs at the *end* of
  `run_frame`, so the light bit a read returns during frame N was sampled from
  frame N−1. The game therefore received its probe **exactly inverted**: on the
  blanked frame it read the previous, bright frame; on the target frame it read
  the blanked one. The shot was discarded before hit-testing, which is why aiming
  made no difference.

  The **beam-relative light model is now the default** (`zapper_temporal_light`,
  opt-in since v2.2.3). It derives the light bit from where the CRT beam is at
  the moment of the read — dark before the beam paints the aim row, lit for the
  ~19-26-scanline photodiode hold, dark once drained — which is what the hardware
  does and what the frame model structurally cannot express.

  A second defect had to go with it: the beam-relative sampler read aperture rows
  the beam had **not finished painting**, which still hold the previous frame, so
  it asserted light on an all-black screen. Measured directly — at scanline 96
  the beam was 5 dots into row 96 and the sampler saw the previous frame's sky at
  luma 152 on a frame whose mean luma was 0. Rows at or after the current
  scanline are now excluded (`aperture_is_bright_painted`).

  **The reason it shipped that way was a wrong claim, not a missing oracle.**
  v2.2.3 kept the model off because "no pass/fail light-gun test ROM exists… the
  supported titles are satisfied by either model". The second half was false, and
  the first was beside the point: the game is the oracle. Measured A/B on the same
  ROM, aim and inputs — frame model: score 000000, duck still flying;
  beam-relative: score 000500, duck marked hit. Pinned by
  `duck_hunt_zapper_shot_can_score`, which asserts Duck Hunt's own scoreboard and
  was mutation-checked (with the model forced off it fails on identical score
  pixels). New `zapper_light_probe` diagnostic reproduces the whole sequence from
  the game's `$4017` traffic.

  This changes emulation behaviour when a Zapper is attached, so the gates were
  re-run rather than assumed: **AccuracyCoin 141/141** (RAM decoder), nestest
  0-diff, 2,038 workspace tests green. Pass `set_zapper_temporal_light(false)` to
  restore the pre-v2.3.6 model.

- **The Zapper's aim was off by the letterbox.** Its cursor mapping stretched the
  256×240 image across the whole window, so the aim was wrong by the bar size
  whenever the window did not match the NES aspect, and a click on a black bar
  registered as a hit on a real pixel — while the comment directly above it
  claimed "letterbox bars read as off-screen — the correct Zapper 'no light'
  behavior", which a full-window stretch cannot produce. It now shares
  `gfx::window_to_nes_pixel` with the provenance picker, so bars are genuinely
  dark and the aim tracks the pixel actually under the cursor at any window size,
  pixel-aspect setting or overscan crop. The Input Display's on-screen indicator
  uses the same converter, so the HUD and the core agree. The Vaus paddle keeps
  its full-window sweep deliberately: a knob has no off-screen state, and how far
  the hand travels per turn is a feel decision no oracle adjudicates.

- **The libretro `.info` description is corrected.** It now advertises native
  `RETRO_ENVIRONMENT_SET_MEMORY_MAPS` support and native Game Genie cheats —
  both long-standing capabilities that the description omitted — plus the two
  v2.3.5 additions, region-correct NTSC/PAL/Dendy timing and NES Zapper support.
  "Written entirely in safe Rust" becomes "written in pure Rust, with an
  unsafe-free `#![no_std]` emulation core": the chip stack is `unsafe`-free, but
  the libretro wrapper is an FFI boundary and is not, so the scoped claim is both
  accurate and more informative. This is the file RetroArch's core-information
  screen displays.

- **A probe trial no longer clears or pollutes the caller's rewind ring.** Two
  defects at the one site every trial shares. The first is a bug an earlier fix in
  this same release reported as closed and did not close: trials restored their
  anchor with the loud `Nes::restore`, which clears the rewind ring, and
  `latency::measure_in_place` runs up to 21 trials against the live emulator — so
  asking how much input lag a game has destroyed the user's rewind history,
  twenty-one times over. The earlier fix had changed only the final restore, not the
  per-trial one. With the wipe fixed a second defect became visible: the ring then
  *grows*, because trial frames are captured like any others, and those frames are
  re-simulated and never happened on the user's timeline. Both fixed; the test
  asserts the ring returns exactly as it was, since a weaker "not cleared"
  assertion is what let the incomplete fix pass review.

  Separately pinned: a trial's samples do not change when the caller has rewind
  armed, so **no measurement taken before this fix needed re-running**. The engine's
  premise is that a replay from one anchor is bit-identical, so anything that
  silently perturbed state would have invalidated the primitive rather than one
  measurement.

- **The audio observable was structurally dead.** `Observable::AudioEnergy` never
  saw any audio: the probe's trial loop emptied its buffer and never filled it, so
  the energy reduction summed an empty slice and every frame of every trial
  reported zero. Nothing failed, because a lens that returns a constant never
  disagrees with itself — so the Latency Oracle's audio fallback stage, the one
  that exists for a game whose reaction is audible before it is visible, silently
  degraded to work RAM, and the RAM Atlas's audio lens would have reported **every
  address inert**. The comment above the missing call said "Drain EVERY frame,
  whatever the observable" and explained at length why. Found in review on #392.

- **The RAM Atlas is unavailable during locked sessions.** Both of its actions
  advance the live emulator and Verify pokes work RAM, so it is now gated on the
  same `writes_locked || hardcore_blocked` predicate `emu.write` uses — netplay, a
  TAS record or replay, and RetroAchievements hardcore. Under netplay or a movie it
  would diverge a timeline other peers are lockstepped to; under hardcore it is the
  memory write that mode exists to forbid. The disabled state names which reason
  applies.

- **An un-perturbable address is reported `Untested`, not `Inert`.** `verify_liveness`
  skipped the poke for an address outside work RAM and then let the two identical
  trials agree, producing a confident verdict for a byte it never touched — the
  exact failure mode that module documents itself as never producing. It is public
  and takes a full `u16`, so a CPU-space mirror such as `$0810` is a plausible
  caller input.

- **The Latency Oracle's felt-latency figure is derived, not transcribed.** It
  multiplied by a hardcoded NTSC 16.639 ms, understating PAL and Dendy by 20.2% —
  the identical figure and mechanism as the v2.3.5 libretro defect, where a
  hardcoded 60.0988 fps had lost all connection to the constant it was copied from.
  It now uses the console's own `frame_duration`, captured at measurement time.

### Changed

- **Upstream libretro `.info` syncs are batched to MINOR releases.** RetroArch
  reads its copy from `libretro/libretro-super`, which nothing syncs
  automatically; the next sync is v2.4.0, so that copy reads `v2.3.5` through this
  line by decision rather than oversight. A stale `display_version` misreports a
  number — the v2.2.9 incident was a stale **licence**, misreporting the terms of
  distribution — so licence, supported extensions, and declared-capability changes
  still sync immediately. Recorded in `docs/libretro/UPSTREAM_SYNC.md`.

- **The Tools and Debug menus are regrouped by task.** Tools had reached twenty flat
  entries spanning cheats, TAS authoring, media capture, multiplayer, ROM inspection
  and provenance analysis; Debug listed "CPU" and "Lua Script" as peers in a
  fifteen-item column. Tools becomes Cheats at the top level, then Movies &
  Recording, Audio, Input, Game Data, Analysis and HD Pack, with Netplay and
  RetroAchievements below a separator — they change what the *session* is rather
  than being tools pointed at the game. Debug splits into Chip State, Memory and
  Execution, plus Symbols. Emulation's two FDS entries become one Famicom Disk
  System submenu.

  No entry is removed, none changes what it dispatches, and nothing moves between
  top-level menus — only the depth at which it sits. The movie transport's gating
  changes shape but not effect: the ROM/netplay condition moves from deciding
  whether the submenu can *open* to per-item enabling, so the reachable set is
  identical but the user can see which entries are unavailable instead of facing one
  opaque disabled label. `docs/user-guide/menus.md` is corrected, and was already
  stale before this release touched it — it listed five Tools entries against an
  actual twenty, and still documented a "Show Debugger" toggle removed in v1.7.1.

- **`MAX_RUN_AHEAD_DEPTH` is shared rather than redeclared.** The Latency Oracle's
  clamp was a third independent `3`; that constant exists precisely because two
  earlier caps drifted apart (PR #358).

### Documented

- **APU Workstream D is closed.** The 18.7%-of-frame-time figure stands — it is a
  correct v2.3.1 subsystem attribution, visible only because that pass attributed by
  source file, since fat LTO inlines the APU into `cpu_clock`. What is settled is
  narrower: the figure is **not recoverable by gating per-cycle bookkeeping**, the
  only strategy the workstream ever tried. One adoption (C1, shipped in v2.3.5 at
  −3.3% to −4.2%), three measured rejections (D1, D3, D6), one declined on
  inspection (D5), and two left unmeasured deliberately (D2, D4).

  The three rejections share one mechanical cause, and it generalises to the two
  remaining levers: under `lto = "fat"` with `codegen-units = 1` the guarded code is
  already inlined, its repeated loads already merged by common-subexpression
  elimination, and the elided branches always-not-taken and so perfectly predicted.
  Swapping predictable not-taken branches for an equivalent count of loads plus a
  predicate is arithmetically a wash. Stated as the rule worth keeping: *"this work
  is inert on almost every cycle" predicts a win only if the work is actually
  executed* — and under fat LTO with perfect prediction it largely is not.

  Also recorded: D1's run 1 looked like a textbook win at −3.81% (p = 0.00) on a
  shipped default workload and was **entirely an artifact** — the order-bias control,
  benching the reference against *itself*, drifted −3.73% on that same workload with
  no code change at all, because `ab_check.sh` benchmarks the reference immediately
  after a 44.9-second fat-LTO compile across all cores. Three conditions that would
  justify reopening the workstream are written down, none a variation on per-cycle
  gating. Full numbers in `docs/performance.md`.

## [2.3.5] - 2026-08-16 - "Manifest" (what the core declares about itself — and the APU, measured at last)

### Fixed

- **The libretro core metadata advertised the pre-relicense MIT/Apache-2.0
  license.** Corrected to `GPLv3+` — libretro metadata uses short tokens and
  marks "or later" with a trailing `+`, so the bare `GPLv3` carried since v2.3.0
  understated RustyNES as GPL-3.0-only. The description's mapper count is
  corrected 172 → 174.

  **This is the repo-side half only.** RetroArch reads a separate copy in
  `libretro/libretro-super` that this project does not control, so nothing in
  this release changes what an end user currently sees; the user-visible fix
  completes when that sync and the `libretro/docs` page merge upstream. Since
  RustyNES's license is itself the outcome of a corrected provenance failure,
  a frontend misreporting it is a compliance matter rather than a cosmetic one.

  A standing audit (`libretro_info_audit.rs`) now pins the `.info`'s `license`,
  `display_version` and `supported_extensions` — the last derived from the core's
  own `retro_get_system_info` declaration rather than a repeated literal — so the
  local file cannot drift and the upstream sync is a copy rather than a
  re-derivation. `docs/libretro/UPSTREAM_SYNC.md` records the full investigation,
  the token mapping and its evidence, and the surfaces that must move together.

- **PAL and Dendy games ran ~20% too fast in RetroArch.** The libretro core
  reported a hardcoded 60.0988 fps — the NTSC rate — for every cartridge, and
  never implemented `retro_get_region` at all, so RetroArch was told every game
  was NTSC on both axes at once. The emulation was never wrong: `Nes::region()`
  and `FRAME_DURATION_PAL` (19.9972 ms, 50.0070 Hz) have always been correct.
  Only what the wrapper advertised was. Both now follow the loaded cartridge, and
  the NTSC figure is derived from the core's own constant rather than
  transcribed — it reproduces the old value exactly, so nothing changes for the
  NTSC majority.

- **RetroArch's Reset did nothing.** `retro_reset` was never implemented, so it
  fell through to the library's default, which is literally a no-op. The menu
  entry and the hotkey both appeared to work and had no effect for the core's
  entire existence. Now soft-resets the console — `Nes::reset`, the RESET line,
  which preserves RAM and the CPU/PPU phase alignment; Game Genie codes survive,
  as they do on hardware through a pass-through cartridge. A Vs. cabinet resets
  both of its cross-wired consoles together.

- **Per-game state leaked across unloads.** `retro_unload_game` was also a
  default no-op. The cartridge handles were replaced on the next load, but the
  Game Genie map — keyed by the frontend's cheat *index* — survived, so indices
  from a previous game stayed live and a later removal could act on a code
  belonging to a cartridge no longer inserted.

- **The libretro controller tables were dangling stack pointers.** Found in
  review, and verified against RetroArch's handler rather than the header's
  prose, because `libretro.h` does not specify the lifetime either way:
  `SET_CONTROLLER_INFO` shallow-`memcpy`s the outer `retro_controller_info` array
  and **retains** each entry's `types` pointer, dereferencing it later when the
  Controls menu is built. Built as locals, those pointed into a stack frame that
  died when the environment call returned — a use-after-free read at menu-open
  time, on code that compiled cleanly. The tables are now `static`; the outer
  array stays a local deliberately, because it *is* copied, and that asymmetry is
  documented at the call site. The neighbouring `set_input_descriptors` call was
  checked rather than assumed, and is safe: RetroArch walks that array during the
  call and retains only `'static` string pointers.

- **The advertised display aspect assumed square pixels.** The core sent
  `aspect_ratio = 0.0`, which tells the frontend to derive the ratio from the
  pixel dimensions — 256/240 ≈ 1.067. A NES does not produce square pixels, and
  RustyNES's own desktop frontend applies 8:7, so RetroArch and the native app
  disagreed about the shape of the same frame. Now ≈1.219, and doubled for a Vs.
  cabinet's 512-wide side-by-side present, which a single fixed ratio could not
  have served.

### Added

- **An APU throughput bench** (`crates/rustynes-apu/benches/apu_throughput.rs`),
  alongside the CPU's and the PPU's. The APU is **18.7% of frame time** by
  v2.3.1's per-source-file attribution, and it is the largest core cost that had
  never been examined — because fat LTO inlines it wholesale into `cpu_clock`, so
  it does not appear in a symbol profile at all, which is why the ten-candidate
  v2.3.1 hot-path sweep never reached it. Three workloads (silent floor, all five
  channels running, and the expansion-audio path), each one NTSC frame of 29,780
  CPU cycles so the numbers compare directly against the `full_frame` bench the
  >3% adoption bar is adjudicated on. **81% of the active per-cycle cost is paid
  with every channel disabled** — the overhead is very largely unconditional.

- **NES Zapper support in the libretro core.** The emulation has existed for a
  long time — `Nes::set_zapper` resolves the photodiode against the CRT beam —
  but the wrapper polled joypads only and never implemented
  `retro_set_controller_port_device`, so light-gun games were unplayable through
  RetroArch despite being fully emulated. Ports 1 and 2 now offer "NES Zapper"
  in the Controls menu via `RETRO_ENVIRONMENT_SET_CONTROLLER_INFO`; ports 3 and 4
  — a Vs. cabinet's SUB console — are advertised pad-only, since a cabinet has no
  light gun. Off-screen
  and "reload" reports are forwarded as a trigger pull at a guaranteed-dark
  position rather than dropped, which is how a real Zapper behaves when pointed
  away from the television — the mechanism the shoot-off-screen behaviour in
  those games depends on.

- **The libretro crate's first unit tests** (it had none). Eight, pinning the
  region/timing derivation, that NTSC is unchanged by it, that Dendy still shares
  PAL's frame duration — the assumption the region fold depends on — the declared
  sample rate against the rate the APU is actually built with, the display
  aspect, and the port-device defaults.

### Documented

- **Why RustyNES does not appear in RetroArch on iOS / iPadOS / tvOS.** Not a
  build failure: the buildbot carries a valid, current core for every Apple
  target. iOS cannot download cores, so the App Store build bundles a hardcoded
  list in `libretro/RetroArch`'s `pkg/apple/update-cores.sh`, and RustyNES is
  absent from it while every NES competitor is present. The remedy is a one-line
  upstream addition covering all three Apple platforms. Details and the
  submission requirements are in `docs/libretro/UPSTREAM_SYNC.md`; the PR is
  tracked separately, as it too lands in a repository this project does not
  control.

### Changed

- **The APU's default-configuration mix takes a specialized path** (−3.3% to
  −4.2% on `nes_run_frame_nestest`, across two full replicates). Every CPU cycle
  at 1.789 MHz, `tick_with_external` evaluated a per-channel `gate` closure
  branching on `channel_mask`, a per-channel `scale` closure branching on
  `channel_gain`, a 6-wide `f32` array copy, and a sixth mask test — all of them
  the identity at the shipped default, which the determinism contract guarantees
  the oracle never leaves. The specialization hoists that question out of the
  per-cycle body, the same shape as the PPU fast dot path. `mix()` receives
  exactly the same five arguments, so output is **byte-identical by
  construction**; a 2,048-point sweep pins it anyway, and AccuracyCoin holds
  **141/141** with nestest 0-diff.

  Recorded in `docs/performance.md` with a caveat rather than a clean story: the
  absolute saving on `nes_run_frame_nestest` (~124 µs) is about **three times**
  what the standalone APU bench attributes to the change, and ~8× the saving on
  `flowing_palette` — from a component doing identical work in both. The likely
  mechanism is an LTO/register-allocation knock-on in `cpu_clock`, which is the
  same inlining that hid the APU from the profile to begin with. Adopted on the
  measurement, not on the explanation.

## [2.3.4] - 2026-08-15 - "Ledger" (mapper coverage + the load path the harness could not see)

### Added

- **Mapper 154 (NAMCOT-3453) and mapper 243 (Sachen SA-020A).** Both surfaced from
  the coverage sweep once the per-game database began reaching the harness, which
  is what routes *Devil Man* from its mapper-88 header to 154 and 美女拳 *Honey
  Peach* from its mapper-150 header to 243. Each is the only game on its board,
  and both now boot: *Devil Man* to its intro cutscene, *Honey Peach* to gameplay.
  Mapper breadth **172 → 174 families**, both `BestEffort` (their dumps are staged
  but not redistributable, so neither can be honestly oracle-gated).

  Neither needed a new type, because neither is a new chip. **154 is mapper 88**
  plus a one-screen nametable bit — decoded across the whole `$8000-$FFFF` range,
  not just the bank-select window — so it is a third `Namco118Board` variant.
  **243 is the same ASIC as mapper 150** wired to a different PCB, which NESdev
  records under its Errata; only the bank-bit significance differs (R2 is the CHR
  LSB on the SA-020A and the MSB on the SA-150), so it is a `Sa020aBoard` variant.
  Ten unit tests. `Namco118`'s save-state moves to v2: 154 makes mirroring mutable
  on a family where it had been constant, and a v1 blob would restore the wrong
  CIRAM page.
- **Mapper 176 submapper 2 (WAIXING-FS005/FS006).** The 8025 ASIC's incompatible
  variants split by NES 2.0 submapper, and only the FK23C half was implemented.
  FS005 adds the `$A001` RAM Configuration Register — 32 KiB of banked WRAM, the
  `$5000-$5FFF` register-window disable that the Waixing copy-protection sequence
  is built on, and a mapper-195-like mixed CHR-ROM/CHR-RAM mode — plus two-bit
  `$A000` mirroring with single-screen pages gated on that register, PRG A21-A25
  from `$5xx0.3/7` and `$5xx2.5/6/7`, and the `$46`/`$47` bank-select swap (which
  applies only with the PRG-invert bit set; `$06`/`$07` are unswapped).
  Implemented from the NESdev wiki page; unlike the FK23C banking transforms in
  the same file, no reference-emulator source was consulted for any of it.

### Fixed

- **The per-game database destroyed correct mapper numbers on every Sachen
  cartridge.** The vendored table uses `0` in its Mapper column as the
  unfilled-row default, with no separate empty marker, so
  `80D63472, PAL, 0, 0, …` — `Sidewinder`, a Sachen SA-72007 and genuinely
  **mapper 145** — was read as "force NROM". The header was overwritten and the
  ROM then failed NROM's size check and would not load at all. **12 staged ROMs:
  every Sachen board in the corpus** (133, 143, 145, 146, 147, 148, 149, 150).
  A `0` in that column is now treated as "unspecified"; the 11 legitimate
  non-zero overrides are unaffected. Present since v1.2.0 and reaching users, not
  only CI, because the frontend applies these on every ROM load.

  Same failure mode as the mirroring column freezing *Wizards & Warriors*
  (ADR 0031), and fixed the same way: refuse to apply an override that cannot be
  distinguished from "no data".
- **Three Waixing dumps were being emulated as a board that cannot exist.**
  `Chu Liu Xiang`, `Mo Shen Fa Shi` and `Shui Hu Zhuan` carry iNES headers
  declaring **mapper 30 with 256 KiB of CHR-ROM**, and UNROM-512 is a CHR-RAM-only
  board — so the header refutes itself. They are FS005 cartridges, and the loader
  now routes any mapper-30 image that declares CHR-ROM to mapper 176 submapper 2.
  Genuine mapper-30 images declare zero CHR-ROM and are untouched. Two of the
  three now boot to their title screens and menus; `Chu Liu Xiang` still renders
  no tiles and is recorded as an open residual rather than claimed as fixed.
- **Bandai FCG (mapper 16): a debug-build panic on real ROMs.** The I2C EEPROM
  byte-address counter is a `u8` advanced as `(addr + 1) & addr_mask()`, and the
  mask is `0xFF` on every chip but the X24C01 — so the mask was written to express
  a counter that rolls over, but the add traps on the wrap first. Now
  `wrapping_add(1)`, which is what the hardware ring does.
- **The coverage harness could not observe game-database fixes.** It loaded ROMs
  with `Nes::from_rom` while the frontend rewrites the header first, so a ROM
  fixed through the per-game database still reported blank in the regression net —
  the fix was real and the net simply could not see it. The database is now its
  own `rustynes-gamedb` crate that both consume (`rustynes-frontend` re-exports it,
  so no call site moved), and its user-overlay directory became injectable rather
  than hard-wired, so the harness cannot pick up a developer's local overrides.
  **Seicross** now renders under the harness, closing the loop on PR #127, as does
  `AV Pachi-Slot`. The risk was measured rather than assumed: all 25 staged ROMs
  whose mapper or submapper the database changes were run, and none went blank.

## [2.3.3] - 2026-08-14 - "Cadence" (display pacing + the run-ahead throttle)

### Added

- **Frontend: the run-ahead budget throttle engages lower and degrades gracefully
  (v2.3.3 F21).** The gate was 85% of the frame budget; F18 measured
  `run_ahead = 2` at 77.4% with **10.7%** of frames held for the wrong number of
  refreshes and the gate unfired. It is now **75%** — between the two measured
  points, 52.1% healthy and 77.4% harmful — and the throttle removes **one depth
  at a time** (`runahead_throttle_steps`) instead of dropping to zero,
  re-measuring each median window so an unaffordable host still converges to 0
  without the cliff. Release predicts the cost of giving back one step using
  F18's per-frame-linear model. Measured at `run_ahead = 2`, three arms in a
  Latin square: unthrottled **6.43%** of frames wrong → all-or-nothing **0.87%**
  but at depth **0** → step-down **0.84%** at depth **1** — the same cadence as
  disabling run-ahead while keeping a frame of latency reduction. A budget guard
  firing earlier and more gently, **not** a shudder fix: `display_produce_due`
  was measured first and delivers 98.3% correct holds at the shipped
  `run_ahead = 1`.
- **Frontend: the display refresh can now come from the Wayland compositor.**
  A new `wayland_presentation` module reads the refresh period straight off
  `wp_presentation`'s `presented` event — the compositor's own figure, which
  does not depend on the `wl_output` global whose absence leaves winit's
  `current_monitor()` answering `None` for an entire session. It binds against
  winit's existing connection and surface, polls without blocking, and settles
  on one value per session, so the pacing regime cannot oscillate. A declared
  refresh still always wins; this only fills the gap where there is none.
  Wayland-only and best-effort — every failure path returns `None` and leaves
  X11, Windows and macOS on exactly the code they ran before. The perf-log
  header gains `refresh_source` (`declared` | `presentation` | `none`), because
  two captures with the same refresh from different sources are not the same
  experiment. Measured over 16 asserted 45 s captures on four ROMs: display-sync
  now engages and **holds every row of every run**, where Bad Dudes (MMC3) and
  Bandit Kings (MMC5) previously never left the wall-clock pacer. Dropped frames
  on Bad Dudes go from **114-186 per 45 s to 1-6**, audio underruns are 0
  throughout, and console-rate error stays within 0.16%. One measure did not
  improve and is recorded as open in `docs/performance.md` v2.3.3 F4: the
  `produced` interval p95 sits at 27-33 ms, and the control run intended to
  attribute it was confounded by capture order, so no cause is claimed.

- **Frontend: instrumentation for the unresolved display-sync shudder** — two
  named suspects were tested and **both refuted**, which is the result. New:
  display-tick arm counters (`tick_ok` / `tick_timeout` / `tick_dropped`), a
  winit-thread emulator-mutex blocking series (`rlock_*`, the mirror of the
  producer's `wait_*` — only the producer side had ever been measured), and an
  **env-gated, default-off per-frame trace** (`RUSTYNES_FRAME_TRACE=1`) writing
  one row per produce and per present, with `scripts/perf/trace_shape.py` to
  classify its temporal shape. `rwork` is now `rtot - rwait - rlock`.

- **Core: a slim restore for run-ahead was measured and REJECTED (v2.3.3 F19).**
  Run-ahead, netplay rollback and TAS seek all re-simulate immediately after
  restoring, so the 245,760-byte framebuffer they restore is overwritten before
  it is seen — and `PPU_SNAPSHOT_SLIM_FLAG` already omits it. Projected ~110 µs
  per restore; **measured 6.9 µs** (122.8 → 115.9 µs), which is **0.25%** of the
  2.802 ms run-ahead increment and an order of magnitude under the >3% bar. The
  estimate came from "the framebuffer is 94% of the snapshot **bytes**" carried
  silently into a claim about **time** — 245 KiB is ~12-25 µs of memcpy, so it
  was never 94% of a 122 µs restore. Rejected before implementation, which also
  avoided a save-state correctness hazard and two contract renegotiations
  (netplay hashes the framebuffer as a desync classifier; TAS seek has a test
  asserting it equals linear replay). A `nes_restore_quiet_slim_*` probe bench
  keeps the evidence. What it did establish: restore costs ~114 µs with **no
  framebuffer at all**, so its 8.3× asymmetry against `snapshot_core_into` is
  per-section deserialization, not the payload.
- **Run-ahead's cost is the frames, and depth 3 throttles itself (v2.3.3 F18).**
  Sixteen captures, four per depth, in a **Latin square** (each depth in each
  round-position exactly once — F13's correction: alternation balances drift
  direction but does not buy exchangeability), every one validity-gated by F16
  and **16/16 passing**. Emulation cost is **linear in depth** at the core's own
  ~4.3 ms per frame — increments +4.491 and +4.213 ms, equal within 6% — so with
  snapshot + restore at ~136 µs (F19), run-ahead's cost is the emulated frames
  and essentially nothing else: **the price of the feature, not overhead around
  it**. Budget shares: 25.1% / 52.1% / **77.4%** at depths 0/1/2. Depth 3
  measures like depth 0 because it **throttles itself** — `run_ahead_throttled`
  is `true` in every one of its captures, the guard engaging at 85% of the frame
  budget against depth 3's ~17.2 ms; correct behaviour that reads as a defect
  without that column. It also retires an earlier cross-session artefact: the
  +3.09/+4.20 ms increments that suggested a non-linear cost structure were
  session noise, not structure.
  **`tick_lat` / `tick_iv` — the trigger is late, not the emulator (v2.3.3
  F15).** Two new independently-ranked series decomposing the produce interval,
  whose variance tracks missed presents at r = 0.937. The display tick's channel
  payload became a `CLOCK_MONOTONIC` timestamp instead of `()`, which is what
  makes the cross-thread hop measurable. First verified capture (SMB,
  `run_ahead = 1`, display-sync /2, window confirmed on screen by the F16 gate):
  the winit→emu hop is **0.033-0.050 ms** — negligible — while `produced` p95
  (24.635 ms) matches `tick_iv` p95 (24.578 ms) to within 0.06 ms. With `rlock`
  at 0.000 and `tick_timeout` at 0 of 1494, **the produce tail is inherited
  wholesale from the trigger interval**: the emulator is not late, it is asked
  late. First positive location the campaign has produced rather than an
  elimination. Also `crates/rustynes-frontend/src/clock.rs`, holding the
  frontend's only `clock_gettime` call site.
  **The unexplained between-session cadence spread is emulation budget margin
  (v2.3.3 F17).** F14 left one quantity open and called it the largest
  unexplained question in `docs/performance.md`: display cadence error varied
  **0.69-3.08%** in one session and **8.6-18.9%** in others, *on the same binary
  pair*. Across **27 captures**, plotting it against what fraction of the NTSC
  frame budget the emulator consumes at p95 (`cost_p95` / 16.639 ms) separates
  them **completely — no overlap**: every capture under 60% utilisation has
  ≤ 8.59% error, every one at or above has ≥ 9.02% (r = +0.836). Two independent
  routes get there — the run-ahead **baseline** (34% of budget at depth 0, 52% at
  1, **78% at 2**) and a host-contention **tail** — and the error follows the
  total, not the cause. The `run_ahead = 2` captures are the decisive evidence
  that this is causal rather than a shared symptom of host load: their tail ratio
  is 1.04-1.05 (evidence against a large contention *spread*, not proof of zero
  contention), their cost is elevated **structurally by design**, and they still
  show 14.77-18.93% error — showing emulation cost is *sufficient* to produce the
  effect, not that it is the only contributor. **The spread was never a property of the
  pacing code**, which is also why the F13 A/B could not resolve a ~0.4-point
  configuration effect sitting inside ten points of variance. Limits stated in
  full in F17: `ra = 2` is n = 3 from one session, 60% is an observed separator
  rather than a derived threshold, and this does not identify the reported
  shudder — only the spread between these captures.

  **The display-cadence metric was measuring the producer — F12 and F13
  corrected (v2.3.3 F14).** Both quantified "frames shown for the wrong
  duration" by counting refreshes between consecutive **produce** timestamps.
  Both ends of that interval are producer-side, so a produce firing 3 ms early
  followed by one 3 ms late scores as a mistimed pair **even when the panel
  showed both frames for exactly two refreshes**. The display-side series was
  already in the trace and unused: `since_present`, recorded on the present,
  whose gaps between frame-carrying presents are all exactly the divisor
  under a healthy cadence. Pooled over seventeen captures the two read **32.96%**
  and **5.41%** wrong — a factor of six. Every "N% of frames shown for the wrong
  duration" figure in F12 and F13 is retracted; the display was ~94.6% correct
  while the document said 65-74%. Re-running the F13 A/B on the correct metric
  **removes its display-side conclusion entirely**: two of four pairs reverse and
  the exact paired test gives **p = 0.25**, so the fix's effect on what the panel
  shows is not distinguishable from noise. (The first version of this correction
  was itself wrong — it divided by presents rather than displayed frames, and
  tested run-lengths against 1, which holds only at divisor 2; both were caught
  in review on PR #362 and the divisor is now inferred as the modal gap.) The `rlock` 8.707 -> 0.000 ms collapse and the double-replay
  removal are direct measurements and are unaffected. Three mechanisms were
  measured and refuted along the way (presentation-path flipping — `flags` is a
  constant 7; compositor sequence numbers — `seq` is 0, this compositor reports
  none; produce margin — no phase dependence). `trace_shape.py` now leads with
  the display-side metric, labels the old one as producer jitter, declines to
  rate fewer than 100 runs, and **verifies the clock join by span overlap**
  instead of assuming it — `clock_id` was `unknown` in every trace ever written
  because it is read before the Wayland registry answers, and is now emitted as
  a comment row once known.

  **`present_discarded` — an unpresented surface now reports itself (v2.3.3
  F16).** New `PerfView` field and perf-log column carrying the compositor's
  cumulative count of frames it composited but never scanned out. Sustained
  discards stop the *measured* refresh from settling, which costs display-sync
  only where no *declared* refresh is available either; where both are absent the
  session silently holds the wall-clock fallback. `PresentationClock::discarded()`
  had existed since scanout tracing landed and **was read by nobody**, so the
  condition was invisible. `perf_log_check.py` now fails closed on a non-zero
  count, since an occluded capture yields plausible but meaningless pacing
  numbers. Measurements, mechanism and limits in `docs/performance.md` v2.3.3
  F16.

  Later joined by **`rcpu`** — the winit thread's own `CLOCK_THREAD_CPUTIME_ID`
  differenced across the `rwork` span, so `rwork - rcpu` is time the thread spent
  off-CPU rather than computing. It was built to test whether the 9-32 ms `rwork`
  tail was descheduling, and it showed wall and CPU **identical** on an idle host
  and 8 us apart under 20 spinning threads — while **the tail did not reproduce
  at all**. Every tail-bearing capture had been taken while a `cargo build` was
  running: the tail was the measurement environment, not the frontend. `rcpu`
  stays in the tree as the check that tells the two apart. Also
  `scripts/perf/trace_shape.py --warmup-s N`, making the 8 s startup-transient
  discard (a host-tuned heuristic) overridable instead of hard-coded.

  **Superseded in part by F14 (below): the display-side claims in this entry and
  the next were computed with a producer-side metric and are retracted.**

  Measured on six 45 s SMB captures: the 25 ms tick watchdog **never fires** at
  the shipped `run_ahead = 2` (0 of ~1855 ticks) and drops no ticks, so its
  numeric coincidence with the 25-36 ms `produced` p95 was exactly that; and the
  winit thread does **not** block on the emulator mutex there (`rlock` p99 =
  0.000 ms), so the 13 ms `rwork` p99 is neither lock, nor egui, nor GPU, and
  stays unattributed. A **ragged refreshes-per-frame cadence** was then measured
  and, on further investigation, **disqualified as mis-measured**: present
  intervals are bimodal (31.8% under 1 ms, 38.9% at 12-16.7 ms, only 1.9% near
  one refresh), which is the known triple-buffered-Fifo signature — so
  `record_presented` timestamps *queue submission*, not scanout, and any cadence
  derived from it counts queue slots rather than refreshes on screen. The
  shudder remains **unexplained**; this round removed two candidates and
  disqualified a third line of evidence. Next instrument identified:
  `wp_presentation`'s `presented` event already delivers real scanout
  timestamps, which the handler currently discards. See `docs/performance.md`
  v2.3.3 F10.

  **Root cause then found (F11/F12).** Recording those scanout timestamps showed
  the display misses **4.6% of refreshes**; joining them to the produce series
  through a `CLOCK_MONOTONIC` anchor showed only **65.3% of produced frames get
  the intended 2 scanouts** — 18.3% get one, 10.6% get three, and **3.2% are
  never displayed at all**. Completing the `rlock` series (it had missed four
  acquisition sites, which is why F10 read it as zero) moved the whole
  unattributed 13 ms `rwork` tail into it: `rlock` p95 is **8.707 ms** against
  an 8.334 ms refresh period, and `rwork` p99 drops to 0.109 ms. The winit
  thread blocks on the emulator mutex for more than a refresh, so redraws land
  late and frames miss their slot. `pump_watchpoints` takes that lock on every
  redraw unconditionally.

  **That acquisition is now removed (F13).** A conservative, emulator-free
  predicate (`DebuggerOverlay::wants_emu_pump`) gates the work, and the call
  moved off the redraw path into the lock `post_produce_housekeeping` already
  holds — which also fixes a second defect: at divisor 2 there are two redraws
  per produced frame, so the old placement replayed each frame's debug logs
  **twice**. `rlock` p95/p99 go **8.707/9.008 ms → 0.000/0.000 ms**. It also
  improves **producer-interval regularity**, confirmed by an A/B with both
  binaries rebuilt from the two adjacent commits that differ only by this
  change, run **alternately** rather than in blocks, four captures each:
  produce intervals spanning exactly two refreshes go **67.75% → 73.77%**, ranges
  non-overlapping. Alternation is **not** randomisation, so the correct test is
  paired, giving **p = 0.0625** — suggestive, not established.

  **What this does NOT show (see F14 below).** That 67.75 → 73.77% is a
  **producer-side** statistic, not a display one: it counts refreshes between
  consecutive *produce* timestamps. Re-run on the display-side metric the same
  eight captures give **p = 0.25 with two of four pairs reversed**, so the
  change's effect on what the panel shows is **not established in either
  direction**. Every display-side figure originally published for this change is
  retracted. The `rlock` collapse above is a direct measurement and stands.

### Changed

- **Frontend: refresh measurement from redraw intervals is removed.** Shipped
  earlier in this cycle as the fallback for a silent windowing API, it worked
  on light ROMs and failed on the ones that needed it: a redraw interval
  measures the *application*, not the display, so on a ~14 ms/frame commercial
  ROM it reported **20.032 Hz on a 119.991 Hz panel** and display-sync
  correctly refused to engage. No retry schedule fixes a signal measuring the
  wrong quantity — three were tried — so the sampling half is deleted rather
  than left disabled, and `wp_presentation` above replaces it. The median +
  stability-quorum estimator was never the flawed half and is unchanged, now
  shared by both callers along with its tests; `best_divisor`, the
  phase/rate split and the console-rate fallback are untouched.

- **Frontend perf gates: `cost_p95` and `produced_dropped` are reported, not
  enforced; a console-rate gate replaces them.** Both thresholds were derived
  from the contaminated cost metric described under *Fixed* below. `cost_p95` legitimately scales with
  `run_ahead`, which multiplies work by design, and `produced_dropped` is a
  property of the display (the same build and host drops 1–9 frames per 45 s
  under display-sync and 35–131 under the wall-clock fallback), so gating either
  reports the user's hardware as a regression. The new gate checks
  `produced_mean` against the capture's `target_ms` within 0.5% — the one thing
  that is the emulator's own responsibility and is independent of both the
  display and `run_ahead`.

- **Frontend: the framebuffer is no longer re-uploaded when it has not changed.**
  In `Mailbox` present mode the frontend presents faster than the emulator
  produces — measured across the captures in `perf-logs/`, four of six runs show
  137–156 duplicate presents per second against ~60 produced frames, so roughly
  70% of presents were re-sending pixels the GPU texture already held (~35 MB/s
  of redundant staging and copy traffic). The upload is now gated on an FNV-1a
  hash of the frame. No visual change; no frame-time change is claimed.
- **Frontend: the status bar's mapper label is cached at ROM load.** It was
  rebuilt every displayed frame from `Nes::mapper_info()` — which constructs a
  whole debug structure (MMC3's runs ~25 `format!` calls and four `Vec`
  allocations) — inside the emulator lock, keeping only the name. Measured at
  1,367 ns/frame, i.e. 0.008% of a frame: this is allocation hygiene, not an
  optimization.

### Fixed

- **Frontend: a `run_ahead = 3` host reaches a sustainable depth in ~3 s instead
  of ~12 s (v2.3.3 F28).** F27's median window is correct and it cost
  convergence speed: every throttle step waited a full 10 s for the ring to turn
  over, so a configuration starting well over budget stayed there while it
  stepped down. The engage arm now computes rather than waits — within a single
  evaluation it steps while the *predicted* cost at the reduced depth is still
  over the band, using the per-frame-linear model (F18) that the release arm
  already relied on. Releasing is unchanged and still demands a full window and a
  real measurement, because releasing on a stale median is what produced the
  oscillation F27 fixed. The naive reading of "engage faster" is a bug, not a
  fix: at depth 3 the cost exceeds the band at every depth, so engaging on less
  evidence would cascade to depth 0 and discard the feature — the cascade stops
  where the prediction fits, which a test pins directly. Measured at
  `run_ahead = 3` over five paired, Latin-square rounds: convergence **12.12 s →
  2.80 s**, frames held for the wrong duration **4.82% → 2.24%**, 5/5 pairs on
  both, exact one-sided sign p = 0.0312. An alternative arm that cleared the
  produce-cost ring on each depth change converged in 4.0 s and matched on
  cadence but produced an audio underrun in **every** capture, and was rejected.
  Additive-only (65 insertions, 0 deletions) inside the engage branch, which the
  shipped `run_ahead = 1` default never enters.

- **Frontend: the run-ahead throttle no longer oscillates — it was pacing itself
  against a fifth of a median window (v2.3.3 F27).** The throttle is gated to one
  depth change per median window, but the gate used **120** frames — the number
  of samples the produce-cost ring needs before it will report at all — where the
  ring's actual capacity is **600**. A p50 sits at index 300 of 600, so 120
  frames of turnover cannot move it, and a second transition was permitted while
  80% of the median still described the depth the first had just left. Depth then
  walked `2 → 1 → 0` on one measurement, and each change displaces the displayed
  frame by the run-ahead depth — the picture jumping forward and back. Per-window
  logging caught transitions arriving in pairs sharing a median to three decimals
  (`12.958` engaging twice, `4.994` releasing twice); in the release pair the
  unchanged cost is divided by a depth that already changed, so the predicted
  cost halves without any frame getting cheaper. The gate is now expressed in
  terms of the ring it reads, so the two cannot drift apart again. Measured at
  `run_ahead = 2` over three captures: **6-7 transitions per 24 s → 1**, with
  zero spurious releases, and **1.31%** of frames held for the wrong duration
  against a measured 10.7% at depth 2. The release predicate was never wrong —
  its input was: at depth 1 it now reads an honest 8.670 ms (matching the
  independently measured 8.671 ms) instead of a stale 4.994 ms. Frontend-only and
  output-identical; AccuracyCoin and nestest are untouched by construction.

- **Frontend: display-sync consumed a frame slot per redraw without producing a
  frame, whenever the emulation thread was not driving.** `display_produce_due`
  is a stateful mutator — it advances the wall-clock schedule and resets the
  refresh counter — and both the post-present path and `display_sync_produce`
  called it for the same redraw. At divisor 2 the discarded second call could
  advance the schedule by a full period with no frame behind it, so the wall
  clock (the rate authority) outran frame production and the console ran
  **slow**, on exactly the high-refresh panel the divisor exists to serve.
  Ownership now follows `emu_thread_drives()`, the same predicate
  `display_sync_produce` stands down on.
- **Frontend: the F8 work/wait instrument measured the wrong interval, and its
  headline figure was arithmetically invalid.** The wait clock started before
  the render branch, so it spanned the framebuffer copy, the HD composite and
  the egui build in addition to the blocking present; and "render work" was
  derived as `rtot p95 − rwait p95`, a difference of percentiles rather than a
  percentile of the difference. The published table had `work p95` *below*
  `work p50`, which is impossible. The clock now restarts immediately before
  each GPU call, and work is recorded per sample as its own series (`rwork_*`).
  Re-measured, this **reverses the conclusion drawn from it**: render work is
  0.013 ms at p50 but reaches **13–22 ms at p99**, a real tail the broken
  instrument hid and on the strength of which the render loop had been
  eliminated as a suspect. Recorded as an open lead, not a diagnosis — see
  `docs/performance.md` v2.3.3 F8.
- **Frontend: `RenderPerf::clear()` left the wait samples behind**, mixing the
  previous ROM's or regime's blocking-present figures into the next
  experiment's while every other render series started fresh.
- **Frontend: a `DisplaySync::slack` field documented as "half a display
  refresh" was cached but never read**, while the guard it purported to
  describe uses half a *console frame period*. The field is deleted and the
  docs corrected rather than left to contradict the code.
- **Frontend: `close_rom` left a stale cached mapper name** behind the ROM it
  described; it now goes through `EmuCore::clear_rom`.
- **Frontend: `refresh_probe::effective_period` could panic the render loop.**
  `Duration::from_secs_f64` rejects non-finite and negative values, and the
  refresh reaching it is compositor- or API-supplied rather than checked. It
  now returns `Option<Duration>`, matching its sibling `best_divisor`.
- **Frontend: the `wp_presentation` sample set could wedge permanently.** At the
  cap the newest report was dropped rather than the oldest, so a set that
  straddled an output change could never re-form a quorum while still creating
  one feedback object per present for the rest of the session. It is now a
  sliding window.
- **Frontend: `PresentationClock` dropped winit's window before the Wayland
  objects backed by its pointers.** Rust drops fields in declaration order and
  `_window` was declared first; it is now declared last, so the foreign-backed
  `conn`/`queue`/`surface` are gone before the `Arc<Window>` is released.
- **Build: `wayland-client`'s `system` feature is now declared explicitly.**
  `Backend::from_foreign_display` and `ObjectId::from_ptr` require it, and it
  resolved only by unification through winit/sctk/rfd — a change in any of them
  would have turned into a confusing missing-API error.
- **`scripts/perf/perf_capture.sh`: the documented exit-3 "unverifiable
  capture" path was unreachable.** `grep` exiting 1 on an absent header
  propagated out of the command substitution under `set -euo pipefail` and
  killed the script first, with status 1 and no message — in precisely the case
  the message exists for. The metadata reader also imported `tomllib` outside
  its `try`, aborting the whole capture on Python < 3.11 instead of degrading.
- **CI: the PGO workflow's BOLT probe could report a runtime it had not
  linked.** Failed `sudo mkdir`/`ln` left `bolt_dir` populated (the step carries
  no `set -e`), publishing `have_bolt=true` without `libbolt_rt_instr.a`. It now
  verifies the resulting file and prefers the selected toolchain's own prefix
  over a system-wide search.
- **Security: `webbrowser` bumped 1.2.1 → 1.2.4** for RUSTSEC-2026-0257 (Unix
  `BROWSER` argument injection), reached transitively via `egui-winit`'s `links`
  feature.
- **Frontend: display-sync's occlusion watchdog never ran.** `about_to_wait`
  early-returned with `ControlFlow::Wait` whenever the emulation thread drives
  (the default build), and that return sat above the display-sync branch — so
  the watchdog was unreachable in every shipped build. Display-sync is
  self-driving from the present success path, so a compositor that stops
  delivering frame callbacks (minimised or fully occluded window) left nothing
  to re-arm the redraw and nothing scheduled to wake the loop, stopping
  emulation and audio. The stall path is additionally guarded so it does not
  produce frames on the winit thread while the emulation thread is also
  producing.
- **Frontend: the run-ahead throttle could not release without re-engaging.**
  It engaged on produce cost measured *with* run-ahead (>85% of the frame
  budget) but released on cost measured *without* it (<40%) — two different
  quantities, with the hysteresis band sitting between the two states rather
  than spanning them. At depth 2 any ROM whose base cost lands between ~28%
  and 40% of budget oscillated on a ~2 s period, and each toggle shifts the
  displayed frame by the run-ahead depth. Measured at three toggles per 45 s
  on Bad Dudes. Release now predicts the re-enabled cost.
- **Frontend: the display-sync produce phase was a marginal wall-clock test.**
  It re-decided `now + slack >= next` on every refresh with `slack` at half a
  *refresh*, putting the decision boundary 4.167 ms from an 8.334 ms grid, so
  ordinary redraw jitter flipped it between adjacent refreshes. The phase now
  comes from a refresh count, with the wall clock retained as rate guards in
  both directions. Worth ~10% of the produced-interval p95; see
  `docs/performance.md` v2.3.3 F6 for why that is reported as partial.
- **Frontend: dropped frames and stutter traced to display pacing, and fixed.**
  On a 120 Hz host the emulator produced NTSC frames perfectly (`produced_mean`
  measured 16.64 ms in every capture) while the display-synchronised pacer never
  engaged, leaving a free-running wall-clock producer beating against the
  compositor's frame callbacks — 135–254 dropped frames per 45 s. Three causes,
  all fixed: display-sync only ever supported **one emulated frame per refresh**,
  so every 120/144 Hz panel was rejected by construction; refresh detection went
  solely through winit's `current_monitor()`, which reports nothing on a
  compositor that advertises no `wl_output` (and was consulted once at startup
  before the monitor was known, then never revisited); and the regime tied
  console *rate* to present rate, so render-loop hiccups slowed the console.
  Display-sync now selects an integer divisor (120 Hz → one frame per two
  refreshes), can measure the refresh cadence itself when the windowing API is
  silent, and takes its rate from the wall-clock schedule while taking only its
  *phase* from the display. Measured: dropped frames **135–254 → 1–9** per 45 s,
  audio underruns **0–19 → 0**, console-rate error within 0.12%. Frontend-only —
  the deterministic core, save-state and movie formats, and every golden vector
  are untouched.
- **Frontend: display-sync no longer downgrades itself while it is winning.**
  Its sustained-miss fallback tripped on presented-interval p95, a proxy that
  reports the host's compositor rather than whether the regime is working. On
  Super Mario Bros — a materially heavier ROM than the synthetic one the regime
  was tuned against, 13.5 ms of work at `run_ahead = 2` versus 9.2 ms — that
  p95 sat at 25.3–27.2 ms against a 24.96 ms limit, so whether a session kept
  the good regime came down to run-to-run variance, and the fallback is sticky.
  Measured, the regime it fell back to was far worse: 1–15 dropped frames per
  45 s under display-sync against 35–147 under wall-clock. Display-sync now
  falls back on **console-rate error** instead (2% band, a structural safety
  net — the wall-clock rate authority makes a breach a genuine defect), and
  holds 4/4 runs on that ROM at both run-ahead levels with 1–8 drops. `vrr`
  keeps the present-based test, because its failure is the opposite shape: the
  emulator produces correctly at 16.64 ms while the display shows ~20 fps.
- **Frontend: `pacing_mode = "vrr"` no longer collapses on a non-VRR display.**
  It had no sustained-miss fallback, so on a fixed-refresh panel it degraded to
  ~20 fps (49.74 ms presented, 1170 dropped frames in 40 s) and stayed there. It
  now shares display-sync's health check and sticky fallback to wall-clock.
- **Frontend: the producer's mutex wait is no longer billed as emulation cost.**
  The three produce paths started their timer before acquiring the emulator
  mutex, so time blocked on the winit thread was recorded as work — making a
  contention stall indistinguishable from an expensive frame, and pinning the
  reported tail to almost exactly one display refresh. Work and wait are now
  measured separately (new `wait_*` columns in the perf-log CSV). The corrected
  figures show emulation at ~4.1 ms of the 16.639 ms budget with run-ahead off,
  and no mutex contention at any percentile.
- **`scripts/perf/perf_log_check.py` crashed on valid captures.** A run ended
  mid-write (how every timed capture ends) yields a short final row, which
  `csv.DictReader` fills with `None`; `float(None)` raises `TypeError`, which the
  bare `except ValueError` did not catch.
- **The perf-log gate ignored the metric that matters.** It tracked only
  `produced_max_ms` against a 150 ms threshold — nine times the NTSC frame
  budget, and a single sample — so a capture peaking at 128.9 ms with 62
  catch-up bursts passed every threshold it tracked. The gate now trips on
  `catchup_bursts` (200 → **16**) and `snap_forwards` (40 → **8**), both derived
  from the eight captures on file rather than chosen: healthy runs sit at 0
  bursts, the borderline one at 12, the degraded ones at 32 and 62.
  - An absolute-millisecond p99 gate was tried first and **rejected on
    measurement**: p99 tracks the host display's beat against the console rate,
    not frontend health, so it is now reported rather than gated (opt-in flags
    remain for single-machine comparisons). Full figures and reasoning:
    `docs/performance.md`.
- **The BOLT stage reported a speedup it had not measured.** Its bench fallback
  timed a plain, non-BOLT build and labelled the result "BOLT speedup vs plain
  release", and the stage benched the core while BOLT optimizes the frontend
  binary — so it could not have measured its subject in any case. Both gate
  steps are now disabled with the reasoning inline, the runtime probe fails
  closed, and the artifact is named for what it contains
  (`rustynes-bolt-optimized`). **BOLT is deferred and explicitly unmeasured**;
  PGO (measured 6.43% faster and byte-identical) remains the shipping
  optimization. Investigation trail: `docs/performance.md`.

### Performance

- **Run-ahead measurement, corrected.** An earlier entry in this section
  claimed run-ahead "blows the frame budget at the shipped default", citing
  `cost_p95` rising 4.51 ms → 24.15 ms and dropped frames 10 → 303. **Both
  figures were artefacts of the mutex-wait timing bug fixed above**, which billed
  time blocked on the winit thread to the emulator. Corrected, run-ahead costs
  ~6 ms of the 16.639 ms budget at `run_ahead = 1` and ~9.7 ms at the shipped
  `run_ahead = 2`; the dropped frames were display pacing, not run-ahead, and are
  themselves fixed above. Snapshot slimming was then measured directly and is
  **not** the lever it was assumed to be: `snapshot_core_into` is 14.8 µs and
  `restore_quiet` 122 µs, together ~2.2% of a run-ahead frame, so removing the
  245,760-byte framebuffer would buy ~0.66% of the frame budget — below the
  project's standing >3% adoption bar. It retains a real justification on
  rewind-ring *memory*, where the framebuffer is ~94% of every per-frame
  snapshot, and should be argued there rather than on frame time. See
  `docs/performance.md` (v2.3.3 F1).

- **The frontend optimization items were measured before being built, and all
  three are under 0.1% of a frame** — the framebuffer copy chain 13.2 µs
  (0.079%), `perf.view()` for a closed panel 16.2 µs (0.098%), and the
  `mapper_info()` storm 1.4 µs (0.008%). Every claim in the plan was factually
  true and verified in source; they are simply small. The core needs ~3.78 ms of
  the 16.639 ms budget, so the frontend runs with ~12.8 ms of slack and mean
  frame time was never the constraint. See `docs/performance.md` (v2.3.3 F1).

## [2.3.2] - 2026-08-11 - "Lucid" (pixel provenance + replay attestation)

### Added

- **Pixel provenance, phase 1 — per-byte write attribution** (`debug-hooks`,
  default off). Every byte of CIRAM, OAM, and palette RAM now remembers the
  **program counter and CPU cycle of the instruction that last wrote it** — the
  edge that lets the forthcoming provenance panel walk from a pixel on screen
  back to the code that produced it. Nothing in RustyNES recorded this before:
  the Trace Logger has the PC but no effect, the Event Viewer has the write and
  its PPU position but not the PC or the resolved destination, and the memory
  access counter has a cycle stamp but no PC.
  - Recording is **split across the bus/PPU boundary**, because neither side
    knows enough alone: the bus has the program counter and the PPU has the
    effective destination (a `STA $2007` lands in a nametable or in palette RAM
    depending on the PPU's internal `v`). `Nes::run_frame` pushes the executing
    instruction's context down once per instruction, inside the block that
    already performs the breakpoint check — so no new `CpuBus` hook was needed
    and `rustynes-cpu` is untouched.
  - **An OAM DMA burst is attributed to its trigger, not its victim.**
    `STA $4014` only arms the transfer; its 513/514 cycles are stolen from the
    instructions that follow, so the live context would name whichever
    instruction was being halted — true about the timing, wrong about the cause.
    The bus latches the triggering instruction, and all 256 bytes name it.
  - Attribution is **invalidated on power-cycle and on both save-state restore
    paths**: a restored state's bytes were not written by anything this session
    ran, so the honest answer is "no record" rather than a PC from a timeline
    that no longer exists.
  - CHR writes are deliberately **not** attributed (mapper-owned, so a byte
    offset is not a stable identity across a bank switch), nor is the
    `$2004`-during-rendering write the hardware discards.
  - Output-only and lazily allocated: unarmed it costs one `Option` test per PPU
    memory write, armed it costs ~37 KiB. Framebuffer, audio, and cycle counts
    are bit-identical either way, and the default build is unchanged —
    **AccuracyCoin holds at exactly 141/141** with nestest 0-diff.
  - Spec: `docs/pixel-provenance.md`.
- **Pixel provenance, phase 2 — the per-pixel causal record** (`debug-hooks`,
  default off). Every emitted pixel now records the layer that won the priority
  decision, the exact `$3Fxx` palette address behind its color, and the
  nametable / attribute / pattern addresses of the tile **actually on screen**.
  Composes with phase 1: the palette index and nametable address are the keys
  into the write-attribution store, so pixel → byte → writing instruction is one
  chain.
  - **`v` cannot answer the "which tile" question.** By the time a tile's pixels
    reach the screen, `v` has advanced two tiles past it, so an address derived
    from `v` at emit time is wrong for every pixel — and wrong in a way that
    looks plausible. The addresses ride the same `latch` → `next` → `cur`
    cascade that moves the pattern bytes through the shift registers.
  - **A tile is defined when its PATTERN is fetched, not when its nametable byte
    is read.** The PPU performs two dummy nametable fetches at dots 337-340,
    which clobbered the pending tile and made pixels x=8..15 report the tile
    belonging to x=16..23. Found by the test failing, not by review.
  - The attribute address is carried rather than derived, because an MMC5
    vertical split supplies one the standard `$23C0 | ...` arithmetic cannot
    produce.
  - The plan had been to widen the existing `hd-pack` `HdTileSource` gate; that
    was the wrong shape (it carries Mesen HD-pack tile *keys*, not addresses, and
    widening it would have pulled eight fetch-telemetry fields into every
    `debug-hooks` build). A separate lazily-allocated record leaves `hd-pack`
    byte-identical by construction rather than by review.
  - Unarmed cost in `emit_pixel` is one predicted `bool` branch — same shape as
    the bus's existing `event_logging` flag. **AccuracyCoin holds at exactly
    141/141**, nestest 0-diff.
- **Pixel provenance, phase 3 — the inspector panel.** New **Tools → Pixel
  Provenance**: pin a screen pixel and read its whole causal chain — the dot and
  scanline that emitted it, which layer won, the palette entry and the
  instruction that wrote it, the background tile's nametable / attribute /
  pattern addresses with their CIRAM offsets and writing instructions, and the
  winning sprite's slot, priority, and OAM attribution.
  - Detachable into its own OS window through the shared v2.3.0
    `detachable_window` helper. Read-only over the emulator; its only side effect
    is the two arming checkboxes, both default off and both determinism-neutral.
  - PC → source-line resolution reuses the existing `.dbg` source map when one is
    loaded. `Nes::ciram_offset_for_nametable_addr` resolves a nametable address
    to the offset attribution is keyed on, sharing its mirroring resolution with
    the PPU's own fetch path so a per-game mirroring override cannot make the two
    disagree.
  - Frontend-only, so the deterministic core is untouched: **AccuracyCoin holds
    at exactly 141/141**, nestest 0-diff.
- **Deterministic replay attestation** — a `.rnm` movie can now carry a rolling
  hash of its run that anyone else can independently re-derive:
  `rustynes verify <movie.rnm> --rom <game.nes>`. Because the core re-derives
  every pixel from the same ROM and inputs, a third party can replay the movie
  and confirm it reproduces bit-for-bit. The digest is **tamper-evident, not
  forgery-resistant** (64-bit FNV-1a): it catches accidental divergence and
  casual edits, not a motivated forger who recomputes it.
  - **No format-version bump.** `.rnm` already had a precedent for additive
    trailing fields (`rerecord_count`), so the attestation is appended the same
    way behind a marker. `MOVIE_FORMAT_VERSION` stays at 2, every existing movie
    round-trips unchanged, and a pre-v2.3.2 reader parses an attested movie as a
    plain one.
  - **The hash covers the input applied AND the framebuffer it produced**, so
    the record states *these inputs, applied to this ROM, produce this video*.
    Hashing video alone would not pin the input stream at all for a ROM that
    ignores the controller.
  - A checkpoint every 64 frames localizes a mismatch to a 64-frame window rather
    than reporting only a verdict. Exit codes are distinct: 0 verified, 1
    mismatch, 3 not attested — a movie that makes no claim has not failed.
  - Hashing the core snapshot would detect more, and was rejected: the snapshot
    schema is versioned and bumps between releases, which would silently
    invalidate every previously-recorded attestation. Audio is not covered, and
    the docs say so rather than implying it.
  - Recording arms automatically **unless run-ahead is on** — run-ahead presents
    a frame ahead of the persistent timeline, so an attestation recorded under it
    could never verify. If it is toggled on mid-recording the frame counts
    diverge and the tail is dropped at load: the failure mode is "no
    attestation", never "a wrong one".

## [2.3.1] - 2026-08-06 - "Plumb Line" (measurement apparatus + ten measured rejections)

### Performance

- **No emulation-core changes. Ten hot-path optimization candidates were
  measured and all ten rejected**, through six distinct mechanisms: LLVM already
  performed the transformation; the item's premise was factually false; the work
  was real but absorbed off the critical path; the elision was real but bought
  nothing; the target was too small to matter; the ownership model forbids it.
  Full numbers, controls and reasoning are in `docs/performance.md`
  (entries G1–G10). **AccuracyCoin remains at exactly 141/141 and nestest
  0-diff**, verified after every experimental probe was reverted.
- New measurement tooling, all of which found something the previous apparatus
  could not:
  - `crates/rustynes-test-harness/src/bin/frame_probe.rs` — harness-free
    steady-state frame cost, with no criterion in the process image (criterion's
    own rayon/`exp`/sort work had been ~17% of every profile).
  - `scripts/perf/frame_breakdown.sh` — per-subsystem attribution by **source
    file**, which recovers work the symbol profile hides. It shows the **APU at
    18.7% of frame time**, invisible under `perf report` because fat LTO inlines
    it wholesale into `cpu_clock` (`perf report --inline` does not recover it).
  - `scripts/perf/ab_check.sh` — adoption A/B with an **A/B/A order-bias
    control**: the reference is benched a third time, last, against its own first
    run, so drift from position-in-the-run is reported rather than mistaken for a
    result.
- `scripts/bench_relative_check.sh` now declines to emit a verdict when the host
  was too noisy to resolve the effect it tests for, keyed on a robust
  MAD-based coefficient of variation.

### Fixed

- **The PGO workflow's BOLT probe reported success without BOLT present.** It ran
  `apt-get install bolt` and trusted the exit status — but on Ubuntu that package
  is the *Thunderbolt 3 device manager*, an unrelated project that owns the name.
  The stage then failed on the tool it had just "confirmed", instead of skipping
  as its best-effort contract intends. The probe now locates the actual
  `llvm-bolt` binary and reports honestly when it is absent.

### Documentation

- `docs/performance.md` records every rejected experiment with its numbers, its
  order-bias control, and the mechanism behind the null result — including two
  near-misses that a single measurement would have adopted.

## [2.3.0] - 2026-08-05 - "Datum II" (PPU-accuracy capstone + true multi-viewport tool windows)

Closes the **v2.2.6 → v2.3.0 NESdev-remediation line**. Both remaining
forum-reported accuracy concerns were investigated and found *already correct*;
the release's substance turned out to be elsewhere — real OS-window tool panels,
and a frame-pacing defect that had been degrading every session with a debugger
panel open.

### Added

- **True multi-viewport tool-window detach.** Every tool panel can now pop out
  into a *real OS window* (native), finally resolving the Windows-10
  "trapped window" report. v2.2.9's affordance only *embedded* the panel, because
  `show_viewport_immediate` needs a multi-viewport integration to produce a real
  window. The new `rustynes-frontend/src/detached.rs` gives each detached panel
  its own winit window + egui context/state/renderer + wgpu surface, sharing the
  main device — **with no `unsafe`**, unlike eframe's immediate-viewport path
  (which erases an `ActiveEventLoop` lifetime into a `'static` thread-local).
  The nine panels that predated the shared helper (CPU, Cartridge Info, Lua
  Script, BasicBot, Input bindings, TAStudio, Settings, Netplay,
  RetroAchievements) were converted, so *all* tool windows are detachable.
  Detached windows inherit the main window's theme, zoom, and locale, and open at
  the size their docked window actually had. wasm keeps panels docked.

### Fixed

- **Frame stutter / high produced-interval p99 whenever a debugger or tool panel
  was open.** The overlay-visible render path held the emulator mutex "until
  after the present call" — and the *blocking* `Surface::get_current_texture`
  runs before the egui pass — so the winit thread owned the lock across a
  swapchain wait, the whole egui build, the encode and the present, while the
  emulation thread sat parked on `emu.lock()` unable to produce a frame.
  `render_shell` is split into `run_shell_ui` (locked, needs `&mut Nes`) and
  `paint_shell` (unlocked GPU work); the guard now covers only the UI build.
- **`pace_frames` took the emulator mutex on every `about_to_wait` iteration** —
  a tight spin in the wall-clock regime — which could block the UI thread for a
  full produce (~4 ms) each time. It now reads the lock-free `EmuControl::has_rom`
  atomic, falling back to the locked read only when no emulation thread exists.

### Changed

- **PPU per-dot helpers optimized: −5.13% / −3.51% frame cost** (nestest /
  flowing-palette, both clearing the project's >3% adoption bar, p = 0.00).
  `perf annotate` showed `tick_sprite_eval_per_dot`'s own `push`/`ret` were the
  two hottest instructions in its body — pure call overhead across 89,342
  calls/frame — and that `tick_oam_bus` derived values it discarded before its
  dot-0 early-out. Byte-identical: AccuracyCoin **141/141**, nestest 0-diff, PPU
  units 91/91. Documented as `v2.3.0 P1` in `docs/performance.md`.
- **Detached panels repaint on per-panel tiers** — Live (60 Hz) for
  continuously-changing state, Throttled (~10 Hz) for status, and
  interaction-only for static panels (Cheats, ROM Info, Settings) — so a wall of
  open tool windows costs almost nothing while idle.
- **AccuracyCoin gate pinned to an exact 141/141.** The gate asserted only a
  coarse 60% floor — too loose to catch a single-test regression (an A/B probe
  disabling the delayed-`CopyV` dropped exactly the Hybrid Addresses test to
  140/141 yet still cleared 60%). It now also asserts zero failing tests.
- **The `≤ 2 ms` frame-cost figure is now labeled a design-phase aspiration, not
  a gate.** It was written before the cycle-accurate core existed; the core
  measures ~3.8 ms (~23% of the NTSC budget) and that is knowingly accepted. The
  remaining bulk is accuracy-required work, and the obvious levers were already
  measured and *rejected* (`emit_pixel` elision and the SIMD blitter were both
  **slower**). Recorded so no contributor optimizes toward it by trading accuracy.
- **libretro core license declared as `GPLv3`**, matching the notation mesen /
  melonDS / bsnes use (SPDX `GPL-3.0-or-later` stays in the Cargo metadata).

### Verified (no change required)

- **SMB left-edge and the hybrid-address (Rad Racer) render.** Both were
  investigated under reproduce-before-fixing discipline and found *already
  correct* in the shipped build — resolved by the v2.0.0 "Timebase" rewrite and
  the v2.0.3 2-cycle-ALE promotion, predating the report. SMB's leftmost
  background column renders real content; the hybrid-address model passes the
  authoritative AccuracyCoin test and renders Rad Racer cleanly. See ADR 0030's
  v2.3.0 update.

### Documentation

- **Hybrid-address provenance finalized** — `NOTICE`, `docs/originality-and-provenance.md`
  §4 and ADR 0030 move from "TriCNES-calibrated, being reworked" to "verified
  correct, documentation/oracle-derived".
- **TriCNES is no longer described as "transistor-level"** — it is a
  cycle-accurate C# emulator with a sub-cycle state machine. The term properly
  denotes die-derived simulations (`Visual2C02` / `phantom2c02`), which the repo
  cites correctly. Corrected in source, `NOTICE`, README, ADR 0030, and the
  published v2.0.2 / v2.2.5 release notes.
- **GeraNES reference comments corrected** — dangling source-file paths and a
  quoted C++ line were reworded to state honestly that its source was consulted
  as a cross-reference for nesdev-documented behavior, with no code copied
  (reviewed two-sided against the upstream source and the nesdev register maps).
  Recorded as an assessment for expert review, not a self-certification.

## [2.2.9] - 2026-08-04 - "Studio II" (relicense to GPLv3 + TAS/movie wiring + detachable tool windows)

The fourth step of the **v2.2.6 → v2.3.0** NESdev-remediation line. Its headline is
a **licensing and provenance correction** — RustyNES is **relicensed to
GPL-3.0-or-later** as the derivative work of GPL emulators it is — alongside three
forum-reported fixes (TAStudio edits, `.bk2` playback, tool-window detachment). The
code changes are frontend-only, so the deterministic chip stack, save-states, and
every golden vector are byte-identical: **AccuracyCoin 141/141, nestest 0-diff**.

> **Windowing — honest scope.** The detach affordance is **native-only and currently
> *embeds***: the frontend is a single-viewport `egui_winit` integration, so
> `show_viewport_immediate` renders a detached panel *inside* the main window, not as
> a separate OS window — so this does **not** yet fully resolve the Windows-10
> "trapped window" report. True OS-window detach needs multi-viewport render-loop
> wiring, tracked as a v2.3.0 follow-up.

### Changed — License: MIT/Apache-2.0 → GPL-3.0-or-later

- **RustyNES is relicensed to GPL-3.0-or-later** (ADR 0036). A NESdev community review
  established that it **incorporates code derived from GPL emulators** — principally
  **Mesen2** (GPL-3.0-or-later: CPU unstable stores, PPU sprite-eval/OAM, ~15 mapper
  boards, Bisqwit NTSC tables, EEPROM/UNIF/debug-symbol/PGO code) and, for several
  mappers and the FDS drive model, **puNES / FCEUX / Nestopia** (GPL-2.0-or-later).
  The pre-v2.2.5 comments said as much ("Faithful port of Mesen2's …"); the v2.2.5
  "no GPL source incorporated" / MIT-Apache position was wrong and is withdrawn.
- **Credit is given, per subsystem.** `docs/originality-and-provenance.md` leads with
  the file-by-file derivation table; `NOTICE` attributes every GPL upstream; each
  derived file carries an accurate `SPDX-License-Identifier` + provenance header (the
  old imprecise "port of" comments are not restored — the headers are their accurate
  replacement). Incorporated permissive components (emu2413/MIT, TriCNES/MIT,
  rcheevos/MIT, blip_buf/LGPL-2.1-or-later, fonts) keep their notices. Zero
  emulation-core behavior change.

### Fixed

- **TAStudio piano-roll edits now drive the emulator.** `handle_tas_requests` applied
  `SetInput` to the editor's `input_log` only and never re-seeked the `Nes`; it now
  re-derives through `TasEditor::seek` after the batch, matching the scripting bridge.
- **`.bk2` playback honors the movie's `LogKey` column order.** The importer mapped
  columns by a fixed order and ignored the `LogKey:` header, driving the wrong buttons
  on movies ordered differently; it now parses the real order (falling back to the
  standard order when absent) and surfaces parse errors on the status bar.

### Added — Provenance & license firewall (+ import hardening)

- **Guardrails ruleset + post-mortem.** `docs/ai-emulator-provenance-guardrails.md` (a
  preventive, console-agnostic reference-firewall / attribution / license ruleset,
  shared as community best-guidance) and `docs/provenance-failure-postmortem.md` (the
  forensic root-cause of how GPL code was reproduced despite a black-box instruction,
  then laundered, and how it was corrected). Themed PDFs of both in `ref-docs/`.
  Ingested into `AGENTS.md` as the top development rule.
- **Reference firewall — the reference-emulator clone removed.** The local clone is
  deleted from disk and stays gitignored (+ excluded from dockerignore /
  markdownlintignore / pre-commit / CodeRabbit) so the *copyleft* references' source
  is out of reach; in-source citations were normalized to upstream form (comments-only,
  byte-identical), and the §1 derivation table audited for completeness. MIT TriCNES
  is the deliberate exception, vendored in-repo with attribution.
- **`.bk2` import hardened against a `LogKey` allocation-amplification DoS** —
  `parse_log_key` reads only the console/P1/P2 groups from the `split('#')` iterator
  instead of collecting every `#`-group; behavior is identical for valid movies (new
  `log_key_bounded_against_pathological_group_padding` regression test).
- **Detachable / floating tool windows (native).** A shared `detachable_window` helper
  gives 18 debugger/tool panels a "⧉ Detach" button via `show_viewport_immediate`,
  each preserving its geometry (wasm stays docked). Currently embeds rather than
  opening a separate OS window — see the honest-scope note above.

## [2.2.8] - 2026-08-04 - "Aperture II" (gamma-aware scanlines + sharper CRT)

A **presentation-fidelity** release addressing the NESdev-forum feedback on
gamma-aware resampling and bilinear-soft scanlines. **Presentation-only — nothing
here touches the emulation core**, so the pre-shader framebuffer, save-states, and
every golden vector are byte-identical (AccuracyCoin 141/141, nestest 0-diff), and
the **shipped native default is byte-identical** to v2.2.7 (the native sRGB
surface passes `aux = 0`, which selects the exact pre-v2.2.8 scanline profile;
the new linear-light + sharper-scanline path activates only for a non-zero
`aux`, set on the WebGL2 non-sRGB path and when the scanline knob is raised).
The base BLEP audio and the advanced CRT stacks
(royale/guest/megatron, already gamma-correct) are untouched.

> **Visual verification pending.** These are shader/appearance changes; naga
> validates that the WGSL compiles and the native/wasm builds are clean, but the
> on-screen result must be confirmed on a real display + a browser (WebGL2).

### Changed

- **Gamma-correct scanlines + aperture mask (base CRT pass, `CRT_WGSL`).** The
  scanline/mask *darkening* now happens in **linear light**. On the native path
  (sRGB texture + surface) the sampler/surface already convert, so the shader
  leaves it linear (`aux.y = 0`, output byte-identical). On a plain UNORM path
  (**WebGL2**, which does neither) the shader now sRGB-decodes on read and
  re-encodes before output (`aux.y = 1`) — fixing a real browser-only gamma bug so
  a scanline valley is 50% of the *linear* luminance, not the encoded value. The
  round-trip uses the **exact IEC 61966-2-1 piecewise sRGB transfer** (the
  `0.04045` / `0.0031308` breakpoints + a 2.4 exponent), not a `pow(2.2)`
  approximation, so the WebGL2 result matches the hardware sRGB surface the native
  path uses bit-for-bit.
- **Sharper scanlines (`aux.x`, default 0.5).** The scanline profile blends from
  the original soft parabola (0) to a narrow Gaussian beam (1) for crisp vertical
  boundaries instead of the linear-sampler blur — the sharper scanlines the
  feedback asked for. Only visible when scanlines are enabled; `aux.x = 0`
  reproduces the pre-v2.2.8 profile exactly. Wired on the desktop
  (`rustynes-frontend`), Android (`rustynes-android`), and iOS Metal
  (`rustynes-ios`) hosts via the shared 16-float CRT uniform
  (`rect + crop + params + aux`) — all three set `aux` identically for the
  scanline/CRT filters, so the corrected profile is consistent across platforms.

## [2.2.7] - 2026-08-04 - "Timbre II" (expansion-audio fidelity: VRC6 + Sunsoft 5B)

An **expansion-audio accuracy** release addressing NESdev-forum feedback. Driven by a
measure-first cross-reference of VRC6 and Sunsoft 5B against **11 reference emulators**
(Mesen2/MesenCE, ares, higan, nestopia, fceux, tetanes, rustico, GeraNES, puNES, BizHawk)
plus the NESdev wiki — because a Mesen2-only comparison hides where Mesen2 itself is the
outlier. **The base 2A03 output is byte-identical** (these are expansion-only changes:
`mix_audio()==0` for non-expansion mappers), so **AccuracyCoin holds 141/141 (100.00%)**,
nestest is 0-diff, and blargg/kevtris are unchanged. The base BLEP decimator was
independently verified excellent (SFDR **81.6 dB**, `rustynes-apu` spectral test).

### Changed

- **VRC6 level recalibrated to the field/hardware consensus** — a full-volume VRC6 pulse
  is now **≈1.0×** a 2A03 pulse (was ~1.506×). `VRC6_MIX_SCALE` 979 → 650. The prior
  1.506× mirrored **Mesen2's specifically louder mixer convention** (Mesen2 weights VRC6
  `×5`); a reviewer flagged VRC6 as too loud, and the cross-reference confirmed Mesen2 is
  the loud outlier: the NESdev wiki says the VRC6 pulses are "roughly equivalent to the
  pulse channels of the 2A03", and rustico / tetanes / BizHawk encode a VRC6 pulse == a
  2A03 pulse *exactly* (ares/higan/nestopia reach the same via `sum/61`). The `db_vrc6a/b`
  oracle target moved 1.506 → 1.000 and the two snapshots were re-blessed (audio-only —
  framebuffer + cycle count byte-identical). VRC6's per-channel balance (linear
  `pulse+pulse+saw`, saw 0–31 vs pulse 0–15) was already correct and is unchanged.
- **Sunsoft 5B envelope now uses the exact 5-bit 1.5 dB/step DAC** — the envelope-mode
  amplitude path indexes a new 32-level `SUNSOFT5B_LOG_VOL32` table (×1.1885/step = +1.5 dB,
  matching nestopia/rustico) at full 5-bit resolution, instead of truncating the envelope
  to 4-bit (the wiki-named 3 dB approximation). Fixed 4-bit volume tones (already correct
  3 dB/step) and the 5B absolute level (1.265×) are unchanged; the odd entries of the
  32-level table equal the 4-bit table exactly (guarded by a new unit test). Extant 5B
  test-ROM snapshots stay byte-identical; envelope-modulated 5B music now gets the exact
  curve.

## [2.2.6] - 2026-08-04 - "Almanac" (de-monetization + provenance accuracy)

A **de-monetization and provenance** release. RustyNES is now permanently
open-source and **income/profit-free forever** (ADR 0035): all planned
monetization is removed and the native Android/iOS apps are kept as **free FOSS
apps** — no ads, no tracking, no paid unlock, every feature available. **Zero
emulation-core behavior changes** — the `#![no_std]` chip stack, save-state / TAS
/ netplay formats, and every golden vector are byte-identical, so **AccuracyCoin
holds 141/141 (100.00%)** and nestest is 0-diff by construction.

### Removed

- The `rustynes-monetization` crate and `docs/monetization/` are deleted and the
  workspace member removed (no emulation crate ever depended on it). The Android
  paid layer (Play Billing `LicenseManager`, the AppLovin MAX / RevenueCat
  `MonetizationGate` + ad gates, the demo/paywall UI + strings, AdMob/AppLovin
  manifest entries, and the billing/ad Gradle deps + BuildConfig keys + the
  monetization cargo/uniffi tasks) is removed; `MainActivity` no longer gates any
  feature behind an unlock/demo. The iOS paid layer (the StoreKit `StoreManager`,
  the `appStore` monetization build channel, billing entitlements) is removed.

### Changed

- The `foss` / `play` Android flavor split is retained but now only distinguishes
  the pure-AOSP build from the build carrying the *free* Google-Play services
  (Play Games achievements, Cast, Integrity, in-app update, cloud save) — no ads,
  no billing. Nightly Rust is now used only by `cargo fuzz`.
- ROADMAP / `docs/STATUS.md` / version plans reframed to the OSS/income-free
  position; the freed v2.3.0 slot is repurposed for accuracy/fidelity work.
- **Provenance accuracy (ADR 0035 + ADR 0030):** `NOTICE` and
  `docs/originality-and-provenance.md` now disclose honestly that the PPU
  octal-latch / hybrid-address *timing* was calibrated to TriCNES's per-dot
  behavior (beyond black-box oracle use), which reproduced a TriCNES-specific
  artifact that mis-renders mid-render `$2006` writes (e.g. Rad Racer). This is
  scheduled to be reworked to be documentation-derived in v2.3.0.

### Added

- **ADR 0035** "RustyNES is permanently non-commercial (no monetization)";
  **ADR 0025** marked Superseded and **ADR 0027** amended (its App-Store §4.7
  ROM-compliance rules stay — valid for a free app; the ad/ATT/StoreKit-unlock
  clauses are removed).

## [2.2.5] - 2026-08-03 - "Colophon" (provenance, licensing, and documentation integrity)

A **provenance, licensing, and documentation-integrity** release, prompted by
community review of the project's licensing and AI-assisted origins. **Zero
emulation-core behavior changes**, so **AccuracyCoin holds 141/141 (100.00%)**, nestest is
0-diff, and the `#![no_std]` chip stack, save-state / TAS / netplay formats, and
every golden vector are byte-identical to v2.2.4 by construction.

### Changed

- **In-source "port" comments corrected.** A full-tree audit found comments that
  described implementations of publicly-documented hardware behavior (the CPU
  unstable-store opcodes, the PPU sprite-evaluation / OAM models, and numerous
  mapper register decoders) as "ports of" copyleft emulators (Mesen2 — GPLv3;
  puNES — GPLv2). Those behaviors are implemented from the NESdev wiki, published
  datasheets, and the documented 6502 behavior, and were cross-checked against
  reference emulators as *oracles*; the comments were reworded to say so. No
  GPL-licensed emulator source is incorporated.
- **CRT shaders & NTSC filters reworded.** `crt_royale` / `crt_guest` / `megatron`
  and the Bisqwit / EMMIR NTSC filters were reviewed at source level and reframed
  from "port / condensation of X" to independent single-pass reimplementations of
  the *look and technique* (copyright protects code expression, not a visual look);
  no upstream shader source is incorporated. The comment claiming tables were
  "ported verbatim from Bisqwit's C" was corrected — those tables encode the
  NESdev-documented NES composite signal.
- **`blip.rs`** no longer mislabels `blip_buf` as BSD/MIT (it is LGPL-2.1+); the
  file is an independent BLEP implementation and now says so.
- **README** toned down and corrected: added an AI-assistance disclosure, removed
  a comparison graphic with inaccurate details, fixed a mislabeled
  ("sub-cycle accuracy") screenshot caption, and synced Acknowledgments with
  `NOTICE`.

### Added

- **`NOTICE` rewritten** to disclose the behavioral-oracle use of GPL emulators
  (Mesen2/MesenCE, higan, **GeraNES**, ares, FCEUX, Nestopia UE, puNES — no code
  incorporated), attribute the incorporated permissive components (emu2413,
  TriCNES, rcheevos — all MIT, with the MIT text), the bundled fonts (Font Awesome;
  Press Start 2P / OFL) and test ROMs, and credit the CRT-shader / NTSC-filter
  visual influences as independent reimplementations. GeraNES (GPL-3.0-only), cited
  across ~58 files, was previously undisclosed.
- **New `docs/originality-and-provenance.md`** — an honest account of where
  RustyNES advances, diverges from, or independently re-derives NES emulation
  technique, its development timeline, and its full license posture (including that
  the project is heavily AI-assisted).
- **Press Start 2P OFL text** added to the Android app assets (it shipped without
  the required OFL text; desktop and iOS already carried it).

### Fixed

- **`tests/roms/LICENSES.md`** — a false exclusion claim (four Holy Mapperel mapper
  ROMs stated as excluded were in fact committed), a stale crate path, and the
  AccuracyCoin sub-test count; and added blanket coverage for the committed
  directories not individually tabulated (328 committed `.nes` total, none
  commercial).

## [2.2.4] - 2026-07-24 - "Cartridge" (libretro core builds/installs for RetroArch)

A **libretro / RetroArch distribution** cut. Its purpose is that the RustyNES
core builds and installs cleanly through the Libretro buildbot
(<https://git.libretro.com/libretro/RustyNES>) so RetroArch users can pull it
from the in-app core downloader. **Zero emulation-core changes**, so
AccuracyCoin holds **141/141 (100.00%)**, nestest is 0-diff, and the `#![no_std]`
chip stack, save-state / TAS / netplay formats, and every golden vector are
byte-identical to v2.2.3 by construction.

### Libretro / distribution

- **The libretro core is confirmed complete and up-to-date with every recent
  change, and builds for the buildbot ABIs.** `crates/rustynes-libretro` wraps
  `rustynes-core`, so it inherits the v2.2.3 work automatically and required no
  code change to carry it: the fast PPU dot path (now the core default) is
  active; the `PPU_SNAPSHOT_VERSION` 8 + APU v4 save-state schema is transparent
  because `get_serialize_size` / `on_serialize` size and emit the *current*
  snapshot via `Nes::snapshot_core_into` rather than a hardcoded layout; the
  `Mapper::mix_audio` i32 widening, the Zapper model, and the `mNNN_` mapper
  rename are all below the crate's public dependency surface. Both buildbot
  cross-ABIs the GitHub early-warning gate models — `x86_64-pc-windows-gnu` and
  `aarch64-linux-android` — `cargo check --release -p rustynes-libretro`
  clean.
- **`rustynes_libretro.info` metadata corrected** (the file RetroArch's core
  downloader reads to learn the core's capabilities):
  - **`disk_control` `false` → `true`** — the real fix. The FDS multi-side Disk
    Control interface (`enable_disk_control_interface()` + the
    `on_set_eject_state` / `on_get_image_index` / … callback trampolines) has
    been wired since the buildbot recipe landed, but the `.info` advertised it
    as absent, so RetroArch's Quick Menu → Disk Control never surfaced multi-disk
    FDS swapping.
  - `display_version` `v1.0.0` → `v2.2.4` (stale since the v1.0.0 era).
  - Description mapper count `168` → `172`, and a note that FDS multi-disk
    swapping runs through the Disk Control interface.
- Documented follow-up: libretro **core options** (region / overscan / palette /
  accuracy toggles) remain unexposed. `core_options = "false"` is accurate, not
  stale — a deliberate future enhancement, not a v2.2.4 gap.

### Tooling

- **The Antigravity PR reviewer is standardized onto the shared template**
  (`scripts/agy-review.sh` + `.github/workflows/antigravity-review.yml`), the
  same canonical version now installed across RustyNES / RustySNES / RustyN64.
  It carries the large-diff handling (a diff too big to inline goes to `agy` as
  a file), the 20,000-line `gh pr diff` API-limit local-`git diff` fallback, the
  `isCrossRepository` fork gate, fail-closed metadata, default-branch checkout
  with `persist-credentials: false`, and the `synchronize` auto-re-review
  trigger.
- **Reviewer security hardening (found by the reviewer itself).** The
  Antigravity reviewer, run against this PR, flagged five security regressions
  the standardized template had relative to RustyNES's prior version — all
  fixed: `printf '%q '`-escaped `script(1)` fallback (was a raw `${flags[*]}` in
  `sh -c` — command injection), an author-scoped comment-deletion filter (was
  marker-only — arbitrary comment deletion), removal of the unscoped SQLite
  conversation-store fallback (a shared-runner data-leak vector), stripping
  `GH_TOKEN` / `GITHUB_TOKEN` from `agy`'s environment (`env -u`), and the
  `issue_comment` author-association re-check restored in the script.
- **Large-diff handoff made readable (found by the reviewer, fixed on the
  runner).** The reviewer files a diff too large to inline into a gitignored
  working-tree scratch dir (`.agy-review-work/`, relocated from `.git/` after the
  reviewer flagged the hidden-dir read risk) and tells `agy` to read it — but
  that on-disk handoff never actually worked: `agy`'s sandboxed file tool
  resolves relative paths against its own workspace root, not the shell CWD, so
  the file came back "does not exist" and the review was empty. Latent until a
  diff first crossed the ~90 KB inline budget (below it the diff is inlined and
  the file path is never exercised). Root cause and fix proven on the live `agy`
  runner with a three-way probe under the exact review flags: the prompt now
  hands `agy` an **absolute** path, and `--add-dir "$PWD"` adds the checkout to
  `agy`'s sandbox workspace — but only in file-handoff mode, so an inline review
  keeps zero filesystem access. All changes ride the canonical template so the
  three consuming repos stay in sync.

## [2.2.3] - 2026-07-23 - "Datum" (fast dot path promoted + PGO shipped + the last two mapper residuals closed)

A performance and accuracy-closure patch. No *regression* on the deterministic core —
**AccuracyCoin 141/141, nestest 0-diff**, `visual_regression` and the APU oracles unmoved.
(This release does change shipped-default behavior by design: the fast PPU dot path became
the default, two Holy Mapperel mapper residuals were closed, the Sunsoft 5B level was
calibrated, and the save-state schema gained `PPU_SNAPSHOT_VERSION` 8 + an APU v4 tail —
each an intentional, oracle-gated change, detailed below.)

### Performance

- **The specialized PPU fast dot path is now the default** (~−11% frame time on
  rendering-heavy content; `nes_run_frame_nestest` 4.43 ms → 3.93 ms). Differential-tested
  byte-identical every frame since v2.1.8 (`fast_dotloop_diff.rs`); it had shipped off
  with no reachable caller. A `[emulation] fast_dotloop` escape hatch defaults on.
- **Release builds ship the PGO binary on `x86_64-unknown-linux-gnu`.** `release.yml`
  now consumes the profile-guided build behind the existing >3%-faster-and-byte-identical
  gate; a gate miss silently keeps the plain asset (macOS/Windows unchanged).
- **A same-runner relative frame-time regression gate** (`bench_relative_check.sh`) now
  fails a >10% back-to-back slowdown, closing the hole where the loose absolute ceiling
  let a 2.5× slowdown pass.

### Fixed

- **The last two Holy Mapperel residuals are closed — all 17 ROMs report `detail=0000`**:
  MMC1's two software WRAM write-protect layers (`$E000` bit 4 + SNROM's `chr_is_ram`
  CHR-register layer) and FME-7's open bus on the RAM-selected-but-disabled window, both
  via the trait's `cpu_read_unmapped` contract. Validated 60/60 commercial (incl. seven
  battery-backed MMC1 saves) + 138/138 extended.
- **Sunsoft 5B expansion audio calibrated (~23 dB louder).** The DAC shape was already
  exact; the level was blocked by `Mapper::mix_audio` returning `i16` (full-scale 5B =
  34,761). Widened to **`i32`** and calibrated against Mesen2 (`db_5b` 0.069× → 1.265×);
  `nsf_expansion::mix` likewise widened + unclamped. Every other board is byte-identical.
- **Run-ahead cost three AccuracyCoin tests** (138/141 in-app vs 141 headless): the PPU
  snapshot omitted the sprite-evaluation FSM + OAM-data-bus state. A new
  `PPU_SNAPSHOT_VERSION` **v8** tail restores **141/141 with run-ahead on**; an APU **v4**
  tail closes a matching warm-reset `$4017` gap. Netplay/TAS take the same round-trip.
- Seven stale commercial-oracle audio rows re-blessed (level constants changed in
  v2.1.6 / this line; a new `expansion_level_tripwire` CI test pins them), and the
  expansion-audio snapshot window widened so it actually observes the expansion chip.

### Changed

- **Mapper modules renamed for the board they emulate** (`sprintN.rs` → `mNNN_<board>.rs`,
  27,631 lines, ~110 boards) — proven content-preserving by a byte-for-byte item
  comparison (930 items, 0 altered) and an identical 172-ID dispatch table.
- `PPU_SNAPSHOT_VERSION` 7 → 8 **breaks pre-v8 `.rns` save states** (clear
  `VersionMismatch`, per ADR 0028); movies/netplay re-derive from power-on and are
  unaffected.

### Added

- **Two optimizations measured and REJECTED, documented with their numbers**
  (`docs/performance.md`): `ppu-idle-line-fast` (made the shipped default slower — off)
  and the P4 `cpu_clock` levers (already implemented; remaining lever ≤1.9%).
- A **save-state schema audit** standing test (`snapshot_schema_audit.rs`) that fails if
  a chip field is added without its serializer — the mechanical net that found the v8/v4
  gaps above.
- An opt-in **Zapper beam-relative light model** (default off; no pass/fail light-gun ROM
  exists to adjudicate it) and the Antigravity self-hosted PR reviewer (CI-only).

Full detail: the GitHub Release and `.github/release-notes/v2.2.3.md`.

## [2.2.2] - 2026-07-21 - "Conduit" (libretro buildbot 10/10 + CI supply-chain hardening + single-source toolchain)

A **build, distribution, and CI-integrity patch**. **Zero emulation-core changes** — no
file under `crates/rustynes-{cpu,ppu,apu,mappers,core}` is touched, so **AccuracyCoin
holds 141/141 (100.00%)** by construction, nestest 0-diff, and `pal_apu_tests` 10/10 /
`visual_regression` / the 60-ROM oracle are unchanged from v2.2.1. The one behavioral
improvement in a shipped artifact: the libretro **tvOS** core now builds with
`panic = "abort"` like every other platform.

### Fixed

- **Libretro buildbot: 1 of 10 jobs green → all ten building** (the last step before
  RustyNES lands in RetroArch's built-in core downloader). Three independent, our-side
  defects, all invisible until the buildbot ran: 8 jobs missing cross-compile targets
  (each now `rustup target add ${RUST_TARGET}` into the pinned toolchain); the upstream
  `rust-libretro 0.3.2` MinGW keycode-signedness bug (worked around by pointing bindgen's
  clang at the MSVC triple for the `-gnu` targets); and the tvOS `+nightly -Zbuild-std`
  override, now dropped since `aarch64-apple-tvos` ships a complete prebuilt std
  (`panic_abort` included).

### Security

- **`persist-credentials: false` on all 19 CI checkouts** (closes #318) —
  `actions/checkout` otherwise writes `GITHUB_TOKEN` into `.git/config`, readable by the
  unreviewed PR code nearly every job builds/runs. Audited: no job needs the credential;
  the highest-exposure site was `web.yml`'s `build` (`pages: write` + `id-token: write`).
- **The release tag-existence check is now fail-closed** (`release-auto.yml`): a
  `gh api git/matching-refs` call that can never confuse "lookup failed" with "tag
  absent" (which used to risk re-releasing a shipped version), with an explicit
  non-array guard.
- **`dtolnay/rust-toolchain` SHA-pinned** off the moving `@master` branch (it installs
  the compiler and feeds 12 of 19 checkouts), keeping the Dependabot-readable `# v1`
  marker.

### Changed

- **One toolchain everywhere: `rust-toolchain.toml`'s `channel` is the single CI source
  of truth.** `rust-setup` parses it and fails closed; no toolchain version literal
  remains under `.github/`, and **no `nightly` on any build path** (nightly survives
  only for `cargo fuzz`).
- **New `libretro-cross` CI job** cross-checks `rustynes-libretro` against the buildbot
  ABI families a Linux runner can model (MinGW-Windows, Android/NDK) — the early-warning
  gate that was previously absent.
- **Dependabot #313–#315 consolidated** into one reviewed change plus a `cargo update`
  sweep, no source changes (lz4_flex 0.13 → 0.14 with an explicit `alloc` feature,
  tokio 1.52.3 → 1.53.1, and the production-dependencies group).

### Added

- The libretro buildbot recipe (`.gitlab-ci.yml`, issue #311) covering all ten platform
  jobs, plus libretro core feature completion (native memory-maps for `rcheevos`, an FDS
  load-path fix + multi-side disk-control, native Game Genie cheats, fast-forward
  audio-skip).

Full detail: the GitHub Release and `.github/release-notes/v2.2.2.md`.

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

An expansion-audio fidelity cut. **Base 2A03 NTSC output stays byte-identical** —
expansion audio is a separate additive `mix_audio` term — so AccuracyCoin holds
**141/141**, `blargg_apu_2005`, nestest, and `visual_regression` are unchanged; only the
three `db_vrc6`/`db_mmc5` expansion snapshots were re-blessed (audio hash only).

### Added

- **Expansion-audio decibel oracle** (`audio_expansion.rs` `level_db_*`) — each
  bbbradsmith `db_*` ROM now asserts the measured expansion/reference peak ratio against
  the Mesen2 / hardware target (triangle ÷ square ≈0.524, VRC6 ≈1.506, MMC5 ≈1.000, N163
  1-ch ≈6.02), upgrading the prior byte-exact `insta` snapshots into a real level oracle.
- **Audio Mixer panel** (Tools → Audio Mixer) — per-source gain sliders + mutes for the
  five 2A03 channels and the detected on-cart expansion chip (VRC6/VRC7/MMC5/N163/5B/FDS),
  presets (Authentic / Balanced / Expansion boost), and per-channel oscilloscope + VU
  meters. A frontend re-weight of the determinism-safe `channel_gain`/`channel_mask`
  overlay (byte-identical at unity, never serialized).
- **VRC7 patch-set verification** — all 15 melodic (+3 rhythm) patches pinned
  byte-identical to the canonical Nuke.YKT dump; plus Sunsoft 5B log-DAC and Namco 163
  long-period wavetable unit tests.

### Changed

- **Expansion-audio channel levels calibrated to the hardware / Mesen2 `db_*` levels:**
  VRC6 `256 → 979` (≈0.39× → ≈1.51× the 2A03 pulse), MMC5 `256/16 → 650/40` (≈1.0×), and
  **Namco 163 `64 → 261`** (≈1.48× → ≈6.02× 1-channel — it was ~12 dB too quiet; no
  reference emulator attenuates N163). The N163 fix is shared with the NSF path. *(VRC6
  was later re-corrected to ~1.0× in v2.2.7.)*

### Deferred (documented)

- Sunsoft 5B absolute level and VRC7 FM level remain honest documented gaps
  (`docs/accuracy-ledger.md`) — the 5B needed a wider-than-`i16` mix path (closed in
  v2.2.3); the VRC7 FM level has no clean square-vs-square oracle.

## [2.1.5] - 2026-07-11 - "Fathom" (regression net & residual — Holy Mapperel mapper regression net + PAL APU frame-counter 10/10 + real TURN NAT-retransmit production fix + fat-LTO A/B validation + MMC3 F5.0 A12-phase study; "Vernier")

A regression-net and residual cut. Additive/observational and NTSC-byte-identical —
AccuracyCoin holds **141/141**, nestest 0-diff, the commercial byte-identity oracle
unchanged.

### Added

- **Mapper bank-reachability + IRQ regression net** — the tepples **Holy Mapperel**
  cartridge-assembly ROMs wired into CI (`holy_mapperel.rs`, 17 zlib ROMs,
  framebuffer-hash pinned with *settled* + *non-blank* guards): each detects its mapper
  from bank/mirroring response, proves every PRG/CHR bank reachable, and exercises WRAM +
  MMC3/FME-7 IRQ. 15/17 report `detail=0000`; the two MMC1 + two FME-7 ROMs surface a
  documented WRAM-protection residual (closed later in v2.2.3), recorded in
  `docs/accuracy-ledger.md`.
- **First PAL-region APU oracle** — blargg's `pal_apu_tests` (10 sub-ROMs) wired into CI
  via a new on-screen-verdict runner, which also **corrects a false oracle** (the prior
  `$6000`-status check passed vacuously on these PRG-RAM-less NROMs). Modeled the 2A07 PAL
  frame-counter step positions (region-gated, NTSC tables untouched) and fixed the length
  halt/reload write-ordering — **10/10 pass** (honestly 3/10 pre-model). NTSC byte-identity
  preserved: `blargg_apu_2005` 11/11, AccuracyCoin 141/141.
- **MMC3 R1/R2 residual A12-phase study** (ADR 0002 F5.0) — a default-off observational
  probe (`mmc3-a12-phase-probe`) that refines the F5.0 finding with fresh instrumentation:
  the two `scanline_timing` residuals have zero post-access IRQ-clocking rises, but the two
  "reload/set-IRQ-every-clock" residuals have 4 each, so "no post-access rise" is
  ROM-specific, not structural. No default/scheduler change; all four residuals stay
  `#[ignore]`'d; the ares-style M2-edge low-time filter remains the one untested axis-B
  lever.

### Changed

- **fat-LTO release profile measured, documented, and validated** — the existing
  `lto = "fat"` + `codegen-units = 1` default is now backed by an in-repo same-host A/B
  (**+8.4%** to **+20.8%** on cross-crate paths, within noise on the single-crate control),
  verified byte-identical (AccuracyCoin 141/141, nestest 0-diff). No default change;
  corrects `docs/performance.md`'s stale "thin" text and the stale `139/139` PGO comments.

### Fixed

- **Netplay: the native TURN client now retransmits (RFC 5389 §7.2.1)** — a real production
  bug where symmetric-NAT relay fallback aborted on a single dropped UDP datagram
  (`Allocate`/`CreatePermission` were sent once). It now retransmits every 250 ms until
  timeout, recovering transparently (STUN/TURN requests are idempotent). This also fixed
  the intermittent `nat_connect_loopback_relay` flake on `windows-latest` that had been
  blocking `release-auto`. The determinism contract (session-digest agreement) is unchanged.

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
