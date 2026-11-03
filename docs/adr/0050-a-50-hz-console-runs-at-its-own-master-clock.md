# 50. A 50 Hz console runs at its own master clock

Date: 2026-10-28

## Status

Accepted (v3.6.0). Directed by the maintainer on 2026-10-25: "we should
accomplish any clock changes necessary, so there are no slowdowns in PAL and
Dendy console modes". Designed and measured in the MiSTer sibling
(`feat/v3.6.0-pal`, commit `b7f6d7c`), from the Intel and MiSTer framework
documentation only; no third-party NES HDL was opened (ADR 0037).

**Amends [ADR 0039](0039-sdram-clock-domain.md)** in one respect.
ADR 0039 fixed the SDRAM clock at exactly 4x the master clock. That stays true
of NTSC; a PAL console or a Dendy runs the SDRAM at exactly **3x** its own
master clock. Everything else ADR 0039 argues -- a separate SDRAM domain, an
exact integer ratio from one PLL, a synchronous crossing with a fixed phase,
determinism untouched -- holds in both configurations. ADR 0039 is not edited;
this ADR records the amendment.

## Context

v3.6.0 (FB-7) gives the MiSTer core the 2A07 / 2C07 and the Dendy, selected by
a Region option. The first implementation kept the PLL at NTSC's 21.477272 MHz,
so a 50 Hz console would have run at 80.7 % speed on a board: 40.4 frames a
second, its audio 3.7 semitones flat. Both 50 Hz consoles need 26.6017125 MHz
(`nesdev_wiki/Cycle_reference_chart`; the Dendy is "26.60~ MHz / 15").

ADR 0039 says to revisit it "if the console's Fmax ever rises above the SDRAM
clock". The question here is the reverse one: whether the SDRAM's 4x can follow
the console to PAL's clock. 4x would be 106.4 MHz.

## Decision

1. **Two PLL configurations, switched at run time.** NTSC: 21.477272 /
   85.909088 MHz (4x). PAL and Dendy: 26.6017125 / 79.8051375 MHz (**3x**).
   The SDRAM pin clock is at 270 degrees in both. The PLL is a reconfigurable
   `altera_pll` whose parameters are Quartus's own generator output for each
   configuration; they differ only in M, its fraction K, and C0. C1 = C2 = 6 in
   both, because the pin clock's phase is a counter preset the reconfiguration
   port cannot rewrite, and a 50 Hz configuration with C1 = 5 is illegal (M =
   7.98, a fraction outside 0.05..0.95). NTSC's outputs keep their frequencies
   and phases; its VCO moves from 859 to 1031 MHz.
2. **The switch** (`rtl/pll_region.sv`, clocked by CLK_50M, never by a clock it
   rewrites): console held in reset; SDRAM into self refresh; the console and
   SDRAM-controller clocks stopped at clock-control blocks; M, K and C0 written
   through the framework's `sys/pll_cfg`; START; a PLL reset to re-establish
   the outputs' relative phase; 20 us of continuous lock; the clocks released;
   the SDRAM out of self refresh (tXSR honoured) and the console out of reset.
3. **On a 50 Hz console the SDRAM arbiter grants PRG before CHR.** At 3x each
   deadline, set in master clocks, has three quarters of NTSC's SDRAM cycles.
   With NTSC's order the CPU's read had one cycle to spare; with PRG first both
   consumers have four (measured).

## Alternatives rejected

- **SDRAM at 4x, 106.4 MHz.** CAS latency 2 is illegal at 9.4 ns (tCK at CL2 is
  at least 10 ns), tRCD, tRP, tRAS and tRFC each need another cycle, and the
  SDRAM-domain paths measured about 11 ns against 9.4. It needs a controller
  with a second timing set and a restructured address path while keeping NTSC
  bit for bit: the larger and riskier change.
- **One faster base clock with fractional clock enables.** Every register under
  `nes_top` runs on every master clock, so the enable would gate all of them
  and every M10K port, and NTSC's master ticks would jitter against the SDRAM
  slot schedule, which is built on exact 4 : 1 phases.
- **Leave 50 Hz at 80.7 %.** What the first implementation did; the
  maintainer's direction rules it out.

## Consequences

- **Measured deadlines at 3x** (off the die, the byte delayed in whole SDRAM
  cycles until a gate failed): PRG 21, CHR 24, on both 50 Hz consoles. The
  ladder's budget gate holds every 50 Hz run to CHR <= 24 and PRG <= 20.
- **Timing is analysed in both configurations** on the same netlist: the 50 Hz
  clocks are declared on the PLL's output nodes with `-add`, physically
  exclusive with the NTSC set, and the off-die PRG multicycle is per
  configuration (four SDRAM cycles on NTSC, two at 50 Hz). Seeds 1 and 3 close
  every corner of both builds; seed 2 misses by 0.472 ns off the die, in the
  framework's HDMI scaler clock rather than the core.
- **The core's clocks pause during a region switch**, and the PLL's output
  clock names in the timing reports are now `counter[N]`.
- **What only a board can establish:** that the reconfigured PLL locks where
  the generator says, that the clock-control blocks hold through the relock,
  that the framework's HPS handshake tolerates the paused core clock, that self
  refresh behaves on the real part, and that Main_MiSTer re-reads the video
  mode on `new_vmode`. No board has run it.
- **Not done:** a PAL `yc.txt` entry for composite and S-video. It would have
  NTSC's 2^40 / 6 shape, since 26.6017125 MHz is six times the PAL subcarrier.
