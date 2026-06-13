-- Like mesen2_controller_trace.lua but also captures master_clock parity.

local CONFIG = {
    OUT_PATH    = os.getenv("MESEN2_CTRL_TRACE_OUT") or "/tmp/mesen2_controller_trace_v2.csv",
    MAX_FRAMES  = tonumber(os.getenv("MESEN2_CTRL_TRACE_MAX_FRAMES") or "60"),
    RESULT_ADDR = tonumber(os.getenv("MESEN2_CTRL_TRACE_RESULT_ADDR") or "0x045F"),
}

local file = nil
local frame_count = 0
local stopped = false
local rows = 0

local function open_output()
    local f, err = io.open(CONFIG.OUT_PATH, "w")
    if not f then return false end
    file = f
    file:write("cpu_cycle,master_clock,cyc_parity,mclk_parity,access,addr,value\n")
    return true
end

local function close_output()
    if file then file:close(); file = nil end
end

local function stop_now(reason)
    if stopped then return end
    stopped = true
    close_output()
    emu.log("[ctrl_trace_v2] STOP: " .. tostring(reason))
    emu.stop(0)
end

local function write_row(access, addr, value)
    if not file or stopped then return end
    local state = emu.getState()
    local cycle = state["cpu.cycleCount"] or 0
    local mclk = state["cpu.masterClock"] or 0
    file:write(string.format(
        "%d,%d,%d,%d,%s,$%04X,$%02X\n",
        cycle, mclk, cycle % 2, mclk % 2, access, addr, value
    ))
    rows = rows + 1
end

local function on_4016_write(addr, value) write_row("W", addr, value) end
local function on_4016_read(addr, value) write_row("R", addr, value) end

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
        if v ~= 0 and rows > 0 then
            stop_now(string.format("result_set_$%02X", v))
            return
        end
    end
end

if not open_output() then return end

emu.addMemoryCallback(on_4016_write, emu.callbackType.write, 0x4016, 0x4016)
emu.addMemoryCallback(on_4016_read,  emu.callbackType.read,  0x4016, 0x4016)
emu.addEventCallback(on_start_frame, emu.eventType.startFrame)
emu.addEventCallback(close_output, emu.eventType.scriptEnded)
