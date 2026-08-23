# Alternative homes for the core: Retro Remake, openFPGA, and why they are planned rather than contingent

**Dated supplemental reference, 2026-08-23.** `ref-docs/` is immutable.

**Sources**, searched and fetched 2026-08-23: `retroremake.co` ·
`retrorgb.com/mister-superstation-one-review.html` ·
`analogue.co/developer/docs/overview` ·
`openfpga-library.github.io/analogue-pocket/` ·
`timeextension.com` (openFPGA core index) · `retrorgb.com` (Neo Geo core ported to
Analogue Pocket).

---

## Why this file exists

The prior plan ranked *"the core is declined as a duplicate"* as risk 5, and it is
the one risk this project cannot mitigate by working harder: `NES_MiSTer` exists,
is competent, and MiSTer discourages redundant cores. **The honest response is to
know the alternatives before submitting, not after being declined** — which also
means the RTL should not accumulate MiSTer-only assumptions it does not need to.

---

## 1. Retro Remake — SuperStation One

A Cyclone V console with **128 MB integrated BGA SDRAM**, from the makers of
MiSTer Pi, shipping through 2026. Marketed as a PS1-style machine that is
**fully compatible with MiSTer FPGA cores** — load a core and it runs.

**Three consequences, all favourable:**

1. **It removes a hardware prerequisite.** A DE10-Nano needs the SDRAM add-on for
   any NES core; the SS1 has it on the motherboard. Bring-up is cheaper here.
2. **Distribution reaches both.** Retro Remake forks `Distribution_MiSTer` and
   `Downloader_MiSTer`.
3. **It is a second home.** Retro Remake maintains its own public repositories and
   hosts cores itself, so a core declined by MiSTer-devel still reaches real users.

**The claim worth verifying rather than inheriting:** sources say SS1 runs MiSTer
cores "without modifications", but none confirms the *identical bitstream*
byte-for-byte. Rung 6 tests that one `.rbf` boots both boards. Any divergence is a
publishable finding, and that is a reason to own both.

## 2. Analogue Pocket — openFPGA

Analogue's openFPGA opens the Pocket's FPGA to third-party cores, with public
developer documentation. **MiSTer cores have a demonstrated porting path**:
Furrtek's Neo Geo core was ported by UltraFP64, and there are openFPGA ports of
the MiSTer ZX Spectrum core and others. There is an active core index
(`openfpga-library.github.io/analogue-pocket`).

**What a port would require, and why it is not free:**

- A different framework — openFPGA's own host interface, not `sys/`/`hps_io`.
- Different video and audio plumbing.
- A different (smaller) FPGA budget, so a core that only just closes timing on
  Cyclone V is not automatically portable.

**What survives a port unchanged:** `rtl/cpu6502.sv`, `rtl/ppu2c02.sv`,
`rtl/apu2a03.sv`, the cartridge/mapper modules — everything that is *the NES*
rather than *the platform*. **And the entire co-simulation apparatus**, which is
platform-independent by construction: it drives `nes_top`, never `emu`.

**Design rule this implies, and it costs nothing to follow:** keep every MiSTer
assumption inside `emu`/`sys/`-facing code, and keep `nes_top.sv` a plain
NES-shaped module with a cartridge interface, a video output and an audio output.
The testbench already forces this discipline, since it instantiates `nes_top`
directly.

## 3. MiSTeX

Searched; no authoritative current source surfaced in this pass. Recorded as
**unverified** rather than described from memory — a project this file cannot cite
should not appear in it as fact. Worth a second look before v2.7.0 if the primary
route is declined.

---

## 4. What this means for the plan

- **The alternatives are real, and one of them is already the hardware target.**
  That materially reduces risk 5 from "the work may be wasted" to "the work has at
  least two homes".
- **The portability rule above is free** — it is the module boundary the
  co-simulation harness already enforces — so it should be stated as a standing
  design constraint rather than a porting task.
- **The evidence apparatus retains value independently of any of them.** A
  per-cycle co-simulation record against a 141/141 emulator is publishable on its
  own terms, and is the deliverable that cannot be declined.
