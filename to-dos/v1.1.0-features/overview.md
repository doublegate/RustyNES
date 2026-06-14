# v1.1.0 — Feature Release (Lua flagship + four feature areas)

**Status:** PLANNED (begins after v1.0.1 ships)
**Type:** minor release — **additive only**.
**Hard gate (every beta + final):** determinism contract held (same seed + ROM +
input ⇒ bit-identical framebuffer + audio); AccuracyCoin 100% (139/139);
commercial-ROM oracles byte-identical; all CI gates green.

Anything that mutates emulator state (Lua writes, new cheat types) is gated OFF
during netplay / TAS replay / RA-hardcore — reuse the existing Game Genie /
raw-RAM cheat gating.

## Release train (staged betas; each independently shippable as a GitHub pre-release)

| Tag | Theme | Sprints |
|---|---|---|
| `v1.1.0-beta.1` | Look & Feel | A (visual filters) + B (input/peripherals + game DB) |
| `v1.1.0-beta.2` | Devtools & Audio | C (debugger devtools) + D (NSF player + EQ) |
| `v1.1.0-beta.3` / `-rc` | **Lua (flagship)** + stabilization | E (Lua scripting) |
| `v1.1.0` | Final | docs, CHANGELOG, binaries, Pages |

Cumulative on `main` via short-lived `feat/*` branches → PR → squash/FF.

## Sprints

| Sprint | Area | File |
|---|---|---|
| A | Visual polish & filters | `sprint-A-visual-filters.md` |
| B | Input, peripherals & QoL | `sprint-B-input-peripherals.md` |
| C | Debugger & devtools | `sprint-C-debugger-devtools.md` |
| D | Audio & NSF player | `sprint-D-audio-nsf.md` |
| E | Lua scripting (flagship) | `sprint-E-lua-flagship.md` |

## Do-not-rebuild (already in v1.0.0)

PPU nametable/pattern/palette + OAM viewers, APU scope, memory hex viewer, mapper
panel, cheats, perf panel — all in `crates/rustynes-frontend/src/debugger/*_panel.rs`.
v1.1.0 **extends** the debugger (breakpoints/trace/event); it does not re-add viewers.
Reuse: the 512-entry colour LUT, letterbox/overscan passes, settings-panel toggle
infra, the `InputDevice` enum, the mapper-registration match in
`crates/rustynes-mappers/src/lib.rs`, and `save_state`/`load_state`.

## Reference emulators (`ref-proj/`)

Mesen2 (accuracy + feature gold standard), tetanes (closest Rust peer), nestopia
(filters), fceux (Lua/assembler), puNES (palette/peripherals). Per-feature anchors
are in each sprint file.

## Out of scope (later / separate initiatives)

Vs. DualSystem two-CPU games; browser/wasm RetroAchievements; mobile (iOS/Android);
the long-tail march toward ~300 mappers; 100% TASVideos; browser Lua. The external
RA-allowlisting is a non-code request tracked separately.
