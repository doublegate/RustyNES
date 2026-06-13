mem="/home/parobek/.claude/projects/-home-parobek-Code-Commercial-Private-Projects-RustyNES-v2/memory/project_r4_oam_dma_oracle.md"
txt=open(mem).read()
marker="## §159 — FINAL (2026-05-30 session 5 end): 5th test is the r4-DMA cycle-position residual"
if marker not in txt:
    addition = """

---

""" + marker + """

DEFINITIVE, full-battery, HARNESS-decoder facts (supersede ALL of §158's
sprite-0-hit / M2-skew / BG-pipeline analysis above — those were measured on the
STANDALONE sub-test ROM, a RED HERRING that can't run the test without the battery's
setup):

- `$2002 flag timing` ($048D) PASSES on legacy/default build (RAM-direct 79.14%;
  catalog `failing_tests` does NOT list it). "Sprite 0 Hit behavior" ($0457) PASSES
  too. So sprite-0-hit + the 086ce4d BG-pipeline fix (CONFIRMED in branch: single
  post-emit `self.shift_bg()` at ppu.rs:1360, phase-0 reload at ppu.rs:1279) work
  correctly in the real battery. There is NO sprite-0-hit bug and NO M2-skew bug.
- `$2002 flag timing` ($0048D) FAILS ONLY under r4 (78.42%; catalog failing_tests
  `[error 1]`; value 0x06). The legacy↔r4 failing-set diff = EXACTLY this one test
  (everything else r4 fails — SH*/DMA/Stale-shift cluster — legacy ALSO fails; those
  are pre-existing shared residuals, NOT r4 regressions).
- Legacy and r4 share the IDENTICAL atomic $2002 read code (cpu_read_register reg2,
  bus.rs:3220). Since legacy's atomic read PASSES $048D, the test does NOT require
  the M2 sub-cycle skew, and the r4 failure is NOT a $2002-read or sprite-0-hit
  issue. It is r4's CPU-driven DMA re-phasing the CUMULATIVE timing of the post-DMA
  $2002 sample — the cycle-stream-position residual that the §157 OAM-span get/put
  parity fix did NOT close. Same axis as the original 4 §151 regressions + §149-153.

TOOLING TRAP confirmed: the curated Cascade-A diagnostic block in accuracycoin.rs
prints `0x048D = 0x01 PASS` even on the r4 build, CONTRADICTING the authoritative
catalog `failing_tests` (which correctly lists $048D [error 1] under r4). The
hardcoded $048D curated probe is address-aliased/stale — ALWAYS trust
accuracy_coin_catalog `failing_tests`, never the curated hardcoded reads.

STATUS: §157 fix (4 of 5, 75.54%->78.42%) shipped + solid. r4 is exactly 1 test
($2002 flag timing) below legacy's 79.14%. This residual is genuine r4-DMA-timing
work, DEFERRED to a focused session. Session-5 deliverables (all committed+pushed,
HEAD==origin 6c9837d on refactor/v2.0-master-clock): d35c50f (the §157 fix +
diagnostics), 6e7f531 / 41bd39b / 6c9837d (research + redirect + correction docs).

NEXT FOCUSED SESSION (the real residual): trace the FULL-BATTERY $2002-flag-timing
window (NOT the standalone sub-test) under legacy vs r4 via ppu2002_trace keyed by
master_clock; pin the residual cumulative-cycle delta at the test's tight bracket;
reconcile r4's process_pending_dma cycle accounting to legacy's drain_dma at that
point. Guardrails: HARNESS failing_tests for pass/fail; feedback_adr0002_ask_before_rollback;
r4 is opt-in so default stays safe, but re-confirm 60-ROM oracle + sacred trio if
touching shared start_cycle/end_cycle.
"""
    open(mem,"w").write(txt+addition)
    open("/tmp/RustyNES/fix_mem_result.txt","w").write("APPENDED ok, new len="+str(len(txt+addition)))
else:
    open("/tmp/RustyNES/fix_mem_result.txt","w").write("ALREADY PRESENT, no change")
