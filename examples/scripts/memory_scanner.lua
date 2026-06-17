-- RustyNES Lua example (v1.5.0 Workstream B): a Cheat-Engine-style RAM scanner.
--
-- Demonstrates the `memory:` table + `cart:` queries. Each frame it scans the
-- 2 KiB CPU work RAM ($0000-$07FF), keeping only the addresses whose value
-- still matches the last snapshot's value MINUS the configured `decay` (i.e.
-- "find the value that decreased by N since last frame" — the classic
-- next-scan workflow). Logs the surviving candidate count + the first few.
--
-- This is purely observational (reads only), so it runs unperturbed even in a
-- locked netplay / TAS-replay / RA-hardcore session.
--
-- API used: memory:read_range, cart:mapper_id/prg_size/chr_size, emu.frame,
-- emu.log.

local RAM_LEN = 0x0800     -- 2 KiB internal work RAM
local decay = 0            -- "value decreased by this much" per frame (0 = equal)
local candidates = nil     -- nil until the first scan; then {addr = last_value}
local prev = nil

emu.onFrame(function()
    local now = memory:read_range(0, RAM_LEN) -- 1-based array of bytes

    if prev == nil then
        prev = now
        emu.log(string.format(
            "scanner armed: mapper %d, PRG %d KiB, CHR %d KiB",
            cart:mapper_id(), cart:prg_size() // 1024, cart:chr_size() // 1024))
        return
    end

    if candidates == nil then
        -- First real scan: seed every address as a candidate.
        candidates = {}
        for i = 1, RAM_LEN do candidates[i - 1] = prev[i] end
    end

    -- Next scan: keep only the candidates that changed by exactly `decay`.
    local survivors = {}
    local count = 0
    for addr, last in pairs(candidates) do
        local cur = now[addr + 1]
        if (last - cur) % 256 == decay then
            survivors[addr] = cur
            count = count + 1
        end
    end
    candidates = survivors
    prev = now

    if emu.frame % 30 == 0 then
        local sample = {}
        for addr, _ in pairs(candidates) do
            sample[#sample + 1] = string.format("$%04X", addr)
            if #sample >= 4 then break end
        end
        emu.log(string.format("frame %d: %d candidates  %s",
            emu.frame, count, table.concat(sample, " ")))
    end
end)

emu.log("memory_scanner.lua loaded")
