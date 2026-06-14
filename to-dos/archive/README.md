# Archived to-do folders

These directories are kept for historical traceability only. They are **not**
live work plans — do not pick tickets from them.

| Folder | Why archived |
|---|---|
| `legacy-v0.8-todos/` | The original RustyNES (v0.1.0–v0.8.6) to-do tree, superseded when the emulation core was replaced for v1.0.0. |
| `phase-7-nesdev-accuracy-hardening/` | Engine-lineage accuracy plan. The work it targets (nesdev accuracy hardening) is **already accomplished** by the v1.0.0 cycle-accurate core — AccuracyCoin is **100% (139/139)**. Archived 2026-06-13. |
| `phase-8-v1.2.0-accuracy-residuals/` | Engine-lineage plan for "v1.2.0 accuracy residuals" + the "v2.0 master-clock refactor". The master clock is **already the only scheduler** in the v1.0.0 core and the accuracy program is complete, so these tracks are retired. Archived 2026-06-13. |

The remaining genuinely-open accuracy items (a handful of by-design `#[ignore]`
test probes) are documented in-place at their test sites as **permanent-by-design**
— per project policy they are documented, not ground on. See `docs/STATUS.md` and
`docs/compatibility.md` for the current, authoritative state.

For the live forward roadmap, see `to-dos/v1.0.1-compat-hygiene/` and
`to-dos/v1.1.0-features/`.
