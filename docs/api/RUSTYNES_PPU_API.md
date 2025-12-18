# RustyNES PPU Crate API Reference

**Crate:** `rustynes-ppu`
**Version:** 0.1.0
**License:** MIT/Apache-2.0

The `rustynes-ppu` crate provides a cycle-accurate implementation of the NES 2C02 Picture Processing Unit, responsible for rendering backgrounds, sprites, and generating video output.

---

## Table of Contents

- [Quick Start](#quick-start)
- [Core Types](#core-types)
- [PPU Struct](#ppu-struct)
- [Register Interface](#register-interface)
- [VRAM Access](#vram-access)
- [Rendering](#rendering)
- [Sprite Handling](#sprite-handling)
- [Debug Interface](#debug-interface)
- [Integration](#integration)
- [Examples](#examples)

---

## Quick Start

```rust
use rustynes_ppu::{Ppu, PpuBus, Mirroring};

// Implement PPU bus for cartridge CHR access
struct CartridgeChrBus {
    chr_rom: Vec<u8>,
    chr_ram: [u8; 8192],
    mirroring: Mirroring,
}

impl PpuBus for CartridgeChrBus {
    fn read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_rom.is_empty() {
                    self.chr_ram[addr as usize]
                } else {
                    self.chr_rom[addr as usize]
                }
            }
            _ => 0,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        if addr < 0x2000 && self.chr_rom.is_empty() {
            self.chr_ram[addr as usize] = value;
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }
}

fn main() {
    let chr_bus = CartridgeChrBus {
        chr_rom: vec![0; 8192],
        chr_ram: [0; 8192],
        mirroring: Mirroring::Vertical,
    };

    let mut ppu = Ppu::new(chr_bus);

    // Run PPU for one frame (89,341 dots NTSC)
    for _ in 0..89_341 {
        ppu.tick();
    }

    // Get rendered frame
    let framebuffer = ppu.framebuffer();
}
```

---

## Core Types

### Color and Pixel Types

```rust
/// RGB color value
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// RGBA pixel (for framebuffer output)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pixel {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

/// NES palette index (0-63)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaletteIndex(pub u8);

/// Frame dimensions
pub const FRAME_WIDTH: usize = 256;
pub const FRAME_HEIGHT: usize = 240;
```

### Mirroring Mode

```rust
/// Nametable mirroring configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirroring {
    /// Horizontal mirroring (vertical scrolling games)
    Horizontal,
    /// Vertical mirroring (horizontal scrolling games)
    Vertical,
    /// Single-screen, lower bank
    SingleScreenA,
    /// Single-screen, upper bank
    SingleScreenB,
    /// Four unique nametables (requires extra VRAM)
    FourScreen,
}

impl Mirroring {
    /// Convert VRAM address to physical address
    pub fn translate_address(&self, addr: u16) -> u16 {
        let addr = addr & 0x0FFF;
        match self {
            Mirroring::Horizontal => {
                let table = (addr / 0x400) & 1;
                (addr & 0x3FF) | (table * 0x400)
            }
            Mirroring::Vertical => addr & 0x7FF,
            Mirroring::SingleScreenA => addr & 0x3FF,
            Mirroring::SingleScreenB => (addr & 0x3FF) | 0x400,
            Mirroring::FourScreen => addr,
        }
    }
}
```

### Sprite Data

```rust
/// Sprite attributes from OAM
#[derive(Debug, Clone, Copy)]
pub struct Sprite {
    /// Sprite X position
    pub x: u8,
    /// Sprite Y position (scanline - 1)
    pub y: u8,
    /// Tile index number
    pub tile_index: u8,
    /// Attribute byte
    pub attributes: SpriteAttributes,
    /// OAM index (0-63)
    pub oam_index: u8,
}

bitflags! {
    /// Sprite attribute flags
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct SpriteAttributes: u8 {
        /// Palette selection (0-3)
        const PALETTE       = 0b0000_0011;
        /// Unused bits
        const UNUSED        = 0b0001_1100;
        /// Priority (0=front of BG, 1=behind BG)
        const PRIORITY      = 0b0010_0000;
        /// Flip sprite horizontally
        const FLIP_H        = 0b0100_0000;
        /// Flip sprite vertically
        const FLIP_V        = 0b1000_0000;
    }
}
```

---

## PPU Struct

### Definition

```rust
/// NES 2C02 Picture Processing Unit
pub struct Ppu<B: PpuBus> {
    /// Cartridge CHR bus
    bus: B,

    /// Internal VRAM (2KB nametables)
    vram: [u8; 2048],

    /// Palette RAM (32 bytes)
    palette_ram: [u8; 32],

    /// Object Attribute Memory (256 bytes)
    oam: [u8; 256],

    /// Secondary OAM (32 bytes, 8 sprites)
    secondary_oam: [u8; 32],

    /// PPUCTRL register ($2000)
    ctrl: ControlRegister,

    /// PPUMASK register ($2001)
    mask: MaskRegister,

    /// PPUSTATUS register ($2002)
    status: StatusRegister,

    /// OAM address ($2003)
    oam_addr: u8,

    /// VRAM address register (v)
    v: VramAddress,

    /// Temporary VRAM address (t)
    t: VramAddress,

    /// Fine X scroll (3 bits)
    fine_x: u8,

    /// Write toggle (w)
    write_toggle: bool,

    /// Read buffer for $2007
    read_buffer: u8,

    /// Current scanline (0-261)
    scanline: u16,

    /// Current dot within scanline (0-340)
    dot: u16,

    /// Frame counter
    frame: u64,

    /// Odd frame flag
    odd_frame: bool,

    /// NMI output line
    nmi_pending: bool,

    /// Rendered framebuffer
    framebuffer: [Pixel; FRAME_WIDTH * FRAME_HEIGHT],
}
```

### Constructor

```rust
impl<B: PpuBus> Ppu<B> {
    /// Create new PPU with cartridge bus
    pub fn new(bus: B) -> Self {
        Self {
            bus,
            vram: [0; 2048],
            palette_ram: [0; 32],
            oam: [0; 256],
            secondary_oam: [0; 32],
            ctrl: ControlRegister::empty(),
            mask: MaskRegister::empty(),
            status: StatusRegister::empty(),
            oam_addr: 0,
            v: VramAddress(0),
            t: VramAddress(0),
            fine_x: 0,
            write_toggle: false,
            read_buffer: 0,
            scanline: 261, // Pre-render scanline
            dot: 0,
            frame: 0,
            odd_frame: false,
            nmi_pending: false,
            framebuffer: [Pixel::default(); FRAME_WIDTH * FRAME_HEIGHT],
        }
    }

    /// Reset PPU to power-on state
    pub fn reset(&mut self) {
        self.ctrl = ControlRegister::empty();
        self.mask = MaskRegister::empty();
        self.write_toggle = false;
        self.oam_addr = 0;
        self.scanline = 261;
        self.dot = 0;
        self.odd_frame = false;
    }
}
```

### Tick Method

```rust
impl<B: PpuBus> Ppu<B> {
    /// Execute one PPU cycle (dot)
    ///
    /// Returns true if NMI should be triggered
    pub fn tick(&mut self) -> bool {
        let mut trigger_nmi = false;

        match self.scanline {
            0..=239 => self.visible_scanline(),
            240 => { /* Post-render, idle */ }
            241 => {
                if self.dot == 1 {
                    self.status.insert(StatusRegister::VBLANK);
                    if self.ctrl.contains(ControlRegister::NMI_ENABLE) {
                        trigger_nmi = true;
                        self.nmi_pending = true;
                    }
                }
            }
            261 => self.pre_render_scanline(),
            _ => { /* VBlank scanlines 242-260, idle */ }
        }

        // Advance dot and scanline
        self.dot += 1;
        if self.dot > 340 {
            self.dot = 0;
            self.scanline += 1;
            if self.scanline > 261 {
                self.scanline = 0;
                self.frame += 1;
                self.odd_frame = !self.odd_frame;
            }
        }

        trigger_nmi
    }

    /// Run PPU until next VBlank
    pub fn run_until_vblank(&mut self) -> bool {
        loop {
            if self.tick() {
                return true;
            }
            if self.scanline == 241 && self.dot == 2 {
                return false;
            }
        }
    }

    /// Run PPU for one complete frame
    pub fn run_frame(&mut self) {
        let start_frame = self.frame;
        while self.frame == start_frame {
            self.tick();
        }
    }
}
```

---

## Register Interface

### CPU-Accessible Registers

```rust
impl<B: PpuBus> Ppu<B> {
    /// Read PPU register (called by CPU)
    pub fn read_register(&mut self, addr: u16) -> u8 {
        match addr & 0x07 {
            0 => 0, // PPUCTRL (write-only)
            1 => 0, // PPUMASK (write-only)
            2 => self.read_status(),
            3 => 0, // OAMADDR (write-only)
            4 => self.read_oam_data(),
            5 => 0, // PPUSCROLL (write-only)
            6 => 0, // PPUADDR (write-only)
            7 => self.read_data(),
            _ => unreachable!(),
        }
    }

    /// Write PPU register (called by CPU)
    pub fn write_register(&mut self, addr: u16, value: u8) {
        match addr & 0x07 {
            0 => self.write_ctrl(value),
            1 => self.write_mask(value),
            2 => { /* PPUSTATUS (read-only) */ }
            3 => self.oam_addr = value,
            4 => self.write_oam_data(value),
            5 => self.write_scroll(value),
            6 => self.write_addr(value),
            7 => self.write_data(value),
            _ => unreachable!(),
        }
    }
}
```

### Register Details

```rust
impl<B: PpuBus> Ppu<B> {
    /// Read PPUSTATUS ($2002)
    fn read_status(&mut self) -> u8 {
        let status = self.status.bits();

        // Clear VBlank flag
        self.status.remove(StatusRegister::VBLANK);

        // Reset write toggle
        self.write_toggle = false;

        // Race condition: VBlank suppression
        if self.scanline == 241 && self.dot < 3 {
            self.nmi_pending = false;
        }

        status
    }

    /// Write PPUCTRL ($2000)
    fn write_ctrl(&mut self, value: u8) {
        let was_nmi_enabled = self.ctrl.contains(ControlRegister::NMI_ENABLE);
        self.ctrl = ControlRegister::from_bits_truncate(value);

        // Update nametable select in t
        self.t.set_nametable_select((value & 0x03) as u16);

        // NMI edge detection
        let nmi_enabled = self.ctrl.contains(ControlRegister::NMI_ENABLE);
        let in_vblank = self.status.contains(StatusRegister::VBLANK);

        if !was_nmi_enabled && nmi_enabled && in_vblank {
            self.nmi_pending = true;
        }
    }

    /// Write PPUSCROLL ($2005)
    fn write_scroll(&mut self, value: u8) {
        if !self.write_toggle {
            // First write: X scroll
            self.t.set_coarse_x((value >> 3) as u16);
            self.fine_x = value & 0x07;
        } else {
            // Second write: Y scroll
            self.t.set_coarse_y((value >> 3) as u16);
            self.t.set_fine_y((value & 0x07) as u16);
        }
        self.write_toggle = !self.write_toggle;
    }

    /// Write PPUADDR ($2006)
    fn write_addr(&mut self, value: u8) {
        if !self.write_toggle {
            // First write: high byte
            self.t.0 = (self.t.0 & 0x00FF) | ((value as u16 & 0x3F) << 8);
        } else {
            // Second write: low byte
            self.t.0 = (self.t.0 & 0xFF00) | value as u16;
            self.v = self.t;
        }
        self.write_toggle = !self.write_toggle;
    }

    /// Read PPUDATA ($2007)
    fn read_data(&mut self) -> u8 {
        let addr = self.v.0 & 0x3FFF;

        let data = if addr < 0x3F00 {
            // Buffered read for non-palette
            let buffered = self.read_buffer;
            self.read_buffer = self.internal_read(addr);
            buffered
        } else {
            // Palette read is immediate (but buffer updated with nametable "underneath")
            self.read_buffer = self.internal_read(addr - 0x1000);
            self.internal_read(addr)
        };

        // Increment VRAM address
        let increment = if self.ctrl.contains(ControlRegister::VRAM_INCREMENT) {
            32 // Going down
        } else {
            1 // Going across
        };
        self.v.0 = self.v.0.wrapping_add(increment);

        data
    }

    /// Write PPUDATA ($2007)
    fn write_data(&mut self, value: u8) {
        let addr = self.v.0 & 0x3FFF;
        self.internal_write(addr, value);

        let increment = if self.ctrl.contains(ControlRegister::VRAM_INCREMENT) {
            32
        } else {
            1
        };
        self.v.0 = self.v.0.wrapping_add(increment);
    }
}
```

---

## VRAM Access

### Internal Memory Access

```rust
impl<B: PpuBus> Ppu<B> {
    /// Read from PPU address space
    fn internal_read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.bus.read(addr),
            0x2000..=0x3EFF => {
                let mirrored = self.bus.mirroring().translate_address(addr - 0x2000);
                self.vram[mirrored as usize]
            }
            0x3F00..=0x3FFF => {
                let index = (addr & 0x1F) as usize;
                // Handle palette mirroring
                let index = match index {
                    0x10 | 0x14 | 0x18 | 0x1C => index - 0x10,
                    _ => index,
                };
                self.palette_ram[index]
            }
            _ => 0,
        }
    }

    /// Write to PPU address space
    fn internal_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => self.bus.write(addr, value),
            0x2000..=0x3EFF => {
                let mirrored = self.bus.mirroring().translate_address(addr - 0x2000);
                self.vram[mirrored as usize] = value;
            }
            0x3F00..=0x3FFF => {
                let index = (addr & 0x1F) as usize;
                let index = match index {
                    0x10 | 0x14 | 0x18 | 0x1C => index - 0x10,
                    _ => index,
                };
                self.palette_ram[index] = value & 0x3F;
            }
            _ => {}
        }
    }
}
```

---

## Rendering

### Background Rendering

```rust
impl<B: PpuBus> Ppu<B> {
    /// Render visible scanline background
    fn render_background_pixel(&mut self, x: u16) {
        if !self.mask.contains(MaskRegister::SHOW_BACKGROUND) {
            return;
        }

        // Handle left column clipping
        if x < 8 && !self.mask.contains(MaskRegister::SHOW_BG_LEFT) {
            return;
        }

        // Calculate tile and attribute data
        let fine_x_offset = (x as u8 + self.fine_x) & 0x07;
        let bit_select = 7 - fine_x_offset;

        let pattern_lo = (self.bg_shift_lo >> bit_select) & 1;
        let pattern_hi = (self.bg_shift_hi >> bit_select) & 1;
        let pattern = (pattern_hi << 1) | pattern_lo;

        if pattern == 0 {
            return; // Transparent
        }

        let attr_lo = (self.attr_shift_lo >> bit_select) & 1;
        let attr_hi = (self.attr_shift_hi >> bit_select) & 1;
        let palette = (attr_hi << 1) | attr_lo;

        let color_index = self.palette_ram[(palette * 4 + pattern) as usize];
        self.set_pixel(x as usize, self.scanline as usize, color_index);
    }
}
```

### Frame Output

```rust
impl<B: PpuBus> Ppu<B> {
    /// Get reference to framebuffer
    pub fn framebuffer(&self) -> &[Pixel; FRAME_WIDTH * FRAME_HEIGHT] {
        &self.framebuffer
    }

    /// Get framebuffer as RGBA bytes
    pub fn framebuffer_rgba(&self) -> Vec<u8> {
        self.framebuffer
            .iter()
            .flat_map(|p| [p.r, p.g, p.b, p.a])
            .collect()
    }

    /// Get framebuffer as RGB bytes (no alpha)
    pub fn framebuffer_rgb(&self) -> Vec<u8> {
        self.framebuffer
            .iter()
            .flat_map(|p| [p.r, p.g, p.b])
            .collect()
    }

    /// Set pixel in framebuffer
    fn set_pixel(&mut self, x: usize, y: usize, palette_index: u8) {
        if x >= FRAME_WIDTH || y >= FRAME_HEIGHT {
            return;
        }
        let color = PALETTE_COLORS[palette_index as usize & 0x3F];
        self.framebuffer[y * FRAME_WIDTH + x] = Pixel {
            r: color.r,
            g: color.g,
            b: color.b,
            a: 255,
        };
    }
}
```

---

## Sprite Handling

### OAM Operations

```rust
impl<B: PpuBus> Ppu<B> {
    /// Write OAM DMA (256 bytes from CPU memory)
    pub fn oam_dma(&mut self, data: &[u8; 256]) {
        self.oam.copy_from_slice(data);
    }

    /// Get sprite from OAM
    pub fn get_sprite(&self, index: u8) -> Sprite {
        let base = (index as usize) * 4;
        Sprite {
            y: self.oam[base],
            tile_index: self.oam[base + 1],
            attributes: SpriteAttributes::from_bits_truncate(self.oam[base + 2]),
            x: self.oam[base + 3],
            oam_index: index,
        }
    }

    /// Get all visible sprites for debugging
    pub fn get_all_sprites(&self) -> Vec<Sprite> {
        (0..64).map(|i| self.get_sprite(i)).collect()
    }
}
```

### Sprite 0 Hit Detection

```rust
impl<B: PpuBus> Ppu<B> {
    /// Check sprite 0 hit conditions
    fn check_sprite_0_hit(&mut self, x: u16) {
        if self.status.contains(StatusRegister::SPRITE_0_HIT) {
            return; // Already hit this frame
        }

        if !self.mask.contains(MaskRegister::SHOW_BACKGROUND) ||
           !self.mask.contains(MaskRegister::SHOW_SPRITES) {
            return;
        }

        if x < 8 && (!self.mask.contains(MaskRegister::SHOW_BG_LEFT) ||
                     !self.mask.contains(MaskRegister::SHOW_SPRITE_LEFT)) {
            return;
        }

        if x == 255 {
            return; // Right edge
        }

        // Both sprite 0 and background must be opaque
        if self.sprite_0_opaque && self.bg_opaque {
            self.status.insert(StatusRegister::SPRITE_0_HIT);
        }
    }
}
```

---

## Debug Interface

### State Inspection

```rust
impl<B: PpuBus> Ppu<B> {
    /// Get current scanline (0-261)
    pub fn scanline(&self) -> u16 {
        self.scanline
    }

    /// Get current dot (0-340)
    pub fn dot(&self) -> u16 {
        self.dot
    }

    /// Get current frame number
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// Check if in VBlank
    pub fn in_vblank(&self) -> bool {
        self.status.contains(StatusRegister::VBLANK)
    }

    /// Get VRAM address (v register)
    pub fn vram_address(&self) -> u16 {
        self.v.0 & 0x3FFF
    }

    /// Get scroll position
    pub fn scroll_position(&self) -> (u16, u16) {
        let x = (self.v.coarse_x() << 3) | self.fine_x as u16;
        let y = (self.v.coarse_y() << 3) | self.v.fine_y();
        (x, y)
    }

    /// Get register values
    pub fn get_registers(&self) -> PpuRegisters {
        PpuRegisters {
            ctrl: self.ctrl.bits(),
            mask: self.mask.bits(),
            status: self.status.bits(),
            oam_addr: self.oam_addr,
            v: self.v.0,
            t: self.t.0,
            fine_x: self.fine_x,
            write_toggle: self.write_toggle,
        }
    }
}

/// PPU register snapshot for debugging
#[derive(Debug, Clone, Copy)]
pub struct PpuRegisters {
    pub ctrl: u8,
    pub mask: u8,
    pub status: u8,
    pub oam_addr: u8,
    pub v: u16,
    pub t: u16,
    pub fine_x: u8,
    pub write_toggle: bool,
}
```

### Pattern Table Rendering

```rust
impl<B: PpuBus> Ppu<B> {
    /// Render pattern table for debugger
    ///
    /// Returns 128x128 pixel image (256 8x8 tiles)
    pub fn render_pattern_table(&self, table: u8, palette: u8) -> [Color; 128 * 128] {
        let mut output = [Color { r: 0, g: 0, b: 0 }; 128 * 128];
        let base = (table as u16) * 0x1000;

        for tile_y in 0..16 {
            for tile_x in 0..16 {
                let tile_index = tile_y * 16 + tile_x;
                let tile_addr = base + (tile_index * 16);

                for row in 0..8 {
                    let lo = self.bus.read(tile_addr + row);
                    let hi = self.bus.read(tile_addr + row + 8);

                    for col in 0..8 {
                        let bit = 7 - col;
                        let color_index = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);

                        let palette_index = if color_index == 0 {
                            self.palette_ram[0]
                        } else {
                            self.palette_ram[palette as usize * 4 + color_index as usize]
                        };

                        let x = (tile_x as usize * 8) + col as usize;
                        let y = (tile_y as usize * 8) + row as usize;
                        output[y * 128 + x] = PALETTE_COLORS[palette_index as usize & 0x3F];
                    }
                }
            }
        }

        output
    }

    /// Render nametable for debugger
    ///
    /// Returns 256x240 or 512x480 pixel image
    pub fn render_nametable(&self, nametable: u8) -> [Color; 256 * 240] {
        let mut output = [Color { r: 0, g: 0, b: 0 }; 256 * 240];
        let nt_base = 0x2000 + (nametable as u16) * 0x400;
        let attr_base = nt_base + 0x3C0;
        let pattern_table = if self.ctrl.contains(ControlRegister::BG_PATTERN_TABLE) {
            0x1000
        } else {
            0x0000
        };

        for tile_y in 0..30 {
            for tile_x in 0..32 {
                let tile_index = self.internal_read(nt_base + tile_y * 32 + tile_x);
                let tile_addr = pattern_table + (tile_index as u16) * 16;

                // Get attribute byte
                let attr_index = (tile_y / 4) * 8 + (tile_x / 4);
                let attr_byte = self.internal_read(attr_base + attr_index);
                let shift = ((tile_y % 4) / 2) * 4 + ((tile_x % 4) / 2) * 2;
                let palette = (attr_byte >> shift) & 0x03;

                for row in 0..8 {
                    let lo = self.bus.read(tile_addr + row);
                    let hi = self.bus.read(tile_addr + row + 8);

                    for col in 0..8 {
                        let bit = 7 - col;
                        let color_index = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);

                        let palette_index = if color_index == 0 {
                            self.palette_ram[0]
                        } else {
                            self.palette_ram[palette as usize * 4 + color_index as usize]
                        };

                        let x = (tile_x as usize * 8) + col as usize;
                        let y = (tile_y as usize * 8) + row as usize;
                        output[y * 256 + x] = PALETTE_COLORS[palette_index as usize & 0x3F];
                    }
                }
            }
        }

        output
    }
}
```

---

## Integration

### With CPU

```rust
use rustynes_cpu::Bus;
use rustynes_ppu::Ppu;

impl<B: PpuBus> NesBus<B> {
    pub fn tick_ppu(&mut self) -> bool {
        // Run 3 PPU cycles per CPU cycle
        let nmi1 = self.ppu.tick();
        let nmi2 = self.ppu.tick();
        let nmi3 = self.ppu.tick();
        nmi1 || nmi2 || nmi3
    }

    pub fn handle_oam_dma(&mut self, page: u8) {
        let base = (page as u16) << 8;
        let mut data = [0u8; 256];
        for i in 0..256 {
            data[i] = self.read(base + i as u16);
        }
        self.ppu.oam_dma(&data);
    }
}
```

---

## Examples

### Basic Rendering Loop

```rust
fn render_frame(ppu: &mut Ppu<impl PpuBus>) -> &[Pixel] {
    // Run PPU for one frame
    ppu.run_frame();

    // Return rendered image
    ppu.framebuffer()
}
```

### Debug Visualization

```rust
fn show_debug_info(ppu: &Ppu<impl PpuBus>) {
    let regs = ppu.get_registers();
    println!("PPU State:");
    println!("  Scanline: {}, Dot: {}", ppu.scanline(), ppu.dot());
    println!("  Frame: {}", ppu.frame());
    println!("  CTRL: {:08b}", regs.ctrl);
    println!("  MASK: {:08b}", regs.mask);
    println!("  STATUS: {:08b}", regs.status);
    println!("  v: {:04X}, t: {:04X}", regs.v, regs.t);

    let (scroll_x, scroll_y) = ppu.scroll_position();
    println!("  Scroll: ({}, {})", scroll_x, scroll_y);
}
```

---

## References

- [NESdev Wiki: PPU](https://www.nesdev.org/wiki/PPU)
- [Visual 2C02](https://www.nesdev.org/wiki/Visual_2C02)
- [Loopy's PPU Document](https://www.nesdev.org/wiki/PPU_scrolling)

---

**Related Documents:**
- [PPU_2C02_SPECIFICATION.md](../ppu/PPU_2C02_SPECIFICATION.md)
- [PPU_TIMING_DIAGRAM.md](../ppu/PPU_TIMING_DIAGRAM.md)
- [PPU_SCROLLING_INTERNALS.md](../ppu/PPU_SCROLLING_INTERNALS.md)
- [PPU_SPRITE_EVALUATION.md](../ppu/PPU_SPRITE_EVALUATION.md)
