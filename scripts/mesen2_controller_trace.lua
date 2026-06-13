-- mesen2_controller_trace.lua
--
-- Focused Mesen2 Lua trace for AccuracyCoin's `Controller Strobing` test.
-- Captures every $4016 / $4017 write and read with per-cycle context
-- (CPU cycle, frame, scanline, dot, value, parity-derived M2 phase).
--
-- Phase 3 of v1.0.0-final brief (Session-24, 2026-05-23). Used in
-- conjunction with the matching RustyNES binary
-- `trace_controller_strobing` to cross-diff M2-low vs M2-high strobe
-- latch behavior between the two emulators.
--
-- USAGE:
--   xvfb-run -a ~/AppImages/mesen.appimage --testRunner \
--       tests/roms/AccuracyCoin/sub-tests/controller-strobing.nes \
--       scripts/mesen2_controller_trace.lua
--
-- ENV VARS:
--   MESEN2_CTRL_TRACE_OUT       : output CSV path
--                                 (default /tmp/mesen2_controller_trace.csv)
--   MESEN2_CTRL_TRACE_MAX_FRAMES: stop after this many frames (default 60)
--   MESEN2_CTRL_TRACE_RESULT_ADDR: stop when this addr becomes non-zero
--                                 (default 0x045F == result_ControllerStrobing)
--
-- CSV schema:
--   cpu_cycle, ppu_frame, ppu_scanline, ppu_dot, m2_phase,
--   access, addr, value, prev_strobe_bit
--
-- m2_phase: derived from `cpu_cycle & 1` (per Mesen2 NesApu.cpp
-- _state.PutCycle convention). 0 = put (even), 1 = get (odd).
-- We label even cycles 'L' (M2-low, matches RustyNES's bus convention)
-- and odd cycles 'H' (M2-high). The exact polarity is symmetric — the
-- cross-diff only cares about RELATIVE alignment between the two
-- emulators' writes, not the absolute phase label.

local CONFIG = {
    OUT_PATH    = os.getenv("MESEN2_CTRL_TRACE_OUT") or "/tmp/mesen2_controller_trace.csv",
    MAX_FRAMES  = tonumber(os.getenv("MESEN2_CTRL_TRACE_MAX_FRAMES") or "60"),
    RESULT_ADDR = tonumber(os.getenv("MESEN2_CTRL_TRACE_RESULT_ADDR") or "0x045F"),
}

local file = nil
local frame_count = 0
local stopped = false
local rows = 0
local prev_4016_bit0 = -1

local function open_output()
    local f, err = io.open(CONFIG.OUT_PATH, "w")
    if not f then
        emu.log("[ctrl_trace] could not open " .. CONFIG.OUT_PATH .. ": " .. tostring(err))
        return false
    end
    file = f
    file:write("cpu_cycle,ppu_frame,ppu_scanline,ppu_dot,m2_phase,access,addr,value,prev_strobe_bit\n")
    emu.log("[ctrl_trace] writing CSV to " .. CONFIG.OUT_PATH)
    return true
end

local function close_output()
    if file then
        file:close()
        file = nil
        emu.log(string.format("[ctrl_trace] wrote %d rows across %d frames", rows, frame_count))
    end
end

local function stop_now(reason)
    if stopped then return end
    stopped = true
    close_output()
    emu.log("[ctrl_trace] STOP: " .. tostring(reason))
    emu.stop(0)
end

local function write_row(access, addr, value)
    if not file or stopped then return end
    local state = emu.getState()
    local cycle = state["cpu.cycleCount"] or 0
    local frame = state["ppu.frameCount"] or 0
    local scanline = state["ppu.scanline"] or 0
    local dot = state["ppu.cycle"] or 0
    local m2 = (cycle % 2 == 0) and "L" or "H"
    file:write(string.format(
        "%d,%d,%d,%d,%s,%s,$%04X,$%02X,%d\n",
        cycle, frame, scanline, dot, m2, access, addr, value, prev_4016_bit0
    ))
    rows = rows + 1
    if access == "W" and addr == 0x4016 then
        prev_4016_bit0 = value & 1
    end
end

local function on_4016_write(addr, value)
    write_row("W", addr, value)
end

local function on_4017_write(addr, value)
    write_row("W", addr, value)
end

local function on_4016_read(addr, value)
    write_row("R", addr, value)
end

local function on_4017_read(addr, value)
    write_row("R", addr, value)
end

local function on_start_frame()
    if stopped then return end
    frame_count = frame_count + 1
    if frame_count >= CONFIG.MAX_FRAMES then
        stop_now("max_frames")
        return
    end
    if CONFIG.RESULT_ADDR > 0 then
        local nes_mem = emu.memType.nesMemory
        local v = emu.read(CONFIG.RESULT_ADDR, nes_mem, false) & 0xFF
        if v ~= 0 then
            -- Allow a few extra frames after the result is committed so
            -- we capture any post-test cleanup writes.
            if rows > 0 then
                stop_now(string.format("result_set_$%02X", v))
                return
            end
        end
    end
end

local function on_script_ended()
    close_output()
end

if not open_output() then return end

emu.addMemoryCallback(on_4016_write, emu.callbackType.write, 0x4016, 0x4016)
emu.addMemoryCallback(on_4017_write, emu.callbackType.write, 0x4017, 0x4017)
emu.addMemoryCallback(on_4016_read,  emu.callbackType.read,  0x4016, 0x4016)
emu.addMemoryCallback(on_4017_read,  emu.callbackType.read,  0x4017, 0x4017)
emu.addEventCallback(on_start_frame, emu.eventType.startFrame)
emu.addEventCallback(on_script_ended, emu.eventType.scriptEnded)

emu.log(string.format("[ctrl_trace] armed: max_frames=%d result_addr=$%04X out=%s",
    CONFIG.MAX_FRAMES, CONFIG.RESULT_ADDR, CONFIG.OUT_PATH))
