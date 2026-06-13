# Salvaged /tmp working artifacts (2026-06-08)

Experimental working artifacts rescued from the scattered `/tmp` project scratch-dirs
(`/tmp/RustyNES_v2`, `/tmp/rustynes-m2`, `/tmp/rustynes-research`) before reboot wipe, on a second
thorough sweep of the **whole** `/tmp` tree (not just `/tmp/RustyNES_v2`). These are **historical
records of refuted/superseded experiments** — kept for provenance, NOT maintained code. The canonical
state is the committed branches + the curated `docs/audit/v2.0-*.md` docs. The large regenerable trace
CSVs/TXTs and the `verify-wt` worktree were discarded.

## progress/
- `rustynes-m2-FLOOR.md`, `rustynes-m2-PROGRESS.md` — the Program-M counter-collapse agent's running
  notes (FLOOR baseline @ 88df82a + the per-iteration scan_dma_abort arrays). The conclusions are
  captured in `docs/audit/v2.0-m2-counter-collapse-2026-06-08.md` + the session handoff; this is the
  raw iteration log.

## patches/ (13 experimental diffs, mostly superseded)
- PPU sub-dot: `p_2004_ppu.patch`, `p2007.patch`, `ppu-clean{,-v2}.patch`, `ppu_cluster.patch`,
  `ppu-subpos-v2{,.valid}.patch` (the `ppu-subpos` line landed as the `mc-ppu-subpos` feature).
- DMA/UC lineage: `uc5-clean.patch`, `wf-dma-port.patch`.
- Snapshot/cumulative experiments: `ss_1c47da5.patch`, `ss_86471b8.patch`, `ss_a3dc910.patch`,
  `ss_cumulative.patch`.

## snapshots/ (transient source snapshots — likely superseded by the committed branches)
- `branch_ppu.rs` — a `nes-ppu/ppu.rs` experiment snapshot.
- `mc_cpu.rs` — a master-clock `nes-cpu/cpu.rs` experiment snapshot.
- `rustynes-research-bus.rs` — a `nes-core/bus.rs` research snapshot.

See also: `v2.0-status-report-2026-06-06-dma-tail-closure.md` (the from-main worktree's untracked status
report) + `v2.0-gemini-dma-tail-experiment-stash-2026-06-06.patch` (the recovered git stash), both
salvaged into `docs/audit/` alongside this folder.
