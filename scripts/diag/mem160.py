mem="/home/parobek/.claude/projects/-home-parobek-Code-Commercial-Private-Projects-RustyNES-v2/memory/project_r4_oam_dma_oracle.md"
txt=open(mem).read()
m="## §160 — r4 $2002 residual: CUMULATIVE DMA DRIFT confirmed (2026-05-30 session 6)"
if m not in txt:
    add="""

---

""" + m + """

Root-cause progress on the last r4 §151 regression ($2002 flag timing, $048D).
Added `crates/nes-test-harness/src/bin/trace_2002_window.rs` (drives full battery,
finds the $048D result-byte transition cycle, windows the ppu2002 $2002 trace).

SOLID MEASURED FACT (transcribed from the bin output, not invented): the $048D
result byte transitions at **legacy frame 3217 / cyc 105,778,724 (PASS 0x01)** vs
**r4 frame 3174 / cyc 104,530,002 (FAIL 0x06)** — r4 reaches the test ~43 frames /
~1.25M CPU cycles EARLIER. So real CUMULATIVE timing drift between r4
process_pending_dma and legacy drain_dma REMAINS even after §157 equalized the
per-OAM-DMA span (513==513). The drift shifts the CPU/PPU sub-cycle phase so the
test's dot-exact $2002 measurement read (Sync_ToLine0Dot1 + ReadFrom2002WithExactTiming,
1-dot bracket on pre-render dot-1 flag clear) lands on the wrong side → fail. NOT
the M2 skew (legacy's identical atomic read passes), NOT sprite-0-hit (passes in
the real battery). Refines §159.

NOT YET PINNED (honest): the precise per-DMA mechanism (which DMA class, how many
dots). The ppu2002_trace was UNRELIABLE in this window — its master_clock column
logs the DEAD self.master_clock (frozen garbage 1503712392 in default/r4 builds;
only master-clock-scheduler writes it), and its dot column showed values >340
(invalid; 0-340 range). I did NOT draw a "+N dot" conclusion from that
(fabrication-avoidance). EARLIER THIS SESSION I DID make that error — wrote a
"1-dot offset at cyc 2,367,028" claim from a trace window aimed at the WRONG range
(2.2M, nowhere near the frame-3200 test, because I guessed the window instead of
running pass-1 first); deleted that draft. LESSON: always run the transition-finder
pass FIRST to get the real cycle, then window there.

NEXT (focused session): (1) FIX ppu2002_trace::row to log self.ppu_clock (not the
dead self.master_clock) + verify the self.ppu.dot() capture point (>340 = read at a
transient). (2) reg_read_trace (logs ppu_clock+fc_seq, both builds) over the whole
battery → find the FIRST frame r4's per-frame cycle count starts leading legacy's,
bisect to the DMA class. (3) dma_trigger_trace (RUSTYNES_DMA_TRIG_TRACE_CSV) +
r4 dma_loop_trace cross-diff to attribute the per-DMA cycle/dot delta. Fix candidate
= process_pending_dma / start_cycle+end_cycle run_ppu_to accounting vs legacy
drain_dma's direct tick_one_cpu_cycle per DMA cycle. HIGH blast radius, r4-gated,
validate (HARNESS r4>=79.14% + $048D flips + 60-ROM oracle + sacred trio + units)
and ASK before landing.

COMMITTED+PUSHED this session (HEAD==origin on refactor/v2.0-master-clock):
docs/audit/2002-flag-timing-r4-dma-rootcause-2026-05-30.md + trace_2002_window.rs.
Plus the earlier session commits d35c50f(§157 fix) + 6e7f531/41bd39b/6c9837d(docs).
Default build clean; only unrelated README.md M remains in the tree.
"""
    open(mem,"w").write(txt+add)
    print("APPENDED")
else:
    print("ALREADY PRESENT")
