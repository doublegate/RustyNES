//! In-app Documentation browser (v1.5.0 "Lens" Workstream I10).
//!
//! A searchable, egui-native manual that reuses the SAME structured help-topic
//! registry as the `rustynes help` CLI / ratatui TUI ([`crate::cli::HELP_TOPICS`])
//! so the terminal help and the GUI manual can never drift. On top of the shared
//! CLI topics it adds:
//!
//! - **GUI-specific topics** (menu map, debugger devtools, settings) that only
//!   make sense in the desktop shell (the CLI help is terminal-only).
//! - An **About** section (version, license, author, build target + links).
//! - A **per-release CHANGELOG** selector (the embedded `CHANGELOG.md` split by
//!   its `## [version]` headings) so users can read what changed in any release.
//!
//! A `/`-style search box filters the left topic list (matching the topic title
//! OR body text). Native-only: the topic registry lives in the native-only
//! `cli` module (a browser tab has no terminal), so this whole panel is gated to
//! `cfg(not(target_arch = "wasm32"))` in the module tree.

use crate::cli::HELP_TOPICS;

/// The embedded changelog (split per release at render time). Small + factual,
/// already shipped in the repo root.
const CHANGELOG: &str = include_str!("../../../../CHANGELOG.md");

/// A documentation section the panel can display in its content pane.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DocSection {
    /// One of the shared `rustynes help` registry topics (by index).
    Topic(usize),
    /// A GUI-only topic (by index into [`GUI_TOPICS`]).
    Gui(usize),
    /// The About card.
    About,
    /// The changelog browser.
    Changelog,
}

/// GUI-only topics, authored here (the CLI registry is terminal-scoped). Each is
/// `(title, body)`; the body is plain text the content pane renders verbatim in
/// a monospace block, matching the CLI topic style.
const GUI_TOPICS: &[(&str, &str)] = &[
    ("Menus (GUI)", MENUS_BODY),
    ("Debugger & devtools", DEVTOOLS_BODY),
    ("Settings (GUI)", SETTINGS_BODY),
];

/// Persistent state of the Documentation window.
pub struct DocPanelState {
    /// The currently-selected section.
    selected: DocSection,
    /// The `/`-search filter text (matches topic title OR body).
    filter: String,
    /// The selected changelog release index (into the parsed list).
    changelog_idx: usize,
}

impl Default for DocPanelState {
    fn default() -> Self {
        Self {
            // Land on the first shared topic (Controls), like the CLI.
            selected: DocSection::Topic(0),
            filter: String::new(),
            changelog_idx: 0,
        }
    }
}

/// Split the embedded `CHANGELOG.md` into `(heading, body)` per `## [version]`
/// release section, in file order (newest first). Built lazily.
fn changelog_releases() -> &'static [(String, String)] {
    use std::sync::OnceLock;
    static RELEASES: OnceLock<Vec<(String, String)>> = OnceLock::new();
    RELEASES.get_or_init(|| {
        let mut out: Vec<(String, String)> = Vec::new();
        let mut cur_head: Option<String> = None;
        let mut cur_body = String::new();
        for line in CHANGELOG.lines() {
            if let Some(rest) = line.strip_prefix("## ") {
                // Flush the previous section.
                if let Some(h) = cur_head.take() {
                    out.push((h, std::mem::take(&mut cur_body)));
                }
                // The heading text, trimmed of the surrounding `[` `]` noise for
                // a clean selector label (keep the full line in the body header).
                cur_head = Some(rest.trim().to_string());
            } else if cur_head.is_some() {
                cur_body.push_str(line);
                cur_body.push('\n');
            }
        }
        if let Some(h) = cur_head.take() {
            out.push((h, cur_body));
        }
        out
    })
}

/// Render the Documentation window. `open` toggles visibility.
pub fn show(ctx: &egui::Context, open: &mut bool, state: &mut DocPanelState) {
    egui::Window::new("Documentation")
        .open(open)
        .resizable(true)
        .default_width(720.0)
        .default_height(520.0)
        .min_width(520.0)
        .show(ctx, |ui| {
            body(ui, state);
        });
}

fn body(ui: &mut egui::Ui, state: &mut DocPanelState) {
    // Two-pane layout: a left topic list (with the search filter) and the
    // remaining area as a scrollable content pane (egui 0.34 `Panel::left`
    // hosted inside the window's root `Ui` via `show_inside`).
    egui::Panel::left("doc_topics")
        .resizable(true)
        .default_size(190.0)
        .show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("\u{1F50D}"); // magnifier
                ui.add(
                    egui::TextEdit::singleline(&mut state.filter)
                        .hint_text("/ search")
                        .desired_width(f32::INFINITY),
                );
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    topic_list(ui, state);
                });
        });

    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            content(ui, state);
        });
}

/// Whether `needle` (already lowercased) matches a topic's title or body.
fn matches(needle: &str, title: &str, bodies: &[&str]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if title.to_ascii_lowercase().contains(needle) {
        return true;
    }
    bodies
        .iter()
        .any(|b| b.to_ascii_lowercase().contains(needle))
}

fn topic_list(ui: &mut egui::Ui, state: &mut DocPanelState) {
    let needle = state.filter.trim().to_ascii_lowercase();

    ui.label(egui::RichText::new("Manual").strong().weak());
    for (i, t) in HELP_TOPICS.iter().enumerate() {
        if matches(&needle, t.title, &[t.body]) {
            let sel = state.selected == DocSection::Topic(i);
            if ui.selectable_label(sel, t.title).clicked() {
                state.selected = DocSection::Topic(i);
            }
        }
    }

    ui.add_space(4.0);
    ui.label(egui::RichText::new("Desktop app").strong().weak());
    for (i, (title, gbody)) in GUI_TOPICS.iter().enumerate() {
        if matches(&needle, title, &[gbody]) {
            let sel = state.selected == DocSection::Gui(i);
            if ui.selectable_label(sel, *title).clicked() {
                state.selected = DocSection::Gui(i);
            }
        }
    }

    ui.add_space(4.0);
    ui.label(egui::RichText::new("Reference").strong().weak());
    if matches(&needle, "About", &[ABOUT_GUI_BODY]) {
        let sel = state.selected == DocSection::About;
        if ui.selectable_label(sel, "About").clicked() {
            state.selected = DocSection::About;
        }
    }
    if matches(&needle, "Changelog", &["changelog release history"]) {
        let sel = state.selected == DocSection::Changelog;
        if ui.selectable_label(sel, "Changelog").clicked() {
            state.selected = DocSection::Changelog;
        }
    }
}

fn content(ui: &mut egui::Ui, state: &mut DocPanelState) {
    match state.selected {
        DocSection::Topic(i) => {
            if let Some(t) = HELP_TOPICS.get(i) {
                ui.heading(t.title);
                ui.add_space(4.0);
                ui.monospace(t.body);
            }
        }
        DocSection::Gui(i) => {
            if let Some((title, gbody)) = GUI_TOPICS.get(i) {
                ui.heading(*title);
                ui.add_space(4.0);
                ui.monospace(*gbody);
            }
        }
        DocSection::About => about_card(ui),
        DocSection::Changelog => changelog_view(ui, state),
    }
}

fn about_card(ui: &mut egui::Ui) {
    ui.heading("RustyNES");
    ui.label(egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).weak());
    ui.add_space(6.0);
    ui.monospace(ABOUT_GUI_BODY);
    ui.add_space(8.0);
    ui.hyperlink_to(
        "Project home (github.com/doublegate/RustyNES)",
        "https://github.com/doublegate/RustyNES",
    );
    ui.hyperlink_to(
        "Playable web demo + API docs",
        "https://doublegate.github.io/RustyNES/",
    );
}

fn changelog_view(ui: &mut egui::Ui, state: &mut DocPanelState) {
    let releases = changelog_releases();
    if releases.is_empty() {
        ui.label("No changelog available.");
        return;
    }
    state.changelog_idx = state.changelog_idx.min(releases.len() - 1);
    ui.heading("Changelog");
    ui.horizontal(|ui| {
        ui.label("Release:");
        egui::ComboBox::from_id_salt("doc-changelog-release")
            .selected_text(releases[state.changelog_idx].0.clone())
            .show_ui(ui, |ui| {
                for (i, (head, _)) in releases.iter().enumerate() {
                    ui.selectable_value(&mut state.changelog_idx, i, head.clone());
                }
            });
    });
    ui.separator();
    ui.monospace(&releases[state.changelog_idx].1);
}

// ===========================================================================
// GUI-only topic bodies (authored here; the CLI registry is terminal-scoped).
// ===========================================================================

const MENUS_BODY: &str = "\
Menu bar (File / Emulation / View / Tools / Debug / Help)
=========================================================

File ......... Open ROM (F12), Open Recent, Close ROM, Save States
               submenu, Take Screenshot + Copy to Clipboard, Quit.
Emulation .... Pause/Resume, Reset (F2), Power Cycle (F3), Frame
               Advance (Backslash, while paused), Fast Forward (hold
               Tab), Run-Ahead, Speed presets, Region, Vs. Insert Coin
               (F10), Swap Disk Side (F9, FDS).
View ......... Settings, Theme, 8:7 Pixel Aspect, Hide Overscan,
               Fullscreen (F11), Window Size, Show FPS, Pause When
               Unfocused, Show Menu Bar (M).
Tools ........ Cheats, Movies (TAS), Netplay, RetroAchievements, Input
               Display, Input Miniatures, NSF Player, Replay / TAS,
               ROM Database, HD Pack.
Debug ........ Show Debugger (`), Performance Monitor, and the chip /
               state inspectors: CPU / PPU / APU / Memory / Memory
               Compare / OAM / Mapper / Trace Logger / Event Viewer /
               Lua Script.
Help ......... Documentation, Keyboard Shortcuts, About.

Menu items show their bound accelerator key and are enabled/disabled
in context (e.g. Frame Advance is only active while paused; Vs. Insert
Coin only appears for Vs. System games). Tool windows open as floating
panels without forcing the debugger overlay on.";

const DEVTOOLS_BODY: &str = "\
Debugger & devtools
===================

Press the backtick key (`) to toggle the chip-inspector overlay; the
dedicated tool windows (below) open from the Tools / Debug menus
without it.

Chip inspectors (Debug menu)
  CPU ............ registers, disassembly, step, breakpoints, event
                   breakpoints, loaded-symbol labels.
  PPU ............ nametable / pattern / OAM / palette viewers, plus a
                   per-scanline scroll-write trace and CHR -> PNG export.
  APU ............ per-channel scope, volume meters, register dump.
  Memory ......... CPU + PPU hex viewer with go-to-address.
  Memory Compare . diff two RAM snapshots (cheat hunting).
  Mapper ......... id/submapper, board, tier, ROM/RAM sizes + banks,
                   live bank windows, mirroring, IRQ + audio/NVRAM.
  Trace Logger ... ring of executed instructions, export to file.
  Event Viewer ... a 341x312 per-dot read/write heatmap.

Visualizers (Tools menu)
  Input Display / Input Miniatures . live controller + device state.
  NSF Player ....................... NSF/NSFe playback + scope.
  Replay / TAS ..................... movie control + seek.

Symbols: Debug -> Load Symbols accepts .sym / .mlb / .nl label files
to annotate the disassembler, breakpoints, and trace.

The devtools are output-only and never perturb emulation, so the
determinism contract and AccuracyCoin are unaffected.";

const SETTINGS_BODY: &str = "\
Settings (View -> Settings)
===========================

A tabbed window; every control auto-saves to config.toml on change
(no separate Save step). Tabs:

  Video ...... theme, 8:7 pixel aspect, overscan, custom .pal palette,
               NTSC filter selection, present mode / pacing.
  Shaders .... the composable shader stack (CRT / scanline / NTSC
               passes, run top to bottom) + CRT preset bank.
  Audio ...... master volume / mute, per-channel mute + volume, a
               five-band graphic EQ, output latency + DRC.
  Input ...... rebindable controller + system-hotkey bindings, Four
               Score, gamepad deadzone, turbo/autofire, port-2 device.
               'Export config...' writes a copy to a chosen .toml file.
  Emulation .. run-ahead depth, rewind buffer size, default region.

Settings live in the OS config directory under RustyNES/config.toml.";

const ABOUT_GUI_BODY: &str = "\
RustyNES - a cycle-accurate Nintendo Entertainment System emulator
written in pure Rust (winit + wgpu + cpal + egui).

  License ...... MIT OR Apache-2.0
  Author ....... DoubleGate
  Accuracy ..... AccuracyCoin 100% (139/139); nestest 0-diff;
                 blargg / kevtris suites green.
  Frontend ..... always-on egui shell, dedicated emulation thread,
                 display-sync pacing, lock-free audio ring.
  Features ..... 100+ mapper families, FDS, Vs. System / PlayChoice-10,
                 rollback netplay, RetroAchievements, TAS movies,
                 save-states, rewind, run-ahead, Lua scripting.

Run `rustynes help <topic>` in a terminal for the same manual on the
command line, or `rustynes help` for the interactive browser.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changelog_splits_into_releases() {
        let r = changelog_releases();
        assert!(!r.is_empty(), "changelog must parse some release sections");
        // The Unreleased section heads the file.
        assert!(
            r.iter().any(|(h, _)| h.contains("Unreleased")),
            "an [Unreleased] section should be present"
        );
        // A real release section (e.g. 1.4.0) should parse with non-empty body.
        assert!(
            r.iter()
                .any(|(h, b)| h.contains("1.4.0") && !b.trim().is_empty()),
            "the 1.4.0 release section should parse with body text"
        );
    }

    #[test]
    fn filter_matches_title_and_body() {
        assert!(matches("", "anything", &["body"]));
        assert!(matches("ontrol", "Controls", &["body"])); // title substring
        assert!(matches("zapper", "Devices", &["the zapper light gun"])); // body substring
        assert!(!matches("xyzzy", "Controls", &["nothing here"]));
    }

    #[test]
    fn gui_topics_are_present() {
        assert_eq!(GUI_TOPICS.len(), 3);
        assert!(GUI_TOPICS.iter().any(|(t, _)| *t == "Menus (GUI)"));
    }
}
