# TriCNES cross-diff evidence (the source now lives outside the repository)

TriCNES is the AccuracyCoin author's own emulator (Chris "100th_Coin" Siebert, **MIT**), which
passes the full AccuracyCoin battery and served as the per-cycle cross-diff oracle for the
DMA-tail / Program-M work. Several of its models are ported into RustyNES and attributed in
`NOTICE` and `docs/originality-and-provenance.md` section 1.

**Moved out in v3.0.1 (maintainer decision, 2026-10-07).** The vendored source trees
(`tricnes-full-src/`, byte-identical to upstream `94f1b117`, and `tricnes-harness-src/`, that
source plus 88 instrumentation lines) were removed from the repository so no search over it
reaches reference-emulator source. They now live at:

- `~/reference-oracles/TriCNES`: a clone of `github.com/100thCoin/TriCNES` at `94f1b117`
  (checked file-for-file against the vendored copy before removal; upstream additionally carries
  its `.sln`, `icon.ico`, `SDL2.dll` and settings file, which were never vendored);
- `~/reference-oracles/TriCNES-rustynes-harness`: the instrumented harness, with `LICENSE`.
  Build: `dotnet build -c Release` (.NET 10 SDK).

On another machine, re-create them from upstream; the harness is in this repository's history
before the v3.0.1 removal commit. When and how the source may be consulted (AccuracyCoin work,
after rungs 1-3, always attributed) is `docs/ai-emulator-provenance-guardrails.md` section 3a.

What stays here is evidence, not source: the committed cross-diff outputs
(`implicit_abort_*_xdiff_*.txt`, `implicit_abort_region.txt`, `implicit_540_grid_xdiff_*.txt`).
Reverse-engineered model: `docs/audit/v2.0-f2-tricnes-reference-model-2026-06-02.md`. Tooling:
`docs/tooling/oracle-tooling-setup.md` sections 2 and 2a.
