-- mesen2_irq_trace.lua
--
-- Mesen2 Lua reference-trace script for RustyNES_v2's Track C1 IRQ-timing
-- investigation (Session-15 / C1 attempt 13).
--
-- Emits a CSV trace of IRQ-related events from Mesen2 that can be
-- cross-diffed against RustyNES's per-CPU-cycle IRQ trace fixture
-- (`crates/nes-core/src/irq_trace.rs` + `irq_trace_fixture.rs`).
--
-- =================================================================
-- INSTALLING + RUNNING
-- =================================================================
--
-- 1. Install Mesen2 (AppImage at ~/AppImages/mesen.appimage).
--    Ensure ~/.config/Mesen2/settings.json has "AllowIoOsAccess": true.
-- 2. Run in headless mode (no UI) via --testRunner:
--      xvfb-run -a ~/AppImages/mesen.appimage --testRunner <rom_path> \
--          scripts/mesen2_irq_trace.lua
--    The script writes its .csv output, then calls emu.stop(0) when
--    its frame budget elapses or the test-status hash drives an early exit.
--
-- Environment variables (defaults match the RustyNES `irq_trace_fixture`
-- contract):
--   MESEN2_IRQ_TRACE_OUT       : output CSV path (default
--                                 /tmp/mesen2_irq_trace.csv)
--   MESEN2_IRQ_TRACE_MAX_FRAMES: maximum frames to run (default 600)
--   MESEN2_IRQ_TRACE_BOOT_FRAMES: frames to skip before recording (default 10)
--   MESEN2_IRQ_TRACE_START_CYCLE: lower-bound CPU cycle below which events
--                                 are NOT emitted (default 0).  Phase 1.3 of
--                                 Track C1 attempt 14 uses START_CYCLE=250000
--                                 to skip the boot/anchor offset region so
--                                 the cleaned trace exposes only in-test-loop
--                                 divergence.  Even when set, BOOT_FRAMES is
--                                 still honored (frames-skip happens at the
--                                 startFrame callback; START_CYCLE filters
--                                 individual event-emission sites further).
--
-- =================================================================
-- CAPABILITY ANALYSIS (Phase 1A probe, 2026-05-22)
-- =================================================================
--
-- Mesen2 Lua API exposes:
--   * emu.eventType.irq (= 1)  : fires when the CPU services an IRQ
--                                (i.e. after the IRQ has been latched
--                                AND the CPU has reached the IRQ-poll
--                                point AND I-flag is clear; Mesen2
--                                fires this on the cycle the interrupt
--                                vector is read).
--   * emu.eventType.nmi (= 0)  : same for NMI.
--   * emu.callbackType.exec (= 2): per-opcode-fetch CPU callback.
--   * apu.frameCounter.irqFlag (bool): APU IRQ line state, polled at exec.
--   * apu.dmc.irqEnabled (bool)     : DMC IRQ-enabled flag (NB: the
--                                     DMC actively-asserted flag is
--                                     NOT directly exposed; we infer
--                                     by edge from irqEnabled+state).
--   * cpu.nmiFlag (number)          : CPU NMI-line latch.
--   * cpu.cycleCount, ppu.frameCount, ppu.scanline, ppu.cycle.
--
-- Mesen2 does NOT expose per-CPU-cycle granularity from Lua. Exec
-- callbacks fire at opcode fetch (instruction boundary). So:
--   * IRQ "rising edge" cycles are inferred from emu.eventType.irq
--     firing (the CPU has already polled+services the IRQ).
--   * APU IRQ-flag transitions are detected at exec-callback granularity
--     (the next instruction after the actual rise).
--   * Mapper IRQ-line state is NOT directly exposed; the irq event +
--     vector-fetch context disambiguates mapper-vs-APU origin.
--
-- =================================================================
-- CSV SCHEMA (pre-Session-21)
-- =================================================================
--
--   cpu_cycle, ppu_frame, ppu_scanline, ppu_dot, event_type, pc,
--   apu_irq_flag, nmi_flag
--
-- event_type values:
--   "irq_svc"   : CPU serviced an IRQ on this cycle (emu.eventType.irq)
--   "nmi_svc"   : CPU serviced an NMI on this cycle (emu.eventType.nmi)
--   "apu_set"   : APU IRQ flag transitioned LOW->HIGH (detected at next exec)
--   "apu_clr"   : APU IRQ flag transitioned HIGH->LOW (detected at next exec)
--   "nmi_set"   : cpu.nmiFlag transitioned 0->1 (detected at next exec)
--   "nmi_clr"   : cpu.nmiFlag transitioned 1->0
--   "dmc_set"   : (Session-21) DMC IRQ flag transitioned LOW->HIGH
--   "dmc_clr"   : (Session-21) DMC IRQ flag transitioned HIGH->LOW
--   "dmc_run"   : (Session-21) DMC DMA active state transitioned
--                 enabled <-> disabled (the "sample buffer empty +
--                 sample_length > 0" loop boundary); Mesen2 doesn't
--                 expose DMA pending/halt directly via Lua so this is
--                 the closest analogue to RustyNES's `pending_dmc_dma`.
--   "dmc_irqen" : (Session-21) DMC IRQ-enabled bit transitioned (set
--                 via `$4010` writes).
--
-- Each row's cycle is the cycle AT WHICH MESEN2 NOTICED the transition;
-- for "irq_svc" / "nmi_svc" this is exact (Mesen2 callbacks know the
-- service cycle). For the edge-detected "apu_set"/"apu_clr"/"nmi_set"/
-- "nmi_clr"/"dmc_set"/"dmc_clr"/"dmc_run"/"dmc_irqen" rows this is the
-- cycle of the NEXT instruction's opcode fetch after the transition.
--
-- =================================================================
-- SESSION-21 DMC SCHEMA EXTENSION
-- =================================================================
--
-- The Sprint 1 iteration 2 DMC scheduler audit needs per-cycle DMC
-- state from BOTH emulators to cross-diff.  Mesen2's Lua API does NOT
-- expose `pending_dmc_dma` / `dmc_dma_short` / `dmc_abort_pending` /
-- `dmc_abort_delay` / `dmc_dma_cooldown` directly — these are private
-- to `NesDmc.cpp`.  The Lua-visible signals we DO have:
--
--   * `apu.dmc.irqEnabled`            (bool, `$4010 bit 7`)
--   * `apu.dmc.irqFlag`               (bool, DMC IRQ asserted)
--   * `apu.dmc.bytesRemaining`        (u16, decrements on each DMC get)
--   * `apu.dmc.sampleAddr`            (u16, next DMA fetch address)
--   * `apu.dmc.outputVolume`          (u8)
--   * `apu.dmc.silenceFlag`           (bool, sample buffer empty)
--   * `apu.dmc.loopFlag`              (bool, `$4010 bit 6`)
--
-- From these we INFER DMA activity transitions: a `dmc_run` event is
-- emitted whenever `bytesRemaining` rolls over a zero crossing
-- (enable: any -> nonzero, or fetch-completion: nonzero -> 0 with
-- loopFlag clear).  This is coarse-grained vs RustyNES's cycle-level
-- pending/cooldown trace; cross-diff tooling reconciles the two
-- schemas at the IRQ-line and `bytesRemaining`-delta boundaries.
-- Document the derivation limitation; full per-cycle DMA-state
-- visibility on the Mesen2 side requires building Mesen2 from source
-- with a custom debug hook (out of scope for Phase A).

local CONFIG = {
    OUT_PATH        = os.getenv("MESEN2_IRQ_TRACE_OUT") or "/tmp/mesen2_irq_trace.csv",
    MAX_FRAMES      = tonumber(os.getenv("MESEN2_IRQ_TRACE_MAX_FRAMES") or "600"),
    BOOT_FRAMES     = tonumber(os.getenv("MESEN2_IRQ_TRACE_BOOT_FRAMES") or "10"),
    START_CYCLE     = tonumber(os.getenv("MESEN2_IRQ_TRACE_START_CYCLE") or "0"),
    LOG_EVERY       = 5000,
    -- Stop after the test ROM transitions its $6000 status byte from
    -- $80/$81 to a final status (matches the RustyNES fixture's stop
    -- condition).  If false, runs the full MAX_FRAMES budget.
    EARLY_STOP_ON_STATUS = true,
    -- =====================================================================
    -- SESSION-22 PROTOCOL SELECTION (Sprint 1 iteration 2 Phase 1A)
    -- =====================================================================
    --
    -- "blargg"        : original $6000-status + magic protocol (default).
    -- "accuracycoin"  : disable status-based early stop; run for the full
    --                   MAX_FRAMES budget OR until a per-sub-test
    --                   RAM-result watchdog fires.  AccuracyCoin writes
    --                   one byte per test to a fixed RAM address (see
    --                   `tests/roms/AccuracyCoin/SOURCE_CATALOG.tsv`);
    --                   when MESEN2_IRQ_TRACE_WATCH_ADDR is set the
    --                   script stops when that address transitions from
    --                   `$00` (not run) to a non-zero result byte.
    -- "dmc_events"    : stop after N `dmc_run` events have been captured
    --                   (DMC-event-driven stop; useful for AccuracyCoin's
    --                   long battery when you only need DMC coverage of
    --                   the relevant sub-test window).
    PROTOCOL        = os.getenv("MESEN2_IRQ_TRACE_PROTOCOL") or "blargg",
    -- AccuracyCoin watchdog: stop when this CPU-RAM address holds a
    -- non-zero result byte (the Pass/Fail/Skipped byte).  Default 0
    -- disables the watchdog and runs until MAX_FRAMES elapses.
    WATCH_ADDR      = tonumber(os.getenv("MESEN2_IRQ_TRACE_WATCH_ADDR") or "0"),
    -- DMC-events stop count (PROTOCOL="dmc_events" or "accuracycoin" w/
    -- non-zero limit).  Default 0 disables the stop.
    DMC_EVENT_LIMIT = tonumber(os.getenv("MESEN2_IRQ_TRACE_DMC_EVENT_LIMIT") or "0"),
    -- Safety: cap total emitted rows so a misconfigured Lua doesn't
    -- bloat the CSV past the cross-diff tool's load budget.
    ROW_LIMIT       = tonumber(os.getenv("MESEN2_IRQ_TRACE_ROW_LIMIT") or "100000"),
    -- AccuracyCoin autostart: when PROTOCOL="accuracycoin", press
    -- Start on player 1 for AUTOSTART_PRESS_FRAMES frames beginning at
    -- AUTOSTART_FRAME, then release.  This drives the ROM from the
    -- title-screen menu into the "Automatically Run Every Test in
    -- ROM" path so the battery actually executes (matches the
    -- RustyNES `accuracy_coin::run_battery_capturing_ram` harness).
    AUTOSTART_FRAME        = tonumber(os.getenv("MESEN2_IRQ_TRACE_AUTOSTART_FRAME") or "300"),
    AUTOSTART_PRESS_FRAMES = tonumber(os.getenv("MESEN2_IRQ_TRACE_AUTOSTART_PRESS_FRAMES") or "6"),
    -- Exec-callback gate.  Mesen2's Lua exec callback fires per-CPU-
    -- instruction (~ 500k/frame); each fire calls emu.getState() twice
    -- (once for the main snapshot, once via snapshot_dmc()).  At ~1us
    -- per getState the overhead is ~ 80x real-time slowdown — too slow
    -- for AccuracyCoin's full battery (~4200 frames).  Gate the exec
    -- callback to silently no-op for frame_count < EXEC_START_FRAME so
    -- the boot + early-suite battery runs at near-native speed; the
    -- exec callback engages only when frame_count >= EXEC_START_FRAME
    -- (the user picks this based on which sub-test window matters).
    -- Default 0 means "exec from the start" — preserves Session-21
    -- semantics for blargg sentinels.
    EXEC_START_FRAME       = tonumber(os.getenv("MESEN2_IRQ_TRACE_EXEC_START_FRAME") or "0"),
}

local file = nil
local records_written = 0
local dmc_run_events = 0
local stopped = false
local frame_count = 0
local recording = false

-- Edge-detection state (set on the first exec after recording starts).
local prev_apu_irq = nil
local prev_nmi_flag = nil
-- Session-21 DMC edge-detection state.  Initialized to `nil` so the
-- first exec after recording emits an `init` row capturing the
-- starting values.
local prev_dmc_irq = nil
local prev_dmc_irq_en = nil
local prev_dmc_bytes_rem = nil
local prev_status_6000 = 0x80
local saw_magic = false

local function open_output()
    local f, err = io.open(CONFIG.OUT_PATH, "w")
    if not f then
        emu.log("[mesen2_irq_trace] could not open " .. CONFIG.OUT_PATH .. ": " .. tostring(err))
        return false
    end
    file = f
    -- Session-21: schema extended with 4 DMC columns appended after
    -- `nmi_flag`. Pre-Session-21 schema is a prefix of this schema so
    -- existing diff tooling that parsed the first 8 columns continues
    -- to work; the cross-diff tool was updated in lockstep to consume
    -- the new columns.
    file:write(
        "cpu_cycle,ppu_frame,ppu_scanline,ppu_dot,event_type,pc," ..
        "apu_irq_flag,nmi_flag,dmc_irq_flag,dmc_irq_en,dmc_bytes_rem,dmc_sample_addr\n"
    )
    emu.log("[mesen2_irq_trace] writing CSV to " .. CONFIG.OUT_PATH)
    return true
end

local function close_output()
    if file then
        file:close()
        file = nil
        emu.log(string.format(
            "[mesen2_irq_trace] wrote %d records (across %d frames)",
            records_written, frame_count
        ))
    end
end

local function stop_now(reason)
    if stopped then return end
    stopped = true
    close_output()
    emu.log("[mesen2_irq_trace] STOP: " .. tostring(reason))
    emu.stop(0)
end

local function snapshot_dmc()
    -- Wrapped in pcall so older Mesen2 builds that don't expose every
    -- `apu.dmc.*` key under getState() degrade to zeros instead of
    -- crashing the script (Session-21 — the DMC keys are documented as
    -- of recent Mesen2 versions but the actual exposure is version-
    -- dependent).
    local ok, s = pcall(function() return emu.getState() end)
    if not ok or s == nil then return false, false, 0, 0 end
    local irq    = s["apu.dmc.irqFlag"] or false
    local irq_en = s["apu.dmc.irqEnabled"] or false
    local br     = s["apu.dmc.bytesRemaining"] or 0
    local sa     = s["apu.dmc.sampleAddr"] or 0
    return irq, irq_en, br, sa
end

local function write_record(cycle, frame, scanline, dot, event_type, pc, apu_irq, nmi_flag)
    if not file or not recording or stopped then return end
    -- Phase 1.3 / Track C1 attempt 14: drop events below START_CYCLE.
    -- Even the "init" event is gated so the post-boot trace starts
    -- cleanly at the first qualifying event AFTER the cutoff.
    if cycle < CONFIG.START_CYCLE then return end
    -- Session-21: snapshot DMC state at the moment of every emitted row
    -- so cross-diff tooling can align per-event DMC visibility against
    -- RustyNES's per-cycle DMC trace.
    local dmc_irq, dmc_irq_en, dmc_br, dmc_sa = snapshot_dmc()
    file:write(string.format(
        "%d,%d,%d,%d,%s,%d,%d,%d,%d,%d,%d,%d\n",
        cycle, frame, scanline, dot,
        event_type, pc & 0xFFFF,
        apu_irq and 1 or 0,
        (nmi_flag and nmi_flag ~= 0) and 1 or 0,
        dmc_irq and 1 or 0,
        dmc_irq_en and 1 or 0,
        dmc_br & 0xFFFF,
        dmc_sa & 0xFFFF
    ))
    records_written = records_written + 1
    if event_type == "dmc_run" then
        dmc_run_events = dmc_run_events + 1
        if CONFIG.DMC_EVENT_LIMIT > 0 and dmc_run_events >= CONFIG.DMC_EVENT_LIMIT then
            -- Defer the actual stop to on_start_frame (callbacks can't
            -- safely call emu.stop mid-exec).
            stop_now(string.format("dmc_event_limit_%d", dmc_run_events))
            return
        end
    end
    if CONFIG.ROW_LIMIT > 0 and records_written >= CONFIG.ROW_LIMIT then
        stop_now(string.format("row_limit_%d", records_written))
        return
    end
    if CONFIG.LOG_EVERY > 0 and (records_written % CONFIG.LOG_EVERY) == 0 then
        emu.log(string.format("[mesen2_irq_trace] cycle=%d frame=%d records=%d",
            cycle, frame, records_written))
    end
end

-- IRQ service event: emu fires this at the cycle the IRQ vector is
-- being read.  Snapshot cycle counter + PPU position.
local function on_irq()
    if not recording or stopped then return end
    local state = emu.getState()
    local cycle = state["cpu.cycleCount"] or 0
    local frame = state["ppu.frameCount"] or 0
    local scanline = state["ppu.scanline"] or 0
    local dot = state["ppu.cycle"] or 0
    local pc = state["cpu.pc"] or 0
    local apu = state["apu.frameCounter.irqFlag"] or false
    local nmi = state["cpu.nmiFlag"] or 0
    write_record(cycle, frame, scanline, dot, "irq_svc", pc, apu, nmi)
end

local function on_nmi()
    if not recording or stopped then return end
    local state = emu.getState()
    local cycle = state["cpu.cycleCount"] or 0
    local frame = state["ppu.frameCount"] or 0
    local scanline = state["ppu.scanline"] or 0
    local dot = state["ppu.cycle"] or 0
    local pc = state["cpu.pc"] or 0
    local apu = state["apu.frameCounter.irqFlag"] or false
    local nmi = state["cpu.nmiFlag"] or 0
    write_record(cycle, frame, scanline, dot, "nmi_svc", pc, apu, nmi)
end

-- Exec callback: per-instruction edge-detection of APU IRQ flag + NMI
-- flag transitions.  Fires immediately before each opcode fetch.
local function on_exec(addr, value)
    if stopped then return end
    if not recording then return end
    -- Session-22: exec-callback overhead gate.  Skip the expensive
    -- emu.getState() calls until the user-specified window opens.  This
    -- is the primary throughput optimization for AccuracyCoin's full
    -- battery — boot + early tests run at near-native speed.
    if frame_count < CONFIG.EXEC_START_FRAME then return end
    local state = emu.getState()
    local cycle = state["cpu.cycleCount"] or 0
    local frame = state["ppu.frameCount"] or 0
    local scanline = state["ppu.scanline"] or 0
    local dot = state["ppu.cycle"] or 0
    local pc = state["cpu.pc"] or addr or 0
    local apu_irq = state["apu.frameCounter.irqFlag"] or false
    local nmi_flag = state["cpu.nmiFlag"] or 0
    -- Session-21: snapshot DMC state to do per-instruction edge detection
    -- on (a) the DMC IRQ flag, (b) the DMC IRQ-enable bit, and (c) the
    -- bytesRemaining counter (DMA-activity proxy).
    local dmc_irq, dmc_irq_en, dmc_br, _dmc_sa = snapshot_dmc()
    if prev_apu_irq == nil then
        prev_apu_irq = apu_irq
        prev_nmi_flag = nmi_flag
        prev_dmc_irq = dmc_irq
        prev_dmc_irq_en = dmc_irq_en
        prev_dmc_bytes_rem = dmc_br
        -- Emit an initial-state record so the diff has an anchor.
        write_record(cycle, frame, scanline, dot, "init", pc, apu_irq, nmi_flag)
        return
    end
    if apu_irq ~= prev_apu_irq then
        write_record(cycle, frame, scanline, dot,
                     apu_irq and "apu_set" or "apu_clr",
                     pc, apu_irq, nmi_flag)
        prev_apu_irq = apu_irq
    end
    local nmi_now = (nmi_flag and nmi_flag ~= 0)
    local nmi_prev = (prev_nmi_flag and prev_nmi_flag ~= 0)
    if nmi_now ~= nmi_prev then
        write_record(cycle, frame, scanline, dot,
                     nmi_now and "nmi_set" or "nmi_clr",
                     pc, apu_irq, nmi_flag)
        prev_nmi_flag = nmi_flag
    end
    -- Session-21 DMC edge events.
    if dmc_irq ~= prev_dmc_irq then
        write_record(cycle, frame, scanline, dot,
                     dmc_irq and "dmc_set" or "dmc_clr",
                     pc, apu_irq, nmi_flag)
        prev_dmc_irq = dmc_irq
    end
    if dmc_irq_en ~= prev_dmc_irq_en then
        write_record(cycle, frame, scanline, dot,
                     "dmc_irqen", pc, apu_irq, nmi_flag)
        prev_dmc_irq_en = dmc_irq_en
    end
    -- `dmc_run` event fires on zero crossings of `bytesRemaining`:
    -- transitions where the channel went idle (br > 0 -> 0) or armed
    -- (br == 0 -> nonzero).  The granularity is "next exec after the
    -- transition" — sufficient to align Mesen2's DMC activity with
    -- RustyNES's per-cycle scheduler trace at instruction boundaries.
    if (prev_dmc_bytes_rem > 0) ~= (dmc_br > 0) then
        write_record(cycle, frame, scanline, dot,
                     "dmc_run", pc, apu_irq, nmi_flag)
        prev_dmc_bytes_rem = dmc_br
    elseif prev_dmc_bytes_rem ~= dmc_br then
        -- Don't emit an event for every decrement; just update the
        -- shadow so we catch the next zero crossing.
        prev_dmc_bytes_rem = dmc_br
    end
end

-- Frame callback: count frames, transition from boot-skip to recording,
-- and check for test-ROM final-status transition.
local function on_start_frame()
    if stopped then return end
    frame_count = frame_count + 1
    if not recording and frame_count > CONFIG.BOOT_FRAMES then
        recording = true
        emu.log(string.format("[mesen2_irq_trace] recording on (frame %d)", frame_count))
    end
    if frame_count >= CONFIG.MAX_FRAMES then
        stop_now("max_frames")
        return
    end
    if recording and CONFIG.PROTOCOL == "blargg" and CONFIG.EARLY_STOP_ON_STATUS then
        -- Match the RustyNES fixture's stop condition: status @ $6000
        -- transitions from $80/$81 to a final status, gated on
        -- $DE-$B0-$61 magic bytes at $6001-$6003.
        local nes_mem = emu.memType.nesMemory
        local s = emu.read(0x6000, nes_mem, false) & 0xFF
        local m1 = emu.read(0x6001, nes_mem, false) & 0xFF
        local m2 = emu.read(0x6002, nes_mem, false) & 0xFF
        local m3 = emu.read(0x6003, nes_mem, false) & 0xFF
        if m1 == 0xDE and m2 == 0xB0 and m3 == 0x61 then
            saw_magic = true
        end
        if saw_magic and s ~= 0x80 and s ~= 0x81 and s ~= prev_status_6000 then
            emu.log(string.format("[mesen2_irq_trace] final status $%02X @ frame %d", s, frame_count))
            stop_now(string.format("final_status_$%02X", s))
            return
        end
        prev_status_6000 = s
    elseif recording and CONFIG.PROTOCOL == "accuracycoin" and CONFIG.WATCH_ADDR > 0 then
        -- AccuracyCoin per-sub-test watchdog: each test writes a single
        -- result byte to a fixed RAM address (see
        -- `tests/roms/AccuracyCoin/SOURCE_CATALOG.tsv`).  When that
        -- address transitions from `$00` (not run) to a non-zero
        -- result byte, the sub-test under observation has completed.
        local nes_mem = emu.memType.nesMemory
        local v = emu.read(CONFIG.WATCH_ADDR, nes_mem, false) & 0xFF
        if v ~= 0 then
            emu.log(string.format(
                "[mesen2_irq_trace] watch_addr $%04X = $%02X @ frame %d",
                CONFIG.WATCH_ADDR, v, frame_count))
            stop_now(string.format("watch_addr_$%04X_=_$%02X", CONFIG.WATCH_ADDR, v))
            return
        end
    end
    -- PROTOCOL = "dmc_events" relies on the DMC_EVENT_LIMIT path in
    -- write_record; no per-frame action needed here.
end

local function on_script_ended()
    close_output()
end

-- =================================================================
-- MAIN
-- =================================================================

if not open_output() then return end

emu.addEventCallback(on_irq, emu.eventType.irq)
emu.addEventCallback(on_nmi, emu.eventType.nmi)
emu.addEventCallback(on_start_frame, emu.eventType.startFrame)
emu.addEventCallback(on_script_ended, emu.eventType.scriptEnded)
emu.addMemoryCallback(on_exec, emu.callbackType.exec, 0x0000, 0xFFFF)

-- Session-22: AccuracyCoin Start-press driver (Sprint 1 iter 2).
if CONFIG.PROTOCOL == "accuracycoin" then
    local function on_input_polled()
        if stopped then return end
        local press_start = CONFIG.AUTOSTART_FRAME
        local press_end   = press_start + CONFIG.AUTOSTART_PRESS_FRAMES
        if frame_count >= press_start and frame_count < press_end then
            -- Mesen2 NES input: bool fields a, b, start, select, up,
            -- down, left, right.  Press Start; leave the rest false so
            -- the player retains nothing else (the title-screen menu's
            -- "Run All Tests" entry is the default cursor position).
            local ok = pcall(function()
                emu.setInput({start = true}, 0, 0)
            end)
            if not ok then
                emu.log("[mesen2_irq_trace] setInput failed (Mesen2 API mismatch)")
            end
        end
    end
    emu.addEventCallback(on_input_polled, emu.eventType.inputPolled)
end

emu.log(string.format(
    "[mesen2_irq_trace] armed: protocol=%s boot=%d max_frames=%d start_cycle=%d watch_addr=$%04X dmc_event_limit=%d row_limit=%d -> %s",
    CONFIG.PROTOCOL, CONFIG.BOOT_FRAMES, CONFIG.MAX_FRAMES,
    CONFIG.START_CYCLE, CONFIG.WATCH_ADDR, CONFIG.DMC_EVENT_LIMIT,
    CONFIG.ROW_LIMIT, CONFIG.OUT_PATH
))
