-- mesen2_cpu_boot_trace.lua
--
-- Mesen2 Lua reference-trace script for RustyNES_v2's Session-12
-- per-CPU-instruction boot-trace observability tooling.
--
-- Emits a binary trace file compatible with the
-- `nes_core::cpu_boot_trace::CpuBootTrace` binary format
-- (`crates/nes-core/src/cpu_boot_trace.rs` schema v1, RECORD_SIZE = 32).
-- The companion `cpu_boot_trace_diff` CLI consumes both this script's
-- output and the in-tree fixture's output and reports per-field
-- divergences.
--
-- =================================================================
-- INSTALLING + RUNNING
-- =================================================================
--
-- 1. Install Mesen2 (AppImage or yay -S mesen2-git).
--    Ensure ~/.config/Mesen2/settings.json has "AllowIoOsAccess": true
--    (verified in Session-11; required for io.open).
-- 2. Run in headless mode (no UI) via --testRunner:
--      xvfb-run -a mesen --testRunner <rom_path> <this_script.lua>
--    The script writes its .bin output, then calls emu.stop(0) when
--    END_CYCLE has elapsed.
-- 3. To run inside the GUI instead (e.g. while iterating on the
--    script), load Tools > Script Window > File > Open this .lua file,
--    press F1 to arm, then load the ROM.
--
-- =================================================================
-- CALLBACK STRATEGY
-- =================================================================
--
-- Unlike the per-PPU-dot tracer, Mesen2 DOES expose per-CPU-instruction
-- callbacks via `emu.addMemoryCallback(cb, callbackType.exec, ...)` --
-- the callback fires immediately before each opcode fetch.  We register
-- on the full $0000..=$FFFF range and snapshot CPU + PPU state at the
-- callback boundary, which lines up with RustyNES's
-- `Nes::run_frame` snapshot point (also pre-fetch).
--
-- =================================================================
-- BINARY SCHEMA (must match Rust side)
-- =================================================================
--
-- File header (16 bytes):
--   0..11   "RUSTYNES_CPU"      (12-byte ASCII magic)
--   12..13  schema version      (uint16 LE; currently 1)
--   14..15  reserved flags      (uint16 LE; zero in schema v1)
--
-- Per-record (32 bytes, little-endian throughout):
--   See RECORD_FORMAT below; matches Rust's `CpuBootRecord::to_bytes`
--   packer field-for-field.  Final 5 bytes are zero pad.
--
--   cycle    : u64 (cpu cycle counter at instruction boundary)
--   frame    : u32 (PPU frame counter)
--   scanline : i16 (PPU scanline)
--   dot      : u16 (PPU dot)
--   pc       : u16 (program counter at this instruction)
--   a, x, y, p, s : u8 each (CPU register file)
--   opcode, op1, op2 : u8 each (peeked, side-effect-free)
--   flags    : u8 (bit 0 = PPU NMI line high)
--   pad      : 5 bytes of zero

local CONFIG = {
    OUT_PATH    = os.getenv("MESEN2_CPU_BOOT_TRACE_OUT") or "/tmp/mesen2_cpu_boot_trace.bin",
    START_CYCLE = tonumber(os.getenv("MESEN2_CPU_BOOT_TRACE_START_CYCLE") or "0"),
    END_CYCLE   = tonumber(os.getenv("MESEN2_CPU_BOOT_TRACE_END_CYCLE")   or "200000"),
    LOG_EVERY   = 10000,
}

-- =================================================================
-- HELPERS
-- =================================================================

local function pack_record(rec)
    -- 32-byte layout: cycle(8) + frame(4) + scanline(2) + dot(2) +
    -- pc(2) + a,x,y,p,s,opcode,op1,op2,flags (1 each) + pad(5).
    return string.pack(
        "<I8I4i2I2I2BBBBBBBBB",
        rec.cycle & 0xFFFFFFFFFFFFFFFF,
        rec.frame & 0xFFFFFFFF,
        rec.scanline,
        rec.dot & 0xFFFF,
        rec.pc & 0xFFFF,
        rec.a & 0xFF,
        rec.x & 0xFF,
        rec.y & 0xFF,
        rec.p & 0xFF,
        rec.s & 0xFF,
        rec.opcode & 0xFF,
        rec.op1 & 0xFF,
        rec.op2 & 0xFF,
        rec.flags & 0xFF
    ) .. string.rep("\0", 5)
end

local function write_header(f)
    f:write("RUSTYNES_CPU")            -- magic, 12 bytes
    f:write(string.pack("<I2", 1))     -- schema version
    f:write(string.pack("<I2", 0))     -- reserved flags
end

-- =================================================================
-- STATE
-- =================================================================

local file = nil
local records_written = 0
local stopped = false
-- Cache `emu.read` upvalue to avoid repeated table-lookup in the
-- hot callback path.
local emu_read = nil
local nes_memory = nil

local function open_output()
    local f, err = io.open(CONFIG.OUT_PATH, "wb")
    if not f then
        emu.log("[mesen2_cpu_boot_trace] failed to open " .. CONFIG.OUT_PATH .. ": " .. tostring(err))
        return false
    end
    file = f
    write_header(file)
    emu.log("[mesen2_cpu_boot_trace] writing to " .. CONFIG.OUT_PATH)
    return true
end

local function close_output()
    if file then
        file:close()
        file = nil
        emu.log(string.format(
            "[mesen2_cpu_boot_trace] wrote %d records",
            records_written
        ))
    end
end

-- =================================================================
-- CALLBACKS
-- =================================================================

-- Exec callback: fires immediately before each opcode fetch.  Mesen2's
-- documentation: "exec" callbacks fire ONCE per instruction at the
-- opcode-fetch cycle, which aligns with our pre-step snapshot point.
local function on_exec(addr, value)
    if stopped or not file then
        return
    end
    -- `emu.getState()` returns a FLAT table with dotted-string keys
    -- (Session-11 finding; preserved by this script).
    local state = emu.getState()
    local cycle = state["cpu.cycleCount"] or 0
    if cycle < CONFIG.START_CYCLE then
        return
    end
    if cycle > CONFIG.END_CYCLE then
        if not stopped then
            stopped = true
            close_output()
            emu.log(string.format(
                "[mesen2_cpu_boot_trace] cycle %d > END_CYCLE %d -> stopping",
                cycle, CONFIG.END_CYCLE
            ))
            emu.stop(0)
        end
        return
    end

    local pc = state["cpu.pc"] or addr or 0
    -- Peek opcode + 2 operand bytes side-effect-free using the
    -- non-side-effecting variant of emu.read (4th arg = false).
    local opcode = emu_read(pc, nes_memory, false) & 0xFF
    local op1 = emu_read((pc + 1) & 0xFFFF, nes_memory, false) & 0xFF
    local op2 = emu_read((pc + 2) & 0xFFFF, nes_memory, false) & 0xFF

    -- NMI-line approximation: Mesen2 exposes `cpu.nmiFlag` (a numeric
    -- field) for the CPU's pending NMI latch.  Bit 0 = NMI line high.
    local nmi_val = state["cpu.nmiFlag"]
    local flags = 0
    if nmi_val and nmi_val ~= 0 and nmi_val ~= false then
        flags = flags | 0x01
    end

    local rec = {
        cycle = cycle,
        frame = state["ppu.frameCount"] or 0,
        scanline = state["ppu.scanline"] or 0,
        dot = state["ppu.cycle"] or 0,
        pc = pc & 0xFFFF,
        a = state["cpu.a"] or 0,
        x = state["cpu.x"] or 0,
        y = state["cpu.y"] or 0,
        p = state["cpu.ps"] or 0,
        s = state["cpu.sp"] or 0,
        opcode = opcode,
        op1 = op1,
        op2 = op2,
        flags = flags,
    }
    file:write(pack_record(rec))
    records_written = records_written + 1

    if CONFIG.LOG_EVERY > 0 and (records_written % CONFIG.LOG_EVERY) == 0 then
        emu.log(string.format(
            "[mesen2_cpu_boot_trace] cycle=%d records=%d",
            cycle, records_written
        ))
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

-- Resolve the emu API references once at startup.
emu_read = emu.read
nes_memory = emu.memType.nesMemory

-- Register exec callback on the full address range.  Mesen2 fires
-- this once per opcode fetch (the start of each instruction).
emu.addMemoryCallback(on_exec, emu.callbackType.exec, 0x0000, 0xFFFF)

emu.addEventCallback(on_script_ended, emu.eventType.scriptEnded)

emu.log(string.format(
    "[mesen2_cpu_boot_trace] armed: cycles %d..=%d -> %s",
    CONFIG.START_CYCLE, CONFIG.END_CYCLE, CONFIG.OUT_PATH
))
