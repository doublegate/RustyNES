-- RustyNES Lua example (v1.6.0 Workstream B2): coroutine driving loop.
--
-- Demonstrates the `emu.run(fn)` + `emu.frameadvance()` DRIVING primitives —
-- the FCEUX / BizHawk model where one script *drives* the emulator a frame at a
-- time instead of (only) reacting to per-frame callbacks. This is the building
-- block bots and TAS scripts need: linear "do this, advance a frame, then do
-- that" logic without manually threading state through `onFrame`.
--
-- How it works:
--   * `emu.run(fn)` registers `fn` as the driving coroutine.
--   * Inside it, `emu.frameadvance()` yields control back to the emulator; the
--     host advances EXACTLY one frame, then resumes the coroutine where it left
--     off. So each `emu.frameadvance()` corresponds to one emulated frame.
--   * The loop below holds A for 10 frames, releases for 10, and repeats — a
--     trivial autofire-style driver.
--
-- Determinism: `emu.setInput` (and any other state-mutating effect) is gated
-- exactly like `emu.write` — it is a silent no-op under netplay / TAS-replay /
-- RA-hardcore, so a driving script can never desync a locked session.
--
-- API used: emu.run, emu.frameadvance, emu.setInput, emu.log, emu.frame.

local A = 0x01 -- standard-controller A-button bit.

emu.run(function()
  local held = false
  while true do
    -- Hold A for 10 frames, then release for 10, forever.
    for _ = 1, 10 do
      emu.setInput(0, held and A or 0)
      emu.frameadvance()
    end
    held = not held
    emu.log("toggled A -> " .. tostring(held) .. " at frame " .. emu.frame)
  end
end)
