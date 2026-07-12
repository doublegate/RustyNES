-- RustyNES Lua example: the emu.addEventCallback lifecycle events.
--
-- Registers a handler for each host-fired lifecycle event and logs it, so you
-- can watch the emulator's timeline in the Lua console: frame boundaries,
-- interrupts, the per-frame sprite-0 hit, a soft-reset, and save-state loads.
--
-- The v2.1.10 "Creator Tools" additions are `reset`, `spriteZeroHit`, and
-- `codeBreak`; `startFrame` / `endFrame` / `nmi` / `irq` / `inputPolled` /
-- `stateLoaded` / `stateSaved` round out the Mesen2-parity event surface.
--
-- These events are a NATIVE (mlua) feature. On the wasm (piccolo) backend
-- addEventCallback is a documented no-op (ADR 0012): the script still loads,
-- but the handlers never fire.
--
-- API used: emu.addEventCallback, emu.log, emu.frame.

-- Frame boundaries — fired from the engine's own per-frame pump.
emu.addEventCallback(function()
    -- Keep this quiet; uncomment to trace every frame boundary.
    -- emu.log("startFrame " .. emu.frame)
end, "startFrame")

-- The sprite-0 hit event fires at most once per frame the PPU set the flag —
-- a classic split-screen / status-bar timing signal.
emu.addEventCallback(function(frame)
    emu.log(string.format("sprite-0 hit on frame %d", frame))
end, "spriteZeroHit")

-- A soft-reset / power-cycle of the running ROM.
emu.addEventCallback(function()
    emu.log("reset")
end, "reset")

-- Execution hit a debugger breakpoint (the PC is passed).
emu.addEventCallback(function(pc)
    emu.log(string.format("code break @ %04X", pc))
end, "codeBreak")

-- Interrupt service (replayed from the core's committed interrupt log).
emu.addEventCallback(function()
    emu.log("nmi")
end, "nmi")

-- A save-state was loaded (host or scripted).
emu.addEventCallback(function(slot)
    emu.log(string.format("state loaded (slot %d)", slot))
end, "stateLoaded")

emu.log("lifecycle_events.lua loaded")
