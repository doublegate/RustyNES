# ADR 0039 — the SDRAM controller gets its own clock, at 4x the master clock

**Status:** Accepted (v2.6.13)

**Supersedes nothing. Amends the one-clock property established in v2.6.3
"Mainspring" — see Consequences, because the amendment is narrower than it looks.**

## Context

v2.6.3 collapsed the MiSTer core onto a single clock: 21.477272 MHz, the NTSC
master clock, from which the CPU (÷12) and the dot clock (÷4) are derived by
counting. `rtl/pll.v` says so in its own header — "which is why `nes_top` needs
only this one ... the whole console is one clock domain" — and that property is
load-bearing for determinism, for the co-simulation ladder, and for reasoning
about every gate built since.

v2.6.13 must serve the cartridge from SDRAM, because the approved mapper scope
peaks at 512 KiB PRG + 256 KiB CHR = 768 KiB against roughly 692 KiB of total
M10K on the device. That is not a tuning problem; the cartridge alone exceeds
the die.

**The question this ADR answers is not whether to use SDRAM. It is what clocks
the controller, and the answer is forced by arithmetic rather than chosen.**

### The budget, measured rather than assumed

The cartridge answers today in **one `clk_sys` cycle** — `cart.sv` ends with a
registered read, `chr_dout <= chr[...]`. What replaces it has this much time:

| | |
|---|---|
| one PPU dot | **4 `clk_sys` cycles** (master ÷ 4) |
| CHR address presented | **one dot ahead** — `ppu2c02.sv`'s `addr_phase` looks ahead on odd dots |
| background fetch cadence | one fetch every **2 dots = 8 `clk_sys` cycles**, four fetches per 8 dots |
| CPU access | lands at master clock 7 of a 12-clock CPU cycle |

So the hard budget is **4 cycles** from address to data, inside a fetch slot
recurring every 8, shared with the CPU.

### What an access costs

At the -7 grade (`ref-docs/Datasheets/...Rev1.4...pdf`, Table 16): tRCD 21 ns,
tRP 21 ns, tRAS 42 ns min, CL2 needs tCK >= 10 ns.

Run at `clk_sys` (21.477272 MHz, 46.6 ns period) an activate/read/auto-precharge
sequence is roughly **seven cycles**. Against a four-cycle budget, with the CPU
also wanting the bus. **It does not fit, and no amount of care makes it fit** —
the row activate alone is unavoidable and the console cannot wait.

## Decision

**Give the SDRAM controller its own clock at exactly 4x the master clock —
85.909088 MHz — from the same PLL, and drive the SDRAM pin from a
phase-shifted output of the same frequency.**

At 85.909088 MHz (11.64 ns period) the same sequence is about **13 SDRAM
cycles = 3.25 `clk_sys` cycles**, inside the four-cycle budget with room for the
CPU's accesses in the remaining slots. CL2 is legal there: 11.64 ns exceeds the
10 ns tCK minimum for CL2, with margin.

Three properties make this an amendment to the one-clock rule rather than an
abandonment of it:

1. **The ratio is an exact integer, from one PLL.** 4x is not "a faster clock
   nearby"; every SDRAM edge coincides with a master edge every fourth cycle, so
   the crossing is synchronous and has a fixed, known phase.
2. **The console still sees one clock.** `nes_top` is unchanged. The new domain
   ends at the cartridge's read port, which already had a one-cycle latency
   contract — the contract is preserved, the implementation behind it is not.
3. **Determinism is unaffected.** A synchronous integer-ratio crossing has no
   metastability and no arbitration ambiguity: the same input produces the same
   output on the same cycle, which is the property the determinism contract
   actually requires.

The second output joins the framework's clock group automatically. `sys_top.sdc`
matches `*|pll|pll_inst|altera_pll_i|*[*].*|divclk`, and the `[*]` wildcards the
output index — so an additional output of the SAME PLL is grouped with the
first, and the v2.6.6 defect (a PLL whose name matched no group, giving
-13.901 ns of slack while the compile reported success) cannot recur through
this change.

## Alternatives rejected

**Run the controller at `clk_sys`.** Rejected on the arithmetic above: seven
cycles into a four-cycle budget.

**A cache in front of SDRAM, keeping one clock.** This was the fallback named in
the v2.6.13 plan, and it is genuinely viable — pattern fetches are sequential and
predictable. Rejected for now because it trades a *timing* problem for a
*coherence* problem: a cache must be invalidated on every mapper bank switch, and
the mappers in scope switch banks from CPU writes that land mid-fetch. That is
exactly the class of state the SDRAM controller was kept simple to avoid. Kept as
the documented fallback if the 4x clock fails timing on hardware.

**One clock at 85.909088 MHz, with the master clock as a clock ENABLE.** This is
the obvious question -- the ratio is an exact integer, the console is already
built around enables (`ce` is one PPU dot, `cpu_ce` one CPU cycle), and it is the
standard MiSTer idiom. It would eliminate the second domain outright rather than
merely making the crossing safe, and it would also hand the arbiter the PPU's
fetch phase directly, which is the cleanest solution to scheduling CPU accesses
and refreshes around CHR fetches.

**Rejected, and the reason is measured rather than argued: the console cannot run
that fast.** Its Fmax at the **binding corner (Slow -40C) is 29.83 MHz**. The
master clock needs 21.477272 MHz, so there is 1.39x of headroom -- and
85.909088 MHz would require the console logic to be **2.88x faster than it is**.

*(A first draft of this ADR cited 36.19 MHz, which is the **Slow 100C** figure
`docs/rung6-integration.md` quotes as "the conventional panel". v2.6.7
established that Slow 100C is NOT the binding corner on this design and that a
gate reading it reported roughly three times the real margin. The same mistake,
in a document written two releases later. The conclusion is unchanged -- 2.88x
rather than 2.37x -- which is exactly why it was easy to make.)*

That is not a tuning gap. It is the difference between a design that closes with
+19.045 ns of setup slack on its own clock and one whose every path would have to
shrink by more than half. The enable idiom works for cores whose logic is fast
enough for the memory clock; this one's is not, and no amount of pipelining the
memory path changes that, because the constraint is on the CPU and PPU logic
rather than on anything the SDRAM touches.

**So two domains is forced, and the integer ratio is what makes it safe rather
than what makes it optional.** If the console's Fmax ever rises above the SDRAM
clock this becomes the better design and this ADR should be revisited -- which is
why the number is recorded here rather than left in a timing report.

**A higher multiple (6x, 8x).** No benefit: 4x already fits the budget, and every
additional multiple costs timing closure margin on a design that has twice needed
a fitter re-seed. 8x is also illegal -- 171.8 MHz exceeds the part's 143 MHz
maximum -- so the usable range is 4x or 6x, and 4x is the one that fits.

## Consequences

- **`rtl/pll.v` gains outputs.** Its header's claim that the console needs only
  one clock stays true of the CONSOLE and stops being true of the CORE; the file
  is updated to say which.
- **The one-clock property is now a statement about `nes_top`, not about `emu`.**
  Anyone reasoning from "this design has one clock" must check which of the two
  they mean. This is the kind of quiet scope change that becomes a wrong
  assumption three releases later, so it is written here rather than left in a
  commit message.
- **The phase shift is a hardware-tuned number and is NOT verified here.** A
  behavioural model has no signal integrity and no board; the shift that works is
  found on hardware, and MiSTer's HPS can retune it at runtime via
  `sdram_sz[14:8]`. The value shipped is a documented starting point, not a
  measurement. Rung 6.
- **Timing closure gets harder.** A second clock domain and a memory controller
  land on a design whose worst setup slack was +0.225 ns. If it does not close,
  the cache alternative above is the way back, not a faster clock.
