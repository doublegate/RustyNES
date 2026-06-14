-- RustyNES Lua example: a tiny on-screen HUD.
--
-- Draws the live frame counter and CPU program counter over the game each
-- frame. Load it from the debugger's "Lua Script" console (Debug -> Lua Script
-- -> Load .lua...). Requires a `scripting`-enabled build.
--
-- API used: emu.onFrame, emu.cpu(), emu.frame, emu.drawText, emu.drawRect.

emu.onFrame(function()
    local c = emu.cpu()
    -- A translucent backdrop box so the text is readable over any scene.
    emu.drawRect(2, 2, 92, 18, 0x000000A0)
    emu.drawText(4, 3, string.format("frame %d", emu.frame), 0xFFFFFFFF)
    emu.drawText(4, 11, string.format("PC=%04X A=%02X", c.pc, c.a), 0x80FF80FF)
end)

emu.log("hud.lua loaded")
