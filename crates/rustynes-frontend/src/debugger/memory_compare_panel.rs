#![allow(clippy::too_many_lines, clippy::missing_const_for_fn)]
//! Memory Compare — RAM Search + RAM Watch (v1.3.0 Workstream C C3; upgraded
//! v1.6.0 "Studio" Workstream C C3).
//!
//! The classic emulator "memory search": snapshot the 2 KB CPU work RAM
//! (`$0000-$07FF`) as a baseline, then repeatedly narrow a candidate set.
//! v1.6.0 upgrades it to the BizHawk/FCEUX-class tool:
//!
//! - **Operator × compare-to matrix** — each step keeps candidates whose value
//!   satisfies an operator (`== != < > <= >=`) **either** against the previous
//!   snapshot (find "what went down when you lost a life") **or** against a
//!   typed constant (find "the value that is now 99").
//! - **Sizes** — search 1-, 2-, or 4-byte little-endian values.
//! - **Freeze** — freeze a surviving candidate at its current value; the panel
//!   emits it as a [`RawCheat`] re-applied each frame (the same raw-cheat
//!   overlay the hex editor + cheat panel use).
//! - **RAM Watch** — a named list of `(address, size, label)` watch entries with
//!   live values, freeze, and `.wch` save/load (FCEUX-style, native).
//!
//! Read-only against the core: it samples RAM via the side-effect-free
//! `cpu_bus_peek`, holds its own baseline copy, and the only write path is the
//! freeze raw cheats (applied after each frame, like every other cheat) — so the
//! no-freeze path is byte-identical and determinism is unaffected.

use rustynes_core::Nes;

use crate::cheats::RawCheat;

/// The NES internal work RAM: `$0000-$07FF` (2 KB; mirrored to `$1FFF`). Cheat
/// hunting only ever targets this region.
const RAM_LEN: u16 = 0x0800;
/// Cap on rows drawn in the candidate list (the count is always exact; only the
/// rendered rows are bounded so a fresh, unfiltered search can't spam the UI).
const MAX_SHOWN: usize = 256;

/// The width of a searched / watched value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Size {
    U8,
    U16,
    U32,
}

impl Size {
    const fn label(self) -> &'static str {
        match self {
            Self::U8 => "1 byte",
            Self::U16 => "2 bytes",
            Self::U32 => "4 bytes",
        }
    }

    const fn bytes(self) -> u16 {
        match self {
            Self::U8 => 1,
            Self::U16 => 2,
            Self::U32 => 4,
        }
    }
}

/// The comparison operator applied each step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

impl Op {
    const fn label(self) -> &'static str {
        match self {
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Gt => ">",
            Self::Le => "<=",
            Self::Ge => ">=",
        }
    }

    const fn apply(self, cur: u32, rhs: u32) -> bool {
        match self {
            Self::Eq => cur == rhs,
            Self::Ne => cur != rhs,
            Self::Lt => cur < rhs,
            Self::Gt => cur > rhs,
            Self::Le => cur <= rhs,
            Self::Ge => cur >= rhs,
        }
    }
}

/// What the operator compares the current value against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompareTo {
    /// The value from the previous snapshot (iterative search).
    Previous,
    /// A typed constant.
    Value,
}

impl CompareTo {
    const fn label(self) -> &'static str {
        match self {
            Self::Previous => "previous",
            Self::Value => "value",
        }
    }
}

/// One RAM Watch entry.
#[derive(Debug, Clone)]
struct WatchEntry {
    addr: u16,
    size: Size,
    label: String,
    /// `Some(v)` when frozen at value `v`.
    frozen: Option<u32>,
}

/// Persistent state of the Memory Compare panel.
pub struct MemoryComparePanelState {
    // --- RAM Search ---
    /// Baseline RAM image from the last snapshot (`RAM_LEN` bytes); `None`
    /// until the first search is started.
    baseline: Option<Vec<u8>>,
    /// Addresses still matching every applied step; `None` until a search starts.
    candidates: Option<Vec<u16>>,
    size: Size,
    op: Op,
    compare_to: CompareTo,
    /// Decimal/hex entry for the `CompareTo::Value` operand.
    value_text: String,
    steps: u32,
    // --- RAM Watch ---
    watches: Vec<WatchEntry>,
    watch_addr_text: String,
    watch_label_text: String,
    watch_size: Size,
    /// Last `.wch` save/load status line. Native-only: the `.wch` file dialogs
    /// don't exist on wasm (no filesystem), so the field is gated out there to
    /// avoid a dead-code warning.
    #[cfg(not(target_arch = "wasm32"))]
    wch_status: Option<String>,
}

impl Default for MemoryComparePanelState {
    fn default() -> Self {
        Self {
            baseline: None,
            candidates: None,
            size: Size::U8,
            op: Op::Lt,
            compare_to: CompareTo::Previous,
            value_text: String::new(),
            steps: 0,
            watches: Vec::new(),
            watch_addr_text: String::new(),
            watch_label_text: String::new(),
            watch_size: Size::U8,
            #[cfg(not(target_arch = "wasm32"))]
            wch_status: None,
        }
    }
}

impl MemoryComparePanelState {
    /// The frozen RAM Watch entries as raw cheats (re-applied each frame by the
    /// app, merged with the hex editor + cheat panel lists). A multi-byte freeze
    /// expands to one cheat per little-endian byte. Empty when nothing is frozen,
    /// so the no-freeze path stays byte-identical.
    #[must_use]
    pub fn freeze_cheats(&self) -> Vec<RawCheat> {
        let mut out = Vec::new();
        for w in &self.watches {
            let Some(v) = w.frozen else { continue };
            for i in 0..w.size.bytes() {
                let byte = ((v >> (8 * u32::from(i))) & 0xFF) as u8;
                out.push(RawCheat {
                    address: w.addr.wrapping_add(i),
                    value: byte,
                    compare: None,
                    enabled: true,
                });
            }
        }
        out
    }

    /// Snapshot current RAM and (re)seed the candidate set with every address
    /// that can hold a value of the selected size.
    fn start(&mut self, nes: &mut Nes) {
        self.baseline = Some(snapshot(nes));
        let last = RAM_LEN - self.size.bytes();
        self.candidates = Some((0..=last).collect());
        self.steps = 0;
    }

    /// Apply the operator: keep candidates whose current value (vs the previous
    /// snapshot or the typed constant) satisfies it, then re-snapshot.
    fn apply(&mut self, nes: &mut Nes) {
        let (Some(base), Some(cands)) = (self.baseline.as_ref(), self.candidates.as_mut()) else {
            return;
        };
        let cur_ram = snapshot(nes);
        let size = self.size;
        let op = self.op;
        let typed = parse_value(&self.value_text).unwrap_or(0);
        let compare_to = self.compare_to;
        cands.retain(|&addr| {
            let cur = read_le(&cur_ram, addr, size);
            let rhs = match compare_to {
                CompareTo::Previous => read_le(base, addr, size),
                CompareTo::Value => typed,
            };
            op.apply(cur, rhs)
        });
        self.baseline = Some(cur_ram);
        self.steps += 1;
    }

    /// Add a watch entry from the input boxes.
    fn add_watch(&mut self) {
        let Some(addr) = parse_hex16(&self.watch_addr_text) else {
            return;
        };
        let label = if self.watch_label_text.trim().is_empty() {
            format!("${addr:04X}")
        } else {
            self.watch_label_text.trim().to_string()
        };
        self.watches.push(WatchEntry {
            addr,
            size: self.watch_size,
            label,
            frozen: None,
        });
        self.watch_addr_text.clear();
        self.watch_label_text.clear();
    }
}

/// Copy the 2 KB work RAM via the (logically side-effect-free) peek.
fn snapshot(nes: &mut Nes) -> Vec<u8> {
    (0..RAM_LEN).map(|a| nes.cpu_bus_peek(a)).collect()
}

/// Read a little-endian value of `size` from a RAM image at `addr` (clamped to
/// the buffer; out-of-range high bytes read as 0).
fn read_le(ram: &[u8], addr: u16, size: Size) -> u32 {
    let mut v = 0u32;
    for i in 0..size.bytes() {
        let idx = addr.wrapping_add(i) as usize;
        let byte = ram.get(idx).copied().unwrap_or(0);
        v |= u32::from(byte) << (8 * u32::from(i));
    }
    v
}

/// Read a little-endian value of `size` straight off the live bus.
fn read_le_nes(nes: &mut Nes, addr: u16, size: Size) -> u32 {
    let mut v = 0u32;
    for i in 0..size.bytes() {
        let byte = nes.cpu_bus_peek(addr.wrapping_add(i));
        v |= u32::from(byte) << (8 * u32::from(i));
    }
    v
}

pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    state: &mut MemoryComparePanelState,
    nes: &mut Nes,
) {
    egui::Window::new("Memory Compare")
        .open(open)
        .default_pos([336.0, 480.0])
        .default_size([360.0, 540.0])
        .resizable(true)
        .show(ctx, |ui| {
            // ---------------- RAM Search ----------------
            ui.label(egui::RichText::new("RAM Search").strong());
            ui.horizontal(|ui| {
                if ui
                    .button("New search")
                    .on_hover_text(
                        "Snapshot current RAM ($0000-$07FF) and reset the candidate set.",
                    )
                    .clicked()
                {
                    state.start(nes);
                }
                let can_filter = state.candidates.is_some();
                if ui
                    .add_enabled(can_filter, egui::Button::new("Search"))
                    .on_hover_text("Keep candidates matching the comparison, then re-snapshot.")
                    .clicked()
                {
                    state.apply(nes);
                }
            });
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("memcmp_size")
                    .selected_text(state.size.label())
                    .show_ui(ui, |ui| {
                        for s in [Size::U8, Size::U16, Size::U32] {
                            // Changing size invalidates the in-flight candidate
                            // set (different stride), so reset it.
                            if ui.selectable_value(&mut state.size, s, s.label()).clicked() {
                                state.candidates = None;
                                state.baseline = None;
                            }
                        }
                    });
                egui::ComboBox::from_id_salt("memcmp_op")
                    .selected_text(state.op.label())
                    .show_ui(ui, |ui| {
                        for o in [Op::Eq, Op::Ne, Op::Lt, Op::Gt, Op::Le, Op::Ge] {
                            ui.selectable_value(&mut state.op, o, o.label());
                        }
                    });
                egui::ComboBox::from_id_salt("memcmp_cmp")
                    .selected_text(state.compare_to.label())
                    .show_ui(ui, |ui| {
                        for c in [CompareTo::Previous, CompareTo::Value] {
                            ui.selectable_value(&mut state.compare_to, c, c.label());
                        }
                    });
                if state.compare_to == CompareTo::Value {
                    ui.add(
                        egui::TextEdit::singleline(&mut state.value_text)
                            .desired_width(56.0)
                            .hint_text("$00"),
                    );
                }
            });

            let mut freeze_from_search: Option<(u16, u32)> = None;
            if let Some(cands) = state.candidates.as_ref() {
                ui.label(format!(
                    "candidates: {}   (step {})",
                    cands.len(),
                    state.steps
                ));
                let base = state.baseline.as_ref();
                let size = state.size;
                egui::ScrollArea::vertical()
                    .id_salt("search_results")
                    .max_height(180.0)
                    .show(ui, |ui| {
                        for &addr in cands.iter().take(MAX_SHOWN) {
                            let cur = read_le_nes(nes, addr, size);
                            let b = base.map_or(cur, |bl| read_le(bl, addr, size));
                            ui.horizontal(|ui| {
                                ui.monospace(format!(
                                    "${addr:04X}  {b:0width$X} -> {cur:0width$X}",
                                    width = (size.bytes() * 2) as usize
                                ));
                                if ui.small_button("watch").clicked() {
                                    state.watches.push(WatchEntry {
                                        addr,
                                        size,
                                        label: format!("${addr:04X}"),
                                        frozen: None,
                                    });
                                }
                                if ui.small_button("freeze").clicked() {
                                    freeze_from_search = Some((addr, cur));
                                }
                            });
                        }
                        if cands.len() > MAX_SHOWN {
                            ui.weak(format!(
                                "... {} more (narrow the search to see them all)",
                                cands.len() - MAX_SHOWN
                            ));
                        }
                    });
            } else {
                ui.weak("Press \"New search\" to snapshot RAM and begin.");
            }
            // A "freeze" button in the search list adds a frozen watch entry.
            if let Some((addr, v)) = freeze_from_search {
                state.watches.push(WatchEntry {
                    addr,
                    size: state.size,
                    label: format!("${addr:04X}"),
                    frozen: Some(v),
                });
            }

            ui.separator();

            // ---------------- RAM Watch ----------------
            ui.label(egui::RichText::new("RAM Watch").strong());
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut state.watch_addr_text)
                        .desired_width(56.0)
                        .hint_text("$0300"),
                );
                egui::ComboBox::from_id_salt("watch_size")
                    .selected_text(state.watch_size.label())
                    .show_ui(ui, |ui| {
                        for s in [Size::U8, Size::U16, Size::U32] {
                            ui.selectable_value(&mut state.watch_size, s, s.label());
                        }
                    });
                ui.add(
                    egui::TextEdit::singleline(&mut state.watch_label_text)
                        .desired_width(90.0)
                        .hint_text("label (opt)"),
                );
                if ui.button("Add").clicked() {
                    state.add_watch();
                }
            });

            // Native `.wch` list save / load.
            #[cfg(not(target_arch = "wasm32"))]
            ui.horizontal(|ui| {
                if ui.button("Save .wch…").clicked() {
                    state.wch_status = Some(save_wch(&state.watches));
                }
                if ui.button("Load .wch…").clicked()
                    && let Some(loaded) = load_wch()
                {
                    match loaded {
                        Ok(list) => {
                            let n = list.len();
                            state.watches = list;
                            state.wch_status = Some(format!("loaded {n} watches"));
                        }
                        Err(e) => state.wch_status = Some(e),
                    }
                }
                if let Some(s) = &state.wch_status {
                    ui.weak(s);
                }
            });

            let mut remove = None;
            egui::ScrollArea::vertical()
                .id_salt("watch_list")
                .show(ui, |ui| {
                    for (i, w) in state.watches.iter_mut().enumerate() {
                        let cur = read_le_nes(nes, w.addr, w.size);
                        ui.horizontal(|ui| {
                            let mut frozen = w.frozen.is_some();
                            if ui
                                .checkbox(&mut frozen, "")
                                .on_hover_text("Freeze")
                                .changed()
                            {
                                w.frozen = frozen.then_some(cur);
                            }
                            ui.monospace(format!("${:04X}", w.addr));
                            ui.weak(w.size.label());
                            let width = (w.size.bytes() * 2) as usize;
                            let val = w.frozen.unwrap_or(cur);
                            ui.monospace(format!("{val:0width$X} ({val})"));
                            ui.label(&w.label);
                            if ui.small_button("x").clicked() {
                                remove = Some(i);
                            }
                        });
                    }
                });
            if let Some(i) = remove {
                state.watches.remove(i);
            }
        });
}

/// Parse a `$`/`0x`/decimal/bare-hex value (up to 32-bit) for the search
/// operand.
fn parse_value(s: &str) -> Option<u32> {
    let t = s.trim();
    t.strip_prefix('$')
        .or_else(|| t.strip_prefix("0x"))
        .map_or_else(
            || {
                t.parse::<u32>()
                    .ok()
                    .or_else(|| u32::from_str_radix(t, 16).ok())
            },
            |hex| u32::from_str_radix(hex, 16).ok(),
        )
}

fn parse_hex16(s: &str) -> Option<u16> {
    let t = s.trim().trim_start_matches('$').trim_start_matches("0x");
    u16::from_str_radix(t, 16).ok()
}

/// Serialize the watch list to a simple `.wch` text format
/// (`addr size label`, one per line), and write it via a save dialog.
#[cfg(not(target_arch = "wasm32"))]
fn save_wch(watches: &[WatchEntry]) -> String {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("Watch list", &["wch"])
        .set_file_name("watches.wch")
        .save_file()
    else {
        return "save cancelled".to_string();
    };
    let mut out = String::new();
    use std::fmt::Write as _;
    for w in watches {
        let sz = w.size.bytes();
        let _ = writeln!(out, "{:04X} {} {}", w.addr, sz, w.label);
    }
    match std::fs::write(&path, out) {
        Ok(()) => format!("saved {} watches", watches.len()),
        Err(e) => format!("save failed: {e}"),
    }
}

/// Load a `.wch` list via an open dialog. `None` if the user cancelled.
#[cfg(not(target_arch = "wasm32"))]
fn load_wch() -> Option<Result<Vec<WatchEntry>, String>> {
    let path = rfd::FileDialog::new()
        .add_filter("Watch list", &["wch"])
        .pick_file()?;
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => return Some(Err(format!("read failed: {e}"))),
    };
    Some(Ok(parse_wch(&text)))
}

/// Parse the `.wch` text format. Each non-empty line is `ADDR SIZE [LABEL...]`
/// (hex addr, decimal byte-size); malformed lines are skipped.
#[cfg(not(target_arch = "wasm32"))]
fn parse_wch(text: &str) -> Vec<WatchEntry> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.splitn(3, char::is_whitespace);
        let Some(addr) = it
            .next()
            .and_then(|a| u16::from_str_radix(a.trim_start_matches('$'), 16).ok())
        else {
            continue;
        };
        let size = match it.next().and_then(|s| s.parse::<u16>().ok()) {
            Some(2) => Size::U16,
            Some(4) => Size::U32,
            _ => Size::U8,
        };
        let label = it
            .next()
            .map_or_else(|| format!("${addr:04X}"), str::to_string);
        out.push(WatchEntry {
            addr,
            size,
            label,
            frozen: None,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn op_predicates() {
        assert!(Op::Eq.apply(5, 5));
        assert!(Op::Ne.apply(5, 6));
        assert!(Op::Lt.apply(4, 5));
        assert!(Op::Gt.apply(6, 5));
        assert!(Op::Le.apply(5, 5));
        assert!(Op::Ge.apply(5, 5));
        assert!(!Op::Lt.apply(5, 5));
    }

    #[test]
    fn read_le_sizes() {
        let ram = vec![0x34, 0x12, 0x78, 0x56, 0x00];
        assert_eq!(read_le(&ram, 0, Size::U8), 0x34);
        assert_eq!(read_le(&ram, 0, Size::U16), 0x1234);
        assert_eq!(read_le(&ram, 0, Size::U32), 0x5678_1234);
        // Out-of-range high bytes read as 0.
        assert_eq!(read_le(&ram, 4, Size::U16), 0x0000);
    }

    #[test]
    fn parse_value_forms() {
        assert_eq!(parse_value("$1234"), Some(0x1234));
        assert_eq!(parse_value("0xFF"), Some(0xFF));
        assert_eq!(parse_value("99"), Some(99));
        assert_eq!(parse_value("DEAD"), Some(0xDEAD)); // bare hex fallback
    }

    #[test]
    fn freeze_cheats_expand_multibyte_le() {
        let mut s = MemoryComparePanelState::default();
        s.watches.push(WatchEntry {
            addr: 0x0300,
            size: Size::U16,
            label: "hp".to_string(),
            frozen: Some(0x1234),
        });
        s.watches.push(WatchEntry {
            addr: 0x0010,
            size: Size::U8,
            label: "lives".to_string(),
            frozen: None, // not frozen → no cheat
        });
        let cheats = s.freeze_cheats();
        assert_eq!(cheats.len(), 2); // only the frozen 16-bit entry
        // Little-endian: low byte at $0300, high at $0301.
        assert_eq!(cheats[0].address, 0x0300);
        assert_eq!(cheats[0].value, 0x34);
        assert_eq!(cheats[1].address, 0x0301);
        assert_eq!(cheats[1].value, 0x12);
    }

    #[test]
    fn empty_freeze_is_empty() {
        let s = MemoryComparePanelState::default();
        assert!(s.freeze_cheats().is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn wch_round_trip() {
        let text = "0300 1 lives\n$0301 2 score\n# comment\n\n00FF 4 wide";
        let parsed = parse_wch(text);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].addr, 0x0300);
        assert_eq!(parsed[0].size, Size::U8);
        assert_eq!(parsed[0].label, "lives");
        assert_eq!(parsed[1].addr, 0x0301);
        assert_eq!(parsed[1].size, Size::U16);
        assert_eq!(parsed[2].size, Size::U32);
    }
}
