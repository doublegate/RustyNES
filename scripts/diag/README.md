# scripts/diag/ — salvaged ad-hoc diagnostic scripts

Session-diagnostic Python/shell scripts salvaged from `/tmp` on 2026-06-08 (which is wiped on reboot)
so they survive for the resume of the DMA-tail / Program-M / sub-dot-PPU work. **These are ad-hoc,
not maintained tooling** — paths/filenames inside them may reference `/tmp` dumps that no longer exist;
treat them as starting points, not turnkey utilities. The maintained tooling is the harness binaries
(`scan_dma_abort`, `trace_dma_4015`) + `scripts/tricnes_xdiff.py`.

Grouped by what they compare:

- **Cross-diff / DMA-tail:** `xdiff.py`, `abort_drift.py`, `drift_harness.py`, `align.py`, `greedy.py`,
  `mem160.py`, `fix_mem.py` — RustyNES-vs-TriCNES per-cycle alignment + the abort `$540`/`$50-6F` sweeps.
- **Mesen comparison harnesses:** `mesen_test.py`, `mesen_ftest.py`, `findtest.py`.
- **$2002 / $2004 / $2007 sub-dot PPU:** `diff2002.py`, `ctx2002.py`, `diff2004.py`, `diff2004_new.py`,
  `cmp2007.py`, `cmp2007b.py`, `cmp2007c.py`, `cmp2007d.py`, `eval2007.py`.
- **General trace/cell/frame diff:** `an.py`, `an2.py`, `an3.py`, `cmp3.py`, `celldiff.py`,
  `framediff.py`, `sidebyside.py`, `RustyNES_v2_analyze_span.py`, `RustyNES_v2_sweep.py`, `floor.sh`.

See `docs/audit/v2.0-session-2026-06-08-handoff.md` and `docs/tooling/oracle-tooling-setup.md` §2a for
how these fit the cross-diff oracle workflow.
