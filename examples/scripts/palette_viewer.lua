-- RustyNES Lua example: an on-screen palette + CHR inspector.
--
-- Draws the 32 active palette RAM entries ($3F00-$3F1F) as a grid of swatches,
-- and a tiny readout of the first CHR bytes, demonstrating the v2.1.10
-- `memory:read_palette` and `memory:read_chr` PPU read domains. Both reads use
-- the side-effect-free debug-peek path, so inspecting them never perturbs the
-- deterministic run.
--
-- Because the palette RAM stores a 6-bit NES colour index (not RGB), the
-- swatches here are a coarse index-to-grey mapping — enough to see the palette
-- change between scenes. Load from Debug -> Lua Script -> Load .lua...
--
-- API used: emu.onFrame, memory:read_palette, memory:read_chr, emu.drawRect /
-- drawLine / drawText.

local ORIGIN_X, ORIGIN_Y = 4, 24
local CELL = 8 -- swatch size in NES pixels

-- Map a 6-bit palette index (0..63) to an opaque 0xRRGGBBAA grey so the grid
-- is visible without a full NES-palette lookup.
local function index_to_color(idx)
    local g = math.floor((idx & 0x3F) * 255 / 63)
    return (g << 24) | (g << 16) | (g << 8) | 0xFF
end

emu.onFrame(function()
    -- Backdrop.
    emu.drawRect(ORIGIN_X - 2, ORIGIN_Y - 2, 16 * CELL + 4, 2 * CELL + 14, 0x000000C0)
    emu.drawText(ORIGIN_X, ORIGIN_Y - 10, "palette $3F00-$3F1F", 0xC0C0FFFF)

    -- 32 entries laid out 16 x 2 (background palettes on top, sprites below).
    for i = 0, 31 do
        local col = i % 16
        local row = i // 16
        local x = ORIGIN_X + col * CELL
        local y = ORIGIN_Y + row * CELL
        local idx = memory:read_palette(i)
        emu.drawRect(x, y, CELL - 1, CELL - 1, index_to_color(idx))
    end

    -- A separator line between the BG and sprite halves.
    emu.drawLine(ORIGIN_X, ORIGIN_Y + CELL, ORIGIN_X + 16 * CELL, ORIGIN_Y + CELL, 0xFFFFFF60)

    -- First two CHR bytes (pattern table $0000), as a sanity readout.
    local c0 = memory:read_chr(0x0000)
    local c1 = memory:read_chr(0x0001)
    emu.drawText(ORIGIN_X, ORIGIN_Y + 2 * CELL + 2, string.format("CHR $0000: %02X %02X", c0, c1), 0x80FF80FF)
end)

emu.log("palette_viewer.lua loaded")
