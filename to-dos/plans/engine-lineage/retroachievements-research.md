# RetroAchievements Integration — Research Findings (read-only)

Research-only; no plan-to-build yet. All cites verified against the tree.

## 1. Memory access (READY)
- `Nes::cpu_bus_peek(&mut self, addr:u16)->u8` — nes.rs:979 -> `bus.debug_peek_cpu` (bus.rs:817).
  Side-effect-free, full $0000-$FFFF, NO open-bus/VBL/PPUDATA mutation. IDEAL for RA per-frame reads.
- $0000-$1FFF system RAM: bus.rs:819. $6000-$7FFF WRAM: routes via mapper cpu_read (nrom.rs:127, mmc1.rs:234) — idempotent for prg_ram.
- `Bus::ram_bytes()->&[u8]` bus.rs:1162 (2KiB direct). `Nes::poke_ram` nes.rs:510 (write $0000-$1FFF only).
- Takes &mut self (mapper cpu_read is &mut) but no emulator-visible state advances.

## 2. Frame hook (READY)
- `Nes::run_frame()` nes.rs:202. Frontend call: app.rs:1206 inside `produce_one_frame` (app.rs:1163).
- Hook slot: AFTER run_frame (1206) + AFTER raw cheats loop (1215-1220). Mirror movie.before_frame (app.rs:1191) / netplay (1170-1174).
- Overlay: debugger/mod.rs DebuggerOverlay::ui (toolbar HUD at 290-385: REC/PLAY/Disk/NET labels = toast pattern). render() at 431. Panels: panel::show(ctx,&mut show,&mut state,nes) pattern (387-425). netplay_panel.rs:272 = TextEdit::singleline + ui.button (login dialog pattern).
- Config: config.rs Config struct (530) w/ #[serde(default)] sections; ProjectDirs config_dir (568) / data_dir (575). Add [retroachievements] section for token.

## 3. Hashing (PARTIAL — needs MD5 + raw-ROM-after-header)
- Existing: sha256_of(FULL bytes incl header) nes.rs:78,1066; `rom_sha256()` nes.rs:557. Wrong for RA (RA = MD5 of PRG+CHR AFTER 16-byte header).
- Cartridge retains prg_rom + chr_rom separately (cartridge.rs:257-259) on bus as `cart` (bus.rs:112, pub(crate) — NOT public). Concatenating prg_rom+chr_rom = RA-hashable bytes.
- MISSING: no md5/md-5 crate in Cargo.lock; no public accessor to prg_rom+chr_rom; raw post-header bytes not retained on Nes (only full bytes hashed then dropped). FDS hash differs (RA uses whole FDS file).

## 4. Networking (MISSING http client)
- NO reqwest/ureq/hyper/ehttp/isahc anywhere. sha2 present; NO md5. tokio/tokio-tungstenite ONLY behind nes-netplay `signaling-server` feature (optional, examples/signaling_server.rs). Frontend has zero HTTP.
- netplay uses std::net::UdpSocket (mesh_net.rs). wasm32: std::net absent (must use web-sys fetch). `#![forbid(unsafe_code)]` only in nes-netplay/src/lib.rs (NOT nes-core/nes-frontend).
- Need: add ureq/reqwest (native) + web-sys fetch (wasm) + md5.

## 5. Hardcore disable points (READY)
- Save: handle_save_state app.rs:730 (Nes::snapshot 578). Load: handle_load_state app.rs:746 (Nes::restore 693). Dispatch: app.rs:1901 (SysAction::SaveState), :1911 (LoadState).
- Rewind: SysAction::Rewind app.rs:1920 (no-op; flag toggled in InputState::handle_key); actual step app.rs:1179-1182 rewind_step_back; Nes::enable/disable_rewind nes.rs:731/741.
- Cheats: Nes::add_genie_code 525, poke_ram 510; frontend raw_cheats loop app.rs:1215. Hardcore must gate all these.

## 6. Reference material (NONE for RA)
- No ref-proj/ dir. ref-docs/ = nesdev hardware reports only. nesdev_wiki/ (gitignored HTML) has CPU_memory_map.xhtml + Sample_RAM_map.xhtml (generic, not RA). ZERO retroachievements/rcheevos refs in repo.

## 7. State persistence (READY)
- RA runtime state (unlocked set, session) -> new file under Config::default_data_dir() (config.rs:575) alongside save-state slots / fds-saves. Token in [retroachievements] config section (config.rs:530 + ProjectDirs config_dir).

## GAPS to add: HTTP client, MD5 crate, public prg_rom+chr_rom (or raw-after-header retention) accessor for RA hash, hardcore gating wiring, wasm fetch path.
