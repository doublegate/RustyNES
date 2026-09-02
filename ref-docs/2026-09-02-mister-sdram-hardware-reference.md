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
| max clock | **143 MHz** |
| access time | 5.4 ns |
| voltage | 3.3 V |
| package | 54-TSOP II |

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
- [AS4C32M16SB — Alliance Memory](https://alliancememory.com/datasheets/as4c32m16sb) — 512 Mbit, 32M x 16, 4 banks, 143 MHz, 3.3 V
- [AS4C32M16SB-7TCN — Farnell](https://uk.farnell.com/alliance-memory/as4c32m16sb-7tcn/dram-143mhz-512mbit-tsop-ii-54/dp/4260952) and [Digi-Key](https://www.digikey.com/en/products/detail/alliance-memory-inc/AS4C32M16SB-7TCN/6589201) — packaging and speed grade
- [SuperStation One — Retro Remake](https://retroremake.co/pages/superstation%E1%B5%92%E2%81%BF%E1%B5%89) and [CNX Software](https://www.cnx-software.com/2025/01/29/superstation-one-soc-fpga-based-retro-gaming-console-supports-mister-emulation-playstation-controllers-cd-drive/) — 128 MB integrated BGA SDRAM, Cyclone V, MiSTer core support
- MiSTer FPGA Forum threads on the 128 MB module (two 64 MB chips, inverted single select, signal-integrity ceiling) and on v2.9 vs v3.0 being a layout-only change. **Note:** `misterfpga.org` returns HTTP 403 to automated fetching, so these were read through search result summaries rather than fetched directly, and are the least-firm citations here.
- This repository's vendored `sys/sys_top.v` and `sys/hps_io.sv` — the pin list and the `sdram_sz` encoding
