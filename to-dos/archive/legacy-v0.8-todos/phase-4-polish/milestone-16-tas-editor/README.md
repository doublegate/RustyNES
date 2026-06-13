# Milestone 16: Advanced TAS Editor with Piano Roll Interface

**Phase:** 4 (Polish & Release)
**Duration:** Months 17-18 (2 months)
**Status:** Planned
**Target:** June 2027
**Prerequisites:** M6 MVP Complete, M7 Advanced Run-Ahead, M10 Debugger

---

## Overview

Milestone 16 delivers a professional-grade TAS (Tool-Assisted Speedrun) editor with piano roll interface, greenzone system, and advanced branching. This milestone targets feature parity with FCEUX TAS Editor while leveraging Iced's reactive architecture and egui's immediate-mode debug tools.

**Key Features:**
- **Piano Roll Interface:** Multi-track timeline with frame-accurate input editing
- **Greenzone System:** Verified frame history with instant scrubbing (10,000+ frames)
- **Branch Manager:** Tree-based timeline branching with visual graph
- **Undo/Redo:** Full history stack with branch preservation
- **FM2 Format:** Import/Export FCEUX-compatible TAS movies

**Target Users:** TASers, speedrunners, homebrew developers, QA testers

---

## Goals

### Core TAS Features

- [ ] **Piano Roll Input Editor**
  - Multi-track timeline (A, B, Select, Start, U, D, L, R per controller)
  - Frame-accurate editing (click to toggle, drag to paint)
  - Visual input indicator (color-coded buttons)
  - Zoom/pan controls (mouse wheel, drag)
  - Selection tools (rectangle, lasso, copy/paste)

- [ ] **Greenzone System**
  - Frame history storage (save states + inputs)
  - Instant scrubbing (O(1) frame seek)
  - Memory-efficient snapshots (delta encoding)
  - Frame verification (hash-based validation)
  - Auto-greenzone recording during playback

- [ ] **Branch Manager**
  - Tree-based timeline branching
  - Visual branch graph (egui interactive tree)
  - Branch naming and bookmarks
  - Branch comparison (diff view)
  - Auto-save per branch (SQLite database)

- [ ] **Undo/Redo System**
  - Full operation history (unlimited depth)
  - Per-branch undo stacks
  - Undo scopes (input edit, frame insert, branch creation)
  - Redo with branch preservation

- [ ] **FM2 Import/Export**
  - Full FM2 v3 specification support
  - FCEUX compatibility validation
  - Metadata editing (author, description, ROM hash)
  - Savestates embedding (savestate anchors)

- [ ] **Recording Shortcuts**
  - Keyboard macro system (customizable bindings)
  - Frame advance (F, Shift+F for -1)
  - Input toggle (A, B, Select, Start, arrows)
  - Playback modes (play, pause, frame-by-frame)
  - Auto-fire/turbo (hold key for rapid toggle)

---

## Architecture: Piano Roll with Greenzone Backend

### System Overview

```
┌──────────────────────────────────────────────────────┐
│  Iced TAS Editor UI                                  │
│  ┌────────────────────────────────────────────────┐  │
│  │  Piano Roll Timeline (egui immediate-mode)     │  │
│  │  ┌──────────────────────────────────────────┐  │  │
│  │  │ Frame  | A | B |Sel|Str| U | D | L | R | │  │  │
│  │  │───────────────────────────────────────── │  │  │
│  │  │  0001  | █ |   |   |   |   |   |   |   | │  │  │
│  │  │  0002  | █ |   |   |   |   | █ |   |   | │  │  │
│  │  │  0003  | █ | █ |   |   |   |   |   |   | │  │  │
│  │  │ >0004  |   | █ |   |   | █ |   |   |   | │  │  │
│  │  │  0005  |   |   |   | █ |   |   |   |   | │  │  │
│  │  └──────────────────────────────────────────┘  │  │
│  │  Branch: main | Greenzone: 4/10000 frames      │  │
│  └────────────────────────────────────────────────┘  │
│  ┌────────────────────────────────────────────────┐  │
│  │  Branch Manager (egui tree view)               │  │
│  │  • main (current)                              │  │
│  │    ├─ experiment-1 (frame 2345)                │  │
│  │    │  └─ sub-experiment (frame 2456)           │  │
│  │    └─ backup (frame 1234)                      │  │
│  └────────────────────────────────────────────────┘  │
│                                                      │
│  Playback: [◀◀] [◀] [▶] [▶▶] | Frame: 0004/10000     │
└──────────────────────────────────────────────────────┘
```

### Greenzone Backend Architecture

```rust
/// Greenzone: verified frame history with instant scrubbing
pub struct Greenzone {
    /// Frame snapshots (save state + inputs)
    frames: Vec<GreenzoneFrame>,

    /// Current playback position
    current_frame: usize,

    /// Maximum greenzone capacity (memory limit)
    max_frames: usize,

    /// Snapshot interval (1 = every frame, 10 = every 10th frame)
    snapshot_interval: usize,
}

#[derive(Clone)]
pub struct GreenzoneFrame {
    /// Frame number
    frame: usize,

    /// Full save state (if snapshot frame)
    state: Option<SaveState>,

    /// Input for this frame (always stored)
    input: ControllerInput,

    /// Hash of emulator state (verification)
    state_hash: u64,
}

impl Greenzone {
    /// Seek to frame (instant via snapshot lookup)
    pub fn seek(&mut self, console: &mut Console, target_frame: usize) -> Result<()> {
        // Find nearest snapshot before target
        let snapshot_idx = (target_frame / self.snapshot_interval) * self.snapshot_interval;
        let snapshot = &self.frames[snapshot_idx];

        // Load snapshot state
        console.load_state(snapshot.state.as_ref().unwrap())?;

        // Replay inputs from snapshot to target
        for frame in snapshot_idx..target_frame {
            console.set_input(self.frames[frame].input);
            console.step_frame();
        }

        self.current_frame = target_frame;
        Ok(())
    }

    /// Record new frame (auto-snapshot if interval reached)
    pub fn record_frame(&mut self, console: &Console, input: ControllerInput) {
        let frame_num = self.current_frame;
        let is_snapshot = frame_num % self.snapshot_interval == 0;

        let frame = GreenzoneFrame {
            frame: frame_num,
            state: if is_snapshot {
                Some(console.save_state())
            } else {
                None
            },
            input,
            state_hash: console.state_hash(),
        };

        self.frames.push(frame);

        // Evict old frames if capacity exceeded
        if self.frames.len() > self.max_frames {
            self.frames.remove(0);
        }

        self.current_frame += 1;
    }

    /// Verify greenzone integrity (hash validation)
    pub fn verify(&self, console: &mut Console) -> Result<()> {
        console.reset();

        for frame in &self.frames {
            console.set_input(frame.input);
            console.step_frame();

            if console.state_hash() != frame.state_hash {
                return Err(anyhow!("Greenzone desynced at frame {}", frame.frame));
            }
        }

        Ok(())
    }
}
```

---

## Piano Roll Implementation

### egui Piano Roll Widget

**File:** `crates/rustynes-desktop/src/tas/piano_roll.rs`

```rust
use egui::{Ui, Rect, Pos2, Color32, Stroke, Response};

pub struct PianoRoll {
    /// Current frame range visible (start, end)
    viewport: (usize, usize),

    /// Zoom level (frames per pixel)
    zoom: f32,

    /// Scroll offset (frames)
    scroll: f32,

    /// Selected frames (for copy/paste)
    selection: Vec<usize>,
}

impl PianoRoll {
    pub fn show(&mut self, ui: &mut Ui, greenzone: &Greenzone) -> Response {
        let (response, painter) = ui.allocate_painter(
            ui.available_size(),
            egui::Sense::click_and_drag(),
        );

        let rect = response.rect;

        // Calculate visible frame range
        let frames_visible = (rect.width() * self.zoom) as usize;
        let start_frame = self.scroll as usize;
        let end_frame = start_frame + frames_visible;

        // Draw frame numbers (X axis)
        for frame in start_frame..end_frame {
            let x = rect.min.x + ((frame - start_frame) as f32 / self.zoom);
            painter.text(
                Pos2::new(x, rect.min.y),
                egui::Align2::CENTER_TOP,
                format!("{:04}", frame),
                egui::FontId::monospace(10.0),
                Color32::WHITE,
            );
        }

        // Draw button tracks (Y axis)
        let buttons = ["A", "B", "Sel", "Str", "U", "D", "L", "R"];
        let track_height = rect.height() / buttons.len() as f32;

        for (i, button) in buttons.iter().enumerate() {
            let y = rect.min.y + 20.0 + (i as f32 * track_height);

            // Track label
            painter.text(
                Pos2::new(rect.min.x, y),
                egui::Align2::LEFT_TOP,
                *button,
                egui::FontId::monospace(12.0),
                Color32::LIGHT_GRAY,
            );

            // Draw input cells
            for frame in start_frame..end_frame {
                let input = greenzone.frames.get(frame).map(|f| f.input);
                let button_pressed = input.map_or(false, |inp| inp.is_pressed(i));

                let x = rect.min.x + 30.0 + ((frame - start_frame) as f32 / self.zoom);
                let cell_rect = Rect::from_min_size(
                    Pos2::new(x, y),
                    egui::Vec2::new(track_height * 0.8, track_height * 0.8),
                );

                // Draw cell background
                let color = if button_pressed {
                    Color32::from_rgb(100, 150, 255) // Blue for pressed
                } else {
                    Color32::from_gray(40) // Dark gray for released
                };

                painter.rect_filled(cell_rect, 2.0, color);
                painter.rect_stroke(cell_rect, 2.0, Stroke::new(1.0, Color32::WHITE));

                // Handle click to toggle
                if response.clicked_by(egui::PointerButton::Primary) {
                    if cell_rect.contains(response.interact_pointer_pos().unwrap()) {
                        // Toggle input at this frame
                        // (emit message to parent component)
                    }
                }
            }
        }

        // Handle zoom (mouse wheel)
        if let Some(hover_pos) = response.hover_pos() {
            if ui.input(|i| i.scroll_delta.y != 0.0) {
                let delta = ui.input(|i| i.scroll_delta.y);
                self.zoom *= 1.0 + (delta * 0.001);
                self.zoom = self.zoom.clamp(0.1, 10.0);
            }
        }

        // Handle pan (drag with middle mouse)
        if response.dragged_by(egui::PointerButton::Middle) {
            self.scroll -= response.drag_delta().x * self.zoom;
            self.scroll = self.scroll.max(0.0);
        }

        response
    }
}
```

### Input Editing Operations

```rust
pub enum PianoRollEdit {
    /// Toggle button at frame
    ToggleButton { frame: usize, button: u8 },

    /// Paint button (drag to set multiple frames)
    PaintButton { start: usize, end: usize, button: u8, pressed: bool },

    /// Insert frames
    InsertFrames { at: usize, count: usize },

    /// Delete frames
    DeleteFrames { start: usize, end: usize },

    /// Copy selection
    Copy { frames: Vec<usize> },

    /// Paste inputs
    Paste { at: usize, inputs: Vec<ControllerInput> },
}

impl PianoRollEdit {
    pub fn apply(&self, greenzone: &mut Greenzone) {
        match self {
            Self::ToggleButton { frame, button } => {
                if let Some(gz_frame) = greenzone.frames.get_mut(*frame) {
                    gz_frame.input.toggle_button(*button);
                }
            }

            Self::PaintButton { start, end, button, pressed } => {
                for frame in *start..=*end {
                    if let Some(gz_frame) = greenzone.frames.get_mut(frame) {
                        gz_frame.input.set_button(*button, *pressed);
                    }
                }
            }

            Self::InsertFrames { at, count } => {
                for _ in 0..*count {
                    greenzone.frames.insert(*at, GreenzoneFrame::default());
                }
            }

            Self::DeleteFrames { start, end } => {
                greenzone.frames.drain(*start..=*end);
            }

            Self::Copy { frames } => {
                // Store in clipboard (implementation-specific)
            }

            Self::Paste { at, inputs } => {
                for (i, input) in inputs.iter().enumerate() {
                    if let Some(gz_frame) = greenzone.frames.get_mut(at + i) {
                        gz_frame.input = *input;
                    }
                }
            }
        }
    }
}
```

---

## Branch Manager

### Branch Tree Structure

```rust
use std::collections::HashMap;

pub struct BranchManager {
    /// All branches (ID -> Branch)
    branches: HashMap<BranchId, Branch>,

    /// Current active branch
    current_branch: BranchId,

    /// Root branch (always exists)
    root_id: BranchId,

    /// Next available branch ID
    next_id: BranchId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BranchId(u64);

#[derive(Debug, Clone)]
pub struct Branch {
    /// Unique ID
    id: BranchId,

    /// Human-readable name
    name: String,

    /// Parent branch (None for root)
    parent: Option<BranchId>,

    /// Child branches
    children: Vec<BranchId>,

    /// Frame where this branch diverged from parent
    divergence_frame: usize,

    /// Greenzone for this branch
    greenzone: Greenzone,

    /// Bookmarks (frame -> label)
    bookmarks: HashMap<usize, String>,

    /// Creation timestamp
    created_at: std::time::SystemTime,
}

impl BranchManager {
    pub fn new() -> Self {
        let root_id = BranchId(0);
        let mut branches = HashMap::new();

        branches.insert(root_id, Branch {
            id: root_id,
            name: "main".to_string(),
            parent: None,
            children: Vec::new(),
            divergence_frame: 0,
            greenzone: Greenzone::new(),
            bookmarks: HashMap::new(),
            created_at: std::time::SystemTime::now(),
        });

        Self {
            branches,
            current_branch: root_id,
            root_id,
            next_id: BranchId(1),
        }
    }

    /// Create new branch from current frame
    pub fn create_branch(&mut self, name: String, from_frame: usize) -> BranchId {
        let new_id = self.next_id;
        self.next_id.0 += 1;

        // Clone greenzone up to divergence point
        let parent_greenzone = &self.branches[&self.current_branch].greenzone;
        let mut new_greenzone = parent_greenzone.clone();
        new_greenzone.truncate(from_frame);

        let branch = Branch {
            id: new_id,
            name,
            parent: Some(self.current_branch),
            children: Vec::new(),
            divergence_frame: from_frame,
            greenzone: new_greenzone,
            bookmarks: HashMap::new(),
            created_at: std::time::SystemTime::now(),
        };

        // Add to parent's children
        self.branches.get_mut(&self.current_branch).unwrap().children.push(new_id);
        self.branches.insert(new_id, branch);

        new_id
    }

    /// Switch to different branch
    pub fn switch_branch(&mut self, branch_id: BranchId, console: &mut Console) -> Result<()> {
        let branch = self.branches.get(&branch_id)
            .ok_or_else(|| anyhow!("Branch not found"))?;

        // Load greenzone
        branch.greenzone.seek(console, branch.greenzone.current_frame)?;

        self.current_branch = branch_id;
        Ok(())
    }

    /// Compare two branches (diff view)
    pub fn compare_branches(&self, a: BranchId, b: BranchId) -> Vec<FrameDiff> {
        let branch_a = &self.branches[&a];
        let branch_b = &self.branches[&b];

        let min_len = branch_a.greenzone.frames.len().min(branch_b.greenzone.frames.len());
        let mut diffs = Vec::new();

        for i in 0..min_len {
            let input_a = branch_a.greenzone.frames[i].input;
            let input_b = branch_b.greenzone.frames[i].input;

            if input_a != input_b {
                diffs.push(FrameDiff {
                    frame: i,
                    input_a,
                    input_b,
                });
            }
        }

        diffs
    }
}

pub struct FrameDiff {
    pub frame: usize,
    pub input_a: ControllerInput,
    pub input_b: ControllerInput,
}
```

### Branch Tree Visualization (egui)

```rust
pub fn show_branch_tree(ui: &mut egui::Ui, manager: &BranchManager) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        show_branch_node(ui, manager, manager.root_id, 0);
    });
}

fn show_branch_node(ui: &mut egui::Ui, manager: &BranchManager, branch_id: BranchId, depth: usize) {
    let branch = &manager.branches[&branch_id];
    let is_current = branch_id == manager.current_branch;

    ui.horizontal(|ui| {
        // Indent based on depth
        ui.add_space(depth as f32 * 20.0);

        // Branch icon
        ui.label(if branch.children.is_empty() { "•" } else { "├─" });

        // Branch name (clickable)
        let text = if is_current {
            egui::RichText::new(&branch.name).color(egui::Color32::GREEN).strong()
        } else {
            egui::RichText::new(&branch.name)
        };

        if ui.selectable_label(is_current, text).clicked() {
            // Emit message to switch branch
        }

        // Frame count
        ui.label(format!("({} frames)", branch.greenzone.frames.len()));

        // Context menu
        ui.menu_button("⋮", |ui| {
            if ui.button("Rename").clicked() {
                // Open rename dialog
            }
            if ui.button("Delete").clicked() {
                // Confirm and delete branch
            }
            if ui.button("Export FM2").clicked() {
                // Export this branch to FM2 file
            }
        });
    });

    // Recursively show children
    for child_id in &branch.children {
        show_branch_node(ui, manager, *child_id, depth + 1);
    }
}
```

---

## FM2 Import/Export

### FM2 Format Specification

```
version 3
emuVersion 28000
rerecordCount 1234
palFlag 0
romFilename Super Mario Bros. (JU) (PRG0) [!].nes
romChecksum base64:+/BzOBRBx2xr3c6NQXSrZQ==
guid 12345678-1234-5678-1234-567812345678
fourscore 0
port0 1
port1 1
port2 0
subtitle RustyNES TAS Movie
comment author:TASer
|0|........||
|0|A.......||
|0|A....D..||
|0|.B......||
|0|...S....||
```

### FM2 Parser/Writer

```rust
use std::io::{BufRead, Write};

pub struct Fm2Movie {
    pub metadata: Fm2Metadata,
    pub inputs: Vec<ControllerInput>,
}

pub struct Fm2Metadata {
    pub version: u32,
    pub emu_version: u32,
    pub rerecord_count: u32,
    pub pal_flag: bool,
    pub rom_filename: String,
    pub rom_checksum: String,
    pub guid: String,
    pub port0: u8,
    pub port1: u8,
    pub subtitle: String,
    pub comment: String,
}

impl Fm2Movie {
    /// Import FM2 file
    pub fn from_file(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);

        let mut metadata = Fm2Metadata::default();
        let mut inputs = Vec::new();
        let mut in_header = true;

        for line in reader.lines() {
            let line = line?;

            if in_header {
                if line.starts_with('|') {
                    in_header = false;
                } else if let Some((key, value)) = line.split_once(' ') {
                    match key {
                        "version" => metadata.version = value.parse()?,
                        "emuVersion" => metadata.emu_version = value.parse()?,
                        "rerecordCount" => metadata.rerecord_count = value.parse()?,
                        "palFlag" => metadata.pal_flag = value == "1",
                        "romFilename" => metadata.rom_filename = value.to_string(),
                        "romChecksum" => metadata.rom_checksum = value.to_string(),
                        "guid" => metadata.guid = value.to_string(),
                        "port0" => metadata.port0 = value.parse()?,
                        "port1" => metadata.port1 = value.parse()?,
                        "subtitle" => metadata.subtitle = value.to_string(),
                        "comment" => metadata.comment = value.to_string(),
                        _ => {}
                    }
                }
            }

            if !in_header {
                // Parse input line: |0|A.......||
                let parts: Vec<&str> = line.split('|').collect();
                if parts.len() >= 3 {
                    let input_str = parts[2];
                    let input = ControllerInput::from_fm2(input_str);
                    inputs.push(input);
                }
            }
        }

        Ok(Self { metadata, inputs })
    }

    /// Export to FM2 file
    pub fn to_file(&self, path: &Path) -> Result<()> {
        let mut file = std::fs::File::create(path)?;

        // Write metadata
        writeln!(file, "version {}", self.metadata.version)?;
        writeln!(file, "emuVersion {}", self.metadata.emu_version)?;
        writeln!(file, "rerecordCount {}", self.metadata.rerecord_count)?;
        writeln!(file, "palFlag {}", if self.metadata.pal_flag { 1 } else { 0 })?;
        writeln!(file, "romFilename {}", self.metadata.rom_filename)?;
        writeln!(file, "romChecksum {}", self.metadata.rom_checksum)?;
        writeln!(file, "guid {}", self.metadata.guid)?;
        writeln!(file, "port0 {}", self.metadata.port0)?;
        writeln!(file, "port1 {}", self.metadata.port1)?;
        writeln!(file, "subtitle {}", self.metadata.subtitle)?;
        writeln!(file, "comment {}", self.metadata.comment)?;

        // Write input frames
        for input in &self.inputs {
            writeln!(file, "|0|{}||", input.to_fm2())?;
        }

        Ok(())
    }
}

impl ControllerInput {
    fn from_fm2(s: &str) -> Self {
        let mut input = Self::default();
        let chars: Vec<char> = s.chars().collect();

        if chars.len() >= 8 {
            input.set_button(0, chars[0] == 'A'); // A
            input.set_button(1, chars[1] == 'B'); // B
            input.set_button(2, chars[2] == 'S'); // Select
            input.set_button(3, chars[3] == 's'); // Start
            input.set_button(4, chars[4] == 'U'); // Up
            input.set_button(5, chars[5] == 'D'); // Down
            input.set_button(6, chars[6] == 'L'); // Left
            input.set_button(7, chars[7] == 'R'); // Right
        }

        input
    }

    fn to_fm2(&self) -> String {
        format!(
            "{}{}{}{}{}{}{}{}",
            if self.is_pressed(0) { 'A' } else { '.' },
            if self.is_pressed(1) { 'B' } else { '.' },
            if self.is_pressed(2) { 'S' } else { '.' },
            if self.is_pressed(3) { 's' } else { '.' },
            if self.is_pressed(4) { 'U' } else { '.' },
            if self.is_pressed(5) { 'D' } else { '.' },
            if self.is_pressed(6) { 'L' } else { '.' },
            if self.is_pressed(7) { 'R' } else { '.' },
        )
    }
}
```

---

## Undo/Redo System

### Undo Stack with Branch Support

```rust
pub struct UndoRedoStack {
    /// Undo history (per branch)
    undo_stacks: HashMap<BranchId, Vec<UndoableAction>>,

    /// Redo history (per branch)
    redo_stacks: HashMap<BranchId, Vec<UndoableAction>>,

    /// Maximum undo depth
    max_depth: usize,
}

pub enum UndoableAction {
    /// Input edit (frame, old input, new input)
    EditInput {
        frame: usize,
        old: ControllerInput,
        new: ControllerInput,
    },

    /// Frame insertion (at, count)
    InsertFrames {
        at: usize,
        count: usize,
    },

    /// Frame deletion (start, end, deleted frames)
    DeleteFrames {
        start: usize,
        end: usize,
        frames: Vec<GreenzoneFrame>,
    },

    /// Branch creation (branch ID)
    CreateBranch {
        id: BranchId,
    },

    /// Branch deletion (branch)
    DeleteBranch {
        branch: Branch,
    },
}

impl UndoRedoStack {
    pub fn push(&mut self, branch_id: BranchId, action: UndoableAction) {
        let stack = self.undo_stacks.entry(branch_id).or_default();
        stack.push(action);

        // Evict oldest if depth exceeded
        if stack.len() > self.max_depth {
            stack.remove(0);
        }

        // Clear redo stack (new action invalidates redo)
        self.redo_stacks.entry(branch_id).or_default().clear();
    }

    pub fn undo(&mut self, branch_id: BranchId, greenzone: &mut Greenzone, manager: &mut BranchManager) -> Result<()> {
        let stack = self.undo_stacks.entry(branch_id).or_default();
        if let Some(action) = stack.pop() {
            // Apply inverse action
            let inverse = action.undo(greenzone, manager)?;

            // Push to redo stack
            self.redo_stacks.entry(branch_id).or_default().push(inverse);

            Ok(())
        } else {
            Err(anyhow!("Nothing to undo"))
        }
    }

    pub fn redo(&mut self, branch_id: BranchId, greenzone: &mut Greenzone, manager: &mut BranchManager) -> Result<()> {
        let stack = self.redo_stacks.entry(branch_id).or_default();
        if let Some(action) = stack.pop() {
            // Apply action
            let inverse = action.undo(greenzone, manager)?;

            // Push to undo stack
            self.undo_stacks.entry(branch_id).or_default().push(inverse);

            Ok(())
        } else {
            Err(anyhow!("Nothing to redo"))
        }
    }
}

impl UndoableAction {
    fn undo(&self, greenzone: &mut Greenzone, manager: &mut BranchManager) -> Result<Self> {
        match self {
            Self::EditInput { frame, old, new } => {
                greenzone.frames[*frame].input = *old;
                Ok(Self::EditInput { frame: *frame, old: *new, new: *old })
            }

            Self::InsertFrames { at, count } => {
                greenzone.frames.drain(*at..(*at + *count));
                Ok(Self::DeleteFrames {
                    start: *at,
                    end: *at + *count,
                    frames: Vec::new(), // Would need to store deleted frames
                })
            }

            Self::DeleteFrames { start, end, frames } => {
                for (i, frame) in frames.iter().enumerate() {
                    greenzone.frames.insert(start + i, frame.clone());
                }
                Ok(Self::InsertFrames { at: *start, count: frames.len() })
            }

            Self::CreateBranch { id } => {
                manager.branches.remove(id);
                Ok(Self::DeleteBranch { branch: Branch::default() }) // Would need full branch
            }

            Self::DeleteBranch { branch } => {
                manager.branches.insert(branch.id, branch.clone());
                Ok(Self::CreateBranch { id: branch.id })
            }
        }
    }
}
```

---

## Implementation Plan

### Sprint 1: Greenzone Foundation

**Duration:** 2 weeks

- [ ] Implement `Greenzone` struct with frame storage
- [ ] Snapshot system (configurable interval)
- [ ] Instant seeking (O(1) lookup)
- [ ] Delta encoding for memory efficiency
- [ ] Frame verification (hash-based)
- [ ] Unit tests (10,000+ frame stress test)

### Sprint 2: Piano Roll Interface

**Duration:** 2 weeks

- [ ] egui piano roll widget (multi-track timeline)
- [ ] Frame-accurate editing (click to toggle)
- [ ] Zoom/pan controls (mouse wheel, drag)
- [ ] Visual input indicators (color-coded buttons)
- [ ] Selection tools (rectangle, lasso)
- [ ] Copy/paste functionality

### Sprint 3: Branch Manager

**Duration:** 2 weeks

- [ ] `BranchManager` struct with tree storage
- [ ] Branch creation/deletion
- [ ] Branch switching (load greenzone)
- [ ] Visual branch tree (egui interactive graph)
- [ ] Branch comparison (diff view)
- [ ] Bookmarks (per branch)

### Sprint 4: Undo/Redo & FM2 Support

**Duration:** 2 weeks

- [ ] Undo/redo stack (per branch)
- [ ] FM2 parser (import)
- [ ] FM2 writer (export)
- [ ] FCEUX compatibility validation
- [ ] Metadata editing UI
- [ ] Recording shortcuts (keyboard macros)

---

## Acceptance Criteria

### Functionality

- [ ] Piano roll supports all 8 buttons (A, B, Select, Start, U, D, L, R)
- [ ] Greenzone handles 10,000+ frames without lag
- [ ] Branching system supports 100+ branches
- [ ] Undo/redo works across all operations
- [ ] FM2 import/export passes FCEUX validation
- [ ] Recording shortcuts customizable

### Performance

- [ ] Frame seeking <50ms (any frame in greenzone)
- [ ] Piano roll rendering at 60 FPS (1,000 frames visible)
- [ ] Undo/redo <10ms per operation
- [ ] FM2 export <1s for 10,000-frame movie

### User Experience

- [ ] Piano roll intuitive (drag to paint, click to toggle)
- [ ] Branch tree clear (visual hierarchy)
- [ ] Keyboard shortcuts documented
- [ ] FCEUX users feel at home (feature parity)

---

## Dependencies

### Prerequisites

- **M6 MVP Complete:** Iced GUI established, wgpu rendering
- **M7 Advanced Run-Ahead:** Deterministic emulation verified
- **M10 Debugger:** egui overlay integration

### Crate Dependencies

```toml
# crates/rustynes-desktop/Cargo.toml

[dependencies]
egui = "0.28"         # Piano roll UI
rusqlite = "0.31"     # Branch database (SQLite)
uuid = "1.10"         # FM2 GUID generation
base64 = "0.22"       # FM2 checksum encoding
```

---

## Related Documentation

- [FM2_FORMAT.md](../../docs/formats/FM2_FORMAT.md) - FCEUX movie format specification
- [M6-OVERVIEW.md](../../phase-1-mvp/milestone-6-gui/M6-OVERVIEW.md) - Iced + egui hybrid architecture
- [M7-README.md](../../phase-2-features/milestone-7-advanced-runahead/README.md) - Deterministic emulation
- [M10-README.md](../../phase-2-features/milestone-10-debugger/README.md) - egui overlay patterns

---

## Success Criteria

1. Piano roll interface functional with all editing tools
2. Greenzone supports 10,000+ frames with instant seeking
3. Branch manager supports unlimited branches with tree visualization
4. Undo/redo works reliably across all operations
5. FM2 import/export passes FCEUX compatibility tests
6. Recording shortcuts customizable and documented
7. TASers report feature parity with FCEUX TAS Editor
8. Zero performance degradation during editing
9. M16 milestone marked as ✅ COMPLETE

---

**Milestone Status:** ⏳ PLANNED
**Blocked By:** M6 MVP Complete, M7 Advanced Run-Ahead, M10 Debugger
**Next Milestone:** v1.0 Release (December 2027)
