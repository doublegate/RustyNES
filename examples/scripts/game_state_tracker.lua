-- RustyNES Lua example (v1.5.0 Workstream B): a symbol-aware game-state tracker.
--
-- Demonstrates the `sym:` table (debugger symbol-label queries) + `memory:`
-- reads + an on-screen HUD. Load a `.sym` / `.mlb` / `.nl` label file first
-- (Debug -> Load Symbols...) that defines labels for the addresses you care
-- about; this script resolves them by NAME and draws their live values.
--
-- If no symbols are loaded, the WATCH names fall back to raw addresses so the
-- HUD still works.
--
-- Purely observational (reads + overlay only) — safe in any locked session.
--
-- API used: sym:addr, sym:name, memory:peek, emu.drawRect/drawText, emu.onFrame.

-- The labels (or raw $addr fallbacks) to display, top to bottom.
local WATCH = { "player_x", "player_y", "lives", "score_lo" }

-- Resolve a watch entry to a CPU address: a label via sym:addr, else parse a
-- "$XXXX" literal, else nil.
local function resolve(name)
    local a = sym:addr(name)
    if a ~= nil then return a end
    local hex = name:match("^%$(%x+)$")
    if hex ~= nil then return tonumber(hex, 16) end
    return nil
end

emu.onFrame(function()
    emu.drawRect(2, 2, 118, 8 + #WATCH * 8, 0x000000B0)
    emu.drawText(4, 3, "game state", 0xFFFF80FF)
    local y = 12
    for _, name in ipairs(WATCH) do
        local addr = resolve(name)
        if addr ~= nil then
            -- Prefer the symbol name for this address if one exists.
            local label = sym:name(addr) or name
            emu.drawText(4, y, string.format("%-10s %02X", label, memory:peek(addr)),
                0xC0FFC0FF)
        else
            emu.drawText(4, y, string.format("%-10s  ??", name), 0xFF8080FF)
        end
        y = y + 8
    end
end)

emu.log("game_state_tracker.lua loaded (load a .sym/.mlb/.nl for named fields)")
