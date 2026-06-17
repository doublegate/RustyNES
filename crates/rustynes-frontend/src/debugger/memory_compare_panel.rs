//! Memory Compare — cheat-hunting RAM search (v1.3.0 Workstream C, C3).
//!
//! The classic emulator "memory search": snapshot the 2 KB CPU work RAM
//! (`$0000-$07FF`) as a baseline, then repeatedly narrow a candidate set by how
//! each byte changed since the previous snapshot — *changed* / *unchanged* /
//! *increased* / *decreased* / *equals N*. ("Search for the value that went
//! DOWN when you lost a life, then UNCHANGED while idle, ..." until one address
//! remains — feed it to the raw-RAM cheat panel.)
//!
//! Read-only: it samples RAM via the side-effect-free `cpu_bus_peek`, holds its
//! own baseline copy, and never writes the core — determinism is unaffected.

use rustynes_core::Nes;

/// The NES internal work RAM: `$0000-$07FF` (2 KB; mirrored to `$1FFF`). Cheat
/// hunting only ever targets this region.
const RAM_LEN: u16 = 0x0800;
/// Cap on rows drawn in the candidate list (the count is always exact; only the
/// rendered rows are bounded so a fresh, unfiltered search can't spam the UI).
const MAX_SHOWN: usize = 256;

/// How a candidate byte must have moved (vs the baseline) to survive a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Filter {
    Changed,
    Unchanged,
    Increased,
    Decreased,
    EqualsValue,
}

impl Filter {
    const fn label(self) -> &'static str {
        match self {
            Self::Changed => "changed",
            Self::Unchanged => "unchanged",
            Self::Increased => "increased",
            Self::Decreased => "decreased",
            Self::EqualsValue => "equals value",
        }
    }

    /// Does `cur` (vs `base`) satisfy this filter? `eq` is the target for
    /// `EqualsValue` (ignored otherwise).
    const fn matches(self, base: u8, cur: u8, eq: u8) -> bool {
        match self {
            Self::Changed => cur != base,
            Self::Unchanged => cur == base,
            Self::Increased => cur > base,
            Self::Decreased => cur < base,
            Self::EqualsValue => cur == eq,
        }
    }
}

/// Persistent state of the Memory Compare panel.
#[derive(Debug)]
pub struct MemoryComparePanelState {
    /// Baseline RAM image from the last snapshot (`RAM_LEN` bytes); `None`
    /// until the first search is started.
    baseline: Option<Vec<u8>>,
    /// Addresses still matching every applied step; `None` until a search starts
    /// (a fresh search seeds it with all of `$0000-$07FF`).
    candidates: Option<Vec<u16>>,
    /// The selected comparison for the next step.
    filter: Filter,
    /// Decimal/hex entry for the `EqualsValue` target.
    eq_text: String,
    /// Number of narrowing steps applied since the search started.
    steps: u32,
}

impl Default for MemoryComparePanelState {
    fn default() -> Self {
        Self {
            baseline: None,
            candidates: None,
            filter: Filter::Changed,
            eq_text: String::new(),
            steps: 0,
        }
    }
}

impl MemoryComparePanelState {
    /// Snapshot current RAM and (re)seed the candidate set with every address.
    fn start(&mut self, nes: &mut Nes) {
        self.baseline = Some(snapshot(nes));
        self.candidates = Some((0..RAM_LEN).collect());
        self.steps = 0;
    }

    /// Apply the selected filter: keep candidates whose current byte satisfies
    /// the comparison against the baseline, then re-snapshot so the next step
    /// compares against this one (the standard iterative-search semantics).
    fn apply(&mut self, nes: &mut Nes) {
        let (Some(base), Some(cands)) = (self.baseline.as_ref(), self.candidates.as_mut()) else {
            return;
        };
        let eq = parse_byte(&self.eq_text).unwrap_or(0);
        let filter = self.filter;
        // Snapshot once, then reuse it for both the filter compare and the next
        // baseline (avoids a second full 2 KB peek pass).
        let cur_ram = snapshot(nes);
        cands.retain(|&addr| filter.matches(base[addr as usize], cur_ram[addr as usize], eq));
        self.baseline = Some(cur_ram);
        self.steps += 1;
    }
}

/// Copy the 2 KB work RAM via the (logically side-effect-free) peek. Takes
/// `&mut Nes` because `cpu_bus_peek` mirrors the bus read path's `&mut self`.
fn snapshot(nes: &mut Nes) -> Vec<u8> {
    (0..RAM_LEN).map(|a| nes.cpu_bus_peek(a)).collect()
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
        .default_size([300.0, 460.0])
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .button("New search")
                    .on_hover_text(
                        "Snapshot current RAM ($0000-$07FF) and reset the candidate \
                         set to all 2048 addresses.",
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
                egui::ComboBox::from_id_salt("memcmp_filter")
                    .selected_text(state.filter.label())
                    .show_ui(ui, |ui| {
                        for f in [
                            Filter::Changed,
                            Filter::Unchanged,
                            Filter::Increased,
                            Filter::Decreased,
                            Filter::EqualsValue,
                        ] {
                            ui.selectable_value(&mut state.filter, f, f.label());
                        }
                    });
                if state.filter == Filter::EqualsValue {
                    ui.label("=");
                    ui.add(
                        egui::TextEdit::singleline(&mut state.eq_text)
                            .desired_width(48.0)
                            .hint_text("$00"),
                    );
                }
            });
            ui.separator();

            let Some(cands) = state.candidates.as_ref() else {
                ui.label(
                    egui::RichText::new("Press \"New search\" to snapshot RAM and begin.").weak(),
                );
                return;
            };
            ui.label(format!(
                "candidates: {}   (step {})",
                cands.len(),
                state.steps
            ));
            let base = state.baseline.as_ref();
            egui::ScrollArea::vertical().show(ui, |ui| {
                for &addr in cands.iter().take(MAX_SHOWN) {
                    let cur = nes.cpu_bus_peek(addr);
                    let b = base.map_or(cur, |bl| bl[addr as usize]);
                    ui.monospace(format!("${addr:04X}   {b:02X} -> {cur:02X}   ({cur:3})"));
                }
                if cands.len() > MAX_SHOWN {
                    ui.label(
                        egui::RichText::new(format!(
                            "... {} more (narrow the search to see them all)",
                            cands.len() - MAX_SHOWN
                        ))
                        .weak(),
                    );
                }
            });
        });
}

/// Parse a `$`/`0x`/decimal byte for the `EqualsValue` target.
fn parse_byte(s: &str) -> Option<u8> {
    let t = s.trim();
    t.strip_prefix('$')
        .or_else(|| t.strip_prefix("0x"))
        .map_or_else(
            || {
                t.parse::<u8>()
                    .ok()
                    .or_else(|| u8::from_str_radix(t, 16).ok())
            },
            |hex| u8::from_str_radix(hex, 16).ok(),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_predicates() {
        assert!(Filter::Changed.matches(0x10, 0x11, 0));
        assert!(!Filter::Changed.matches(0x10, 0x10, 0));
        assert!(Filter::Unchanged.matches(0x10, 0x10, 0));
        assert!(Filter::Increased.matches(0x10, 0x20, 0));
        assert!(!Filter::Increased.matches(0x20, 0x10, 0));
        assert!(Filter::Decreased.matches(0x20, 0x10, 0));
        assert!(Filter::EqualsValue.matches(0x00, 0x42, 0x42));
        assert!(!Filter::EqualsValue.matches(0x00, 0x41, 0x42));
    }

    #[test]
    fn parse_byte_forms() {
        assert_eq!(parse_byte("$0A"), Some(10));
        assert_eq!(parse_byte("0x10"), Some(16));
        assert_eq!(parse_byte("42"), Some(42));
        assert_eq!(parse_byte("FF"), Some(255)); // bare hex fallback
        assert_eq!(parse_byte("999"), None); // out of u8 range, not valid hex
    }
}
