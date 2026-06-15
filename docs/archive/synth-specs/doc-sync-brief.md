# RustyNES doc-sync brief — current state (read this first)

Project: `/home/parobek/Code/OSS_Public-Projects/RustyNES/` (the PUBLIC repo). A synthesis just
landed: the old v0.8.6 parent emulation core was REPLACED with a transplanted, renamed copy of the
cycle-accurate "RustyNES_v2" v2.8.0 engine, the parent's polished UX was ported onto it, and the
result is being cut as **v1.0.0**. Your job: bring documentation/TODOs into agreement with this
reality (the new core AND the new UI/UX), fix version references, organize/synthesize, keep nested
README.md indexes current. You edit only your assigned subtree (named in your task prompt).

## Hard rules

- **NEVER** describe committing commercial Nintendo ROMs. The oracle uses gitignored
  `tests/roms/external/` (user-supplied) + committable CC0/public-domain ROMs only.
- **SKIP `docs/audit/` entirely** — it is gitignored (dev history, not part of the public repo).
  Do not edit anything under `docs/audit/`.
- **DO NOT touch `README.md` or `CHANGELOG.md`** at the repo root — reserved for a separate final pass.
- No emojis. Match each file's existing structure/voice. Read a file before editing it.
- Prefer synthesis over duplication: if two files cover the same topic, combine into one
  authoritative doc, keep all unique knowledge, drop repeated text. A file unique to one project
  carries as-is. Nothing silently dropped.

## Architecture now (crate-by-crate)

- Workspace crates: `rustynes-cpu`, `rustynes-ppu`, `rustynes-apu`, `rustynes-mappers`,
  `rustynes-core`, `rustynes-frontend`, plus `rustynes-netplay`, `rustynes-cheevos`,
  `rustynes-test-harness`. Binary: `rustynes`. (Old names were `nes-*` and `rustynes-desktop` — both
  obsolete; the eframe desktop crate was retired.)
- Frontend = winit 0.30 + wgpu + egui 0.29 + cpal, with a dedicated emulation thread (default
  `emu-thread` feature), a synchronous path (`--no-default-features`), and wasm (`wasm-winit` /
  `wasm-canvas`). NOT SDL2, NOT eframe/glow.
- Edition 2021, Rust 1.86 pinned. Dual-licensed MIT OR Apache-2.0.
- The PPU is the master clock (lockstep, PPU-dot resolution). Determinism is a hard contract.

## Capabilities at v1.0.0 (what to claim as done)

AccuracyCoin 100% (139/139), the 60-ROM + 52-entry commercial oracles byte-identical, nestest
0-diff; 51 mapper families; Famicom Disk System (real BIOS); Vs. System + PlayChoice-10 (RGB PPU);
netplay (GGPO-style rollback over UDP + browser WebRTC, 2-4 players); RetroAchievements (opt-in,
native-only); TAS movie record/playback; save-states; rewind; run-ahead (default 1); display-sync
pacing matrix + audio dynamic-rate-control; a dedicated emulation thread (best-effort Linux priority
elevation); an egui debugger (CPU/PPU/APU/memory/OAM/mapper panels); a WebAssembly / GitHub-Pages
playable demo. Lua scripting is the one advertised-but-unbuilt feature → post-1.0.

## The new UX shell (port of the parent's polish onto the v2 stack) — keep docs accurate to THIS

Always-on egui menu bar + status bar (toggle the bar with `M`):

- **File**: Open ROM, Open Recent (MRU, max 10, persisted) + Clear Recent, [disk-side cycle for FDS],
  Save State, Load State, Save Slot (0-9) / Save to Slot / Load from Slot, Take Screenshot, Quit.
- **Emulation**: Pause, Reset, Power Cycle, Frame Advance, Run-Ahead (0-3 submenu), Insert Coin (Vs).
- **Tools**: Cheats... (Game Genie + raw RAM), Movies (TAS) submenu (record/play/branch), Netplay...,
  RetroAchievements..., Performance Monitor.
- **View**: Settings... (tabbed window), Theme (Light/Dark/System submenu), Fullscreen, Window Size
  (1x-4x — scales only the GAME; the chrome stays a fixed readable size and the game letterboxes),
  [hide menu bar].
- **Debug**: Toggle Debugger (`~`), chip panels (CPU/PPU/APU/Memory/OAM/Mapper).
- **Help**: Keyboard Shortcuts (window), About (window — "Created by DoubleGate", a GitHub hyperlink,
  MIT OR Apache-2.0).
- Also: a first-run **Welcome** modal ("Get Started" / "Keyboard Shortcuts"), the **Settings window**
  (Display/Audio/Input/Advanced tabs), **themes** (light/dark/system), an **8:7 pixel-aspect**
  correction toggle, a **status bar** (ROM name / run state / fading messages / FPS), and an opt-in
  **Pause When Unfocused** option.

### Default keybindings (all rebindable; `[input.system]` in config.toml)

- Pause/Resume: **Space** · Reset: **F2** · Power Cycle: **F3** · Quit: **Escape**
- Save State: **F1** · Load State: **F4** · Rewind (hold): **F5**
- Open ROM: **F12** · Screenshot: (menu) · Toggle Debugger: **~** (Backquote) · Toggle Menu Bar: **M**
- Toggle Fullscreen: **F11** · Fast-Forward (hold, audio muted): **Tab** · Frame Advance (while paused): **Backslash**
- TAS Movie Record/Play/Branch: **F6 / F7 / F8** · FDS Disk Swap: **F9** · Vs. Insert Coin: **F10**
- Player 1 pad: Arrows = D-pad, **Z** = A, **X** = B, **Right Shift** = Select, **Enter** = Start
- Player 2: WASD = D-pad, **Q** = A, **E** = B, **L** = Select, **P** = Start
- Player 3: IJKL, **U** = A, **O** = B, **M** = Select, **Period** = Start  ·  Player 4: Numpad
- USB gamepads auto-bind (Xbox-style: South = A, West = B, D-Pad, Start, Back = Select); rebindable.

## Re-versioning rule (CRITICAL for every version reference you touch)

RustyNES's own release line is `v0.1.0…v0.8.6` (parent) → `v0.9.0…v0.9.7` (engine-lineage stages) →
`v1.0.0` (this synthesis). The inbound v2 docs are full of `v1.x`/`v2.x` engine release tags — those
are the ENGINE's prior lineage, NOT RustyNES releases. Reconcile every reference:

- A feature "added in v2.x" → reframe as a **v1.0.0** capability (or describe it untagged).
- Where deep accuracy-program history needs the old anchors, relabel them as **upstream engine
  lineage** ("developed across the engine's v2.0–v2.8 line; shipped here at v1.0.0") so it reads as
  history, not as this project's release numbers. ADR/audit/release-note FILES keep their dates.
- Map (use when a stage label helps): engine v1.0.0 → RustyNES **v0.9.0**; engine v1.1.0–v1.4.0 →
  **v0.9.1**; engine v1.5.0–v1.7.0 → **v0.9.2**; engine v2.0.0–v2.0.1 → **v0.9.3**; engine
  v2.1.0–v2.2.0 → **v0.9.4**; engine v2.3.0–v2.5.0 → **v0.9.5**; engine v2.6.0–v2.7.1 → **v0.9.6**;
  engine v2.8.0 → **v0.9.7**; the synthesis itself (parent UX shell + docs + production polish) =
  **v1.0.0**. The single shipped tag is **v1.0.0**.
- After editing, no file in your subtree should present `v2.x` as a RustyNES release, claim "current
  version 2.x", or carry a v2.x badge/feature-tag. Deliberately-retained "engine lineage" mentions
  are fine when clearly labelled as upstream history.

## Deliverable

Edit your assigned subtree to reflect all of the above. Keep nested `README.md` / index files
accurate to their folder's current contents. Report: files changed (with one-line why), any
files you moved/merged (from→to), any version refs fixed, and anything you found that another
subtree owns (so it can be handled) — do NOT edit outside your assigned subtree.
