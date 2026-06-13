-- mesen2_ppu_trace.lua
--
-- Mesen2 Lua reference-trace script for RustyNES's Session-10
-- per-PPU-dot observability tooling.
--
-- Emits a binary trace file compatible with the
-- `rustynes_ppu::state_trace::PpuStateTrace` binary format
-- (`crates/rustynes-ppu/src/state_trace.rs` schema v1, RECORD_SIZE = 113).
-- The companion `ppu_trace_diff` CLI consumes both this script's
-- output and the in-tree fixture's output and reports per-field
-- divergences.
--
-- See `docs/ppu-trace-tooling.md` for usage and `docs/adr/0005-ppu-state-trace.md`
-- for the design.
--
-- =================================================================
-- INSTALLING + RUNNING
-- =================================================================
--
-- 1. Install Mesen2: https://www.mesen.ca/ or
--    `yay -S mesen2-git` on Arch-family distros.  AppImage:
--    https://github.com/SourMesen/Mesen2/releases.
-- 2. Run Mesen2 in headless mode (no UI) via --testRunner:
--      mesen --testRunner <rom_path> <this_script.lua>
--    The script writes its .bin output, then calls emu.stop(0) once
--    END_FRAME + tail margin has elapsed.  On Linux you may need to
--    wrap the call in `xvfb-run -a` if no display is available — the
--    .NET Avalonia front-end initialises the X11 toolkit even in
--    --testRunner mode.
-- 3. To run inside the GUI instead (e.g. when iterating on the
--    script) load Tools > Script Window > File > Open this .lua file,
--    edit CONFIG below, press F1 to arm, then load the ROM.
--
-- =================================================================
-- CALLBACK GRANULARITY CAVEAT (important!)
-- =================================================================
--
-- Mesen2's Lua API exposes per-event callbacks (`startFrame`,
-- `endFrame`, `nmi`, `irq`, `reset`, `scriptEnded`, `inputPolled`,
-- `stateLoaded`, `stateSaved`, `codeBreak`) but does NOT expose a
-- per-scanline or per-PPU-cycle callback (verified 2026-05-20 against
-- `https://raw.githubusercontent.com/SourMesen/Mesen2/master/UI/Debugger/Documentation/LuaDocumentation.json`,
-- enum `eventType`).
--
-- This means a faithful per-dot OR per-scanline reference trace
-- canNOT be produced by Lua alone. This script therefore emits ONE
-- record per frame at the `endFrame` boundary (PPU dot 0 of scanline
-- 0 of the NEXT frame, equivalently `scanline = -1` of the just-
-- completed frame's pre-render at the moment vblank ends).
--
-- The matching RustyNES-side fixture should be configured with
-- `PpuTraceConfig::endframe_only(range)` — see the diff tool's
-- `--reference-granularity per-frame` mode (Session-11 addition) for
-- the comparator that drops per-dot RustyNES records that have no
-- per-frame Mesen2 counterpart.
--
-- For finer-than-per-frame reference resolution, two paths exist
-- (both deferred past Session-11):
--   (a) Parse Mesen2's built-in trace log file format (text). The
--       log is per-CPU-instruction so PPU state at the instruction
--       boundary is interpolable.
--   (b) Patch Mesen2's C++ core to expose a new
--       `emu.eventType.scanline` or `cycle` event. See
--       `docs/ppu-trace-tooling.md` § "Approach B" — out of scope
--       for the v0.9.0 / v1.0.0 investigation.
--
-- =================================================================
-- BINARY SCHEMA (must match Rust side)
-- =================================================================
--
-- File header (16 bytes):
--   0..11   "RUSTYNES_PPU"      (12-byte ASCII magic)
--   12..13  schema version      (uint16 LE; currently 1)
--   14..15  reserved flags      (uint16 LE; zero in schema v1)
--
-- Per-record (113 bytes, little-endian throughout):
--   See `RECORD_FORMAT` below; matches Rust's
--   PpuStateRecord::to_bytes packer field-for-field. Fields that
--   Mesen2 cannot supply at endFrame granularity are zero-filled
--   and intended to be `--skip-fields`'d in the diff:
--     * sprite-eval FSM (n, m, found, sec_idx, copying,
--       overflow_search, done, read_latch)
--     * sprite line-up arrays (spr_shift_lo/hi, spr_attr, spr_x)
--     * BG attribute / latch fields (at_shift_lo/hi, nt_latch,
--       at_latch, bg_lo_latch, bg_hi_latch)
--   spr_count, spr_zero, and bg_shift_lo/hi are emitted from the
--   getState() snapshot at endFrame.

local CONFIG = {
    OUT_PATH    = os.getenv("MESEN2_PPU_TRACE_OUT") or "/tmp/mesen2_ppu_trace.bin",
    -- Frames to capture (inclusive). AccuracyCoin's test runner
    -- starts around frame 306 (after a 300-frame splash + menu
    -- + 6-frame Start press), so 310..=350 is a typical
    -- diagnostic window. Override via env var for headless runs.
    START_FRAME = tonumber(os.getenv("MESEN2_PPU_TRACE_START") or "310"),
    END_FRAME   = tonumber(os.getenv("MESEN2_PPU_TRACE_END")   or "350"),
    -- Number of additional frames to run past END_FRAME before
    -- calling emu.stop(0). Gives any deferred records time to
    -- flush. 5 is generous; the script writes synchronously so 0
    -- would also work.
    TAIL_FRAMES = 5,
    -- Status-print cadence (every N frames; 0 = silent).
    LOG_EVERY   = 10,
    -- Frame range during which Start button is held (inclusive). When
    -- set this script will issue `emu.setInput({start=true}, 0)` from
    -- the `inputPolled` callback for every frame in [START_PRESS_LO,
    -- START_PRESS_HI]. AccuracyCoin's RustyNES boot sequence presses
    -- Start across frames 300..=305 (matching
    -- `accuracy_coin::run_battery_capturing_ram`); replicate that on
    -- the Mesen2 side to keep the per-frame trace in lockstep.
    -- Defaults match the RustyNES fixture exactly.
    START_PRESS_LO = tonumber(os.getenv("MESEN2_PPU_TRACE_START_PRESS_LO") or "300"),
    START_PRESS_HI = tonumber(os.getenv("MESEN2_PPU_TRACE_START_PRESS_HI") or "305"),
}

-- =================================================================
-- HELPERS
-- =================================================================

-- FNV-1a 64-bit hash, matching `rustynes_ppu::state_trace::fnv1a64`.
-- The Rust offset basis / prime are the canonical FNV-1a constants
-- (http://www.isthe.com/chongo/tech/comp/fnv/).
--
-- Lua 5.3+ has 64-bit integers natively; we use bit operators
-- (~, &, |, <<, >>) which are integer operations on 5.3+. (Mesen2
-- embeds Lua 5.4.)
local function fnv1a64(bytes)
    local FNV_OFFSET = 0xCBF29CE484222325
    local FNV_PRIME  = 0x100000001B3
    local h = FNV_OFFSET
    for i = 1, #bytes do
        h = h ~ bytes[i]
        h = (h * FNV_PRIME) & 0xFFFFFFFFFFFFFFFF
    end
    return h
end

-- Write 16-byte header to a freshly-opened binary file handle.
local function write_header(f)
    f:write("RUSTYNES_PPU")               -- magic, 12 bytes
    f:write(string.pack("<I2", 1))        -- schema version
    f:write(string.pack("<I2", 0))        -- reserved flags
end

-- Build a 113-byte record from the current `emu.getState()` plus
-- OAM + secondary OAM reads. The field order is the SAME order
-- as the Rust packer in `crates/rustynes-ppu/src/state_trace.rs`:
-- to_bytes; if either side drifts the `ppu_trace_diff` tool's
-- anchor check will catch it on the first record.
local function build_record(state, secondary_oam_bytes, oam_hash)
    -- Mesen2's `emu.getState()` returns a FLAT table with dotted-
    -- string keys (verified 2026-05-20 against Mesen2 0.42+ via
    -- pairs() introspection in --testRunner mode). There is NO
    -- nested `state.ppu` subtable — every leaf field is keyed at
    -- the top level as `"ppu.frameCount"`, `"ppu.scanline"`, etc.
    --
    -- This is a documented gotcha and the reason the original
    -- Session-10 script (which assumed `state.ppu.frameCount`-
    -- style access via subtables) silently emitted zero records.
    --
    -- Keys observed for the NES (subset relevant to this trace):
    --   "ppu.frameCount"                  (u32, the just-completed
    --                                      frame number)
    --   "ppu.scanline"                    (i16; 240 at endFrame —
    --                                      Mesen2 fires endFrame
    --                                      at scanline 240 dot 0,
    --                                      which is the start of
    --                                      post-render)
    --   "ppu.cycle"                       (u16, 0..=340; 0 at endFrame)
    --   "ppu.control.verticalWrite"       (bool)
    --   "ppu.control.spritePatternAddr"   (number — 0 or 0x1000)
    --   "ppu.control.backgroundPatternAddr" (number — 0 or 0x1000)
    --   "ppu.control.largeSprites"        (bool)
    --   "ppu.control.nmiOnVerticalBlank"  (bool)
    --   "ppu.mask.{grayscale,backgroundMask,spriteMask,...}" (bool×8)
    --   "ppu.statusFlags.spriteOverflow"  (bool)
    --   "ppu.statusFlags.sprite0Hit"      (bool)
    --   "ppu.statusFlags.verticalBlank"   (bool)
    --   "ppu.spriteRamAddr"               (OAMADDR)
    --   "ppu.videoRamAddr"                (v)
    --   "ppu.tmpVideoRamAddr"             (t — bottom 2 nametable
    --                                      bits encoded in bits 10-11)
    --   "ppu.xScroll"                     (fine X)
    --   "ppu.writeToggle"                 (bool, w)
    --   "ppu.lowBitShift", "ppu.highBitShift" (BG shifters)
    --   "ppu.secondarySpriteRam<0..31>"   (each byte of secondary
    --                                      OAM — alternative to
    --                                      emu.read(memType.nesSecondarySpriteRam))

    local function f(k) return state[k] end -- shorthand for flat access

    local frame    = f("ppu.frameCount") or 0
    local scanline = f("ppu.scanline") or 0
    local dot      = f("ppu.cycle") or 0

    -- $2000-$2003 register snapshots.
    local ctrl_byte = 0
    do
        -- Nametable base address (bits 0-1) is encoded in
        -- tmpVideoRamAddr bits 10-11.
        local t_val = f("ppu.tmpVideoRamAddr") or 0
        ctrl_byte = ctrl_byte | ((t_val >> 10) & 0x03)
        if f("ppu.control.verticalWrite")                       then ctrl_byte = ctrl_byte | 0x04 end
        if (f("ppu.control.spritePatternAddr") or 0)   == 0x1000 then ctrl_byte = ctrl_byte | 0x08 end
        if (f("ppu.control.backgroundPatternAddr") or 0) == 0x1000 then ctrl_byte = ctrl_byte | 0x10 end
        if f("ppu.control.largeSprites")                        then ctrl_byte = ctrl_byte | 0x20 end
        if f("ppu.control.nmiOnVerticalBlank")                  then ctrl_byte = ctrl_byte | 0x80 end
    end

    local mask_byte = 0
    if f("ppu.mask.grayscale")          then mask_byte = mask_byte | 0x01 end
    if f("ppu.mask.backgroundMask")     then mask_byte = mask_byte | 0x02 end
    if f("ppu.mask.spriteMask")         then mask_byte = mask_byte | 0x04 end
    if f("ppu.mask.backgroundEnabled")  then mask_byte = mask_byte | 0x08 end
    if f("ppu.mask.spritesEnabled")     then mask_byte = mask_byte | 0x10 end
    if f("ppu.mask.intensifyRed")       then mask_byte = mask_byte | 0x20 end
    if f("ppu.mask.intensifyGreen")     then mask_byte = mask_byte | 0x40 end
    if f("ppu.mask.intensifyBlue")      then mask_byte = mask_byte | 0x80 end

    local status_byte = 0
    if f("ppu.statusFlags.spriteOverflow") then status_byte = status_byte | 0x20 end
    if f("ppu.statusFlags.sprite0Hit")     then status_byte = status_byte | 0x40 end
    if f("ppu.statusFlags.verticalBlank")  then status_byte = status_byte | 0x80 end

    local oam_addr = f("ppu.spriteRamAddr") or 0
    local v        = f("ppu.videoRamAddr") or 0
    local t        = f("ppu.tmpVideoRamAddr") or 0
    local fine_x   = f("ppu.xScroll") or 0
    local w_tog    = f("ppu.writeToggle") and 1 or 0

    -- Sprite-eval FSM: Mesen2 does not expose these. Emit zeros;
    -- callers MUST pass `--skip-fields
    -- sprite_eval_n,sprite_eval_m,sprite_eval_found,sprite_eval_sec_idx,sprite_eval_copying,sprite_eval_overflow_search,sprite_eval_done,sprite_eval_read_latch`
    -- when comparing.
    local seval_n     = 0
    local seval_m     = 0
    local seval_found = 0
    local seval_sec   = 0
    local seval_copy  = 0
    local seval_overf = 0
    local seval_done  = 0
    local seval_latch = 0

    -- Per-scanline sprite line-up. Mesen2 doesn't expose these
    -- through getState(); emit zeros and skip in the diff.
    local spr_count = 0
    local spr_zero  = 0
    local spr_shift_lo = {0,0,0,0,0,0,0,0}
    local spr_shift_hi = {0,0,0,0,0,0,0,0}
    local spr_attr     = {0,0,0,0,0,0,0,0}
    local spr_x        = {0,0,0,0,0,0,0,0}

    -- BG pipeline.
    local bg_shift_lo = f("ppu.lowBitShift")  or 0
    local bg_shift_hi = f("ppu.highBitShift") or 0
    local at_shift_lo = 0  -- Mesen2 doesn't expose; --skip-fields it.
    local at_shift_hi = 0
    local nt_latch    = 0
    local at_latch    = 0
    local bg_lo_latch = 0
    local bg_hi_latch = 0

    -- nmi_line: Mesen2 exposes the CPU's pending NMI as a numeric
    -- flag at `"cpu.nmiFlag"` in the flat state table. May not be
    -- populated on all consoles; default to 0.
    local nmi_val = state["cpu.nmiFlag"]
    local nmi = (nmi_val and nmi_val ~= 0 and nmi_val ~= false) and 1 or 0

    -- Pack 113 bytes in the canonical order.
    local parts = {}
    parts[#parts + 1] = string.pack("<I4", frame & 0xFFFFFFFF)
    parts[#parts + 1] = string.pack("<i2", scanline)
    parts[#parts + 1] = string.pack("<I2", dot & 0xFFFF)
    parts[#parts + 1] = string.pack("<B", ctrl_byte & 0xFF)
    parts[#parts + 1] = string.pack("<B", mask_byte & 0xFF)
    parts[#parts + 1] = string.pack("<B", status_byte & 0xFF)
    parts[#parts + 1] = string.pack("<B", oam_addr & 0xFF)
    parts[#parts + 1] = string.pack("<I2", v & 0xFFFF)
    parts[#parts + 1] = string.pack("<I2", t & 0xFFFF)
    parts[#parts + 1] = string.pack("<B", fine_x & 0xFF)
    parts[#parts + 1] = string.pack("<B", w_tog)
    parts[#parts + 1] = string.pack("<B", seval_n)
    parts[#parts + 1] = string.pack("<B", seval_m)
    parts[#parts + 1] = string.pack("<B", seval_found)
    parts[#parts + 1] = string.pack("<B", seval_sec)
    parts[#parts + 1] = string.pack("<B", seval_copy)
    parts[#parts + 1] = string.pack("<B", seval_overf)
    parts[#parts + 1] = string.pack("<B", seval_done)
    parts[#parts + 1] = string.pack("<B", seval_latch)
    parts[#parts + 1] = string.pack("<B", spr_count)
    parts[#parts + 1] = string.pack("<B", spr_zero)
    for i = 1, 8 do parts[#parts + 1] = string.pack("<B", spr_shift_lo[i]) end
    for i = 1, 8 do parts[#parts + 1] = string.pack("<B", spr_shift_hi[i]) end
    for i = 1, 8 do parts[#parts + 1] = string.pack("<B", spr_attr[i])     end
    for i = 1, 8 do parts[#parts + 1] = string.pack("<B", spr_x[i])        end
    parts[#parts + 1] = string.pack("<I2", bg_shift_lo & 0xFFFF)
    parts[#parts + 1] = string.pack("<I2", bg_shift_hi & 0xFFFF)
    parts[#parts + 1] = string.pack("<I2", at_shift_lo & 0xFFFF)
    parts[#parts + 1] = string.pack("<I2", at_shift_hi & 0xFFFF)
    parts[#parts + 1] = string.pack("<B", nt_latch    & 0xFF)
    parts[#parts + 1] = string.pack("<B", at_latch    & 0xFF)
    parts[#parts + 1] = string.pack("<B", bg_lo_latch & 0xFF)
    parts[#parts + 1] = string.pack("<B", bg_hi_latch & 0xFF)
    for i = 1, 32 do parts[#parts + 1] = string.pack("<B", secondary_oam_bytes[i] or 0) end
    parts[#parts + 1] = string.pack("<I8", oam_hash & 0xFFFFFFFFFFFFFFFF)
    parts[#parts + 1] = string.pack("<B", nmi)

    return table.concat(parts)
end

-- =================================================================
-- STATE
-- =================================================================

local file = nil
local records_written = 0
local dropped_out_of_window = 0
local end_frame_seen = false

local function open_output()
    local f, err = io.open(CONFIG.OUT_PATH, "wb")
    if not f then
        emu.log("[mesen2_ppu_trace] failed to open " .. CONFIG.OUT_PATH .. ": " .. tostring(err))
        return false
    end
    file = f
    write_header(file)
    emu.log("[mesen2_ppu_trace] writing to " .. CONFIG.OUT_PATH)
    return true
end

local function close_output()
    if file then
        file:close()
        file = nil
        emu.log(string.format(
            "[mesen2_ppu_trace] wrote %d records, dropped %d out-of-window",
            records_written, dropped_out_of_window
        ))
    end
end

-- =================================================================
-- CALLBACKS
-- =================================================================

local function on_end_frame()
    local state = emu.getState()
    -- Mesen2 fires `endFrame` at scanline 240 dot 0 (start of post-
    -- render). At this moment `ppu.frameCount` is the JUST-COMPLETED
    -- frame number (does not increment until the next frame's
    -- pre-render line). Note `state` is a FLAT table (see
    -- `build_record` comment) so we access via string keys.
    local completed_frame = state["ppu.frameCount"] or 0

    if CONFIG.LOG_EVERY > 0 and (completed_frame % CONFIG.LOG_EVERY) == 0 then
        emu.log(string.format(
            "[mesen2_ppu_trace] endFrame completed=%d records=%d",
            completed_frame, records_written
        ))
    end

    if not file then
        return
    end

    if completed_frame < CONFIG.START_FRAME or completed_frame > CONFIG.END_FRAME then
        dropped_out_of_window = dropped_out_of_window + 1
    else
        -- Read OAM (256 bytes) via the debug variant to avoid
        -- side-effects.
        local oam_bytes = {}
        for i = 0, 255 do
            oam_bytes[i + 1] = emu.read(i, emu.memType.nesSpriteRam, false) & 0xFF
        end
        -- Read secondary OAM (32 bytes).
        local secondary_oam = {}
        for i = 0, 31 do
            local ok, b = pcall(emu.read, i, emu.memType.nesSecondarySpriteRam, false)
            secondary_oam[i + 1] = (ok and (b & 0xFF)) or 0
        end

        local oam_hash = fnv1a64(oam_bytes)
        local rec = build_record(state, secondary_oam, oam_hash)
        file:write(rec)
        records_written = records_written + 1
    end

    -- Stop after END_FRAME + TAIL_FRAMES, so the .bin is flushed
    -- and the emulator exits cleanly under --testRunner.
    if completed_frame > CONFIG.END_FRAME + CONFIG.TAIL_FRAMES then
        if not end_frame_seen then
            end_frame_seen = true
            close_output()
            emu.log(string.format(
                "[mesen2_ppu_trace] end reached at completed_frame=%d -> stopping",
                completed_frame
            ))
            emu.stop(0)
        end
    end
end

local function on_script_ended()
    close_output()
end

-- =================================================================
-- MAIN
-- =================================================================

if not open_output() then
    return
end

emu.addEventCallback(on_end_frame,    emu.eventType.endFrame)
emu.addEventCallback(on_script_ended, emu.eventType.scriptEnded)

-- Input overlay: press Start across [START_PRESS_LO, START_PRESS_HI].
-- Matches the RustyNES fixture's start-press window so the per-frame
-- trace lines up at the same frame numbers in both emulators.
if CONFIG.START_PRESS_LO and CONFIG.START_PRESS_HI
    and CONFIG.START_PRESS_HI >= CONFIG.START_PRESS_LO then
    emu.addEventCallback(function()
        local state = emu.getState()
        local frame = state["ppu.frameCount"] or 0
        if frame >= CONFIG.START_PRESS_LO and frame <= CONFIG.START_PRESS_HI then
            -- setInput merges with whatever the (absent) physical
            -- controller would have produced; this is the documented
            -- way to inject programmatic input in --testRunner mode.
            emu.setInput({start = true}, 0)
        end
    end, emu.eventType.inputPolled)
end

emu.log(string.format(
    "[mesen2_ppu_trace] armed: frames %d..=%d (per-frame at endFrame) " ..
    "start-press %d..=%d -> %s",
    CONFIG.START_FRAME, CONFIG.END_FRAME,
    CONFIG.START_PRESS_LO or -1, CONFIG.START_PRESS_HI or -1,
    CONFIG.OUT_PATH
))
