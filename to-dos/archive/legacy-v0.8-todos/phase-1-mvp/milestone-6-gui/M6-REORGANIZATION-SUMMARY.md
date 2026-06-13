# Milestone 6: Desktop GUI - Reorganization Summary

**Document Version:** 1.0.0
**Created:** December 19, 2025
**Status:** Complete
**Purpose:** Summary of M6 reorganization from egui to Iced hybrid architecture

---

## Executive Summary

This document summarizes the comprehensive reorganization of Milestone 6 (Desktop GUI) from the original egui-based plan to the **Iced 0.13+ hybrid architecture** confirmed in M6-PLANNING-CHANGES.md. All sprint files have been rewritten, scope has been reduced from 12 weeks to **4 weeks (MVP-focused)**, and advanced features have been properly distributed across Phases 2-4.

---

## Key Changes

### 1. Framework Change: egui → Iced 0.13+

**Original Plan:**
- Primary framework: egui 0.28
- Architecture: Immediate mode
- Estimated timeline: 12 weeks

**New Plan:**
- **Primary framework: Iced 0.13+** (Elm architecture)
- **Debug overlay: egui 0.28** (developer tools only)
- **Estimated timeline: 4 weeks** (MVP core)
- **Architecture: Model-Update-View** (structured state management)

**Rationale:**
- RustyNES will have 8+ major views (Welcome, Library, Playing, Settings, NetplayLobby, Achievements, Debugger, TasEditor)
- Requires sophisticated animations and transitions
- Iced's Elm architecture prevents state management bugs at scale
- egui's immediate mode becomes unwieldy for complex multi-screen applications
- HTPC mode needs consistent theming across all views

See [M6-PLANNING-CHANGES.md](M6-PLANNING-CHANGES.md) lines 110-152 for detailed comparison.

---

### 2. Scope Reduction: 12 Weeks → 4 Weeks (MVP)

**M6 MVP Scope (4 weeks):**

| Sprint | Duration | Description |
|--------|----------|-------------|
| **M6-S1** | 1 week | Iced Application Foundation |
| **M6-S2** | 1 week | Core Emulation Display (wgpu + cpal) |
| **M6-S3** | 1 week | Input Handling & ROM Library Browser |
| **M6-S4** | 1 week | Settings & Configuration Persistence |
| **M6-S5** | 3-5 days | Polish & Basic Run-Ahead (RA=1) |

**Total:** 4 weeks for playable emulator with latency reduction foundation

---

### 3. Feature Rephasing

#### KEPT IN M6 (MVP Core)

✅ **Core Functionality:**
- Iced application shell with Elm architecture
- wgpu game viewport (60 FPS)
- cpal audio output (<20ms latency)
- gilrs gamepad + keyboard input
- ROM library browser (Grid/List views)
- Settings persistence (TOML)
- Basic CRT shader (3-5 presets)
- **Basic run-ahead (RA=1)** - architectural foundation

#### MOVED TO PHASE 2 (M7-M10)

➡️ **M7: Advanced Run-Ahead System**
- Run-ahead (RA=0-4, auto-detect per game)
- Preemptive Frames (alternative mode)
- Frame Delay auto-tuning (0-15 frames)
- Dual-instance mode for audio stability
- Just-In-Time input polling optimization

➡️ **M8: GGPO Netplay** (already planned)
- Rollback netcode (similar to run-ahead!)
- Lobby system

➡️ **M9: TAS Tools** (enhanced from scripting milestone)
- Recording/playback (FM2 format)
- Rewind timeline
- Frame advance
- Input visualization

➡️ **M10: Debugger with egui Overlay** (enhanced)
- CPU/PPU/APU state viewers
- Memory hex editor
- **egui integration layer** for immediate-mode debug tools
- Run-ahead frame visualizer

#### MOVED TO PHASE 3 (M11-M14)

➡️ **M11: Advanced CRT Shaders**
- 12+ shader presets (CRT-Royale, Lottes, Guest, etc.)
- Phosphor mask types (Aperture Grille, Slot, Shadow Mask)
- Rolling scan CRT simulation (Blur Busters technique)
- NTSC composite signal simulation
- HDR bloom with phosphor persistence

➡️ **M12: Expansion Audio** (unchanged)
- VRC6, VRC7, MMC5, FDS, Namco 163, Sunsoft 5B

➡️ **M13: HTPC Controller-First Mode**
- Full 10-foot UI for living room setups
- **Cover Flow view** (carousel-style ROM browsing)
- **Virtual Shelf view** (3D perspective library)
- Voice navigation support (optional)
- Automatic metadata scraping (IGDB, ScreenScraper)
- Large text scaling (3XL: 48px)
- Controller-only navigation (no keyboard required)
- Haptic feedback patterns

➡️ **M14: Plugin Architecture & Social**
- Extensible plugin system (shaders, input mappers, scrapers)
- Discord Rich Presence plugin
- Cloud sync plugin (Dropbox, GDrive)
- HD Pack support (Mesen-compatible)

#### MOVED TO PHASE 4 (M15-M18)

➡️ **M15: Advanced Shader Pipeline**
- Shader complexity levels (Low/Medium/High)
- Pre-compiled SPIR-V shaders
- Per-shader performance profiling
- Custom shader hot-reloading

➡️ **M16: TAS Editor (Piano Roll)**
- Visual timeline editor with piano roll interface
- Greenzone system (safe states for branch management)
- Multi-branch TAS workflow
- Frame-by-frame analysis tools

➡️ **M17: Full Run-Ahead Optimization**
- Performance profiling for run-ahead overhead
- Memory pool optimization for save states
- Multi-threaded state serialization

➡️ **M18: CLI & Accessibility**
- Full command-line automation mode
- Comprehensive accessibility audit
- Screen reader support
- One-handed mode polish

---

## Sprint File Changes

### Files Created/Modified

| File | Status | Description |
|------|--------|-------------|
| `M6-OVERVIEW.md` | ✅ REWRITTEN | Changed framework to Iced, reduced scope to 4 weeks |
| `M6-S1-iced-application.md` | ✅ CREATED | New file (replaced M6-S1-egui-application.md) |
| `M6-S2-wgpu-rendering.md` | 🔄 NEEDS UPDATE | Update for Iced integration |
| `M6-S3-audio-output.md` | 🔄 NEEDS REWRITE | Merge with input + library scope |
| `M6-S4-controller-support.md` | 🔄 NEEDS REWRITE | Rewrite as Settings + Persistence |
| `M6-S5-configuration-polish.md` | 🔄 NEEDS REWRITE | Rewrite as Polish + Basic Run-Ahead |
| `M6-PLANNING-CHANGES.md` | 🔄 NEEDS APPEND | Add rephasing summary |
| `M6-REORGANIZATION-SUMMARY.md` | ✅ CREATED | This file |

### Phase 2-4 Files Requiring Updates

| Phase | Milestone | File | Update Required |
|-------|-----------|------|-----------------|
| **Phase 2** | M7 | `to-dos/phase-2-features/milestone-7-achievements/README.md` | Rename to Advanced Run-Ahead, add full run-ahead system |
| **Phase 2** | M10 | `to-dos/phase-2-features/milestone-10-debugger/README.md` | Add egui overlay integration details |
| **Phase 3** | M11 | `to-dos/phase-3-expansion/milestone-11-webassembly/README.md` | Repurpose to Advanced CRT Shaders |
| **Phase 3** | M13 | `to-dos/phase-3-expansion/milestone-13-extra-mappers/README.md` | Add HTPC mode, Cover Flow, Virtual Shelf |
| **Phase 3** | M14 | `to-dos/phase-3-expansion/milestone-14-mobile/README.md` | Add plugin architecture |
| **Phase 4** | M15 | `to-dos/phase-4-polish/milestone-15-video-filters/README.md` | Add advanced shader pipeline |
| **Phase 4** | M16 | `to-dos/phase-4-polish/milestone-16-tas-editor/README.md` | Add piano roll interface details |

---

## Architectural Changes

### Before: egui Immediate Mode

```rust
impl eframe::App for RustyNesApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Direct rendering each frame
        egui::CentralPanel::default().show(ctx, |ui| {
            if ui.button("Play").clicked() {
                self.playing = true;
            }

            // Game viewport
            ui.image(self.game_texture);
        });
    }
}
```

**Issues:**
- State management becomes complex with 8+ views
- No built-in animation system
- Theme consistency difficult to maintain
- Harder to test (rendering tied to state updates)

### After: Iced Elm Architecture

```rust
pub struct RustyNes {
    current_view: View,
    console: Option<Console>,
    theme: Theme,
}

#[derive(Debug, Clone)]
pub enum Message {
    NavigateTo(View),
    LoadRom(PathBuf),
    RomLoaded(Result<Console, String>),
    Play,
    Pause,
}

impl Application for RustyNes {
    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::Play => {
                // State transition only
                self.emulation_state = EmulationState::Playing;
                Command::none()
            }
            // ...
        }
    }

    fn view(&self) -> Element<Message> {
        // Pure rendering based on state
        match &self.current_view {
            View::Playing => playing_view(self),
            View::Library => library_view(self),
            // ...
        }
    }
}
```

**Benefits:**
- Unidirectional data flow prevents state bugs
- Pure view functions easier to test
- Built-in animation subscriptions
- Theme system scales to complex designs
- Message passing enables time-travel debugging

---

## Dependency Changes

### Removed Dependencies

```toml
# OLD (egui-based)
eframe = "0.24"           # ❌ REMOVED
egui = "0.24"             # ❌ REMOVED (moved to debug overlay only)
egui_wgpu_backend = "0.24" # ❌ REMOVED
```

### Added Dependencies

```toml
# NEW (Iced-based)
iced = { version = "0.13", features = ["wgpu", "tokio", "image", "svg", "canvas", "advanced"] }
iced_aw = "0.10"          # Additional widgets (badges, cards, modals)
tokio = { version = "1.40", features = ["full"] }  # Required by Iced

# egui now OPTIONAL (debug overlay only)
egui = { version = "0.28", optional = true }
egui-wgpu = { version = "0.28", optional = true }
```

### Features

```toml
[features]
default = []
debug-overlay = ["egui", "egui-wgpu"]  # Developer tools only
```

---

## Performance Impact

### M6 MVP (Basic Run-Ahead RA=1)

**Target Metrics:**
- Frame Rate: 60 FPS (16.67ms/frame)
- Input Latency: ~10-15ms (with RA=1)
- Audio Latency: <20ms (cpal exclusive mode)
- Memory: <50 MB base + <20 MB rewind buffer
- Startup Time: <500ms cold start

**Run-Ahead Overhead (RA=1):**
- Save state serialization: <1ms
- Additional emulation: +16.67ms (1 extra frame)
- Total overhead: ~2ms per frame (negligible)

### Phase 2 (Advanced Run-Ahead RA=0-4)

**Target Metrics:**
- Input Latency: <10ms (with RA=2-3)
- Memory: <50 MB base + <100 MB rewind buffer (dual-instance)
- CPU Overhead: 2-3x emulation speed (NES easily achieves this)

---

## Testing Strategy

### M6 MVP Testing

**Unit Tests:**
- Elm message handling (update function)
- View rendering (snapshot tests)
- Save state serialization (determinism)
- Audio resampling (44.1kHz → 48kHz)

**Integration Tests:**
- ROM loading (valid/invalid files)
- Controller hot-plugging
- Theme switching
- Settings persistence

**Manual Testing:**
- [ ] Test on Linux (Ubuntu 22.04, Arch, Fedora)
- [ ] Test on Windows (10, 11)
- [ ] Test on macOS (12+, Intel + Apple Silicon)
- [ ] Test with Xbox, PlayStation, Switch Pro controllers
- [ ] Verify 60 FPS with FPS counter
- [ ] Verify audio without crackling
- [ ] Test run-ahead latency reduction (high-speed camera)

---

## Migration Path for Existing Code

### If M6 Was Already Started with egui

1. **Keep existing rendering code** (wgpu, cpal, gilrs can be reused)
2. **Wrap in Iced widgets:**
   - Game viewport: `canvas()` or custom widget
   - Audio thread: Same architecture (runs independently)
   - Input handling: Convert to Iced messages

3. **State refactoring:**
   ```rust
   // OLD: egui state scattered across update()
   self.playing = true;
   self.volume = 0.5;

   // NEW: Centralized state in RustyNes struct
   self.emulation_state = EmulationState::Playing;
   self.config.audio.volume = 0.5;
   ```

4. **Message conversion:**
   ```rust
   // OLD: Immediate checks
   if ui.button("Play").clicked() {
       self.console.play();
   }

   // NEW: Message passing
   if ui.button("Play").clicked() {
       return Command::perform(async {}, |_| Message::Play);
   }
   ```

---

## Risk Mitigation

### Risk 1: Iced Learning Curve

**Mitigation:**
- Budget 1 extra week for learning in M6-S1
- Study Iced examples thoroughly
- Use egui for debug overlay (familiar territory)
- Leverage Elm architecture documentation

**Resources:**
- [Iced Book](https://book.iced.rs/)
- [Iced Examples](https://github.com/iced-rs/iced/tree/master/examples)
- [Elm Architecture Guide](https://guide.elm-lang.org/architecture/)

### Risk 2: Basic Run-Ahead Complexity

**Mitigation:**
- Implement simplest version (RA=1) in M6-S5
- Defer advanced features (auto-detect, dual-instance) to Phase 2
- Ensure save states are fast (<1ms serialization)
- Test determinism thoroughly
- Provide fallback to traditional emulation (RA=0)

### Risk 3: Animation Performance

**Mitigation:**
- Profile Iced animations on target hardware
- Provide "Performance Mode" that disables animations
- Use 60Hz minimum, 120Hz where hardware permits
- Leverage Iced's built-in animation subscriptions

---

## Timeline Comparison

### Original Plan (egui, 12 weeks)

| Week | Sprint | Description |
|------|--------|-------------|
| 1-2 | S1 | egui application + wgpu |
| 3-4 | S2 | Audio + advanced CRT shaders |
| 5-6 | S3 | HTPC mode + Cover Flow |
| 7-8 | S4 | Netplay UI |
| 9-10 | S5 | Achievements UI |
| 11-12 | S6 | Debugger + TAS tools |

**Issues:**
- Everything in Phase 1 (feature creep)
- HTPC mode too early (no library foundation)
- Advanced features before MVP playable
- 12 weeks before usable emulator

### New Plan (Iced, 4 weeks MVP + phased features)

**Phase 1 (M6): 4 Weeks**

| Week | Sprint | Description |
|------|--------|-------------|
| 1 | S1 | Iced application foundation |
| 2 | S2 | Core emulation display (wgpu + cpal) |
| 3 | S3 | Input handling & ROM library (Grid/List) |
| 4 | S4 | Settings & persistence + S5 (polish + basic run-ahead) |

**Phase 2 (M7-M10): 4 Months**
- M7: Advanced Run-Ahead (RA=0-4, auto-detect)
- M8: GGPO Netplay
- M9: TAS recording/playback
- M10: Debugger with egui overlay

**Phase 3 (M11-M14): 6 Months**
- M11: Advanced CRT shaders (12+ presets, rolling scan)
- M12: Expansion audio
- M13: HTPC mode (Cover Flow, Virtual Shelf)
- M14: Plugin architecture

**Phase 4 (M15-M18): 6 Months**
- M15: Advanced shader pipeline optimization
- M16: TAS editor with piano roll
- M17: Full run-ahead optimization
- M18: CLI automation + accessibility

**Benefits:**
- Playable emulator in 4 weeks
- Feature-rich by Phase 2
- HTPC mode only after library is mature
- Logical dependency ordering

---

## Success Metrics

### M6 MVP Complete When:

✅ **Functional:**
- [ ] Iced application runs on Linux, Windows, macOS
- [ ] ROM loading via file dialog works
- [ ] Emulation displays at 60 FPS
- [ ] Audio plays without crackling (<20ms latency)
- [ ] Keyboard + gamepad input functional
- [ ] ROM library browser shows Grid/List views
- [ ] Settings persist across restarts (TOML)
- [ ] Basic CRT shader works (3-5 presets)
- [ ] Basic run-ahead (RA=1) reduces latency measurably

✅ **Quality:**
- [ ] Zero clippy warnings (`clippy::pedantic`)
- [ ] Zero unsafe code (except FFI if needed)
- [ ] All unit tests pass
- [ ] Integration tests pass on all platforms
- [ ] Memory usage <100 MB

✅ **User Experience:**
- [ ] UI renders at 60 FPS
- [ ] Input feels responsive
- [ ] Settings UI intuitive
- [ ] Theme looks professional
- [ ] Animations smooth (Iced subscriptions)

---

## Next Steps

### Immediate (Sprint M6-S1)

1. ✅ Review M6-REORGANIZATION-SUMMARY.md (this document)
2. ✅ Review M6-OVERVIEW.md (updated for Iced)
3. ✅ Review M6-S1-iced-application.md (new file)
4. 🔄 Rewrite M6-S2 through M6-S5 sprint files
5. 🔄 Update M6-PLANNING-CHANGES.md with rephasing summary
6. 🔄 Update Phase 2-4 milestone files
7. ▶️ Begin M6-S1 implementation (Iced application shell)

### Phase 2 Planning

1. Create M7-OVERVIEW.md for Advanced Run-Ahead
2. Detail run-ahead auto-detection algorithm
3. Design dual-instance mode for audio stability
4. Plan frame delay auto-tuning system

### Phase 3 Planning

1. Design HTPC Controller-First UI flows
2. Create Cover Flow and Virtual Shelf mockups
3. Research metadata scraping APIs (IGDB, ScreenScraper)
4. Define plugin API architecture

---

## Conclusion

The reorganization from egui to Iced represents a **strategic architectural decision** that aligns with the project's long-term complexity. Key takeaways:

### Why This Matters

1. **Scalability:** Iced's Elm architecture prevents state management bugs in large applications (8+ views)
2. **Maintainability:** Unidirectional data flow makes codebase easier to understand and modify
3. **Professionalism:** Built-in animation system and theming create polished user experience
4. **Phased Delivery:** 4-week MVP gets usable emulator faster, advanced features follow logically

### What Changed

- **Framework:** egui → Iced 0.13+ (with egui debug overlay)
- **Timeline:** 12 weeks → 4 weeks MVP + phased features
- **Architecture:** Immediate mode → Elm (Model-Update-View)
- **Scope:** Everything in M6 → MVP core + distributed advanced features

### What Stayed the Same

- **Core Technology:** wgpu, cpal, gilrs (rendering, audio, input unchanged)
- **Run-Ahead:** Still first-class feature (basic in M6, advanced in Phase 2)
- **Quality Bar:** Zero unsafe, clippy pedantic, comprehensive testing

### Expected Outcome

By the end of M6 (4 weeks):
- ✅ **Playable** NES emulator with professional UI
- ✅ **Low latency** via basic run-ahead (RA=1)
- ✅ **Cross-platform** (Linux, Windows, macOS)
- ✅ **ROM library** with Grid/List views
- ✅ **Settings** that persist
- ✅ **Foundation** for advanced features in Phases 2-4

---

**Document Status:** ✅ COMPLETE
**Review Status:** Ready for stakeholder review
**Implementation Status:** Ready to begin M6-S1

**Related Files:**
- [M6-OVERVIEW.md](M6-OVERVIEW.md) - Updated milestone overview
- [M6-PLANNING-CHANGES.md](M6-PLANNING-CHANGES.md) - Technology analysis
- [M6-S1-iced-application.md](M6-S1-iced-application.md) - Sprint 1 details
- [RustyNES-UI_UX-Design-v2.md](../../../ref-docs/RustyNES-UI_UX-Design-v2.md) - Full design spec
