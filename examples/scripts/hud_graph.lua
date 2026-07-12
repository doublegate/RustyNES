-- RustyNES Lua example: a live value graph drawn with emu.drawLine.
--
-- Plots the low byte of the CPU accumulator over time as a scrolling line
-- graph in the top-left corner, demonstrating the v2.1.10 `emu.drawLine` HUD
-- primitive alongside drawRect / drawText. Swap the sampled address for a
-- game RAM variable (health, x-velocity, RNG) to visualise it frame-by-frame.
--
-- Load from the debugger's "Lua Script" console (Debug -> Lua Script -> Load
-- .lua...). Requires a `scripting`-enabled build. drawLine is at full parity
-- on the wasm (piccolo) backend too.
--
-- API used: emu.onFrame, emu.cpu(), memory:peek, emu.drawRect / drawLine /
-- drawText.

local GRAPH_X, GRAPH_Y = 4, 24 -- top-left of the plot
local GRAPH_W, GRAPH_H = 96, 40 -- plot size in NES pixels
local samples = {} -- ring of the last GRAPH_W values (0..255)

-- Address to plot: default is the accumulator; point this at a game variable
-- (e.g. 0x00B0) to graph it instead.
local WATCH_ADDR = nil

emu.onFrame(function()
    -- Sample either the watched RAM byte or the CPU accumulator.
    local value
    if WATCH_ADDR then
        value = memory:peek(WATCH_ADDR)
    else
        value = emu.cpu().a
    end
    table.insert(samples, value)
    while #samples > GRAPH_W do
        table.remove(samples, 1)
    end

    -- Backdrop + frame so the graph reads over any scene.
    emu.drawRect(GRAPH_X - 2, GRAPH_Y - 2, GRAPH_W + 4, GRAPH_H + 4, 0x000000B0)
    emu.drawText(GRAPH_X, GRAPH_Y - 10, "A (0..255)", 0x80C0FFFF)

    -- Connect consecutive samples with line segments. Value 0 maps to the
    -- bottom of the plot, 255 to the top (y grows downward on the NES frame).
    local function y_of(v)
        return GRAPH_Y + GRAPH_H - math.floor(v * GRAPH_H / 255)
    end
    for i = 2, #samples do
        local x1 = GRAPH_X + (i - 2)
        local x2 = GRAPH_X + (i - 1)
        emu.drawLine(x1, y_of(samples[i - 1]), x2, y_of(samples[i]), 0x40FF80FF)
    end
end)

emu.log("hud_graph.lua loaded")
