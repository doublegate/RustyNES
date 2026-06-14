-- RustyNES Lua example: a RAM watch + write tracer.
--
-- Logs every write to a watched zero-page address, and once per second
-- (~60 frames) prints a short hex dump of the first 8 bytes of RAM. Shows
-- emu.onWrite (per-access callback) + emu.read / emu.readRange + emu.onFrame.
--
-- Change WATCH to the address you care about. Requires a `scripting` build.

local WATCH = 0x0010

emu.onWrite(WATCH, function(addr, value)
    emu.log(string.format("write $%04X = $%02X", addr, value))
end)

local n = 0
emu.onFrame(function()
    n = n + 1
    if n % 60 == 0 then
        local bytes = emu.readRange(0x0000, 8)
        local parts = {}
        for i = 1, #bytes do
            parts[i] = string.format("%02X", bytes[i])
        end
        emu.log("RAM $0000: " .. table.concat(parts, " "))
    end
end)

emu.log("ram_watch.lua watching $" .. string.format("%04X", WATCH))
