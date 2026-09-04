# MiSTer SDRAM — hardware, interface, and what this project needs from it

**Dated supplemental research, 2026-09-02.** `ref-docs/` is immutable; corrections
land as new dated files, never as edits to this one.

Gathered for **v2.6.13**, whose subject is the SDRAM controller and the tickets
it unblocks. Every claim below carries its source. Where a claim is an
*inference* rather than something a source states, it says so.

**Firewall note, stated first because it governs how this file may be used.**
This document is assembled from public hardware documentation, vendor
datasheets, the MiSTer project's own developer documentation, and this
repository's own vendored `sys/`. **No third-party SDRAM controller RTL was
read**, and none may be: the approved v2.7.0 plan says the controller is written
"from spec ... escalate to an ADR before reading any third-party controller",
and ADR 0037 puts any other NES core's `rtl/` out of reach entirely. A
controller written from the parts below is an independent implementation; one
written after reading somebody's `sdram.v` is a derivative work. The difference
is not visible in the result, which is exactly why the rule is procedural.

---

## 1. Why this project needs SDRAM at all

The official `Hardware_MiSTer` repository states the daughter board "provides
128MB of SDR SDRAM memory for cores requiring a large (**>512KB**) memory", and
that the DE10-Nano's own DDR3 "has big latency and cannot fit into timings of
retro EDO DRAM". **The board is described as required, not optional.**

That threshold is this project's case exactly. The approved mapper scope is the
top six — NROM, MMC1, UxROM, CNROM, MMC3, AxROM — whose worst case is:

| | worst case in scope |
|---|---|
| PRG-ROM | **512 KiB** (MMC1, MMC3) |
| CHR-ROM | **256 KiB** (MMC3) |
| total | **768 KiB** |

The 5CSEBA6U23I7 has **553 M10K blocks**, about **692 KiB** of block memory in
total, and v2.6.12 already spends **468 of 553** on a declared 256 KiB PRG +
128 KiB CHR. So the full mapper scope does not fit on-chip, and no amount of
tuning changes that: 768 KiB of cartridge alone exceeds the whole device's block
memory before work RAM, the PPU, or anything else is counted.

**This is the concrete, arithmetic reason the SDRAM controller is the next major
item**, rather than a preference.

---

## 2. The board: what the hardware actually is

| | |
|---|---|
| chip | **Alliance Memory AS4C32M16SB-7TCN** |
| organisation | 32M x 16, internally **4 banks of 8M x 16** |
| density | **512 Mbit = 64 MiB per chip** |
| max clock | **143 MHz** (-7); the -6 grade is 166 MHz |
| access time | 5.4 ns (tAC at CL3) |
| voltage | 3.3 V |
| package | 54-TSOP II; a 54-ball FBGA variant (-7BIN) also exists |

**Which part, verified.** The v3.0 XS-D board is fitted with **AS4C32M16SB-7TCN**,
and the -7TCN, -7TIN and -6TIN grades are interchangeable on it -- the -6 is
faster than any MiSTer core needs. Boards are commonly burn-in tested at 130 MHz,
against 126 MHz for the most demanding published core.

**The SuperStation One's exact part is NOT publicly documented.** It has "128 MB
BGA SDRAM"; whether that is two AS4C32M16SB-7BIN or something else is not stated
anywhere I could find, and the honest answer is that I do not know. It does not
matter for correctness, and the reason is worth stating rather than waving at:
MiSTer cores must run on it unmodified, so whatever it is presents the same
interface at the same pins, and the timings below are the slowest of the
interchangeable grades.

### Which speed grade to build for -- and it is not a preference

From Table 16 of the datasheet, every -7 minimum is greater than or equal to the
matching -6 minimum:

| parameter | -6 (166 MHz) | -7 (143 MHz) |
|---|---|---|
| tRC | 60 ns | **63 ns** |
| tRFC | 60 ns | **63 ns** |
| tRCD | 18 ns | **21 ns** |
| tRP | 18 ns | **21 ns** |
| tRRD | 12 ns | **14 ns** |
| tMRD | 12 ns | **14 ns** |
| tRAS (min) | 42 ns | 42 ns |
| tWR | 12 ns | **14 ns** |

**So a controller configured for -7 satisfies a -6 part as well, and one
configured for -6 would VIOLATE a -7 part.** Since boards ship with either, -7
is the only correct choice, not the cautious one.

### The AC timing table, as read

Table 16, Rev 1.4 (June 2024), -7 column, all minimums:

| symbol | parameter | -7 |
|---|---|---|
| tRC | row cycle time (same bank) | 63 ns |
| tRFC | refresh cycle time | 63 ns |
| tRCD | RAS# to CAS# delay (same bank) | **21 ns** |
| tRP | precharge to refresh / row activate (same bank) | **21 ns** |
| tRRD | row activate to row activate (different banks) | 14 ns |
| tMRD | mode register set cycle time | 14 ns |
| tRAS | row activate to precharge (same bank) | 42 ns **min, 120 us MAX** |
| tWR | write recovery time | 14 ns |
| tCK | clock cycle time | 10 ns at CL2, **7 ns at CL3** |
| tAC | access time from CLK | 6 ns at CL2, 5.4 ns at CL3 |
| tREFI | average refresh interval | **7.8 us** |

And the power-up sequence, Note 11:

1. power applied with CKE low, DQM high, all inputs NOP;
2. **stable clock for a minimum of 200 us**, then CKE high with DQM held high;
3. all banks precharged;
4. Mode Register Set;
5. **a minimum of 2 auto-refresh cycles** -- and the note says explicitly that
   these "can be issue before or after Mode Register Set command".

**The datasheet corrected three things this project had guessed, and two were
guessed in the UNSAFE direction.** Full account in the v2.6.13 release notes; the
short version is that "conservative" is only meaningful once you know which way
is safe for each parameter, and for a minimum-delay constraint that is *larger*.
Two of the guesses were smaller than the real minimum.

### One observation left open

The datasheet's own header describes the AS4C32M16SB as a **"Dual Die Package
(DDP)"**, while the body describes a single logical device of four banks and the
pinout carries one CS#. Whether the community account of the 128 MB board -- two
packages sharing a bus with one inverted select -- is a description of two
packages or of one DDP with a second select on the pin the single-die version
lists as NC, is not settled here. **It has no bearing on this project**, which
needs under 1 MiB and uses the first 64 MiB through one select, and it is written
down rather than resolved because resolving it would need a board in hand.

**The "128 MB" board is two of those chips**, not one larger part: the largest
readily-available SDR SDRAM die is 64 MB, so the 128 MB module carries a pair.

**And they share one chip select.** Community documentation is explicit that,
for want of I/O pins, "instead of each chip having a select signal, a single
select is inverted for one of the two chips" — both chips share the address and
data buses. The practical consequence for a controller:

> **`SDRAM_nCS` on a 128 MB board is effectively address bit A25, not a
> constant-low select.** A controller that ties `nCS` low reaches only the
> first 64 MB. That is harmless for this project — we need under 1 MiB — but it
> is the kind of fact that becomes a defect the first time somebody assumes the
> obvious.

The same source notes the 128 MB module is "already pushing the limits in terms
of signal integrity", which is why 256 MB does not exist.

### Board revisions

**v2.9 and v3.0 are electrically the same board.** v3.0 "is the same as 2.9 but
just with all IC on top of board to make the production simpler" — a layout
change for assembly, not a functional one.

> **So there is nothing for the RTL to do differently between v2.9 and v3.0.**
> The question "which board revision do we support" has the answer "both,
> identically, and the distinction does not reach the HDL."

Earlier revisions (v2.2 XS, v2.5 XSD) appear in the MiSTer Bible's hardware
pages. They differ in size and orientation rather than in interface.

---

## 3. SuperStation One

Retro Remake's SuperStation One is a Cyclone V SoC console that runs MiSTer
cores. It ships **128 MB of BGA SDRAM already integrated**, "avoiding the
separate SDRAM module used by traditional DE10-Nano builds", and loads
unmodified MiSTer cores.

**INFERENCE, not a sourced fact:** because unmodified cores run, and because a
core's SDRAM pin locations come from `sys/sys.tcl` — which is vendored,
identical for every core, and may not be edited — the SuperStation One's
integrated SDRAM must present on the same FPGA pins as the DE10-Nano daughter
board. No public document states this pin-for-pin. Treat it as the working
assumption it is; **verifying it is precisely the "one `.rbf` boots both"
acceptance the v2.7.0 plan already names as rung 6.**

**Consequence for this project:** on a SuperStation One there is nothing to buy
and nothing to fit. On a DE10-Nano the board is a required add-on.

---

## 4. The interface the framework gives us

Read from this repository's own vendored `sys/sys_top.v` — not from a search.
The framework hands the core **raw SDR SDRAM pins and no controller**:

```systemverilog
output [12:0] SDRAM_A,     inout [15:0] SDRAM_DQ,
output        SDRAM_DQML,  output       SDRAM_DQMH,
output        SDRAM_nWE,   output       SDRAM_nCAS,
output        SDRAM_nRAS,  output       SDRAM_nCS,
output  [1:0] SDRAM_BA,    output       SDRAM_CLK,
output        SDRAM_CKE
```

13 row-address bits + 2 bank bits + 10 column bits = 25 address bits x 16 data
bits = 512 Mbit = 64 MiB through one chip select, which is the single-chip
figure from section 2 arriving by a second route.

`rtl/emu.sv` currently ties all of it off:

```systemverilog
assign {SDRAM_CLK, SDRAM_CKE, SDRAM_A, SDRAM_BA, SDRAM_DQML, SDRAM_DQMH,
        SDRAM_nCS, SDRAM_nCAS, SDRAM_nRAS, SDRAM_nWE} = 0;
assign SDRAM_DQ = 'Z;
```

**Writing the controller is entirely on us.** `sys/` provides pins, pin
assignments (`sys.tcl`) and timing constraints, and nothing above that.

### Clocking

SDRAM needs a phase-shifted clock relative to the core clock so that data is
driven and captured against the chip's setup/hold windows. In MiSTer this is
done either at the PLL or with an `altddio_out` primitive giving a fixed 180
degrees; which is better is design-dependent and settled by static timing
analysis rather than by rule.

**The HPS can also shift it at runtime**, which is what the otherwise-cryptic
upper bits of `sdram_sz` are for — see the next section. That matters because it
means a marginal board can be tuned without rebuilding the core.

---

## 5. `sdram_sz`, decoded — and it carries a validity bit

From our own `sys/hps_io.sv:159`, verbatim:

```systemverilog
// [15]: 0 - unset, 1 - set. [1:0]: 0 - none, 1 - 32MB, 2 - 64MB, 3 - 128MB
// [14]: debug mode: [8]: 1 - phase up, 0 - phase down. [7:0]: amount of shift.
output reg [15:0] sdram_sz,
```

| field | meaning |
|---|---|
| `[15]` | **validity**: 0 = the HPS has not told us yet, 1 = the value is real |
| `[1:0]` | 0 = none, 1 = 32 MB, 2 = 64 MB, 3 = 128 MB |
| `[14]` | debug/phase-tuning mode |
| `[8]` | phase up (1) / down (0) |
| `[7:0]` | shift amount |

**Bit 15 is the whole ticket.** `T-MISTER-SDRAM-SZ` is not "print the size"; it
is "do not confuse *not yet told* with *none present*". Without checking bit 15,
a core reads `sdram_sz[1:0] == 0` at power-on and concludes there is no SDRAM
board — the identical failure shape this project has recorded repeatedly, where
an absent signal is read as a value.

Any size from 32 MB up is vastly more than this project needs. **The size field
is a presence check for us, not a capacity calculation.**

---

## 6. Saving — and a v2.6.12 conclusion that was wrong

v2.6.12 attempted `T-MISTER-SAVE`, reverted it, and recorded the blocker as
*"there is no OSD-close signal to flush on"*. **That is refuted by the MiSTer
developer documentation, which states the contract in one line:**

> `ioctl_upload_req` — set to 1 to ask the HPS to initiate an NVRAM save, for
> autosave, **HPS only reads this when the OSD is open**

The core does not detect the OSD closing, and does not need to. It raises the
request; the HPS collects it when the OSD is open. The same page puts ioctl
upload squarely at our use case — **"Use ioctl upload for: smaller NVRAM/save
files"** — with the `sd_*` block interface reserved for virtual hard drives.

The upload itself: `ioctl_upload` goes high, the core presents the byte at
`ioctl_addr` on `ioctl_din`, and `ioctl_rd` strobes each one.

The declaration side is the `F` option's `S` modifier:

> `F[S][#],{Ext}[,{Text}][,{Address}]` — Load file button. Optional `[S]` —
> core supports save files, load a file, and mount a save for reading or writing

**Both routes are present in our vendored `hps_io.sv`** — the ioctl upload path,
and the full `sd_*` block interface (`img_mounted`, `img_readonly`, `img_size`,
`sd_lba`, `sd_rd`, `sd_wr`, `sd_ack`, `sd_buff_*`, `VDNUM` defaulting to 1).

**What survives from v2.6.12's finding, and what does not:**

| claim | status |
|---|---|
| "no OSD-close signal, so there is no trigger" | **REFUTED** — the HPS polls the request while the OSD is open |
| "`ioctl_upload_req` must be supported HPS-side" (quoting `hps_io.sv:152`) | true as quoted, and the support **is** the documented autosave mechanism |
| "no gate in this repository can reach it — the testbench does not instantiate `hps_io`" | **STILL TRUE.** Verification remains rung 6 |

So the ticket's *original* text — implementation unblocked, verification rung 6 —
was right, and v2.6.12's "correction" of it was the error. The lesson is narrow
and worth keeping: **reasoning from the absence of a signal in the framework
source to "the mechanism cannot work" skipped reading the protocol that uses
it.** One page of vendor documentation settled what a session of source-reading
had concluded backwards.

---

## 7. What this means per ticket

| ticket | before | after this research |
|---|---|---|
| `T-MISTER-SDRAM-SZ` | blocked on the controller | still gated on the controller, but the **semantics are now fully known**, including the validity bit that is the actual content of the ticket |
| `T-MISTER-SAVE` | recorded as blocked on a missing trigger | **implementation unblocked** — mechanism documented end to end. Verification stays rung 6 |
| `T-MISTER-4PLAYER` / `ZAPPER` / `PADDLE` / `KEYBOARD` | v2.8+ by the approved plan | **unchanged.** Research does not re-scope an approved plan |
| `T-MISTER-DIRECTVIDEO` / `MENUMASK` | rung 6 | unchanged — both are verified by looking at a display |
| `T-MISTER-VMODE` | needs PAL timing | unchanged |

---

## Sources

- [Hardware_MiSTer (MiSTer-devel)](https://github.com/MiSTer-devel/Hardware_MiSTer) — the >512KB threshold, the DDR3 latency reason, "SDRAM board is required", the AS4C32M16SB-7TCN part
- [hps_io developer documentation](https://mister-devel.github.io/MkDocs_MiSTer/developer/hps_io/) — the `ioctl_upload_req` autosave contract and the ioctl-vs-block-device split
- [Core Configuration String](https://mister-devel.github.io/MkDocs_MiSTer/developer/conf_str/) — the `F[S]` save-file declaration
- [MiSTer FPGA Bible — SDRAM Board](https://boogermann.github.io/Bible_MiSTer/hardware/sdram-board/) — board revisions and per-core clock table
- **`ref-docs/Datasheets/AllianceMemory_512M_SDRAM_Bdie_AS4C32M16SB-7TXN-6TIN-7BIN__Rev1.4_June2024NK.pdf`**
  -- the authoritative source for every timing above (Table 1, Table 16, Note 11).
  Supplied by the maintainer after this file's first draft, which had said the
  vendor PDF could not be retrieved. Revisions 1.2 (March 2020) and 1.3 (August
  2022) are alongside it; 1.4 is current and its revision history shows the
  intervening changes are all to standby/operating CURRENT specifications, so
  **the AC timings are identical across all three** and nothing here depends on
  which revision is read
- [AS4C32M16SB-7TCN — Farnell](https://uk.farnell.com/alliance-memory/as4c32m16sb-7tcn/dram-143mhz-512mbit-tsop-ii-54/dp/4260952) and [Digi-Key](https://www.digikey.com/en/products/detail/alliance-memory-inc/AS4C32M16SB-7TCN/6589201) — packaging and speed grade
- [SuperStation One — Retro Remake](https://retroremake.co/pages/superstation%E1%B5%92%E2%81%BF%E1%B5%89) and [CNX Software](https://www.cnx-software.com/2025/01/29/superstation-one-soc-fpga-based-retro-gaming-console-supports-mister-emulation-playstation-controllers-cd-drive/) — 128 MB integrated BGA SDRAM, Cyclone V, MiSTer core support
- MiSTer FPGA Forum threads on the 128 MB module (two 64 MB chips, inverted single select, signal-integrity ceiling) and on v2.9 vs v3.0 being a layout-only change. **Note:** `misterfpga.org` returns HTTP 403 to automated fetching, so these were read through search result summaries rather than fetched directly, and are the least-firm citations here.
- This repository's vendored `sys/sys_top.v` and `sys/hps_io.sv` — the pin list and the `sdram_sz` encoding
