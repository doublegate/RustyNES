# TriCNES — vendored reference oracle (MIT)

TriCNES is the AccuracyCoin author's own emulator (Chris "100th_Coin" Siebert), which passes the full
144-test battery — the gold oracle for these tests. Re-synced 2026-09-11 to upstream `f388af0`
(previously `f54d8be`, 2026-05-05); that window carries the OAM2-address and OAM-evaluation fixes
matching AccuracyCoin's new `Advanced Sprite Evaluation` page, the 6502 internal-data-bus fix, and
Mapper 66 (GxROM). Vendored here under its **MIT License** (see
`tricnes-full-src/LICENSE`) as the per-cycle cross-diff oracle for the DMA-tail / Program-M work,
salvaged from `/tmp` so it survives reboot.

- **`tricnes-harness-src/`** — the trimmed, **instrumented** harness actually used for the cross-diff:
  `Emulator.cs` (with the per-cycle window logger), `Program.cs`, `6502Documentation.cs`, `mappers/`
  (all 11 `Mapper_*.cs`, required to build — the re-sync added `Mapper_GxROM.cs`), `tricnes-harness.csproj`. Build: `dotnet build -c Release`
  (.NET 10 SDK). The MIT license in `../tricnes-full-src/LICENSE` covers this trimmed copy too.
- **`tricnes-full-src/`** — the complete upstream TriCNES source (`.cs`/`.csproj`/`.resx` + `LICENSE`,
  no build artifacts), for reference / re-trimming the harness.
- **`implicit_abort_*_xdiff_*.txt` / `implicit_abort_region.txt`** — committed cross-diff outputs.

Upstream: `github.com/100thCoin/TriCNES`. Reverse-engineered model:
`docs/audit/v2.0-f2-tricnes-reference-model-2026-06-02.md`. Setup/regeneration:
`docs/tooling/oracle-tooling-setup.md` §2 / §2a.
