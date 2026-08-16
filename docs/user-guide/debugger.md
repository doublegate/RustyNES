# Debugger overlay

Press `` ` `` (Backquote, the `~` key on US keyboards) at any time to
toggle the egui debug overlay. It draws on top of the running game; the
emulator continues to run while the overlay is open. The inspection
panels are read-only — opening, scrolling, or interacting with them never
advances emulator-visible state. (A few opt-in *editing* tools — the hex
editor's write mode, the inline 6502 assembler, and the iNES / NES 2.0
header editor — do write into emulator state when you ask them to; they
are clearly separate from the read-only inspectors.)

The debugger is most useful if you're poking at a homebrew ROM, writing
your own NES game, or chasing a compatibility issue. End-users who only
want to play games can leave it closed.

This page tours the core inspection panels. RustyNES also ships a
Mesen2-class set of advanced debug tools — see
[Advanced debug tools](#advanced-debug-tools) at the end.

## Top toolbar

The toolbar across the top of the overlay shows:

- **Panel toggles** — seven checkboxes (`CPU`, `PPU`, `OAM`, `APU`,
  `Memory`, `Mapper`, `Input`). Each opens or closes one of the panels
  documented below.
- **Frame / cycle counter** — `frame=N cycle=M` reads from the PPU
  frame counter and the cumulative CPU cycle counter.
- **fps readout** — `fps: X.X`, a 60-frame rolling average of the
  emulator's wall-clock production rate. With pacing working correctly
  this reads ~60.1 on NTSC ROMs and ~50.0 on PAL/Dendy. Big deviations
  are diagnostic — see [Troubleshooting](./troubleshooting.md).

## Panels

The default open state is **CPU + PPU**; the others open on first click.
Each panel is an egui window — drag the title bar to move, drag the
edges to resize.

### CPU panel

| Section | What's shown |
|---------|--------------|
| Register row | `A`, `X`, `Y`, `S`, `PC` in hex |
| Flag row | Status register bits `N V _ B D I Z C` color-coded green when set |
| Cycle counter | Cumulative CPU cycles since power-on |
| `JAMMED` indicator | Lights up if the CPU executed an undocumented JAM opcode |
| Goto field | Type a hex address (`$C000` or `C000`) and press Enter to anchor the disassembly there |
| Follow PC checkbox | When ticked, the disassembly retracks PC every frame |
| Disassembly | Scrollable list of 32 instructions starting at the anchor; the current PC line is highlighted yellow |

The disassembler covers all 151 documented 6502 opcodes. Undocumented
opcodes render as `.byte $XX` rather than guessing at the mnemonic.

### PPU panel

Four sub-tabs:

- **Registers** — current scanline / dot / frame; the four memory-mapped
  registers (`PPUCTRL`, `PPUMASK`, `PPUSTATUS`, `OAMADDR`); internal
  scroll state (loopy `v`, `t`, `fine_x`, `w_toggle`); current BG and
  sprite pattern table bases; 8x8 vs 8x16 sprite size; NMI line state.
- **Patterns** — both pattern tables ($0000 and $1000) rendered as 128x128
  egui textures. Background palette 0 is used for the colors.
- **Nametables** — pick one of NT0..NT3 with the radio buttons; the
  selected nametable is drawn at 384x360 with per-tile attribute palette.
  When the current loopy-v points into the same nametable, yellow
  crosshairs mark the scroll cursor.
- **Palette** — the 32 palette RAM bytes shown as 4x4 grids of color
  swatches with the NES color index in hex on each cell.

### OAM panel

Sprite inspection:

- **Header line** — sprite count (always 64) and current sprite size
  (8x8 or 8x16) read from `PPUCTRL`.
- **Scrollable sprite list** — one row per sprite: `#i  x=X y=Y
  tile=$TT  pal=P  pri=fg|bg  flip=-|h|v|hv`.
- **Sprite grid** — all 64 sprites rendered as a 128x128 image (an 8x8
  grid of 2x-upscaled 8x8 tiles). Uses the current sprite pattern base
  and sprite palettes 4..=7. Bear in mind this preview always shows the
  primary 8x8 tile — 8x16 sprites' second halves aren't drawn here.

### APU panel

Audio inspection:

- **Status line** — current output value of each of the five channels
  (`P1`, `P2`, `TRI`, `NSE`, `DMC` — pulses and triangle 0..15, DMC
  0..127) plus the `FRAME-IRQ` and `DMC-IRQ` flags.
- **Per-channel scopes** — 256-sample rolling oscilloscope traces for
  each of the five channels. The sample is taken once per overlay redraw,
  not at the audio output rate, so the scope is a visualization, not a
  bit-exact waveform capture.

The scope buffer only updates while the APU panel is visible — closing
the panel halts the per-frame tap.

### Memory panel

Hex viewer over both buses:

- **CPU bus / PPU bus tabs** — pick which 16-bit address space to view.
- **Goto field** — type a hex address (`$1234` or `1234`) and press
  Enter to jump to a 16-byte-aligned origin near it.
- **-256 / +256 buttons** — page backwards or forwards a row at a time.
- **16x16 byte grid** — 256 bytes per redraw; each row shows the row
  address, 16 bytes in hex, then an ASCII column on the right.

Memory reads through the panel use side-effect-free peek paths — they
don't clear the VBL flag, advance the PPUDATA buffer, or shift
controller bits. The one documented exception is MMC2's CHR-fetch
latch, which can update on a peek and is accepted as such.

### Mapper panel

Mapper-specific bank registers and IRQ state:

- **Header** — `Mapper #N - <name>` and current mirroring mode.
- **PRG banks** — current value of each PRG bank slot the mapper exposes.
- **CHR banks** — same for CHR.
- **IRQ counter** — the mapper's IRQ-related state (counter, reload
  value, enabled flag, pending flag), if it has one.
- **Extra** — anything else worth surfacing (PRG-RAM enable / protect,
  mode bits, ExRAM mode for MMC5, etc.).

The exact set of fields depends on the mapper. Empty sections are
suppressed, so an NROM cartridge shows almost nothing while an MMC5
cartridge fills the window.

### Input panel

The in-app rebind modal:

- **Status line** — last action ("Rebound Player1 A -> KeyZ.", "Saved.",
  "(cancelled)", etc.).
- **Save to disk / Reset to defaults** buttons — persist or revert.
- **15-row grid** — Player 1 D-pad + A/B/Select/Start + every system
  action. Each row shows the action name, the current key, and a
  **rebind** button.

Click `rebind`, then press any key (or Esc to cancel). The captured key
writes back into the in-memory config; "Save to disk" persists it. See
[Controls](./controls.md) for the full flow.

## Notes on overlay behavior

- The inspection panels are **read-only**: opening, closing, or
  interacting with them never advances emulator-visible state. Snapshot
  APIs are designed so the inspection methods only peek; the determinism
  contract (same seed + ROM + input ⇒ bit-identical framebuffer) holds
  whether the overlay is open or closed. The separate editing tools (hex
  write mode, the assembler, the header editor) do mutate state, but only
  on an explicit write.
- The overlay redraws once per emulated frame. Heavy panels (Patterns,
  Nametables, OAM grid, hex viewers) are bounded — pattern textures are
  cached and refreshed in place; the hex viewer reads exactly 256 bytes
  per redraw.
- Keyboard input is gated: while the rebind modal is waiting for a key
  capture, emulator input is suspended so the captured key isn't sent
  to the running game. Once you release the capture (or it cancels via
  Esc), normal input resumes.

## Advanced debug tools

Beyond the core inspection panels above, RustyNES carries a Mesen2-class
set of debugging and authoring tools, reachable from the **Tools** and
**Debug** menus and from the debugger overlay:

- **Breakpoints and watchpoints** — execute / read / write breakpoints,
  with full expression and conditional support.
- **Hex editor** — the Memory panel's write mode, for editing RAM live.
- **RAM search** — Cheat-Engine-style value search to find variables.
- **Trace logger** — log executed instructions to a file.
- **Event viewer** — a per-dot PPU event heatmap and per-scanline trace.
- **Callstack and step modes** — a reconstructed call stack with
  step-into / over / out, plus a memory-access counter and uninit-read
  detection.
- **Symbol-file loading** — `.sym` / `.mlb` / `.nl` symbol files and
  ca65 / cc65 `.dbg` source maps.
- **Inline 6502 assembler** — assemble and patch instructions in place.
- **iNES / NES 2.0 header editor** — edit the cartridge header (mapper,
  submapper, region, mirroring) live.
- **Palette / nametable / CHR / OAM editors** — graphical editors that can
  write back into PPU memory.
- **Memory compare** — diff two snapshots to track what changed.
- **TAStudio** — the piano-roll TAS editor (see [Controls → TAS
  movies](./controls.md#tas-movies-record--playback)).

These are aimed at homebrew developers and TAS authors; you never need
them to play a game.

## Pixel Provenance

**Tools → Pixel Provenance** answers "why is this pixel this colour?" for any
pixel on screen, all the way back to the code that caused it.

How to use it:

1. Open **Tools → Pixel Provenance**.
2. Tick **both** checkboxes — *Per-pixel provenance* and *Write attribution*.
   They are independent and both default off: the first records which bytes
   produced each pixel, the second records which instruction wrote those bytes.
   You want both for the full chain.
3. Let the game run at least one frame.
4. **Click any pixel on the game view**, or type coordinates into the X/Y boxes.

You then get, for that pixel: the PPU dot and scanline that emitted it; whether
the background, a sprite, or the backdrop won; the palette address and entry; the
nametable, attribute and pattern addresses of the tile actually on screen; and —
for each of those bytes — the program counter and CPU cycle of the instruction
that last wrote it. If you have loaded a `.dbg` source map, you also get the
source file and line.

If the panel has nothing to show it tells you which kind of nothing: not armed,
nothing recorded for that pixel yet, or off-screen. Clicking a black letterbox
bar pins nothing, by design.

Things worth knowing:

- **Arming costs memory, not accuracy.** Both stores are output-only. The
  framebuffer, the audio and every cycle count are bit-identical whether they are
  armed or not.
- **A power-cycle or loading a save state clears the records**, because the
  restored bytes were not written by anything the current session ran. Run-ahead
  does *not* clear them — you get the frame you are looking at.
- **CHR (pattern) bytes have no attribution row.** They are mapper-owned, so a
  byte offset is not a stable identity across a bank switch. The panel reports
  which bank supplied them instead.
- **Sprites have no OAM attribution row.** The PPU does not keep the primary OAM
  index at emit time, so naming a writer would risk naming a different sprite's.

> **Fixed in v2.3.6.** In v2.3.2 through v2.3.5 this panel returned an empty
> report for almost everyone: run-ahead (on by default) erased the record before
> the panel could read it, and clicking a pixel was never wired up. If you tried
> it on an older build and nothing happened, that was the bug, not your setup.

## See also

- [Controls](./controls.md) — full rebind flow
- [Display and audio](./display-and-audio.md) — NTSC filter, fps targets
- [Troubleshooting](./troubleshooting.md) — what to look at when fps reads wrong
