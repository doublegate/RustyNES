-- mesen2_dmc_dma_trace.lua
--
-- Mesen2 Lua trace pinned to the DMC DMA scheduler surface. Counterpart
-- to the RustyNES `trace_dmc_dma` binary (Sprint 2.3 Step 3 oracle
-- generation). The cross-diff tool `scripts/dmc_dma_trace_cross_diff.py`
-- aligns the two CSVs to identify per-cycle divergence between
-- RustyNES and Mesen2 on the four compensating delays Session-20
-- identified as load-bearing for `APU Registers and DMA tests ::
-- Implicit DMA Abort`:
--   * `dmc_dma_short` (load vs early-deliver-get path)
--   * `dmc_dma_cooldown` (4 post-load / 5 post-early-deliver)
--   * `dmc_abort_delay_for` (2 -> 2, 3 -> 3)
--   * `dmc_dma_pending` + `in_dmc_dma` (scheduler state pair)
--
-- USAGE:
--   xvfb-run -a ~/AppImages/mesen.appimage --testRunner \
--       tests/roms/AccuracyCoin/sub-tests/apu-implicit-dma-abort.nes \
--       scripts/mesen2_dmc_dma_trace.lua
--
-- ENV VARS:
--   MESEN2_DMC_TRACE_OUT         : output CSV path
--                                   (default /tmp/mesen2_dmc_dma_trace.csv)
--   MESEN2_DMC_TRACE_MAX_FRAMES  : stop after this many frames (default 120)
--   MESEN2_DMC_TRACE_RESULT_ADDR : stop when this addr becomes non-zero
--                                   (default 0 == disabled; the DMC
--                                   sub-test ROM uses $0464; the full
--                                   battery uses per-suite addresses)
--   MESEN2_DMC_TRACE_DMC_EVENTS  : stop after N `dmc_run` events
--                                   (default 4000)
--
-- CSV schema:
--   cpu_cycle, ppu_frame, ppu_scanline, ppu_dot, m2_phase, kind,
--   addr, value, dmc_bytes_rem, dmc_sample_addr, dmc_irq_flag,
--   dmc_irq_en, dmc_silence
--
-- `kind` column:
--   "R" / "W"       = $4010-$4015 register access
--   "dmc_get"       = bytesRemaining decremented since last snapshot
--                     (one DMC DMA fetch per event; aligns to RustyNES
--                     `BusAccess::DmaRead` at $8000..=$FFFF)
--   "dmc_irq_set"   = irqFlag transitioned LOW -> HIGH
--   "dmc_irq_clr"   = irqFlag transitioned HIGH -> LOW
--   "dmc_en_set"    = irqEnabled transitioned LOW -> HIGH
--   "dmc_en_clr"    = irqEnabled transitioned HIGH -> LOW
--   "frame"         = synthetic per-frame heartbeat for cross-diff
--                     alignment
--
-- `m2_phase` is derived from `cpu_cycle & 1` (per Mesen2's ApuFrameCounter
-- `GetMasterClock() & 0x01` convention; symmetric with RustyNES).

local CONFIG = {
    OUT_PATH    = os.getenv("MESEN2_DMC_TRACE_OUT") or "/tmp/mesen2_dmc_dma_trace.csv",
    MAX_FRAMES  = tonumber(os.getenv("MESEN2_DMC_TRACE_MAX_FRAMES") or "120"),
    RESULT_ADDR = tonumber(os.getenv("MESEN2_DMC_TRACE_RESULT_ADDR") or "0"),
    DMC_EVENTS  = tonumber(os.getenv("MESEN2_DMC_TRACE_DMC_EVENTS") or "4000"),
    -- AccuracyCoin autostart: press Start on player 1 for AUTOSTART_PRESS_FRAMES
    -- frames beginning at AUTOSTART_FRAME (default off; set non-zero to enable).
    AUTOSTART_FRAME        = tonumber(os.getenv("MESEN2_DMC_TRACE_AUTOSTART_FRAME") or "0"),
    AUTOSTART_PRESS_FRAMES = tonumber(os.getenv("MESEN2_DMC_TRACE_AUTOSTART_PRESS_FRAMES") or "6"),
    -- Skip recording until START_FRAME. Lets you target a specific test phase
    -- without filling the CSV with boot + menu + early-suite noise.
    START_FRAME            = tonumber(os.getenv("MESEN2_DMC_TRACE_START_FRAME") or "0"),
}

local file = nil
local frame_count = 0
local stopped = false
local rows = 0
local dmc_run_events = 0

-- Last-seen DMC snapshot to compute delta events.
local prev_bytes_rem = nil
local prev_irq_flag  = nil
local prev_irq_en    = nil

local function open_output()
    local f, err = io.open(CONFIG.OUT_PATH, "w")
    if not f then
        emu.log("[dmc_trace] could not open " .. CONFIG.OUT_PATH .. ": " .. tostring(err))
        return false
    end
    file = f
    file:write(
        "cpu_cycle,ppu_frame,ppu_scanline,ppu_dot,m2_phase,kind," ..
        "addr,value,dmc_bytes_rem,dmc_sample_addr,dmc_irq_flag," ..
        "dmc_irq_en,dmc_silence\n"
    )
    emu.log("[dmc_trace] writing CSV to " .. CONFIG.OUT_PATH)
    return true
end

local function close_output()
    if file then
        file:close()
        file = nil
        emu.log(string.format(
            "[dmc_trace] wrote %d rows across %d frames (%d dmc_get events)",
            rows, frame_count, dmc_run_events
        ))
    end
end

local function stop_now(reason)
    if stopped then return end
    stopped = true
    close_output()
    emu.log("[dmc_trace] STOP: " .. tostring(reason))
    emu.stop(0)
end

-- Snapshot the DMC fields; falls back to safe defaults if a key is
-- missing (older Mesen2 builds may not expose every field).
local function snapshot_dmc()
    local s = emu.getState()
    return {
        cycle        = s["cpu.cycleCount"] or 0,
        frame        = s["ppu.frameCount"] or 0,
        scanline     = s["ppu.scanline"]   or 0,
        dot          = s["ppu.cycle"]      or 0,
        bytes_rem    = s["apu.dmc.bytesRemaining"] or 0,
        sample_addr  = s["apu.dmc.sampleAddr"]     or 0,
        irq_flag     = s["apu.dmc.irqFlag"]        or false,
        irq_en       = s["apu.dmc.irqEnabled"]     or false,
        silence      = s["apu.dmc.silenceFlag"]    or false,
    }
end

local function write_row(kind, addr_str, val_str, snap)
    if not file or stopped then return end
    -- Skip everything before START_FRAME (boot + menu noise).
    if snap.frame < CONFIG.START_FRAME then return end
    local m2 = (snap.cycle % 2 == 0) and "L" or "H"
    file:write(string.format(
        "%d,%d,%d,%d,%s,%s,%s,%s,%d,$%04X,%d,%d,%d\n",
        snap.cycle, snap.frame, snap.scanline, snap.dot, m2,
        kind, addr_str, val_str,
        snap.bytes_rem, snap.sample_addr,
        snap.irq_flag and 1 or 0,
        snap.irq_en   and 1 or 0,
        snap.silence  and 1 or 0
    ))
    rows = rows + 1
end

-- Detect delta events vs the last snapshot. Called on every memory
-- callback + on each frame.
local function emit_delta_events(snap)
    -- Only emit `dmc_get` for a real fetch:
    --   - bytes_rem decreased by exactly 1 (canonical DMC fetch decrement)
    --   - DMC is enabled (irq_en field tracks the same enable state)
    --   - the previous bytes_rem was > 0 (otherwise the decrement is
    --     an underflow / disable artifact, not a real fetch)
    -- Without these guards the script over-emits at every $4015 write
    -- that disables DMC (drops bytes_rem to 0).
    if prev_bytes_rem ~= nil
       and snap.bytes_rem == prev_bytes_rem - 1
       and prev_bytes_rem > 0
    then
        write_row("dmc_get", "$0000", "$00", snap)
        dmc_run_events = dmc_run_events + 1
        if dmc_run_events >= CONFIG.DMC_EVENTS then
            stop_now("max_dmc_events")
            return
        end
    end
    if prev_irq_flag ~= nil and snap.irq_flag ~= prev_irq_flag then
        write_row(snap.irq_flag and "dmc_irq_set" or "dmc_irq_clr", "$4015", "$00", snap)
    end
    if prev_irq_en ~= nil and snap.irq_en ~= prev_irq_en then
        write_row(snap.irq_en and "dmc_en_set" or "dmc_en_clr", "$4010", "$00", snap)
    end
    prev_bytes_rem = snap.bytes_rem
    prev_irq_flag  = snap.irq_flag
    prev_irq_en    = snap.irq_en
end

local function on_write(addr, value)
    if stopped then return end
    local snap = snapshot_dmc()
    emit_delta_events(snap)
    write_row("W", string.format("$%04X", addr), string.format("$%02X", value & 0xFF), snap)
end

local function on_read(addr, value)
    if stopped then return end
    local snap = snapshot_dmc()
    emit_delta_events(snap)
    write_row("R", string.format("$%04X", addr), string.format("$%02X", value & 0xFF), snap)
end

local function on_start_frame()
    if stopped then return end
    frame_count = frame_count + 1
    if frame_count % 200 == 0 then
        emu.log(string.format("[dmc_trace] heartbeat lua_frame=%d", frame_count))
    end
    if frame_count >= CONFIG.MAX_FRAMES then
        stop_now("max_frames")
        return
    end
    local snap = snapshot_dmc()
    emit_delta_events(snap)
    write_row("frame", "$0000", "$00", snap)
    if CONFIG.RESULT_ADDR > 0 then
        local nes_mem = emu.memType.nesMemory
        local v = emu.read(CONFIG.RESULT_ADDR, nes_mem, false) & 0xFF
        if v ~= 0 and rows > 0 then
            stop_now(string.format("result_set_$%02X", v))
            return
        end
    end
end

local function on_script_ended()
    close_output()
end

if not open_output() then return end

emu.addMemoryCallback(on_write, emu.callbackType.write, 0x4010, 0x4013)
emu.addMemoryCallback(on_write, emu.callbackType.write, 0x4015, 0x4015)
emu.addMemoryCallback(on_read,  emu.callbackType.read,  0x4015, 0x4015)
emu.addEventCallback(on_start_frame, emu.eventType.startFrame)
emu.addEventCallback(on_script_ended, emu.eventType.scriptEnded)

-- AccuracyCoin Start-press driver: when AUTOSTART_FRAME > 0, press Start
-- on player 1 for AUTOSTART_PRESS_FRAMES frames starting at AUTOSTART_FRAME.
-- Matches `rustynes_test_harness::accuracy_coin::run_battery_capturing_ram`.
if CONFIG.AUTOSTART_FRAME > 0 then
    local logged_open = false
    local logged_close = false
    local presses = 0
    local function on_input_polled()
        if stopped then return end
        local s = CONFIG.AUTOSTART_FRAME
        local e = s + CONFIG.AUTOSTART_PRESS_FRAMES
        if frame_count >= s and frame_count < e then
            if not logged_open then
                emu.log(string.format("[dmc_trace] autostart window OPEN at frame_count=%d (autostart_frame=%d, press_frames=%d)", frame_count, s, e - s))
                logged_open = true
            end
            local ok, err = pcall(function() emu.setInput({start = true}, 0, 0) end)
            if not ok then
                emu.log("[dmc_trace] setInput error: " .. tostring(err))
            end
            presses = presses + 1
        elseif frame_count >= e and not logged_close then
            emu.log(string.format("[dmc_trace] autostart window CLOSE at frame_count=%d (total presses=%d)", frame_count, presses))
            logged_close = true
        end
    end
    emu.addEventCallback(on_input_polled, emu.eventType.inputPolled)
end

emu.log(string.format(
    "[dmc_trace] armed: max_frames=%d result_addr=$%04X max_dmc_events=%d out=%s",
    CONFIG.MAX_FRAMES, CONFIG.RESULT_ADDR, CONFIG.DMC_EVENTS, CONFIG.OUT_PATH
))
