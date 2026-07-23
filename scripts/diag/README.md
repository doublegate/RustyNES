# scripts/diag/ — salvaged ad-hoc diagnostic scripts

Session-diagnostic Python scripts salvaged from `/tmp` on 2026-06-08 (which is wiped on reboot)
so they survive for the resume of the DMA-tail / Program-M / sub-dot-PPU work. **These are ad-hoc,
not maintained tooling** — paths/filenames inside them may reference `/tmp` dumps that no longer exist;
treat them as starting points, not turnkey utilities. The maintained tooling is the harness binaries
(`scan_dma_abort`, `trace_dma_4015`) + `scripts/tricnes_xdiff.py`.

The filenames say what each script does. They previously did not — `an.py`, `an2.py`, `an3.py`,
`cmp3.py`, `cmp2007b/c/d.py`, `diff2004_new.py`, `greedy.py` and `parse.py` were renamed to describe
their subject and computation, since an archive nobody can navigate is not an archive.

## PPU `$2002` (PPUSTATUS) read traces

Consume a `$2002` read trace CSV (`cpu_cycle,master_clock,scanline,dot,value,...`).

| Script | What it computes |
|---|---|
| `ppu2002_read_value_histogram.py` | Value histogram, plus every read near the pre-render boundary carrying a V/S/O flag bit. |
| `ppu2002_isolated_exact_timed_reads.py` | Finds the clockslide-isolated "exact timing" reads by their ~29,550-cycle neighbour gap. |
| `ppu2002_prerender_vbl_reads.py` | Pre-render (scanline 261) reads with VBL set, plus whole-run sprite-0-hit / overflow tallies. |
| `ppu2002_read_context_window.py` | Dumps the surrounding rows for each read, for eyeballing context. |
| `ppu2002_diff_traces.py` | Diffs two `$2002` traces against each other. |

## PPU `$2004` (OAMDATA) vs the AccuracyCoin AnswerKey

| Script | What it computes |
|---|---|
| `ppu2004_diff_vs_answerkey.py` | Per-dot diff of `reg_2004stress.csv` against the 341-byte `AnswerKey1`. **Carries the canonical key** — `ppu2004_mesen_vs_rustynes_scanline128.py` parses it back out of this file. |
| `ppu2004_diff_vs_answerkey_rerun_capture.py` | Byte-for-byte the same script, reading `reg_2004stress_new.csv` instead. The duplication is how it was captured; it is preserved rather than parameterised. |
| `ppu2004_mesen_vs_rustynes_scanline128.py` | Three-way Mesen vs RustyNES vs AnswerKey comparison, restricted to scanline 128. |

## PPU `$2007` (PPUDATA) read-buffer / fetch-bus studies

Reference data: `key2007.txt`.

| Script | What it computes |
|---|---|
| `ppu2007_key_alignment_shift_scan.py` | Scores every candidate shift of the captured data against the key, to find the alignment. |
| `ppu2007_fetch_bus_dump.py` | Reconstructs and prints `fetch_bus[dot]` beside the key's first tiles. |
| `ppu2007_phase_offset_ranking.py` | Ranks sampling offsets per fetch phase; dumps phase-5 (pattern-low) expected vs held. |
| `ppu2007_pattern_value_sets.py` | Compares the set of distinct pattern bytes our data drives against the key's set. |
| `ppu2007_stress_per_index_evaluator.py` | Per-index `$2007` Stress evaluator. |
| `ppu2007_offset_greedy_search.py` | Greedy search over `RUSTYNES_2007_OFFSET` values, re-running `scan_dma_abort` per candidate. |

## Cross-diff / DMA-tail (RustyNES vs TriCNES)

`xdiff.py`, `abort_drift.py`, `drift_harness.py`, `align.py`, `mem160.py`, `fix_mem.py` —
per-cycle alignment plus the abort `$540` / `$50-6F` sweeps.
`dma_loop_span_analyzer.py` and `dma_sweep_analyzer.py` analyse the DMA-loop and DMA-sweep CSV dumps
(previously named `RustyNES_v2_*`, which referred to upstream engine lineage rather than this repo).

## Mesen comparison harnesses

`mesen_test.py`, `mesen_ftest.py`, `locate_first_trace_divergence.py`.

## General trace / cell / frame diff

`celldiff.py`, `framediff.py`, `sidebyside.py`, `dump_trace_csv.py`.

## Patch helpers

`patch_bus.py`, `patch_delay.py`, `patch_trailing.py`.

See `docs/audit/v2.0-session-2026-06-08-handoff.md` and `docs/tooling/oracle-tooling-setup.md` §2a for
how these fit the cross-diff oracle workflow.
