# Glossary

NES emulation has dense jargon. Definitions below; defined here once so other docs can use them without re-explaining.

| Term | Definition |
|------|------------|
| **2A03** | Ricoh CPU+APU IC used in the NTSC NES/Famicom (1.789 MHz). 2A07 is the PAL variant. |
| **2C02** | Ricoh PPU IC for NTSC. 2C07 is PAL. 2C03/2C04/2C05 are arcade variants (Vs. System / PlayChoice-10). |
| **A12** | Bit 12 of the PPU address bus. Toggled when fetching from `$1000-$1FFF`; used by MMC3 to count scanlines. |
| **APU** | Audio Processing Unit. Five channels: pulse 1, pulse 2, triangle, noise, DMC. Integrated into the 2A03. |
| **Attribute table** | The 64-byte block at the end of each nametable specifying the palette for each 32×32 pixel quadrant. |
| **Background** | The tile-based scrolling layer rendered from CHR via the nametables, attributes, and palettes. |
| **Band-limited synthesis** | Generating audio samples by convolving step-responses with a windowed sinc kernel. blip_buf is the canonical implementation. |
| **BCD** | Binary-coded decimal. 6502 has it; the 2A03 disables it (D flag is settable but ADC/SBC ignore it). |
| **Bus** | The shared mutable state container that the CPU borrows during execution. Owns PPU, APU, mapper, RAM, controllers, open-bus latch. |
| **Bus conflict** | A cartridge write where CPU and PRG ROM drive the data bus at the same time, usually producing an AND-like value on discrete mappers. |
| **Cartridge** | The physical ROM cartridge, modeled as PRG-ROM + CHR-ROM/RAM + PRG-RAM + a mapper. |
| **CHR** | Character ROM/RAM. The 8 KB pattern table data that sprites and background tiles draw from. |
| **CPU dot** | Equivalent to a CPU clock cycle. NTSC: ~559 ns. |
| **Cycle accuracy** | An emulator that advances each chip in lockstep at sub-instruction granularity, so mid-instruction PPU/APU events are visible to subsequent CPU code. |
| **Dendy** | Russian PAL famiclone (1990s). Uses PAL crystal but NTSC's PPU clock divider, producing 50 Hz refresh with NTSC-compatible timing. |
| **DMA** | Direct Memory Access. Two units: OAM DMA (`$4014` write copies 256 B to OAM) and DMC DMA (samples from CPU memory to APU's DMC channel). |
| **DMC** | Delta Modulation Channel. The fifth APU channel; plays 1-bit-delta-encoded samples from CPU memory. |
| **Dot** | One PPU clock cycle. The PPU produces one pixel per dot during visible scanlines. |
| **ExRAM** | The 1 KB internal RAM in the MMC5 mapper, with multiple usage modes (extended attributes, fill nametable, etc.). |
| **Famicom** | "Family Computer", the Japanese version of the NES (1983). |
| **FDS** | Famicom Disk System — Japan-only floppy disk add-on. |
| **Fine X / Fine Y** | The 3-bit pixel offset within a tile (0-7). Lives in the PPU's `x` register and the high 3 bits of `v`. |
| **Frame counter** | An APU sub-unit clocking envelopes, sweep, length counters, and (optionally) the frame IRQ at ~240 Hz. |
| **Get / put cycle** | APU/DMA phase terminology. DMA reads occur on get cycles and writes on put cycles; DMC and OAM DMA timing depends on this alignment. |
| **Hijacking** | When an interrupt of higher priority redirects an in-progress lower-priority interrupt's vector fetch to its own vector. NMI hijacks IRQ; IRQ hijacks BRK. |
| **iNES** | Marat Fayzullin's 1996 file format for NES ROM dumps. The 16-byte header is the de facto standard. |
| **IRQ** | Interrupt Request. Level-sensitive on the 6502 — fires while the line is held low and I flag is clear. |
| **Length counter** | Per-channel APU counter that silences the channel when it reaches zero. Decremented on half-frame clocks. |
| **Linear counter** | Triangle-channel-specific counter providing finer length control independent of the length counter. |
| **Lockstep scheduling** | The PPU is the master clock; CPU and APU advance based on PPU dot count. Opposite of catch-up. |
| **Loopy `v/t/x/w`** | The four internal PPU registers governing scroll and VRAM addressing. Named after the late "Loopy" who reverse-engineered them. |
| **Mapper** | The cartridge IC that arbitrates banking, mirroring, and (for some) IRQ generation. ~250 distinct types exist. |
| **Mirroring** | The PPU has only 2 KB of internal nametable VRAM but 4 KB of address space; the cartridge specifies which 1 KB regions mirror each other (horizontal, vertical, single-screen A/B, four-screen). |
| **MMC** | Memory Management Controller (Nintendo). MMC1, MMC2, MMC3, MMC4, MMC5, MMC6 are official Nintendo mappers. |
| **Nametable** | A 1 KB block of PPU memory holding tile indices (32×30 = 960 bytes) plus attribute data (64 bytes). Four logical tables exist; usually only 2 physical are present in the console (mirroring). |
| **NES 2.0** | Extended iNES header format adding 12-bit mappers, submappers, exponent-multiplier ROM sizing, and per-region timing. Detected when `header[7] & 0x0C == 0x08`. |
| **NMI** | Non-Maskable Interrupt. Edge-sensitive on the 6502 — fires once per high-to-low transition. The PPU asserts NMI at the start of vertical blank if PPUCTRL bit 7 is set. |
| **NROM** | iNES mapper 0. The simplest cart: no banking, fixed 16/32 KB PRG and 8 KB CHR. |
| **OAM** | Object Attribute Memory. The 256 B sprite list (64 sprites × 4 bytes: Y, tile, attributes, X). |
| **Open bus** | When a memory location returns the last value on the data bus instead of meaningful data. Pervasive on the NES due to incomplete address decoding. |
| **PPU `_io_db`** | The PPU's internal CPU-facing dynamic data latch. It backs write-only register reads and unused PPUSTATUS bits and decays over time. |
| **PAL** | Phase Alternating Line. The European TV standard; NES PAL has 312 scanlines and 50 Hz. |
| **Pattern table** | An 8 KB region of PPU memory storing tile bitmaps (each tile = 16 bytes = two 8×8 1-bit planes). |
| **PPU** | Picture Processing Unit. The 2C02 chip; runs at 3× CPU clock; renders the picture. |
| **PRG** | Program ROM/RAM. The cart-side memory for code and data, mapped into CPU address space. |
| **Sprite-zero hit** | A PPUSTATUS flag set when sprite 0's first non-transparent pixel overlaps a non-transparent background pixel. The classic mechanism for split-screen status bars. |
| **Sweep** | Per-pulse-channel APU sub-unit that periodically modifies the channel's period (frequency sweep). |
| **Submapper** | NES 2.0 field that distinguishes board or chip variants sharing the same mapper number, such as MMC3 revisions or VRC2/VRC4 wiring. |
| **Tile** | An 8×8 pixel block. Drawn from the pattern table; positioned by the nametable; colored by the attribute table + palette. |
| **VBL / VBlank** | Vertical blank. The period (scanlines 241-260 NTSC) between frames; safe time for the CPU to update PPU state. |
| **VRAM** | Video RAM. The 2 KB internal PPU RAM holding nametable data. |
| **VRC** | Konami's Virtual Rom Controller mapper family (VRC1-7). |
| **`v`, `t`, `x`, `w`** | The four loopy registers in the PPU. See "Loopy `v/t/x/w`". |
