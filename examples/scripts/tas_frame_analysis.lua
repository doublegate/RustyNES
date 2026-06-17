-- RustyNES Lua example (v1.5.0 Workstream B): TAS frame-analysis helper.
--
-- Demonstrates the in-memory save-state slots + `pause_at_frame` + `cart:` for
-- a TAS authoring loop:
--
--   * Every `CHECKPOINT` frames it snapshots state into in-memory slot 1 (a
--     cheap rolling restore point that never touches your on-disk saves).
--   * It auto-pauses at frame `PAUSE_AT` so you can inspect a known boundary.
--   * It tracks per-frame deltas of a watched address (e.g. a position counter)
--     and logs the running speed, the kind of metric a TAS run optimizes.
--
-- `save_state`/`load_state` mutate emulator state, so they are gated exactly
-- like `emu.write`: inert under netplay / TAS-replay / RA-hardcore.
--
-- API used: emu:save_state/load_state, emu:pause_at_frame, memory:peek,
-- cart:region, emu.frame, emu.log.

local CHECKPOINT = 600      -- snapshot to in-memory slot 1 every N frames
local PAUSE_AT   = 1800     -- auto-pause at this frame for inspection
local WATCH      = 0x0010   -- the address whose per-frame delta we report

local last = nil
local armed_pause = false

emu.onFrame(function()
    -- One-shot: arm the inspection pause + announce the timebase.
    if not armed_pause then
        emu:pause_at_frame(PAUSE_AT)
        armed_pause = true
        emu.log(string.format("TAS analysis: %s timebase, pausing at frame %d",
            cart:region(), PAUSE_AT))
    end

    -- Rolling in-memory checkpoint (no disk I/O, never your save slots).
    if emu.frame % CHECKPOINT == 0 and emu.frame > 0 then
        emu:save_state(1)
        emu.log(string.format("checkpoint -> slot 1 @ frame %d", emu.frame))
    end

    -- Per-frame delta of the watched byte (signed, treating $80-$FF as < 0).
    local raw = memory:peek(WATCH)
    local v = raw >= 128 and raw - 256 or raw
    if last ~= nil then
        local d = v - last
        if d ~= 0 then
            emu.log(string.format("frame %d: $%04X %d -> %d (delta %+d)",
                emu.frame, WATCH, last, v, d))
        end
    end
    last = v
end)

emu.log("tas_frame_analysis.lua loaded")
