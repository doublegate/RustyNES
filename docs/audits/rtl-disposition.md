# RTL audit — disposition ledger

Report: [`rtl-audit-report.md`](rtl-audit-report.md), covering
`RustyNES_MiSTer`. Verdict vocabulary and the closing procedure:
[`README.md`](README.md). Target release per
[ADR 0041](../adr/0041-hardware-release-is-v3.0.0.md).

**Read this report with more suspicion than the other three.** Its calibration
found two defects refuted outright and a resource table that does not match the
Quartus fit report. Every claim here is re-derived before it is acted on, and every
fix is written from public documentation and datasheets under the
[ADR 0037](../adr/0037-mister-fpga-core-independent-hdl-implementation.md)
firewall, with a co-simulation gate that fails first.

**Build context the report misses:** the shipped `.rbf` is the **on-die** build
(`rtl/emu.sv:715` `USE_SDRAM_CART = 1'b0`). `sdram` and `sdram_arbiter` are still
instantiated (`emu.sv:249,268`), but nothing in the cart path reads them. v3.0.0
ships the off-die build as a secondary (maintainer decision), which is what makes
the SDRAM rows load-bearing, in v2.8.4. The report's "DE10-Nano" target is
accurate: the build targets the DE10-Nano's 5CSEBA6U23I7 (`sys/sys.tcl`).

| Id | Finding (report §) | Verdict | Evidence | Release | PR |
| --- | --- | --- | --- | --- | --- |
| R-2.4 | Resource utilisation table (§2.4) | REFUTED | `output_files/RustyNES.fit.summary` (2026-09-20): 23,013 / 41,910 ALMs (55%), 468 / 553 RAM blocks (85%), 33 / 112 DSP (29%). The report says 21,865 ALMs, 394 M10K, 0 DSP | — | |
| R-3.1a | Spurious `last_cycle` at `tcyc == 0` for `AM_REL` (§3.1, Fix 11) | PARTIAL | `cpu6502.sv:1285-1286` matches the description, but the only consumer not gated by `ST_EXEC` is the `sh_rdy_low` clear (`:921`), which the SH instruction re-arms. No functional effect found. Fixed only if a gate can show one | v2.8.2 | |
| R-3.1b | Second NMI edge swallowed during NMI entry (§3.1) | PARTIAL | `cpu6502.sv:1860-1862` has no `!int_is_nmi` guard, as described; losing a second edge before the vector fetch may be hardware behaviour. Not a proven defect | v2.8.2 | |
| R-3.2a | `vblank_now` ignores `suppress_vbl` (§3.2, Fix 12) | UNTRIAGED | | v2.8.2 | |
| R-3.2b | `$2002` read latches register state, not the driven bus, into open bus (§3.2) | UNTRIAGED | | v2.8.2 | |
| R-3.3a | `audio_dc_block.sv` 32-bit overflow (§3.3, Fix 1) | REFUTED | `W = 32`, `FRAC = 15`, `mix` is 16-bit unsigned. Worst case `65535<<15 = 2^31-2^15` fits; for unipolar input the high-pass output stays within ±M·2^15 and the ±(32767<<15) clamp (`:120-123`) fires first | — | |
| R-3.3b | Pulse-1 sweep with negate not muted (§3.3, Fix 7) | REFUTED | `apu2a03.sv:230-231` gates overflow behind `!neg`, which is correct: `nesdev_wiki/APU_Sweep.xhtml` says a negative target clamps and negate is the documented way to disable the sweep. The RTL is right; see core ledger T-01 for the oracle side | — | |
| R-3.3c | Triangle / noise length reload overrides a coinciding decrement (§3.3) | UNTRIAGED | | v2.8.2 | |
| R-3.4a | CKE raised on the same edge as the first PRECHARGE (§3.4, Fix 2) | CONFIRMED | `sdram.sv:481-482` `sdram_cke <= 1'b1; cmd <= C_PRECHARGE;`. Off-die build only | v2.8.4 | |
| R-3.4b | DQM held high through the read wait, floating the bus at CL3 (§3.4, Fix 3) | REFUTED | `CAS_LATENCY` is 2 at `CLK_PS = 11640` (`sdram.sv:198`) and READ is issued with DQM low (`:666`); at CL2 that covers the data beat | — | |
| R-3.4c | Arbiter byte-lane select read from `*_hold[0]` at ack time (§3.4, Fix 4) | CONFIRMED | `sdram_arbiter.sv:353` slices on `chr_hold[0]` at ack; `chr_hold <= chr_addr` (`:317`) reloads on a new strobe, and a re-strobe in flight sets no overrun. Latent: the shipped cart path does not use it | v2.8.4 | |
| R-3.4d | Write handshake: `wr_busy` omits `wr_stb` (§3.4) | UNTRIAGED | Off-die build only | v2.8.4 | |
| R-3.5a | MMC1 bit-7 reset checked before the consecutive-write filter (§3.5, Fix 5) | CONFIRMED | `cart.sv:389` tests `cpu_din[7]` before `!mmc1_wrote_last_cycle`; the oracle filters first (`m001_mmc1.rs:315-321`). The report misstates the impact: "resets twice" is harmless; the case that matters is a bit7=0 write followed by a bit7=1 write | v2.8.2 | |
| R-3.5b | MMC3 `$E000` acknowledge vs counter decrement race (§3.5, Fix 6) | UNTRIAGED | Verify against nesdev before any fix | v2.8.2 | |
| R-3.5c | MMC3 Sharp reload-to-zero (§3.5) | UNTRIAGED | As R-3.5b | v2.8.2 | |
| R-3.5d | SNROM PRG-RAM `/CE2` protection missing (§3.5) | UNTRIAGED | | v2.8.2 | |
| R-4.1 | Timing table and per-domain slack figures (§4.1) | UNTRIAGED | The binding figures (+0.421 ns setup, +0.112 ns hold) match the pinned seed 4; the per-domain Fmax values and domain attribution are unverified | v2.8.3 | |
| R-4.2 | Runtime `%` bank dividers in `cart.sv` (§4.2, Fix 10) | NOT A DEFECT | Confirmed present (`cart.sv:679,681,698`; `lpm_divide:Mod0` in the fit report) and deliberate: documented, and timing closed by registering the crossing (`:760-775`). The "~480 ALMs" has no source. Revisited only on a measured need | — | |
| R-4.3 | `dec_rom[din]` 256:1 mux in the fetch path (§4.3) | UNTRIAGED | The "978 LEs" figure is to be measured in Quartus, not quoted | v2.8.3 | |
| R-4.4 | Redundant 16-bit `rmw_addr` adder (§4.4) | UNTRIAGED | As R-4.3 | v2.8.3 | |
| R-4.5 | Cascaded `inc_y(inc_x(v_pipeline))` at dot 256 (§4.5) | UNTRIAGED | Touches the v2.6.23 CHR fix path; the full ladder gates any change | v2.8.3 | |
| R-4.6 | No SDRAM I/O constraints in the SDC (§4.6, Fix 9) | CONFIRMED | `RustyNES.sdc` holds only `derive_pll_clocks` and `derive_clock_uncertainty`; `sys/sys_top.sdc` has no `SDRAM_` constraints, although `RustyNES.sdc:3-4` claims it does (that comment is corrected with the constraints, not before, for the reason in R-5.3). Constraints are provisional until the SS1's SDRAM part is identified at v2.9.2 | v2.8.4 | |
| R-5.1 | Unsynchronised reset into `nes_top` (§5.1, Fix 8) | CONFIRMED | `emu.sv:671-672` ORs asynchronous sources into `reset`, fed straight to `.rst_n(~reset)` (`:850`). The report's "sdram.sv not in the build" is refuted (see header) | v2.8.3 | |
| R-5.2 | Multi-bit CDC address sampling in `cart_sdram.sv` (§5.2) | UNTRIAGED | Off-die build only | v2.8.4 | |
| R-5.3 | PPU register access dropped under PAL parameters (§5.3) | NOT A DEFECT | PAL is not a supported build: `emu.sv:844-847` never overrides `CPU_DIV`/`PPU_DIV`, and `ppu2c02.sv:129` hardcodes NTSC. The real defect is the comment at `nes_top.sv:41` calling PAL "a parameter change". The sibling's `submission-case.md` made the same claim and was corrected in the re-plan; the RTL comment waits for v2.8.3, because any source edit after the compile makes the published `.rbf`'s timing report stale and `check_timing.py` then refuses a release attach | v2.8.3 | |
| R-5.4 | Tab indentation, `V_COPY_DOTS == 0` slice, `LEAD_CPU_CYCLES` comment (§5.4) | UNTRIAGED | | v2.8.3 | |
