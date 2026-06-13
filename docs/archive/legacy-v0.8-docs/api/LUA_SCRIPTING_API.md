# RustyNES Lua Scripting API Reference

Complete API reference for the `rustynes-scripting` crate, providing Lua 5.4 scripting capabilities for automation, tool-assisted speedruns (TAS), debugging, and custom game modifications.

## Table of Contents

1. [Overview](#overview)
2. [Quick Start](#quick-start)
3. [Script Lifecycle](#script-lifecycle)
4. [Core API](#core-api)
5. [Memory API](#memory-api)
6. [Input API](#input-api)
7. [Graphics API](#graphics-api)
8. [Sound API](#sound-api)
9. [State API](#state-api)
10. [Event Callbacks](#event-callbacks)
11. [TAS Functions](#tas-functions)
12. [Debugging API](#debugging-api)
13. [Utility Functions](#utility-functions)
14. [Security Model](#security-model)
15. [Examples](#examples)
16. [Error Handling](#error-handling)
17. [Performance Considerations](#performance-considerations)
18. [References](#references)

---

## Overview

The RustyNES Lua scripting system enables runtime automation and modification of emulator behavior. Scripts can:

- Read and write emulator memory
- Control input programmatically
- Draw overlays on the game display
- Hook into emulator events (frame start, scanline, etc.)
- Create TAS recordings and playback
- Implement cheat systems and game modifications
- Build custom debugging tools

### Design Goals

1. **FCEUX Compatibility**: API designed for maximum compatibility with existing FCEUX Lua scripts
2. **Safety**: Sandboxed running prevents filesystem/network access by default
3. **Performance**: Minimal overhead with lazy evaluation and caching
4. **Extensibility**: Plugin architecture for custom API extensions

### Dependencies

```toml
[dependencies]
mlua = { version = "0.9", features = ["lua54", "vendored", "send"] }
```

---

## Quick Start

```lua
-- hello_world.lua
-- Display a message and show player health

function on_frame()
    -- Read player health from RAM
    local health = memory.read_u8(0x0076)

    -- Draw health bar overlay
    gui.text(10, 10, string.format("Health: %d", health))
    gui.box(10, 20, 10 + health, 28, "green", "darkgreen")
end

-- Register the frame callback
emu.registerafter(on_frame)
```

### Loading Scripts

```rust
use rustynes_scripting::{ScriptEngine, ScriptContext};

// Create script engine with emulator context
let mut engine = ScriptEngine::new()?;

// Load and run a script file
engine.load_file("scripts/display_stats.lua")?;

// Or load from string
engine.load_string(r#"
    function on_frame()
        gui.text(10, 10, "Hello from Lua!")
    end
    emu.registerafter(on_frame)
"#)?;

// Run script callbacks each frame
engine.on_frame_end(&mut emulator)?;
```

---

## Script Lifecycle

### Initialization

Scripts run their top-level code immediately upon loading. Use this for:

- Defining global variables and functions
- Registering event callbacks
- Initial setup and configuration

```lua
-- Top-level code runs once at load time
print("Script loaded!")

-- Define state
local frame_count = 0
local max_health = 0

-- Define callbacks
function on_frame()
    frame_count = frame_count + 1
end

-- Register callbacks
emu.registerafter(on_frame)
```

### Run Order

Each frame, callbacks run in this order:

1. `emu.registerbefore()` - Before frame emulation
2. Frame emulation occurs
3. `emu.registerafter()` - After frame emulation (most common)
4. `gui.*` drawing commands rendered to overlay

### Script Termination

Scripts can be unloaded or replaced at any time. Clean up resources in `emu.registerexit()`:

```lua
local log_file = io.open("game_log.txt", "w")

function on_exit()
    if log_file then
        log_file:close()
    end
end

emu.registerexit(on_exit)
```

---

## Core API

### emu Module

Core emulator control functions.

#### emu.framecount()

Returns the current frame number since emulation started.

```lua
local frame = emu.framecount()
print(string.format("Frame: %d", frame))
```

#### emu.lagcount()

Returns the number of lag frames (frames where no input was read).

```lua
local lags = emu.lagcount()
gui.text(10, 10, string.format("Lag: %d", lags))
```

#### emu.lagged()

Returns `true` if the current frame is a lag frame.

```lua
if emu.lagged() then
    gui.text(10, 10, "LAG!", "red")
end
```

#### emu.emulating()

Returns `true` if emulation is active (not paused).

```lua
if emu.emulating() then
    -- Perform actions only when running
end
```

#### emu.paused()

Returns `true` if emulation is paused.

```lua
if emu.paused() then
    gui.text(100, 100, "PAUSED", "yellow")
end
```

#### emu.pause()

Pauses emulation.

```lua
if health <= 0 then
    emu.pause()
    print("Game Over!")
end
```

#### emu.unpause()

Resumes emulation.

```lua
emu.unpause()
```

#### emu.speedmode(mode)

Sets the emulation speed mode.

| Mode | Description |
|------|-------------|
| `"normal"` | Normal speed (60 fps NTSC) |
| `"nothrottle"` | Maximum speed, no frame limiting |
| `"turbo"` | Fast-forward (configurable multiplier) |
| `"maximum"` | Absolute maximum, skip rendering |

```lua
-- Speed up during automated sections
emu.speedmode("turbo")
do_automated_task()
emu.speedmode("normal")
```

#### emu.frameadvance()

Advances emulation by one frame. Useful for frame-by-frame TAS work.

```lua
-- Advance 60 frames
for i = 1, 60 do
    emu.frameadvance()
end
```

#### emu.softreset()

Triggers a soft reset (like pressing the Reset button).

```lua
emu.softreset()
```

#### emu.hardreset()

Triggers a hard reset (power cycle).

```lua
emu.hardreset()
```

#### emu.message(text)

Displays a message in the emulator's on-screen display.

```lua
emu.message("Checkpoint reached!")
```

### Callback Registration

#### emu.registerbefore(func)

Registers a callback that runs before each frame is emulated.

```lua
function before_frame()
    -- Set up input for this frame
    joypad.set(1, {A = true})
end
emu.registerbefore(before_frame)
```

#### emu.registerafter(func)

Registers a callback that runs after each frame is emulated. Most common callback type.

```lua
function after_frame()
    -- Read and display game state
    local score = memory.read_u16_le(0x0030)
    gui.text(10, 10, string.format("Score: %d", score))
end
emu.registerafter(after_frame)
```

#### emu.registerexit(func)

Registers a callback that runs when the script is unloaded.

```lua
function on_exit()
    print("Script unloaded, goodbye!")
end
emu.registerexit(on_exit)
```

---

## Memory API

### memory Module

Functions for reading and writing NES memory.

#### Address Spaces

| Range | Description |
|-------|-------------|
| `0x0000-0x07FF` | Internal RAM (2KB, mirrored to 0x1FFF) |
| `0x2000-0x2007` | PPU Registers (mirrored to 0x3FFF) |
| `0x4000-0x4017` | APU and I/O Registers |
| `0x4020-0xFFFF` | Cartridge space (PRG-ROM, PRG-RAM) |

### Read Functions

#### memory.read_u8(addr)

Reads an unsigned byte from the specified address.

```lua
local player_x = memory.read_u8(0x0086)
local player_y = memory.read_u8(0x00CE)
```

#### memory.read_s8(addr)

Reads a signed byte (-128 to 127).

```lua
local velocity = memory.read_s8(0x0057)
```

#### memory.read_u16_le(addr)

Reads a 16-bit unsigned value in little-endian format.

```lua
local score = memory.read_u16_le(0x07D7)
```

#### memory.read_u16_be(addr)

Reads a 16-bit unsigned value in big-endian format.

```lua
local timer = memory.read_u16_be(0x0400)
```

#### memory.read_range(addr, length)

Reads a range of bytes, returning a table.

```lua
local sprite_data = memory.read_range(0x0200, 256)
for i, byte in ipairs(sprite_data) do
    print(string.format("Sprite %d: Y=%d", i-1, byte))
end
```

### Write Functions

#### memory.write_u8(addr, value)

Writes an unsigned byte to the specified address.

```lua
-- Set player health to maximum
memory.write_u8(0x0076, 255)
```

#### memory.write_u16_le(addr, value)

Writes a 16-bit value in little-endian format.

```lua
-- Set score
memory.write_u16_le(0x07D7, 999999)
```

#### memory.write_range(addr, data)

Writes a table of bytes to memory.

```lua
-- Write custom sprite data
memory.write_range(0x0200, {0x80, 0x01, 0x00, 0x80})
```

### Register Access

#### memory.getregister(name)

Reads a CPU register value.

| Register | Description |
|----------|-------------|
| `"a"` | Accumulator |
| `"x"` | X Index Register |
| `"y"` | Y Index Register |
| `"s"` or `"sp"` | Stack Pointer |
| `"p"` | Processor Status |
| `"pc"` | Program Counter |

```lua
local pc = memory.getregister("pc")
local a = memory.getregister("a")
print(string.format("PC: $%04X, A: $%02X", pc, a))
```

#### memory.setregister(name, value)

Sets a CPU register value.

```lua
-- Force accumulator value
memory.setregister("a", 0xFF)
```

### Memory Callbacks

#### memory.registerread(addr, func)

Registers a callback that triggers when an address is read.

```lua
memory.registerread(0x0076, function(addr, value)
    print(string.format("Health read: %d", value))
end)
```

#### memory.registerwrite(addr, func)

Registers a callback that triggers when an address is written.

```lua
memory.registerwrite(0x0076, function(addr, value)
    print(string.format("Health changed to: %d", value))
    if value <= 0 then
        emu.pause()
        print("Player died!")
    end
end)
```

#### memory.registerrun(addr, func)

Registers a callback that triggers when code at an address is run (CPU fetches instruction).

```lua
-- Hook the game's main loop
memory.registerrun(0xC000, function(addr)
    print(string.format("Main loop at frame %d", emu.framecount()))
end)
```

---

## Input API

### joypad Module

Controller input reading and manipulation.

#### joypad.get(player)

Returns the current state of a player's controller as a table.

```lua
local p1 = joypad.get(1)
if p1.A then
    print("Player 1 pressing A")
end

-- Button names: A, B, Select, Start, Up, Down, Left, Right
```

#### joypad.set(player, buttons)

Sets controller input for the current frame. Takes effect on the next input poll.

```lua
-- Hold Right and A
joypad.set(1, {Right = true, A = true})

-- Clear all input
joypad.set(1, {})
```

#### joypad.getdown(player)

Returns buttons that were just pressed this frame (not held from previous frame).

```lua
local pressed = joypad.getdown(1)
if pressed.Start then
    print("Start just pressed!")
end
```

#### joypad.getup(player)

Returns buttons that were just released this frame.

```lua
local released = joypad.getup(1)
if released.A then
    print("A button released!")
end
```

### zapper Module (Light Gun)

#### zapper.get()

Returns the current Zapper state.

```lua
local z = zapper.get()
print(string.format("Zapper: X=%d, Y=%d, Fire=%s",
    z.x, z.y, tostring(z.fire)))
```

#### zapper.set(x, y, fire)

Sets the Zapper position and trigger state.

```lua
-- Aim at screen center and fire
zapper.set(128, 120, true)
```

---

## Graphics API

### gui Module

Overlay drawing functions. All drawing is rendered after the game frame.

#### Coordinate System

- Origin (0, 0) is top-left
- X increases rightward (0-255)
- Y increases downward (0-239)

#### Color Specification

Colors can be specified as:

- Color name: `"red"`, `"green"`, `"blue"`, `"white"`, `"black"`, `"yellow"`, `"cyan"`, `"magenta"`, `"gray"`, `"orange"`, `"purple"`, `"pink"`
- Hex string: `"#FF0000"`, `"#00FF00FF"` (with alpha)
- RGB table: `{r=255, g=0, b=0}` or `{255, 0, 0}`
- RGBA table: `{r=255, g=0, b=0, a=128}` or `{255, 0, 0, 128}`

#### gui.text(x, y, text, [fg], [bg])

Draws text at the specified position.

```lua
gui.text(10, 10, "Hello World!")
gui.text(10, 20, "Red text", "red")
gui.text(10, 30, "White on blue", "white", "blue")
gui.text(10, 40, string.format("Frame: %d", emu.framecount()))
```

#### gui.pixel(x, y, color)

Draws a single pixel.

```lua
gui.pixel(128, 120, "red")
```

#### gui.line(x1, y1, x2, y2, color)

Draws a line between two points.

```lua
gui.line(0, 120, 255, 120, "white")
```

#### gui.box(x1, y1, x2, y2, [fill], [outline])

Draws a rectangle (filled or outlined).

```lua
-- Filled box
gui.box(10, 10, 50, 30, "blue")

-- Outlined box
gui.box(60, 10, 100, 30, nil, "red")

-- Filled with outline
gui.box(110, 10, 150, 30, "green", "darkgreen")
```

#### gui.rect(x, y, width, height, [fill], [outline])

Draws a rectangle using width and height instead of corner coordinates.

```lua
gui.rect(10, 50, 40, 20, "yellow")
```

#### gui.circle(x, y, radius, [fill], [outline])

Draws a circle.

```lua
gui.circle(128, 120, 20, "cyan", "white")
```

#### gui.drawimage(x, y, image_path)

Draws an image from file (PNG, BMP supported).

```lua
gui.drawimage(0, 0, "overlay.png")
```

#### gui.transparency(alpha)

Sets the global transparency for subsequent draw calls (0-255, where 0 is fully transparent).

```lua
gui.transparency(128)  -- 50% transparent
gui.box(10, 10, 100, 100, "red")
gui.transparency(255)  -- Back to opaque
```

#### gui.clearuncommitted()

Clears all drawing that hasn't been committed to the screen yet.

```lua
gui.clearuncommitted()
```

#### gui.savescreenshot(filename)

Saves the current frame to a PNG file.

```lua
gui.savescreenshot("screenshot_" .. emu.framecount() .. ".png")
```

---

## Sound API

### sound Module

Audio manipulation and generation.

#### sound.get()

Returns the current state of all APU channels.

```lua
local s = sound.get()
print(string.format("Square1 freq: %d", s.square1.frequency))
```

Returns a table with:

- `square1`, `square2`: `{frequency, volume, duty, enabled}`
- `triangle`: `{frequency, enabled}`
- `noise`: `{frequency, volume, mode, enabled}`
- `dmc`: `{frequency, sample_address, sample_length, enabled}`

#### sound.mute()

Mutes all audio output.

```lua
sound.mute()
```

#### sound.unmute()

Restores audio output.

```lua
sound.unmute()
```

---

## State API

### savestate Module

Save state creation and loading.

#### savestate.create([slot])

Creates a save state object. If slot is provided (1-10), uses that slot.

```lua
local state = savestate.create()
local slot_state = savestate.create(1)
```

#### savestate.save(state)

Saves the current emulator state to the state object.

```lua
local state = savestate.create()
savestate.save(state)
```

#### savestate.load(state)

Loads a previously saved state.

```lua
savestate.load(state)
```

#### savestate.saveslot(slot)

Saves to a numbered slot (1-10).

```lua
savestate.saveslot(1)
```

#### savestate.loadslot(slot)

Loads from a numbered slot.

```lua
savestate.loadslot(1)
```

### Practical Example: Rewind

```lua
local history = {}
local max_history = 600  -- 10 seconds at 60fps

function on_frame()
    -- Save state every frame
    local state = savestate.create()
    savestate.save(state)
    table.insert(history, state)

    -- Trim history
    while #history > max_history do
        table.remove(history, 1)
    end
end

function rewind(frames)
    local target = #history - frames
    if target > 0 then
        savestate.load(history[target])
        -- Trim future
        while #history > target do
            table.remove(history)
        end
    end
end

emu.registerafter(on_frame)
```

---

## Event Callbacks

### Advanced Event Hooks

#### emu.registerscanline(scanline, func)

Registers a callback for a specific scanline (0-261).

```lua
-- Hook at scanline 100
emu.registerscanline(100, function()
    -- Mid-screen effects
end)
```

#### emu.registerwindow(func)

Registers a callback for GUI events (window resize, focus, etc.).

```lua
emu.registerwindow(function(event)
    if event.type == "resize" then
        print(string.format("Window resized to %dx%d", event.width, event.height))
    end
end)
```

---

## TAS Functions

### movie Module

Movie (TAS) recording and playback.

#### movie.active()

Returns `true` if a movie is currently loaded.

```lua
if movie.active() then
    gui.text(10, 10, "Movie playing")
end
```

#### movie.mode()

Returns the current movie mode: `"playback"`, `"record"`, or `nil`.

```lua
local mode = movie.mode()
if mode == "record" then
    gui.text(10, 10, "Recording...", "red")
end
```

#### movie.length()

Returns the total number of frames in the loaded movie.

```lua
local total = movie.length()
local current = emu.framecount()
gui.text(10, 10, string.format("%d / %d", current, total))
```

#### movie.rerecordcount()

Returns the number of re-records for the current movie.

```lua
gui.text(10, 20, string.format("Rerecords: %d", movie.rerecordcount()))
```

#### movie.readonly()

Returns `true` if movie is in read-only playback mode.

```lua
if movie.readonly() then
    gui.text(10, 30, "Read-only")
end
```

#### movie.setreadonly(readonly)

Sets the movie to read-only or read-write mode.

```lua
movie.setreadonly(true)
```

#### movie.play(filename)

Loads and starts playback of a movie file.

```lua
movie.play("tas_run.fm2")
```

#### movie.record(filename)

Starts recording a new movie.

```lua
movie.record("new_run.fm2")
```

#### movie.stop()

Stops playback or recording.

```lua
movie.stop()
```

---

## Debugging API

### debugger Module

Advanced debugging functions.

#### debugger.hitcount(addr)

Returns how many times an address has been run.

```lua
local hits = debugger.hitcount(0xC000)
print(string.format("Main loop ran %d times", hits))
```

#### debugger.getcyclecount()

Returns the total CPU cycle count.

```lua
local cycles = debugger.getcyclecount()
```

#### debugger.getinstruction(addr)

Disassembles the instruction at the specified address.

```lua
local inst = debugger.getinstruction(0xC000)
print(string.format("%04X: %s %s", inst.addr, inst.mnemonic, inst.operand))
-- Output: C000: JMP $C050
```

#### debugger.setbreakpoint(addr, [callback])

Sets a breakpoint at the specified address.

```lua
debugger.setbreakpoint(0xC000, function()
    print("Breakpoint hit!")
    local a = memory.getregister("a")
    print(string.format("A = $%02X", a))
end)
```

#### debugger.removebreakpoint(addr)

Removes a breakpoint.

```lua
debugger.removebreakpoint(0xC000)
```

---

## Utility Functions

### bit Module

Bitwise operations (Lua 5.4 native, also available as compatibility layer).

```lua
local result = bit.band(0xFF, 0x0F)   -- AND
local result = bit.bor(0xF0, 0x0F)    -- OR
local result = bit.bxor(0xFF, 0x0F)   -- XOR
local result = bit.bnot(0xFF)         -- NOT
local result = bit.lshift(0x01, 4)    -- Left shift
local result = bit.rshift(0x80, 4)    -- Right shift (logical)
local result = bit.arshift(0x80, 4)   -- Right shift (arithmetic)
```

### rom Module

ROM information access.

#### rom.getname()

Returns the loaded ROM filename.

```lua
local name = rom.getname()
print("Playing: " .. name)
```

#### rom.gethash()

Returns the ROM's hash (SHA-1 or CRC32).

```lua
local hash = rom.gethash("sha1")
print("SHA1: " .. hash)
```

#### rom.getmapper()

Returns the mapper number.

```lua
local mapper = rom.getmapper()
print(string.format("Mapper: %d", mapper))
```

### print(...)

Outputs text to the script console.

```lua
print("Debug message")
print("Value:", 42, "Text:", "hello")
```

---

## Security Model

### Sandboxed Environment

By default, scripts run in a sandboxed environment with restricted access:

| Capability | Default | Can Enable |
|------------|---------|------------|
| Memory access | Yes | N/A |
| Input control | Yes | N/A |
| GUI drawing | Yes | N/A |
| File read | No | `--allow-file-read` |
| File write | No | `--allow-file-write` |
| Network | No | `--allow-network` |
| OS commands | No | Never |

### Enabling Capabilities

```rust
let mut engine = ScriptEngine::new()?;
engine.set_capability(Capability::FileRead, true);
engine.set_capability(Capability::FileWrite, true);
engine.load_file("script_with_io.lua")?;
```

### Safe Globals

These standard Lua globals are available:

- `string`, `table`, `math` - Standard libraries
- `pairs`, `ipairs`, `next` - Iteration
- `type`, `tostring`, `tonumber` - Type functions
- `print` - Console output
- `error`, `assert`, `pcall`, `xpcall` - Error handling

### Removed Globals

These are removed for security:

- `os` - Operating system access
- `io` - File I/O (unless enabled)
- `dofile`, `loadfile` - File running
- `debug` - Debug library (use `debugger` module instead)
- `require` - Module loading (use explicit API)

---

## Examples

### Health Display with Bar

```lua
-- health_bar.lua
local health_addr = 0x0076
local max_health = 100

function on_frame()
    local health = memory.read_u8(health_addr)
    local percent = health / max_health
    local bar_width = math.floor(50 * percent)

    -- Background
    gui.box(10, 10, 60, 18, "gray", "white")

    -- Health bar (color based on health)
    local color = "green"
    if percent < 0.25 then
        color = "red"
    elseif percent < 0.5 then
        color = "yellow"
    end

    if bar_width > 0 then
        gui.box(10, 10, 10 + bar_width, 18, color)
    end

    -- Text
    gui.text(12, 11, string.format("%d/%d", health, max_health), "white")
end

emu.registerafter(on_frame)
```

### Auto-Pilot Bot

```lua
-- simple_bot.lua
-- Automated player for simple platformer

function on_frame()
    local player_x = memory.read_u8(0x0086)
    local player_y = memory.read_u8(0x00CE)
    local on_ground = memory.read_u8(0x001D) == 0

    local input = {}

    -- Always move right
    input.Right = true

    -- Jump when on ground
    if on_ground then
        input.A = true
    end

    -- Display status
    gui.text(10, 10, string.format("X: %d, Y: %d", player_x, player_y))
    gui.text(10, 20, on_ground and "Grounded" or "Airborne")

    joypad.set(1, input)
end

emu.registerbefore(on_frame)
```

### Memory Watch Table

```lua
-- memory_watch.lua
local watches = {
    {addr = 0x0076, name = "Health", format = "dec"},
    {addr = 0x0086, name = "Player X", format = "hex"},
    {addr = 0x00CE, name = "Player Y", format = "hex"},
    {addr = 0x001D, name = "Ground", format = "bool"},
}

function on_frame()
    local y = 10
    for _, watch in ipairs(watches) do
        local value = memory.read_u8(watch.addr)
        local display

        if watch.format == "hex" then
            display = string.format("$%02X", value)
        elseif watch.format == "bool" then
            display = value == 0 and "false" or "true"
        else
            display = tostring(value)
        end

        gui.text(10, y, string.format("%s: %s", watch.name, display))
        y = y + 10
    end
end

emu.registerafter(on_frame)
```

### RNG Prediction

```lua
-- rng_display.lua
-- Display and predict random number generator state

local rng_addr = 0x0018  -- Common RNG location

function predict_next(rng)
    -- Common NES RNG algorithm (linear congruential)
    return bit.band((rng * 5 + 1), 0xFF)
end

function on_frame()
    local rng = memory.read_u8(rng_addr)
    local next_rng = predict_next(rng)

    gui.text(200, 10, string.format("RNG: $%02X", rng))
    gui.text(200, 20, string.format("Next: $%02X", next_rng))

    -- Show RNG manipulation opportunities
    if rng % 16 == 0 then
        gui.text(200, 30, "GOOD RNG!", "green")
    end
end

emu.registerafter(on_frame)
```

---

## Error Handling

### Lua Errors

Lua errors are caught and reported to the console:

```lua
function on_frame()
    -- This will cause an error
    local value = nil
    print(value.foo)  -- Attempt to index nil
end
```

Error output:

```
Script error at line 4: attempt to index a nil value (local 'value')
Stack trace:
    on_frame (script.lua:4)
    [callback] (internal)
```

### Protected Calls

Use `pcall` for error handling within scripts:

```lua
function safe_read(addr)
    local success, result = pcall(function()
        return memory.read_u8(addr)
    end)

    if success then
        return result
    else
        print("Read failed: " .. result)
        return 0
    end
end
```

---

## Performance Considerations

### Optimization Tips

1. **Cache repeated reads**:

```lua
-- Bad: Reads address every call
function on_frame()
    if memory.read_u8(0x0076) > 50 then
        gui.text(10, 10, string.format("Health: %d", memory.read_u8(0x0076)))
    end
end

-- Good: Read once
function on_frame()
    local health = memory.read_u8(0x0076)
    if health > 50 then
        gui.text(10, 10, string.format("Health: %d", health))
    end
end
```

1. **Minimize string formatting**:

```lua
-- Pre-compute static strings
local health_label = "Health: "

function on_frame()
    gui.text(10, 10, health_label .. memory.read_u8(0x0076))
end
```

1. **Use conditional drawing**:

```lua
-- Only update display when value changes
local last_health = 0

function on_frame()
    local health = memory.read_u8(0x0076)
    if health ~= last_health then
        -- Value changed, update display
        last_health = health
    end
    gui.text(10, 10, "Health: " .. last_health)
end
```

### Performance Limits

- Maximum script run time per frame: 16ms
- Maximum memory callbacks: 256
- Maximum GUI draw calls per frame: 1000

Scripts exceeding limits are automatically throttled or terminated.

---

## References

### Related Documentation

- [FCEUX Lua Reference](http://www.fceux.com/web/help/taseditor/LuaFunctionsList.html)
- [Core API Reference](CORE_API.md)
- [TAS Movie Format](../formats/FM2_FORMAT.md)
- [Save State Format](SAVESTATE_FORMAT.md)

### Source Files

```
crates/rustynes-scripting/
├── src/
│   ├── lib.rs           # Module exports
│   ├── engine.rs        # Script running engine
│   ├── sandbox.rs       # Security sandbox
│   ├── api/
│   │   ├── mod.rs       # API module organization
│   │   ├── emu.rs       # emu.* functions
│   │   ├── memory.rs    # memory.* functions
│   │   ├── joypad.rs    # joypad.* functions
│   │   ├── gui.rs       # gui.* functions
│   │   ├── savestate.rs # savestate.* functions
│   │   ├── movie.rs     # movie.* functions
│   │   └── debugger.rs  # debugger.* functions
│   └── bindings.rs      # Rust-Lua bindings
└── tests/
    └── integration.rs   # API integration tests
```
