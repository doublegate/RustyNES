# Per-PPU-Dot State-Trace Tooling

Operator's guide for the Session-10 PPU observability tooling, with
Session-11 corrections applied. For the design rationale see
`docs/adr/0005-ppu-state-trace.md`. For the broader Cascade A
investigation see `docs/audit/cascade-a-investigation-2026-05-19.md`.
For the Mesen2 Lua API mismatches discovered during the Session-10
script's first real validation run see
`docs/audit/session-11-mesen2-trace-validation-2026-05-20.md`.

## Important caveats from Session-11

* **Mesen2's Lua API has no per-scanline or per-dot callback.** The
  finest granularity is `endFrame` (once per frame at PPU
  `(scanline=240, dot=0)`). A faithful per-PPU-dot reference trace
  canNOT be produced by Lua alone. The reference-trace script
  therefore emits one record per frame; the matching RustyNES-side
  capture should narrow to `scanline=240, dot=0` only (see env-var
  presets below).
* **Mesen2 needs an X11 display even in `--testRunner` mode.** On
  Linux wrap calls in `xvfb-run -a`. The CLI surface is
  `--testRunner <rom> <script.lua>` (note capital R).
* **Mesen2's Lua sandbox blocks `io.open` by default.** Set
  `~/.config/Mesen2/settings.json` `"AllowIoOsAccess": true` once
  per investigator's machine before the reference-trace script can
  write its `.bin` output.
* **`emu.getState()` returns a FLAT table with dotted-string keys.**
  E.g. `state["ppu.frameCount"]` works, `state.ppu.frameCount` is
  always nil. The Session-10 script's nested-table assumption was
  the load-bearing reason it emitted zero records during its first
  validation run; the bug is now fixed.

## Mesen2 vs RustyNES per-frame comparison preset

To capture comparable traces of AccuracyCoin cold boot:

```bash
# 1. RustyNES side — per-frame trace at scanline 240 / dot 0:
env RUSTYNES_PPU_TRACE_RAW_BOOT=1 \
    RUSTYNES_PPU_TRACE_START_FRAME=0 \
    RUSTYNES_PPU_TRACE_END_FRAME=15 \
    RUSTYNES_PPU_TRACE_SCANLINE_LO=240 \
    RUSTYNES_PPU_TRACE_SCANLINE_HI=240 \
    RUSTYNES_PPU_TRACE_DOT_LO=0 \
    RUSTYNES_PPU_TRACE_DOT_HI=0 \
    RUSTYNES_PPU_TRACE_OUT=/tmp/rustynes_cold.bin \
    cargo test -p rustynes-test-harness --release \
        --features test-roms,ppu-state-trace \
        --test ppu_state_trace_fixture -- --nocapture

# 2. Mesen2 side — same window, Lua script handles Start press:
env MESEN2_PPU_TRACE_OUT=/tmp/mesen2_cold.bin \
    MESEN2_PPU_TRACE_START=0 \
    MESEN2_PPU_TRACE_END=15 \
    MESEN2_PPU_TRACE_START_PRESS_LO=300 \
    MESEN2_PPU_TRACE_START_PRESS_HI=305 \
    xvfb-run -a /path/to/mesen.appimage \
        --testRunner tests/roms/accuracycoin/AccuracyCoin.nes \
        scripts/mesen2_ppu_trace.lua

# 3. Diff (Mesen2 cannot emit sprite-eval FSM / sprite line-up /
# BG latches; skip those fields):
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

---

This tooling is the PPU-side analogue of `crates/rustynes-core/src/irq_trace.rs`
(the per-CPU-cycle IRQ trace fixture that empirically unblocked
Phase B4 of Track C1; see ADR 0002 §"Test fixture"). Same design
pattern (linear buffer with overflow counter, feature-gated, binary

* CSV output, integration-test consumer) applied to PPU state.

---

## Feature flag

`ppu-state-trace`, off by default. Lives in:

* `crates/rustynes-ppu/Cargo.toml` — the recorder code itself + the
  storage field on the `Ppu` struct.
* `crates/rustynes-core/Cargo.toml` — forwards to `rustynes-ppu`.
* `crates/rustynes-test-harness/Cargo.toml` — forwards to `rustynes-core`;
  also gates the `ppu_trace_diff` binary + the
  `ppu_state_trace_fixture` integration test.

When the feature is OFF, every byte of overhead is gone:

* The `state_trace` field on `Ppu` is not present (`#[cfg(...)]`).
* The per-tick recording hook does not compile (`#[cfg(...)]`).
* The `enable_state_trace` / `take_state_trace` / `build_state_record`
  API does not exist.
* The `ppu_trace_diff` binary does not link.
* The `ppu_state_trace_fixture` integration test does not run.

Verified: `cargo check -p rustynes-ppu` (no features) is byte-identical
to pre-Session-10.

---

## Recording a trace from RustyNES

### Option A — via the integration-test fixture

The committed fixture at
`crates/rustynes-test-harness/tests/ppu_state_trace_fixture.rs` is the
turn-key invocation. It boots `AccuracyCoin`, runs the
splash + Start press, then captures a configurable window:

```bash
cd /path/to/RustyNES

# Default: frames 310..=320, visible field only (~163k records per frame).
cargo test -p rustynes-test-harness \
    --release \
    --features test-roms,ppu-state-trace \
    --test ppu_state_trace_fixture \
    -- --nocapture
```

The fixture writes:

* `target/ppu_trace/accuracycoin_default.bin` — packed binary
  trace.
* `target/ppu_trace/accuracycoin_default.preview.csv` — first 200
  records as CSV (full CSV would be too big; use the binary +
  diff tool for full inspection).

Override the window via environment variables (no recompile
needed):

```bash
RUSTYNES_PPU_TRACE_START_FRAME=325 \
RUSTYNES_PPU_TRACE_END_FRAME=327 \
RUSTYNES_PPU_TRACE_OUT=/tmp/inc4014_test2.bin \
    cargo test -p rustynes-test-harness --release \
        --features test-roms,ppu-state-trace \
        --test ppu_state_trace_fixture \
        -- --nocapture
```

### Option B — programmatically

```rust
use rustynes_core::Nes;
use rustynes_core::rustynes_ppu::state_trace::{PpuStateTrace, PpuTraceConfig};

let mut nes = Nes::from_rom(&bytes)?;
// Boot the ROM to wherever the bug reproduces ...
for _ in 0..306 { nes.run_frame(); }

let cfg = PpuTraceConfig::visible_only(310..=320);
let trace = PpuStateTrace::with_capacity(4_000_000, cfg);
nes.bus_mut().ppu_mut().enable_state_trace(trace);

for _ in 0..15 { nes.run_frame(); }

let trace = nes.bus_mut().ppu_mut().take_state_trace().unwrap();
std::fs::write("trace.bin", trace.to_binary())?;
```

`PpuTraceConfig` presets:

| Constructor | Scanline window | Dot window | Per-frame records |
|-------------|----------------|-----------|------------------|
| `all(frames)` | all (-1..=260/310) | all (0..=340) | ~89k |
| `visible_only(frames)` | 0..=239 | 0..=340 | ~82k |
| `sprite_eval_window(frames)` | 0..=239 | 64..=256 | ~46k |

Custom windows: build `PpuTraceConfig { frame_range, scanline_range,
dot_range }` directly. All ranges are inclusive.

---

## Recording a reference trace from Mesen2

### Approach A (default) — Lua script, per-scanline granularity

`scripts/mesen2_ppu_trace.lua` is the Mesen2-side reference-trace
emitter. It produces a binary file with the SAME schema the
RustyNES fixture emits, so the `ppu_trace_diff` CLI can compare
the two.

Setup:

1. Install Mesen2: <https://www.mesen.ca/> or
   `yay -S mesen2-git` on Arch-family distros.
2. Load `tests/roms/accuracycoin/AccuracyCoin.nes` in Mesen2.
3. **Tools** > **Script Window** > **File** > **Open** —
   navigate to `scripts/mesen2_ppu_trace.lua`.
4. Edit the `CONFIG` table at the top of the script:
   * `OUT_PATH` — where the `.bin` will be written
     (e.g. `/tmp/mesen2_inc_4014.bin`).
   * `START_FRAME` / `END_FRAME` — capture window. For
     AccuracyCoin the test runner starts at ~frame 306; the
     default `310..=350` window covers the first ~40 frames of
     test execution.
   * `SCANLINE_LO` / `SCANLINE_HI` — visible field is `0..=239`.
5. Press **F1** in the Script Window to arm the script.
6. Press **Start** on the controller (in Mesen2's controller
   config or via your keyboard map) to launch the AccuracyCoin
   battery. The script logs progress every 10 frames.
7. When the `END_FRAME` is reached the script closes the file
   and logs the final record count.

**Granularity caveat**: Mesen2's published Lua API (verified
2026-05-20 against the docs at
<https://www.mesen.ca/docs/apireference/enums.html>) does NOT
expose a per-PPU-cycle event type. The finest granularity is
`emu.eventType.scanline`, which fires once per scanline at dot 0.
The script therefore emits ONE record per scanline (at dot 0).
For per-dot-resolution diff against a RustyNES trace, the
RustyNES side should be captured with a matching `dot_range:
Some(0..=0)` filter.

Recommended `ppu_trace_diff` invocation when the Mesen2 side is
per-scanline:

```bash
./target/debug/ppu_trace_diff \
    --reference /tmp/mesen2_inc_4014.bin \
    --actual target/ppu_trace/accuracycoin_default.bin \
    --skip-fields sprite_eval_n,sprite_eval_m,sprite_eval_found,sprite_eval_sec_idx,sprite_eval_copying,sprite_eval_overflow_search,sprite_eval_done,sprite_eval_read_latch,spr_shift_lo,spr_shift_hi,spr_attr,spr_x,spr_count,spr_zero_in_line,at_shift_lo,at_shift_hi,nt_latch,at_latch,bg_lo_latch,bg_hi_latch,secondary_oam \
    --first-divergence
```

The skipped fields are the ones Mesen2's Lua API does NOT expose
via `emu.getState()`; the script writes zeros into them. The
remaining fields (frame, scanline, dot, ctrl, mask, status,
oam_addr, v, t, fine_x, w_toggle, bg_shift_lo, bg_shift_hi,
oam_fnv1a64, nmi_line) ARE compared.

### Approach B (fallback) — parse Mesen2's trace log

If finer-than-scanline reference resolution is required, the
fallback is to parse Mesen2's built-in debugger trace log
(**Tools** > **Debugger** > **Trace Logger**). Mesen2's trace
log dumps per-CPU-cycle CPU state including the cycle's PPU
scanline + dot. A future companion script
(`scripts/parse_mesen2_trace_log.py`, **not** included in
Session-10 — deferred until needed) would convert that log to
our binary schema by interpolating PPU state from the per-cycle
samples. Documented here as the escape hatch.

### Approach C (landed 2026-05-23) — Mesen2 source patch for `EventType::PpuCycle`

For the v1.0.0-final brief
(`/home/parobek/.claude/plans/generate-a-new-plan-snug-starlight.md`
Phase 0), a small Mesen2 C++ patch lands a new
`EventType::PpuCycle` event that Lua scripts can register for to
get TRUE per-PPU-cycle granularity (89342 events per NTSC frame).
The patch is local to the working clone of upstream Mesen2 at
`~/Code/OSS_Public-Projects/RustyNES/ref-proj/Mesen2/` and lives
in two files:

1. `Core/Shared/EventType.h` — adds `PpuCycle` to the `EventType`
   enum (positioned between `CodeBreak` and the
   `LastValue` sentinel so `magic_enum::enum_entries<EventType>`
   picks it up).
2. `Core/NES/NesPpu.cpp::Exec` — emits
   `_emu->ProcessEvent(EventType::PpuCycle)` immediately after the
   existing `_emu->ProcessPpuCycle<CpuType::Nes>()` call (line
   ~1365 pre-patch).

Build with the standard upstream invocation:

```bash
cd ~/Code/OSS_Public-Projects/RustyNES/ref-proj/Mesen2
# Touch all .cpp files that #include the EventType.h chain so
# magic_enum re-runs at compile time:
find Core -name "*.cpp" | xargs grep -l "ScriptingContext\.h\|EventType\.h" | xargs touch
make -j20
```

The resulting `bin/linux-x64/Release/Mesen` + `MesenCore.so`
expose `emu.eventType.ppuCycle` to Lua. Verify with:

```bash
strings -a bin/linux-x64/Release/MesenCore.so | grep -aE '^PpuCycle$'
```

Should output `PpuCycle`. Lua scripts then register a per-cycle
callback via:

```lua
emu.addEventCallback(function() ... end, emu.eventType.ppuCycle)
```

Inside the callback, `emu.getState().ppu.cycle` /
`emu.getState().ppu.scanline` give the current PPU position
(note flat-key access per Session-11 caveat: use
`emu.getState()["ppu.cycle"]`). Per-cycle Lua overhead is
non-negligible (~10 µs/call); plan for ~1-5 effective FPS under
`xvfb-run -a --testRunner` mode. Acceptable for offline oracle
capture against the custom-sub-test ROMs that boot to target
test by frame ≤ 400.

The patch is **NOT** upstreamed — it lives only in the local
ref-proj clone. CI builds of RustyNES do not depend on a
patched Mesen2; the per-PPU-cycle oracle is invoked only by
investigator-side manual runs during accuracy-fix development.
Documented as Approach C so future investigators can re-apply
the same two-file patch if the ref-proj clone is refreshed.

---

## Diffing two traces

The `ppu_trace_diff` CLI reads two binary traces and reports
field-level divergences:

```bash
# Build (feature-gated).
cargo build -p rustynes-test-harness --features ppu-state-trace --bin ppu_trace_diff

# First divergence only (default).
./target/debug/ppu_trace_diff \
    --reference /tmp/mesen2_inc_4014.bin \
    --actual target/ppu_trace/accuracycoin_default.bin

# All divergences, cap at 5 reports.
./target/debug/ppu_trace_diff \
    --reference /tmp/mesen2_inc_4014.bin \
    --actual target/ppu_trace/accuracycoin_default.bin \
    --all-divergences --max-reports 5

# Skip fields that one side doesn't populate.
./target/debug/ppu_trace_diff \
    --reference /tmp/mesen2_inc_4014.bin \
    --actual target/ppu_trace/accuracycoin_default.bin \
    --skip-fields oam_fnv1a64,t
```

Exit codes:

* `0` — traces equivalent under the chosen comparator.
* `1` — divergence reported.
* `2` — parse / I/O error.

Output format per divergence:

```text
[diff @ frame=315 scanline=12 dot=137]
  (anchor: ref(frame=315,scanline=12,dot=137) vs actual(frame=315,scanline=12,dot=137))
    spr_shift_lo                   ref=[00, 03, 00, 00, ...] actual=[00, 04, 00, 00, ...]
    spr_count                      ref=2                  actual=3
```

---

## Schema versioning

Binary schema version is `1` (Session-10). Bump
`PPU_TRACE_SCHEMA_VERSION` in
`crates/rustynes-ppu/src/state_trace.rs` whenever the
`PpuStateRecord` byte layout changes; the `to_binary` /
`from_binary` round trip will refuse to load a mismatched
version with a clear `schema mismatch: file is vN, this build
expects vM` error.

A const-time `RECORD_SIZE` check in `state_trace.rs` catches
accidental drift at compile time. Run `cargo test
-p rustynes-ppu --features ppu-state-trace state_trace::tests`
to exercise the full schema roundtrip suite.

---

## Determinism contract

The recorder is strictly **read-only** — it reads `&self` at
the end of every `Ppu::tick` and pushes a packed record into a
bounded buffer. It never writes to PPU state, never calls into
the bus, never allocates beyond the bounded `Vec::push`. The
default-feature-off build is byte-identical to pre-Session-10
(see ADR 0005 §"Compliance with the determinism contract").

When the feature is ON, the build's behavior is byte-identical
EXCEPT for the cost of `Vec::push` calls into the trace
buffer (which only happens when `enable_state_trace` has been
called). The fixture asserts a binary-roundtrip invariant on
every run, so latent corruption of the trace is detected.

---

## Future work

* **Per-dot Mesen2 reference**: requires Mesen2 source-side
  modifications. Deferred to v1.x.
* **Compressed binary output**: tail of the trace file is
  highly compressible (most records differ from the prior by a
  handful of fields). LZ4 or gzip the binary out-of-band; the
  diff tool currently reads uncompressed only.
* **In-place per-scanline RLE**: requires schema bump (v2). Use
  reserved-flags bit 4 (TBD).
* **Schema v2 with full per-record OAM**: see ADR 0005 §"Future
  schema bumps".
