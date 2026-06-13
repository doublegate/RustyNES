-- mesen2_apu_reg_activation_trace.lua
--
-- Focused Mesen2 Lua trace for AccuracyCoin's `APU Register Activation`
-- test (result address $045C). Captures every $4014 (OAM DMA trigger),
-- $4015 (APU status read/write), $4016, $4017 access with per-cycle
-- context (CPU cycle, frame, scanline, dot, value, parity-derived M2
-- phase).
--
-- Sprint 2 iteration 4 of v1.0.0-final (Session-26, 2026-05-23). Used in
-- conjunction with the matching RustyNES binary
-- `trace_apu_reg_activation` to cross-diff how the OAM DMA from page
-- $40 interacts with the APU register active-window between the two
-- emulators. The Test 4 axis: after `LDA #$40; STA $4014` (OAM DMA from
-- page $40), the frame-counter IRQ flag in $4015 bit 6 should STILL be
-- set because the OAM DMA's 6502 address bus is NOT in $4000-$401F
-- (the OAM DMA uses the OAM address bus, leaving the 6502 bus parked
-- at the halted address from the prior CPU read).
--
-- USAGE:
--   xvfb-run -a ~/AppImages/mesen.appimage --testRunner \
--       tests/roms/AccuracyCoin/sub-tests/apu-reg-activation.nes \
--       scripts/mesen2_apu_reg_activation_trace.lua
--
-- ENV VARS:
--   MESEN2_AREG_TRACE_OUT       : output CSV path
--                                  (default /tmp/mesen2_apu_reg_activation_trace.csv)
--   MESEN2_AREG_TRACE_MAX_FRAMES: stop after this many frames (default 60)
--   MESEN2_AREG_TRACE_RESULT_ADDR: stop when this addr becomes non-zero
--                                   (default 0x045C == result_APURegActivation)
--
-- CSV schema:
--   cpu_cycle, ppu_frame, ppu_scanline, ppu_dot, m2_phase,
--   access, addr, value, irq_pending
--
-- `m2_phase` is derived from `cpu_cycle & 1` (per Mesen2 NesApu/ApuFrameCounter
-- `GetMasterClock() & 0x01` convention). Even cycles -> 'L', odd cycles -> 'H'.
-- The exact polarity is symmetric -- the cross-diff cares only about the
-- RELATIVE alignment between the two emulators' reads/writes.
--
-- `irq_pending` is the frame-counter IRQ-flag bit (bit 6) value SEEN on
-- the data bus AFTER the access. For $4015 reads we record (value & 0x40)
-- since Mesen2's `GetIrqFlag()` may schedule the clear after returning the
-- pre-clear value.

local CONFIG = {
    OUT_PATH    = os.getenv("MESEN2_AREG_TRACE_OUT") or "/tmp/mesen2_apu_reg_activation_trace.csv",
    MAX_FRAMES  = tonumber(os.getenv("MESEN2_AREG_TRACE_MAX_FRAMES") or "120"),
    RESULT_ADDR = tonumber(os.getenv("MESEN2_AREG_TRACE_RESULT_ADDR") or "0x045C"),
}

local file = nil
local frame_count = 0
local stopped = false
local rows = 0

local function open_output()
    local f, err = io.open(CONFIG.OUT_PATH, "w")
    if not f then
        emu.log("[areg_trace] could not open " .. CONFIG.OUT_PATH .. ": " .. tostring(err))
        return false
    end
    file = f
    file:write("cpu_cycle,ppu_frame,ppu_scanline,ppu_dot,m2_phase,access,addr,value,irq_pending\n")
    emu.log("[areg_trace] writing CSV to " .. CONFIG.OUT_PATH)
    return true
end

local function close_output()
    if file then
        file:close()
        file = nil
        emu.log(string.format("[areg_trace] wrote %d rows across %d frames", rows, frame_count))
    end
end

local function stop_now(reason)
    if stopped then return end
    stopped = true
    close_output()
    emu.log("[areg_trace] STOP: " .. tostring(reason))
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
    local irq_pending
    if access == "R" and addr == 0x4015 then
        irq_pending = (value & 0x40) ~= 0 and 1 or 0
    else
        irq_pending = -1
    end
    file:write(string.format(
        "%d,%d,%d,%d,%s,%s,$%04X,$%02X,%d\n",
        cycle, frame, scanline, dot, m2, access, addr, value, irq_pending
    ))
    rows = rows + 1
end

local function on_write(addr, value)
    write_row("W", addr, value)
end

local function on_read(addr, value)
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

emu.addMemoryCallback(on_write, emu.callbackType.write, 0x4014, 0x4014)
emu.addMemoryCallback(on_write, emu.callbackType.write, 0x4015, 0x4015)
emu.addMemoryCallback(on_write, emu.callbackType.write, 0x4016, 0x4016)
emu.addMemoryCallback(on_write, emu.callbackType.write, 0x4017, 0x4017)
emu.addMemoryCallback(on_read,  emu.callbackType.read,  0x4015, 0x4015)
emu.addMemoryCallback(on_read,  emu.callbackType.read,  0x4016, 0x4016)
emu.addMemoryCallback(on_read,  emu.callbackType.read,  0x4017, 0x4017)
emu.addEventCallback(on_start_frame, emu.eventType.startFrame)
emu.addEventCallback(on_script_ended, emu.eventType.scriptEnded)

emu.log(string.format("[areg_trace] armed: max_frames=%d result_addr=$%04X out=%s",
    CONFIG.MAX_FRAMES, CONFIG.RESULT_ADDR, CONFIG.OUT_PATH))
