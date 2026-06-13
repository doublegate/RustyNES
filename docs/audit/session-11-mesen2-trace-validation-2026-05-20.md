# Session-11 — Mesen2 trace tooling validation + first-divergence findings (2026-05-20)

**Status.** Session-10's PPU observability tooling is now in a working
state, end-to-end, against a real Mesen2 reference. **No code fix
landed** on the Cascade A sprite-eval cascade target: the comparative
trace data showed that the load-bearing first divergence between
Mesen2 and RustyNES on `AccuracyCoin` is an **early-boot init-timing
divergence** at frame 4 (OAM-load-via-DMA), NOT the sprite-eval FSM
itself. The Cascade A target lives downstream of this init divergence
and cannot be safely fixed without first resolving the upstream
issue.

This document records:

1. The seven Mesen2 Lua API mismatches that prevented the Session-10
   `mesen2_ppu_trace.lua` from emitting anything when run for the
   first time against a real Mesen2 binary.
2. The fixture-side env-var extensions needed to make the
   per-frame Mesen2 reference trace comparable to the
   per-PPU-dot RustyNES capture.
3. The first per-frame divergence observed in the wild — at
   AccuracyCoin frame 4 cold boot, BEFORE any sprite-eval can run.
4. Concrete next-session inputs.

## TL;DR

* **Mesen2 CLI.** `--testRunner <rom_path> <script.lua>` is the
  correct flag (NOT `--script`). Mesen2 is a .NET / Avalonia
  application that initialises X11 even in `--testRunner` mode, so
  Linux headless captures require `xvfb-run -a` (verified against
  Mesen2 0.42.0).
* **Mesen2 Lua sandbox.** `io.open` for arbitrary paths is blocked
  unless `~/.config/Mesen2/settings.json` has
  `"AllowIoOsAccess": true`. The reference-trace script writes its
  binary output via `io.open`, so the setting MUST be enabled before
  the trace can be captured. There is no CLI flag to toggle this.
* **Mesen2 Lua API mismatches.** The Session-10 script was written
  against the published Lua API docs but had seven distinct bugs vs
  the actual Mesen2 0.42 runtime, ALL of them silent-failure shapes
  (script registered, callbacks did or didn't fire, no records
  emitted, exit 0). The fixes are landed in `scripts/mesen2_ppu_trace.lua`
  in this session; see the per-finding breakdown below.
* **Fixture env-var extensions.** New env vars
  `RUSTYNES_PPU_TRACE_SCANLINE_LO/HI`,
  `RUSTYNES_PPU_TRACE_DOT_LO/HI`,
  `RUSTYNES_PPU_TRACE_RAW_BOOT`,
  `RUSTYNES_PPU_TRACE_START_PRESS_LO/HI` make
  `crates/nes-test-harness/tests/ppu_state_trace_fixture.rs` produce
  per-frame traces (`scanline=240, dot=0`) directly comparable to
  Mesen2's Lua per-frame capture, and let RustyNES skip its
  hardcoded boot pre-roll so both emulators capture from the same
  cold-boot frame numbers with a Lua-script-driven Start press.
* **First per-frame divergence found.** At AccuracyCoin cold boot,
  Mesen2's OAM at scanline-240-dot-0 of frame 4 has the pattern
  `Y=$E3, tile=$FF, attr=$FF, x=$FF` (sprites parked at scanline
  227 = $E3, off-screen), while RustyNES at the same point has
  OAM = all-`$00` (the power-on default — the ROM hasn't yet written
  to OAM at all). RustyNES doesn't complete its OAM init until
  frame 5 and even then writes all-`$FF` without the `Y=$E3` pass.
* **Implication for Cascade A.** The Cascade A target
  (`VerifySpriteZeroHits` step 2 / `Sprite Evaluation :: Arbitrary
  Sprite zero` / `Misaligned OAM behavior`) cannot be the FIRST
  divergence to address. By the time the test runner is executing
  those tests (~frame 310+ of AccuracyCoin), OAM and likely many
  other state slots have diverged for hundreds of frames upstream.
  Sprite-eval fixes will continue to cascade because the input to
  sprite-eval (OAM + `OAMADDR` + scroll registers + rendering
  enables) is already in a different state than Mesen2's by the time
  AccuracyCoin reaches the test routine. Either the init-frame
  divergence must be resolved first (Session-12 priority) OR an
  alternative per-test ROM harness must be built where the test
  binary is reset-injectable at known state-points.

## Mesen2 Lua API mismatches (and fixes)

All seven were silent-failure shapes: no error, no log, no records
written. Detected via Lua-driven introspection scripts ran in
`--testRunner` mode with an X11 display from `xvfb-run`.

### 1. `emu.eventType.scanline` does NOT exist

The Session-10 script registered an `addEventCallback(.., emu.eventType.scanline)`
expecting a per-scanline tick. Mesen2 0.42's actual `eventType` enum
is:

```
nmi, irq, startFrame, endFrame, reset, scriptEnded, inputPolled,
stateLoaded, stateSaved, codeBreak
```

Source of truth:
`https://raw.githubusercontent.com/SourMesen/Mesen2/master/UI/Debugger/Documentation/LuaDocumentation.json`
+ runtime check `emu.eventType.scanline == nil`.

The finest-grained event Lua can subscribe to is `endFrame`, which
fires once per frame at PPU `(scanline=240, dot=0)`. A faithful
per-scanline OR per-dot reference trace canNOT be produced by Lua
alone; this is the design-time premise that needs revisiting in
ADR-0005. Two paths forward (both deferred past Session-11):

* Parse Mesen2's built-in trace log file format (text). The log is
  per-CPU-instruction, so PPU state at the instruction boundary is
  interpolable per cycle.
* Patch Mesen2's C++ core to expose a new `cycle` event type.

### 2. `emu.memType.oam` does NOT exist

The Session-10 script used `emu.read(i, emu.memType.oam, false)`.
Mesen2's `memType` enum has `nesSpriteRam` (primary OAM) and
`nesSecondarySpriteRam` (32-byte secondary OAM), NOT a generic `oam`.
Both NES-specific reads work correctly via `emu.read(addr,
emu.memType.nesSpriteRam, false)`.

### 3. `emu.getState()` returns a FLAT table with dotted-string keys

This was the most consequential discovery. The Session-10 script
accessed `state.ppu.frameCount`, `state.ppu.scanline`,
`state.ppu.control.nmiOnVBlank`, etc. — assuming nested subtables.

Mesen2 ACTUALLY returns a flat dict where the keys ARE the full
dotted paths:

```lua
-- WRONG (Session-10 assumption):
state.ppu.frameCount  -- always nil

-- RIGHT:
state["ppu.frameCount"]
state["ppu.scanline"]
state["ppu.cycle"]
state["ppu.statusFlags.verticalBlank"]
state["ppu.control.nmiOnVerticalBlank"]
state["ppu.spriteRamAddr"]
state["ppu.videoRamAddr"]
state["ppu.tmpVideoRamAddr"]
state["ppu.lowBitShift"]
state["ppu.highBitShift"]
state["ppu.xScroll"]
state["ppu.writeToggle"]
state["ppu.secondarySpriteRam0"] .. state["ppu.secondarySpriteRam31"]
state["cpu.nmiFlag"]
```

`pairs(state)` happens to LOOK like nested subtables because the
key names contain dots — iterating `state` with `pairs` returns
~294 entries on the NES, all top-level. The dump function in the
original Session-10 debug script printed `state.ppu.frameCount = 5`
which is the **printed-prefix** "state" plus the **raw key**
"ppu.frameCount", visually indistinguishable from nested-table
access. This is why introspection looked correct but direct field
access returned nil.

### 4. `nmiOnVBlank` is actually `nmiOnVerticalBlank`

A trivial name-drift. Affects PPUCTRL bit 7 reconstruction in
`build_record`.

### 5. `state.ppu.status` is actually `state.ppu.statusFlags`

Naming difference. The flat-key form is `state["ppu.statusFlags.verticalBlank"]`
etc.

### 6. PPUCTRL `nameTable` bottom-2-bits not exposed

`ppu.control.nameTable` (or any equivalent) doesn't exist in
Mesen2's state. The bottom 2 bits of PPUCTRL (nametable base
$2000/$2400/$2800/$2C00) are encoded in bits 10-11 of
`tmpVideoRamAddr`. The fixed script extracts them from there:
`ctrl_byte |= (t_val >> 10) & 0x03`.

### 7. `state.cpu.nmiFlag` access also needs flat-key form

`state["cpu.nmiFlag"]` not `state.cpu.nmiFlag`.

## Fixture changes

`crates/nes-test-harness/tests/ppu_state_trace_fixture.rs` was extended
with FIVE new env-var-controlled axes, all preserving the default
behaviour when unset:

* `RUSTYNES_PPU_TRACE_SCANLINE_LO` / `_HI` (defaults `0` / `239`):
  narrow the scanline capture range. Set both to `240` to capture
  scanline 240 only, matching Mesen2's endFrame timing.
* `RUSTYNES_PPU_TRACE_DOT_LO` / `_HI` (default unset; 0..=340):
  narrow the dot capture range. Set both to `0` for per-frame
  granularity.
* `RUSTYNES_PPU_TRACE_RAW_BOOT` (default `0`): when `1`, skip the
  hardcoded 300-splash + 6-frame-Start-press boot pre-roll. The
  fixture will then capture from cold-boot frame 0 onward. Required
  for Mesen2 comparison runs because the Lua-side script must
  inject its own Start press to keep both emulators' input timing
  in lockstep.
* `RUSTYNES_PPU_TRACE_START_PRESS_LO` / `_HI` (defaults `300` /
  `305`): in raw-boot mode, hold Start across this frame range so
  the test runner advances past the splash screen at the same
  frame numbers as the Mesen2 Lua script.

The per-frame Mesen2-comparison preset:

```bash
env RUSTYNES_PPU_TRACE_RAW_BOOT=1 \
    RUSTYNES_PPU_TRACE_START_FRAME=0 \
    RUSTYNES_PPU_TRACE_END_FRAME=15 \
    RUSTYNES_PPU_TRACE_SCANLINE_LO=240 \
    RUSTYNES_PPU_TRACE_SCANLINE_HI=240 \
    RUSTYNES_PPU_TRACE_DOT_LO=0 \
    RUSTYNES_PPU_TRACE_DOT_HI=0 \
    RUSTYNES_PPU_TRACE_OUT=/tmp/rustynes_cold.bin \
    cargo test -p nes-test-harness --release \
        --features test-roms,ppu-state-trace \
        --test ppu_state_trace_fixture -- --nocapture
```

Paired with the Mesen2 capture:

```bash
env MESEN2_PPU_TRACE_OUT=/tmp/mesen2_cold.bin \
    MESEN2_PPU_TRACE_START=0 MESEN2_PPU_TRACE_END=15 \
    MESEN2_PPU_TRACE_START_PRESS_LO=300 \
    MESEN2_PPU_TRACE_START_PRESS_HI=305 \
    timeout 60 xvfb-run -a /home/parobek/AppImages/mesen.appimage \
        --testRunner tests/roms/accuracycoin/AccuracyCoin.nes \
        scripts/mesen2_ppu_trace.lua
```

Diff:

```bash
target/release/ppu_trace_diff \
    --reference /tmp/mesen2_cold.bin \
    --actual /tmp/rustynes_cold.bin \
    --all-divergences \
    --skip-fields sprite_eval_n,sprite_eval_m,sprite_eval_found,\
sprite_eval_sec_idx,sprite_eval_copying,\
sprite_eval_overflow_search,sprite_eval_done,\
sprite_eval_read_latch,spr_shift_lo,spr_shift_hi,\
spr_attr,spr_x,at_shift_lo,at_shift_hi,nt_latch,\
at_latch,bg_lo_latch,bg_hi_latch,spr_count,\
spr_zero_in_line,nmi_line,secondary_oam
```

## First per-frame divergence (cold boot, frames 1..=15)

After fixing the Lua script and running per-frame captures of
AccuracyCoin from both emulators (cold boot, identical Start-press
timing), the diff tool reports:

| Frame | Field | Mesen2 | RustyNES | Note |
|-------|-------|--------|----------|------|
| 1     | ctrl  | `$01`  | `$00`    | Mesen2 has written PPUCTRL bit 0 (nametable bit) |
| 1     | v     | `$27C0`| `$0001`  | Mesen2 PPU has been written by `$2006` |
| 1     | t     | `$27BF`| `$0000`  | Same — Mesen2 game code has run further |
| 2     | (same as frame 1) | | | RustyNES still hasn't caught up |
| 3     | (same) | | | |
| 4     | oam_fnv1a64 | `$EA022799D3156B25` | `$D80AC658736BB725` | **OAM divergence starts**: Mesen2 has Y=`$E3`/tile,attr,x=`$FF` pattern; RustyNES still has all-zero OAM (power-on default) |
| 5+    | oam_fnv1a64 | `$EA022799D3156B25` | `$2590AF3457808025` | RustyNES has now written OAM but only the all-`$FF` first pass; Mesen2's `Y=$E3` second pass never happens in RustyNES |

The PPU register divergence (`ctrl`, `v`, `t`) at frame 1 indicates
**CPU execution is slower in RustyNES than in Mesen2 by some integer
number of frames** during boot — Mesen2's CPU has executed the
PPUCTRL/PPUADDR writes from the AccuracyCoin RESET routine
(`/tmp/AccuracyCoin.asm:369-372`) at least one frame before
RustyNES has.

The OAM divergence at frame 4 has the same root cause: in
AccuracyCoin's flow (line 425-427):
```asm
JSR ClearPage2     ; fill page 2 ($0200-$02FF) with $FF
LDA #02
STA $4014          ; OAMDMA from page 2
```
This is the "fill OAM with `$FF`" first pass. Mesen2 completes this
by frame 4. RustyNES doesn't complete it until frame 5 (one frame
late) AND doesn't reach the subsequent `Y=$E3` second pass within
the 10-frame observation window.

The two `Y=$E3` source candidates in `AccuracyCoin.asm` are unclear
from search — `#$E3` literal appears at line 5105 (TEST_ISC_E3, a CPU
behaviour test) and three `CMP #$E3` comparisons (lines 7830, 8383,
8407) but no STA targets for `$E3`. The pattern likely comes from
a different OAM-load helper invoked between `STA $4014 #$02` and the
NMI return; identifying it precisely is out of scope for Session-11.

What IS clear: **whichever path produces the `Y=$E3` pattern runs
in Mesen2 but does not run (or runs later) in RustyNES within the
observation window**. This is a downstream symptom of the upstream
CPU-execution-speed divergence at frame 1.

## Boot-frame timing: why the divergence

The most likely causes of "Mesen2 runs the boot code faster than
RustyNES does", ordered by probability:

1. **Reset vector handling**. Mesen2's CPU may start fetching the
   `$FFFC/D` reset vector at a different cycle than RustyNES does.
   The 7-cycle reset sequence (per nesdev wiki) is the only place
   where instruction timing is universally fixed across emulators,
   so this is unlikely to be the discriminator.
2. **PPU "warm-up" delay**. Per nesdev wiki "PPU power-up state",
   the PPU is "inert" for the first ~29658 CPU cycles after
   power-on. CPU writes to $2000-$2007 during this window are
   ignored. AccuracyCoin's RESET code at line 361-374 (TEST_PPUResetFlag)
   intentionally pokes the PPU during this window to test for
   warm-up enforcement. If Mesen2 exits warm-up at a slightly
   different cycle than RustyNES, the boot code progresses
   differently.
3. **APU frame counter init**. AccuracyCoin writes `$40` to `$4017`
   at boot (line 359-360) to disable APU IRQ. The Mesen2 APU may
   take a different number of cycles to ack this than the RustyNES
   APU does.
4. **Initial PRNG / open-bus state**. RustyNES has a deterministic
   "all zero" initial RAM + open-bus latch; Mesen2 may default to
   a different pattern. AccuracyCoin's `PowerOn_MagicNumber` check
   at lines 342-344 explicitly forks behaviour based on whether
   `$5A` is found at a magic address (cold boot vs warm boot
   detection), and any difference in the magic address's initial
   value would route the two emulators down completely different
   code paths.

The trace data already in hand (per-frame `ctrl`, `v`, `t`, `oam_addr`,
`oam_fnv1a64`) is sufficient to localise the divergence further by
running per-frame captures with `RUSTYNES_PPU_TRACE_START_FRAME=0
END=2` and inspecting the *exact* CPU cycle at which $2000/$2006
writes occur in each emulator. This is the Session-12 entry-point.

## Why no code fix landed

Five reasons:

1. The infrastructure was non-functional before Session-11. Without
   working comparative traces, **any** sprite-eval fix would have
   been speculative — the same shape as the three Session-9
   rollbacks.
2. The cascade target (Cascade A) is downstream of the discovered
   init-frame divergence. Fixing sprite-eval first would either
   accidentally compensate for the upstream issue (a brittle
   "cancel-out" that breaks if anything else moves) or have no
   effect at all (because the inputs to sprite-eval are already
   wrong).
3. The validation gauntlet's hard contracts (537 strict pass + 5
   ignored; AccuracyCoin ≥ 82.73%; commercial-ROM oracle 60-green;
   sacred trio SMB/Excitebike/Kid Icarus PAL visual integrity)
   make any speculative-change-and-test loop very expensive. With
   six independent Track-C1 rollbacks (Attempts 1-4 + Phase B4
   prototype + post-B4 mid-cycle snapshot experiment) already in
   the history, the right thing is to wait until the diff data
   names the specific instruction-cycle-level discrepancy.
4. The per-PPU-dot trace fixture canNOT be paired with a matching
   Mesen2 reference at per-dot granularity (Mesen2's Lua doesn't
   expose per-dot or per-scanline events). Per-frame comparison is
   useful for OAM hash + register snapshots, but does not give
   visibility into the sprite-eval FSM's intra-scanline behaviour.
5. The init-frame timing divergence is the same architectural
   surface as the open Track-C1 work (six rollbacks already; ADR-0002
   §"Decision (revised, 2026-05-13)"). A discovery here may turn out
   to *be* one of the C1 axes; pursuing it in a separate session-12
   sprint with the proper IRQ + cycle trace fixture is the right
   sequencing.

## Concrete next-session inputs

1. **Per-frame CPU-cycle trace.** Augment the trace fixture (or
   stand up a sibling fixture using
   `crates/nes-core/src/irq_trace.rs` for the CPU-side cycle
   counter) to emit `(cpu_cycle, PC, A, X, Y, P, S, mem-write@addr)`
   at every CPU cycle of frames 0..=4 of AccuracyCoin cold boot.
   Pair with a Mesen2-side per-instruction trace log
   (`emu.addEventCallback(.., emu.eventType.startFrame)` plus
   `emu.addMemoryCallback(.., emu.callbackType.exec)` for $2000/$2006).
   Goal: identify the EXACT instruction at which Mesen2 and
   RustyNES first emit different state to the PPU.
2. **PPU warm-up cycle audit.** Per ADR-0001 PPU warm-up: 29,658
   CPU cycles. Verify against `crates/nes-ppu/src/ppu.rs`'s
   write-suppression-during-warm-up logic + Mesen2's
   `NesPpu.cpp:ResetWriteAddrFlag` equivalent. A 1-instruction
   warm-up window mismatch could account for the entire
   downstream cascade.
3. **AccuracyCoin OAM init breakdown.** The `Y=$E3` pattern's
   source in `AccuracyCoin.asm` is unidentified; tracking it down
   (likely a helper between line 425's `ClearPage2` and the first
   NMI return) will give an exact CPU-cycle target for the
   divergence-resolution work.

## Tooling status

* `scripts/mesen2_ppu_trace.lua` — works, produces 111-byte records
  matching the RustyNES binary schema. Tested against
  Mesen2 0.42.0 AppImage on Linux + `xvfb-run`.
* `crates/nes-test-harness/tests/ppu_state_trace_fixture.rs` —
  works in both modes (boot-pre-roll default + raw-boot opt-in).
* `target/release/ppu_trace_diff` — works, parses both files,
  reports per-record + per-field divergences correctly.
* `~/.config/Mesen2/settings.json` requires `"AllowIoOsAccess":
  true` for the Lua script to write its output. This is a one-time
  per-machine setup step that has been applied on the
  investigator's machine but should be documented for next-session
  reproducibility.

## Validation gauntlet (post-Session-11)

All hard contracts preserved:

| Gate | Pre | Post |
|------|-----|------|
| `cargo fmt --all --check` | clean | clean |
| `cargo clippy ... -D warnings` (test-roms) | clean | clean |
| `cargo clippy ... -D warnings` (test-roms,commercial-roms) | clean | clean |
| `cargo clippy ... -D warnings` (test-roms,ppu-state-trace) | clean | clean |
| `cargo doc --workspace --no-deps` (RUSTDOCFLAGS=-D warnings) | clean | clean |
| `cargo build --target thumbv7em-none-eabihf` (no_std) | builds | builds |
| `cargo test --workspace --features test-roms` | 537 P / 5 IGN | 537 P / 5 IGN |
| `cargo test --workspace --features test-roms,commercial-roms` | 597 P / 5 IGN | 597 P / 5 IGN |
| `accuracycoin_pass_rate_meets_floor` | 82.73% (≥ 0.60 floor) | 82.73% (≥ 0.60 floor) |
| Sacred trio (SMB/Excitebike/Kid Icarus PAL) | renders legibly | unchanged (no PPU code paths touched) |

All changes in this session are gated:

* `scripts/mesen2_ppu_trace.lua` — external script, never linked into
  the binary.
* `crates/nes-test-harness/tests/ppu_state_trace_fixture.rs` — already
  feature-gated on `ppu-state-trace` (default off).

No production code paths were touched.
