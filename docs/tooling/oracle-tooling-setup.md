# AccuracyCoin oracle tooling — setup + regeneration

> **⚠️ REFERENCE FIREWALL (read first).** The removed local reference-emulator clone (formerly under
> the gitignored reference-projects directory) is gone from the repo and the agent's reach — see the
> "MOST IMPORTANT RULE" section of `AGENTS.md` and `docs/ai-emulator-provenance-guardrails.md`. The
> firewall applies to the **copyleft** references — **Mesen2, puNES, FCEUX, GeraNES (GPL)**: those are
> **black-box oracles** whose *output* (per-cycle traces, framebuffers, audio) you may capture and diff
> against, but whose **source you must never open, read, or reproduce into RustyNES**, and any local
> build of them used for the oracle traces below **must live outside this repo and outside the agent's
> allowed paths** (a sibling directory the tool sandbox does not expose). The removed-clone paths that
> appear below are historical and no longer resolve.
>
> **TriCNES is the deliberate exception, and it is not a firewall violation.** TriCNES is **MIT**
> and written by the AccuracyCoin author; its ported models are attributed in `NOTICE` +
> `docs/originality-and-provenance.md` section 1. Its source was vendored in-repo until v3.0.1 and
> now lives **outside** the repository (maintainer decision, 2026-10-07): `~/reference-oracles/TriCNES`
> (upstream at `94f1b117`) and `~/reference-oracles/TriCNES-rustynes-harness`. It may be consulted
> for AccuracyCoin troubleshooting on the terms in `docs/ai-emulator-provenance-guardrails.md`
> section 3a. The committed golden vectors under `crates/rustynes-test-harness/golden/` (plus the
> AccuracyCoin sub-test ROMs) remain the preferred path because they need no live emulator at all.

The v2.0 accuracy push (toward a full pass) cross-diffs RustyNES's per-cycle bus stream against two
reference emulators. `/tmp` is wiped on reboot (CachyOS) — this is the recipe to regenerate.

## 1. Mesen2 unified per-cycle oracle (artifact-free cell trace)

Mesen2 working tree: an **out-of-tree** build outside the repo and the agent's allowed paths
(historically `ref-proj/Mesen2`, now removed — build/run it elsewhere and capture output only).

A patch adds an **artifact-free per-cycle channel** to `Core/NES/NesCpu.cpp`: globals `g_cellTrace`/
`g_cellTraceStart`/`g_cellTraceEnd` (~line 104), env init reading `MESEN_CELL_TRACE_OUT` +
`_START`/`_END` (~line 162, falls back to `MESEN_IRQ_TRACE_START/END`), header
`cpu_cycle,kind,addr,value`. Every `MemoryRead`/`MemoryWrite` logs one row **post-`StartCpuCycle`**;
DMA halt/dummy/align/get log `H`/`D`/`A`/`G` rows; the GET is logged **post-`EndCpuCycle`** so the
`cpu_cycle` is consistent with the other channels (the logging-artifact trap that produced the false
"GET 48 vs 47" / "span 3 vs 4" differences — every channel must log on the same half-cycle).

Build: `cd Mesen2 && make core -j16` → `bin/pgohelperlib.so` (loaded by
`PGOHelper/obj.linux-x64/pgohelper`).

Run a trace (window keyed on the `$4010=$4E` ROM landmark, NOT boot — RustyNES's hardware-correct
`$2002` vblank phase legitimately diverges the boot cycle-count from Mesen):

```bash
MESEN_CELL_TRACE_OUT=/tmp/m_cell.csv MESEN_CELL_TRACE_START=<cyc> MESEN_CELL_TRACE_END=<cyc> \
  scripts/mesen2-irq-oracle/run-irq-trace.sh tests/roms/accuracycoin/AccuracyCoin.nes /tmp/m_irq.csv
```

RustyNES side (same landmark): `trace_dma_4015` with `RUSTYNES_FULL_RANGE="lo,hi"` emits the
contiguous per-cycle stream; landmark-align both on the `$4010=$4E` write and diff (Python; the
DC-6 finding — RTS/stack dummy-read divergence at offsets 38-44 — is the validation that the
oracle is sound).

## 2. TriCNES — the gold oracle (AccuracyCoin author's own emulator)

TriCNES (Chris "100th_Coin" Siebert) passes the full 144-test battery → higher authority than Mesen
for these exact tests. Windows binary run **out-of-tree** (historically `ref-proj/TriCNES/.../TriCNES.exe`,
now removed; obtain from `TriCNES_v1.0.1.zip`, upstream `github.com/100thCoin/TriCNES`) — run it
outside the repo and capture its output only.

Runs under `wine` (`/usr/bin/wine`) as a live ground-truth oracle for observable behavior
(screen/result bytes). For the *model* (the "why"), use the reverse-engineered docs:

Cited by SYMBOL rather than line number on purpose: the 2026-09 re-sync moved `Emulator.cs` by
+912/-576 lines, which silently invalidated the `Emulator.cs:920` / `:4225-4322` citations that
stood here. A grep target survives a re-vendor; a line number does not.

- `docs/audit/v2.0-f2-tricnes-reference-model-2026-06-02.md` — the DMA core: per-cycle interleaved
  DMA (`_6502()` once per CPU cycle), ONE `APU_PutCycle` flip-flop toggled once per cycle
  (grep `APU_PutCycle = !APU_PutCycle`), the GET/PUT priority + halt-clear table (grep `DoDMCDMA` /
  `CannotRunDMCDMARightNow`), the port
  checklist, answer keys (`$0477` DMC+OAM `04 03 04 03 04 03 02 01…`).
- `ref-docs/tricnes-vs-rustynes-accuracy-roadmap-2026-06-02.md` — the roadmap.

### 2a. In-repo buildable instrumented harness (the per-cycle cross-diff oracle)

The cross-diff oracle used for the DMA-tail / Program-M work is a **trimmed, instrumented TriCNES
built from source** (TriCNES is MIT — Chris Siebert 2025). It was vendored in this repo until
v3.0.1 and now lives outside it:

- `~/reference-oracles/TriCNES-rustynes-harness/` — the buildable harness: `Emulator.cs`
  (instrumented with the per-cycle window logger; 88 lines against upstream), `Program.cs`,
  `6502Documentation.cs`, `mappers/` (all 11 `Mapper_*.cs`, **required to build**),
  `tricnes-harness.csproj`, `LICENSE`. Build with `dotnet build -c Release` (.NET 10 SDK). On a
  machine without it, restore the tree from this repository's history before the v3.0.1 removal.
- `~/reference-oracles/TriCNES/` — a clone of `github.com/100thCoin/TriCNES` at `94f1b117`, for
  reference and re-trimming.
- Cross-diff driver: `scripts/tricnes_xdiff.py` (+ the ad-hoc helpers under `scripts/diag/`, salvaged
  session diagnostics — see `scripts/diag/README.md`). RustyNES side: `scan_dma_abort`, `trace_dma_4015`.
- Individual AccuracyCoin sub-test ROMs (MIT, distinct builds) under
  `tests/roms/AccuracyCoin/sub-tests/` — incl. `iflag-latency.nes`, `dma-open-bus.nes`,
  `dmc-bus-conflicts.nes`, `internal-data-bus.nes`, `fc-4step.nes` (added 2026-06-08).

> **Reference-emulator note (updated 2026-08-04 — firewall):** the removed local reference-emulator
> clone has been **deleted entirely** and must not be re-created inside the working tree (it stays
> gitignored). The out-of-tree rule is for the **copyleft** references: if a **Mesen2** build (or
> puNES / FCEUX / GeraNES) is genuinely needed to *regenerate* an oracle trace, keep it **out of tree,
> outside the agent's allowed paths** — build and run it there, capture only its **output**, and diff.
> **TriCNES is MIT and is the deliberate exception:** a TriCNES trace is regenerated from the
> harness at `~/reference-oracles/TriCNES-rustynes-harness` (§2a), out of the repository since
> v3.0.1. The committed golden vectors above make the
> cross-diff oracle self-contained without *any* live emulator, which is the preferred path.

## 3. PPU sub-dot oracles (Phase 6)

`phantom2c02` / `Visual2C02` (transistor-faithful 2C02) for BG Serial In + `$2007` Stress — validate
`bg_toggle` / `read2007_v2`. Regenerable per the upstream repos.

## Anti-fabrication rule

Transcribe pass/fail ONLY from the harness RAM-direct decoder
(`crates/rustynes-test-harness/tests/accuracycoin.rs`) or read-back trace files — never from memory of a
prior run. Pin every cross-emulator divergence with the unified per-cycle oracle (cross-diff, not
distributional histograms) before acting on it.
