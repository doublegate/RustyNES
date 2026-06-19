//! In-app Documentation browser (v1.5.0 "Lens" Workstream I10; overhauled in
//! v1.7.0 "Forge" beta.5, #53).
//!
//! A searchable, egui-native manual that reuses the SAME structured help-topic
//! registry as the `rustynes help` CLI / ratatui TUI ([`crate::cli::HELP_TOPICS`])
//! so the terminal help and the GUI manual can never drift. On top of the shared
//! CLI topics it adds:
//!
//! - **GUI-specific topics** (menu map, debugger devtools, settings) that only
//!   make sense in the desktop shell (the CLI help is terminal-only), each with
//!   navigable **sub-pages** (e.g. one per chip inspector) so the left-sidebar
//!   tree resolves at every depth instead of returning nothing (#53.2).
//! - An **About** section (version, license, author, build target + links).
//! - A **per-release CHANGELOG** selector (the embedded `CHANGELOG.md` split by
//!   its `## [version]` headings) so users can read what changed in any release.
//!
//! v1.7.0 "Forge" beta.5 (#53) fixed four long-standing pane defects:
//!
//! 1. **Word-wrap** — bodies render through [`render_body`], which wraps every
//!    paragraph to the pane width (the old `ui.monospace(body)` overflowed).
//! 2. **Sub-level navigation** — GUI topics expose child pages
//!    ([`GuiTopic::children`]); the sidebar renders the tree and every node
//!    resolves to content.
//! 3. **Colorization** — headings, section underlines, indented "code" lines,
//!    and bullets are tinted for readability ([`render_body`]).
//! 4. **Intra-doc hyperlinks** — a `[[id]]` / `[[label|id]]` token in a body
//!    becomes a clickable link that navigates to another doc page (resolved by
//!    [`resolve_link`]).
//!
//! A `/`-style search box filters the left topic tree (matching the topic title
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
    /// A GUI-only top-level topic (by index into [`GUI_TOPICS`]).
    Gui(usize),
    /// A GUI sub-page: `(gui topic index, child index)` (#53.2).
    GuiChild(usize, usize),
    /// The About card.
    About,
    /// The changelog browser.
    Changelog,
}

/// A GUI sub-page (authored here; the CLI registry is terminal-scoped). `id` is
/// the stable intra-doc link target (#53.4); `title` is the sidebar/heading
/// label; `body` is the plain-text body rendered by [`render_body`].
struct SubPage {
    /// Stable link id (matched by `[[id]]` tokens).
    id: &'static str,
    /// Sidebar + heading title.
    title: &'static str,
    /// Body text (colorized + wrapped at render time).
    body: &'static str,
}

/// A GUI-only top-level topic with optional navigable child pages.
struct GuiTopic {
    /// Stable link id (matched by `[[id]]` tokens).
    id: &'static str,
    /// Sidebar + heading title.
    title: &'static str,
    /// Overview body shown when the parent node is selected.
    body: &'static str,
    /// Navigable sub-pages (empty for a leaf topic).
    children: &'static [SubPage],
}

/// GUI-only topics, authored here. The CLI registry is terminal-scoped; these
/// only make sense in the desktop shell. Each may carry sub-pages (#53.2).
const GUI_TOPICS: &[GuiTopic] = &[
    GuiTopic {
        id: "menus",
        title: "Menus (GUI)",
        body: MENUS_BODY,
        children: &[],
    },
    GuiTopic {
        id: "devtools",
        title: "Debugger & devtools",
        body: DEVTOOLS_BODY,
        children: DEVTOOLS_CHILDREN,
    },
    GuiTopic {
        id: "settings",
        title: "Settings (GUI)",
        body: SETTINGS_BODY,
        children: &[],
    },
    GuiTopic {
        id: "tas",
        title: "TAS & movies",
        body: TAS_BODY,
        children: TAS_CHILDREN,
    },
    GuiTopic {
        id: "scripting-gui",
        title: "Lua scripting & automation",
        body: SCRIPTING_GUI_BODY,
        children: &[],
    },
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
        .default_width(760.0)
        .default_height(540.0)
        .min_width(560.0)
        .show(ctx, |ui| {
            body(ui, state);
        });
}

fn body(ui: &mut egui::Ui, state: &mut DocPanelState) {
    // Two-pane layout: a left topic tree (with the search filter) and the
    // remaining area as a scrollable content pane.
    egui::Panel::left("doc_topics")
        .resizable(true)
        .default_size(210.0)
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

    // #53.4 — link clicks return a navigation target applied AFTER the render
    // pass (so the borrow on `state.selected` is released first).
    let mut nav: Option<DocSection> = None;
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            nav = content(ui, state);
        });
    if let Some(target) = nav {
        state.selected = target;
    }
}

/// Whether `needle` (already lowercased) matches a topic's title or body.
/// Allocation-free case-insensitive substring test — avoids a per-frame
/// `to_ascii_lowercase()` heap allocation for every topic body during search.
fn contains_ci(haystack: &str, needle: &str) -> bool {
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    n.len() <= h.len() && h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
}

fn matches(needle: &str, title: &str, bodies: &[&str]) -> bool {
    if needle.is_empty() {
        return true;
    }
    contains_ci(title, needle) || bodies.iter().any(|b| contains_ci(b, needle))
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
    for (i, topic) in GUI_TOPICS.iter().enumerate() {
        // A parent matches if its own text matches OR any child matches (so a
        // filtered search still surfaces the branch leading to a hit).
        let child_bodies: Vec<&str> = topic.children.iter().map(|c| c.body).collect();
        let parent_match = matches(&needle, topic.title, &[topic.body])
            || topic
                .children
                .iter()
                .any(|c| matches(&needle, c.title, &[c.body]))
            || !child_bodies.is_empty() && matches(&needle, "", &child_bodies);
        if parent_match {
            let sel = state.selected == DocSection::Gui(i);
            if ui.selectable_label(sel, topic.title).clicked() {
                state.selected = DocSection::Gui(i);
            }
            // #53.2 — render the navigable child pages, indented under the
            // parent. Each resolves to its own content (no more dead nodes).
            for (ci, child) in topic.children.iter().enumerate() {
                if matches(&needle, child.title, &[child.body]) {
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        let csel = state.selected == DocSection::GuiChild(i, ci);
                        if ui
                            .selectable_label(csel, egui::RichText::new(child.title).size(13.0))
                            .clicked()
                        {
                            state.selected = DocSection::GuiChild(i, ci);
                        }
                    });
                }
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

/// Render the content pane for the current selection. Returns a navigation
/// target if the user clicked an intra-doc link (#53.4), applied by the caller.
fn content(ui: &mut egui::Ui, state: &mut DocPanelState) -> Option<DocSection> {
    match state.selected {
        DocSection::Topic(i) => {
            if let Some(t) = HELP_TOPICS.get(i) {
                ui.heading(t.title);
                ui.add_space(4.0);
                return render_body(ui, t.body);
            }
            None
        }
        DocSection::Gui(i) => {
            if let Some(topic) = GUI_TOPICS.get(i) {
                ui.heading(topic.title);
                ui.add_space(4.0);
                let nav = render_body(ui, topic.body);
                // A small index of the sub-pages, as inline links (#53.2/#53.4).
                if !topic.children.is_empty() {
                    ui.add_space(6.0);
                    ui.separator();
                    ui.label(egui::RichText::new("In this section").strong());
                    for (ci, child) in topic.children.iter().enumerate() {
                        if ui.link(format!("\u{2192} {}", child.title)).clicked() {
                            return Some(DocSection::GuiChild(i, ci));
                        }
                    }
                }
                return nav;
            }
            None
        }
        DocSection::GuiChild(i, ci) => {
            if let Some(child) = GUI_TOPICS.get(i).and_then(|t| t.children.get(ci)) {
                // Breadcrumb: a clickable parent-section link + the child title
                // (#53.4 — a real navigation target back to the parent).
                let mut nav = None;
                ui.horizontal(|ui| {
                    if let Some(parent) = GUI_TOPICS.get(i) {
                        if ui.link(parent.title).clicked() {
                            nav = Some(DocSection::Gui(i));
                        }
                        ui.label(egui::RichText::new("  /  ").weak());
                    }
                    ui.heading(child.title);
                });
                ui.add_space(4.0);
                let body_nav = render_body(ui, child.body);
                return nav.or(body_nav);
            }
            None
        }
        DocSection::About => {
            about_card(ui);
            None
        }
        DocSection::Changelog => {
            changelog_view(ui, state);
            None
        }
    }
}

/// Resolve an intra-doc link id (from a `[[id]]` token) to a [`DocSection`].
/// Matches shared-topic ids, GUI topic ids, and GUI sub-page ids (#53.4).
fn resolve_link(id: &str) -> Option<DocSection> {
    let id = id.trim();
    if id.eq_ignore_ascii_case("about") {
        return Some(DocSection::About);
    }
    if id.eq_ignore_ascii_case("changelog") {
        return Some(DocSection::Changelog);
    }
    if let Some(i) = HELP_TOPICS
        .iter()
        .position(|t| t.id.eq_ignore_ascii_case(id))
    {
        return Some(DocSection::Topic(i));
    }
    for (i, topic) in GUI_TOPICS.iter().enumerate() {
        if topic.id.eq_ignore_ascii_case(id) {
            return Some(DocSection::Gui(i));
        }
        for (ci, child) in topic.children.iter().enumerate() {
            if child.id.eq_ignore_ascii_case(id) {
                return Some(DocSection::GuiChild(i, ci));
            }
        }
    }
    None
}

/// Colorize + word-wrap a plain-text doc body, rendering `[[id]]` /
/// `[[label|id]]` tokens as clickable intra-doc links. Returns a navigation
/// target when a link is clicked (#53.1/#53.3/#53.4).
///
/// Recognised line shapes (heuristic, matching the CLI body style):
/// - a heading line immediately followed by a line of `===` or `---` → a
///   colored heading (the underline line is consumed);
/// - a line beginning with two-or-more spaces → an indented "code"/detail line
///   (dimmer monospace);
/// - everything else → a wrapped paragraph line.
fn render_body(ui: &mut egui::Ui, body: &str) -> Option<DocSection> {
    // Heading / underline / code tints, picked to read on both light + dark
    // themes (the panel inherits the app theme).
    const HEADING: egui::Color32 = egui::Color32::from_rgb(0x6C, 0xB4, 0xF0);
    const CODE: egui::Color32 = egui::Color32::from_rgb(0xC0, 0xA8, 0x70);
    const BULLET: egui::Color32 = egui::Color32::from_rgb(0x9C, 0xD0, 0x9C);

    let mut nav: Option<DocSection> = None;
    let lines: Vec<&str> = body.lines().collect();
    let mut idx = 0;
    while idx < lines.len() {
        let line = lines[idx];
        let next = lines.get(idx + 1).copied().unwrap_or("");
        let underline = !next.is_empty()
            && (next.bytes().all(|b| b == b'=') || next.bytes().all(|b| b == b'-'));
        if !line.trim().is_empty() && underline {
            // Heading (consume the underline line too).
            ui.label(
                egui::RichText::new(line.trim_end())
                    .heading()
                    .strong()
                    .color(HEADING),
            );
            idx += 2;
            continue;
        }
        if line.starts_with("  ") {
            // Indented detail / code line → dimmer monospace, but still scan it
            // for links so cross-references in tables work.
            if let Some(target) = render_line_with_links(ui, line, CODE, true) {
                nav = nav.or(Some(target));
            }
        } else if line.trim_start().starts_with("- ") || line.trim_start().starts_with("* ") {
            // A bullet line — tint the marker.
            if let Some(target) = render_line_with_links(ui, line, BULLET, false) {
                nav = nav.or(Some(target));
            }
        } else if line.trim().is_empty() {
            ui.add_space(4.0);
        } else if let Some(target) =
            render_line_with_links(ui, line, ui.visuals().text_color(), false)
        {
            nav = nav.or(Some(target));
        }
        idx += 1;
    }
    nav
}

/// Render one body line, splitting out `[[id]]` / `[[label|id]]` link tokens as
/// clickable links and the rest as colored (optionally monospace) text. Wraps
/// to the pane width. Returns a navigation target if a link was clicked.
fn render_line_with_links(
    ui: &mut egui::Ui,
    line: &str,
    color: egui::Color32,
    monospace: bool,
) -> Option<DocSection> {
    // Fast path: no link token → one wrapped label (cheapest + wraps cleanly).
    if !line.contains("[[") {
        let mut rt = egui::RichText::new(line).color(color);
        if monospace {
            rt = rt.monospace();
        }
        ui.add(egui::Label::new(rt).wrap());
        return None;
    }

    let mut nav: Option<DocSection> = None;
    // Build the line as a horizontal wrapped run of text spans + link widgets.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let mut rest = line;
        while let Some(start) = rest.find("[[") {
            let (head, after) = rest.split_at(start);
            if !head.is_empty() {
                let mut rt = egui::RichText::new(head).color(color);
                if monospace {
                    rt = rt.monospace();
                }
                ui.label(rt);
            }
            let after = &after[2..]; // skip "[["
            if let Some(end) = after.find("]]") {
                let token = &after[..end];
                // `label|id` or just `id` (label == id).
                let (label, id) = token.split_once('|').unwrap_or((token, token));
                if ui.link(label).clicked()
                    && let Some(target) = resolve_link(id)
                {
                    nav = Some(target);
                }
                rest = &after[end + 2..];
            } else {
                // Unterminated token — render the rest verbatim and stop.
                ui.label(egui::RichText::new(rest).color(color));
                rest = "";
                break;
            }
        }
        if !rest.is_empty() {
            let mut rt = egui::RichText::new(rest).color(color);
            if monospace {
                rt = rt.monospace();
            }
            ui.label(rt);
        }
    });
    nav
}

fn about_card(ui: &mut egui::Ui) {
    ui.heading("RustyNES");
    ui.label(egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).weak());
    ui.add_space(6.0);
    render_body(ui, ABOUT_GUI_BODY);
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
    render_body(ui, &releases[state.changelog_idx].1);
}

// ===========================================================================
// GUI-only topic bodies (authored here; the CLI registry is terminal-scoped).
// `[[id]]` / `[[label|id]]` tokens are intra-doc links (#53.4).
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
Tools ........ Cheats, Movies (TAS) + Import/Export (.fm2/.bk2), A/V
               Recording, Netplay, RetroAchievements, Input Display,
               NSF Player, Replay / TAS, TAStudio, Export Last 30s
               (.rnm), ROM Database, HD Pack (load / Pixel Inspector /
               Builder).
Debug ........ Show Debugger, Performance Monitor, and the chip /
               state inspectors: CPU / PPU / APU / Memory / Memory
               Compare / OAM / Mapper / Trace Logger / Watch /
               Breakpoints / Event Viewer / Lua Script, plus Cartridge
               Info / Header Editor and Load/Clear Symbols.
Help ......... Documentation, Keyboard Shortcuts, About.

Every menu entry carries a Font-Awesome icon. Items show their bound
accelerator key and are enabled/disabled in context (e.g. Frame Advance
is only active while paused; Vs. Insert Coin only appears for Vs. System
games). Tool windows open as floating panels without forcing the
debugger overlay on.

Note (v1.7.0): the debugger toolbar HUD was removed; the backtick (`)
key now toggles the status-bar RetroAchievements read-out between its
compact and long-form variants. Open the debugger overlay from
Debug -> Show Debugger.

See also: [[Debugger & devtools|devtools]], [[Settings (GUI)|settings]],
[[TAS & movies|tas]].";

const DEVTOOLS_BODY: &str = "\
Debugger & devtools
===================

Every panel opens from the Tools / Debug menus (the v1.7.0 build removed
the old debugger toolbar HUD). Toggle the deep chip-inspector overlay
from Debug -> Show Debugger; the dedicated tool windows open whether or
not it is visible.

The devtools are output-only and never perturb emulation, so the
determinism contract and AccuracyCoin (100%, 139/139) are unaffected.

Sub-pages (also in the sidebar tree):
  - [[CPU & disassembly|dt-cpu]]
  - [[PPU & video viewers|dt-ppu]]
  - [[APU & audio|dt-apu]]
  - [[Memory & search|dt-mem]]
  - [[Mapper inspector|dt-mapper]]
  - [[Trace, Watch & breakpoints|dt-trace]]
  - [[Event viewer|dt-events]]
  - [[Visualizers|dt-vis]]
  - [[Symbols & source maps|dt-sym]]

Symbols: Debug -> Load Symbols accepts .sym / Mesen .mlb / FCEUX .nl
label files to annotate the disassembler, breakpoints, and trace
(details under [[Symbols & source maps|dt-sym]]).";

const DEVTOOLS_CHILDREN: &[SubPage] = &[
    SubPage {
        id: "dt-cpu",
        title: "CPU & disassembly",
        body: "\
CPU inspector (Debug -> CPU)
============================

  Registers ...... A / X / Y / S / P (flag bits) + PC.
  Disassembly .... a scrollable 6502 listing around PC, with an inline
                   assembler (edit bytes in place) and loaded-symbol
                   labels from [[Symbols & source maps|dt-sym]].
  Step ........... step one instruction / over / out, plus the live
                   Call Stack section (v1.7.0 Workstream C) built from
                   the observational exec / interrupt logs.
  Breakpoints .... PC breakpoints + event breakpoints (NMI / IRQ /
                   reset); conditional breakpoints live in the Watch
                   panel ([[Trace, Watch & breakpoints|dt-trace]]).

Source-line annotations (ca65/cc65 .dbg) overlay the original source on
the disassembly when a matching map is loaded.",
    },
    SubPage {
        id: "dt-ppu",
        title: "PPU & video viewers",
        body: "\
PPU inspector (Debug -> PPU)
============================

  Nametables ..... all four logical nametables with the current
                   mirroring + a live scroll cursor.
  Pattern tables . both CHR pattern tables, palette-tinted.
  Palette ........ the 32-entry palette RAM, click to inspect.
  OAM ............ see the dedicated [[Memory & search|dt-mem]] note;
                   the OAM panel (Debug -> OAM) lists + grids sprites.
  Scroll trace ... a per-scanline log of mid-frame scroll writes.
  Export ......... dump the pattern tables to PNG.",
    },
    SubPage {
        id: "dt-apu",
        title: "APU & audio",
        body: "\
APU inspector (Debug -> APU)
============================

  Channel scopes . per-channel waveform scopes (pulse 1/2, triangle,
                   noise, DMC) + any mapper expansion-audio channels.
  Volume meters .. live per-channel output levels.
  Register dump .. the raw $4000-$4017 APU register state.

Per-channel mute + gain and the 5-band EQ live in Settings -> Audio
([[Settings (GUI)|settings]]). NSF/NSFe playback has its own player
under Tools -> NSF Player ([[Visualizers|dt-vis]]).",
    },
    SubPage {
        id: "dt-mem",
        title: "Memory & search",
        body: "\
Memory tools (Debug menu)
=========================

  Memory ......... a CPU + PPU bus hex viewer with go-to-address; the
                   access-counter heatmap tints each byte by read /
                   write / execute frequency (v1.7.0 Workstream C2).
  Memory Compare . snapshot RAM and diff two captures to hunt cheats
                   (equal / less / greater / changed filters).
  OAM ............ the sprite list + a visual sprite grid.

In RetroAchievements hardcore mode the Memory viewer is disabled (it is
a RAM-watch surface); see [[RetroAchievements|netplay]] in the manual.",
    },
    SubPage {
        id: "dt-mapper",
        title: "Mapper inspector",
        body: "\
Mapper inspector (Debug -> Mapper)
==================================

  Identity ....... mapper id + submapper, board name, accuracy tier
                   (Core / Curated / BestEffort), PRG/CHR ROM + RAM
                   sizes.
  Bank windows ... the live PRG + CHR bank mapping (which physical bank
                   each CPU/PPU window points at, updated per frame).
  Mirroring ...... the current nametable mirroring.
  IRQ ............ the mapper IRQ counter / latch / enable state for
                   the IRQ family (MMC3 A12, MMC5 scanline, VRC, FME-7,
                   Namco 163, ...).
  Audio / NVRAM .. expansion-audio chip name + battery-backed RAM flag.

See [[Mappers|mappers]] in the manual for the full family list.",
    },
    SubPage {
        id: "dt-trace",
        title: "Trace, Watch & breakpoints",
        body: "\
Trace Logger + Watch panel (Debug menu)
=======================================

  Trace Logger ... a ring of executed instructions you can export to a
                   file for offline analysis.
  Watch .......... a Mesen2-class expression evaluator over CPU / PPU /
                   memory / access-context tokens (the C-style operator
                   set). It backs:
                     - conditional breakpoints,
                     - read / write / execute watchpoints,
                     - a live watch window,
                     - conditional trace.

The expression evaluator + per-frame observational replay are
frontend-only and do not perturb emulation.",
    },
    SubPage {
        id: "dt-events",
        title: "Event viewer",
        body: "\
Event viewer (Debug -> Event Viewer)
====================================

A 341x312 per-dot read/write heatmap of the current frame: each PPU dot
(x = dot, y = scanline) is tinted by the register accesses that landed
on it, so mid-scanline $2005/$2006 writes, sprite-zero hits, and mapper
IRQ points are visible in their exact timing position.

Pairs naturally with the [[PPU & video viewers|dt-ppu]] scroll trace.",
    },
    SubPage {
        id: "dt-vis",
        title: "Visualizers",
        body: "\
Visualizers (Tools menu)
========================

  Input Display .. the consolidated controller HUD (v1.7.0) — the
                   standard pads PLUS every expansion peripheral
                   (Zapper, Arkanoid Vaus, SNES mouse, Power Pad /
                   Family Trainer mat, Family BASIC / Subor keyboard,
                   Konami / Bandai Hyper Shot, Four Score multitap),
                   with real-time button / axis state.
  NSF Player ..... NSF / NSFe playback + a waveform scope.
  Replay / TAS ... movie playback control + seek + device topology;
                   the deeper editor is [[TAStudio|tas-studio]].",
    },
    SubPage {
        id: "dt-sym",
        title: "Symbols & source maps",
        body: "\
Symbols & source maps (Debug menu)
==================================

  Load Symbols ... pick a .sym / Mesen .mlb / FCEUX .nl label file; the
                   labels annotate the disassembler, breakpoint, watch,
                   and trace views.
  Clear Symbols .. drop all loaded labels.
  Source maps .... a ca65/cc65 .dbg map overlays the original source
                   lines onto the disassembly.

Symbols are also exported to the Lua engine's `sym:` query tables
([[Lua scripting & automation|scripting-gui]]).",
    },
];

const SETTINGS_BODY: &str = "\
Settings (View -> Settings)
===========================

A tabbed window; every control auto-saves to config.toml on change
(no separate Save step). Tabs:

  Video ...... theme, 8:7 pixel aspect, overscan (per-side WYSIWYG),
               custom .pal palette + named-palette editor, NTSC filter
               selection, present mode / pacing, UI scaling +
               high-contrast / colorblind accessibility themes.
  Shaders .... the composable shader stack (CRT / scanline / NTSC
               passes, run top to bottom) + CRT preset bank, plus the
               LMP88959 NTSC/PAL + hqNx/xBRZ filters and a constrained
               .slangp / .cgp import.
  Audio ...... master volume / mute, per-channel mute + volume, a
               five-band graphic EQ, output latency + DRC, and HD-audio
               sample replacement.
  Input ...... rebindable controller + system-hotkey bindings, Four
               Score, gamepad deadzone, turbo/autofire, port-2 device.
               'Export config...' writes a copy to a chosen .toml file.
  Emulation .. run-ahead depth, rewind buffer size, default region, and
               the 'Enhancements' group (sprite-limit-disable / overclock
               are staged-but-inert pending the v2.0 master-clock work).

Settings live in the OS config directory under RustyNES/config.toml.
See also: [[Config]] for the file format, [[Hotkeys]] for the defaults.";

const TAS_BODY: &str = "\
TAS & movies
============

RustyNES records + replays bit-identical input movies (the determinism
contract guarantees same seed + ROM + input => same framebuffer/audio).

Surfaces:
  - Tools -> Movies (TAS) ... quick Record / Play / Branch of an .rnm
                              movie, plus Import / Export of FCEUX .fm2
                              and BizHawk .bk2 files.
  - Tools -> Replay / TAS ... playback control + seek + device topology
                              + timebase read-out.
  - Tools -> TAStudio ....... the full piano-roll editor
                              ([[TAStudio editor|tas-studio]]).
  - Tools -> Export Last 30s  dump the trailing 30 s of the live session
                              (the rewind-ring HistoryViewer) as a
                              replayable .rnm clip.

See also: [[Visualizers|dt-vis]] for the Replay window,
[[Lua scripting & automation|scripting-gui]] for scripted TAS driving.";

const TAS_CHILDREN: &[SubPage] = &[SubPage {
    id: "tas-studio",
    title: "TAStudio editor",
    body: "\
TAStudio piano-roll editor (Tools -> TAStudio)
==============================================

A frame-accurate piano-roll TAS editor (v1.6.0 Workstream A) anchored on
the current emulator state as the project's frame 0.

  Piano roll .... one column per controller button, one row per frame;
                  toggle inputs per frame, drag to paint, and seek by
                  clicking a row.
  Branches ...... fork the timeline into named branches (.rnmproj),
                  with multitrack + pattern support.
  Interop ....... import / export FCEUX .fm2 and BizHawk .bk2
                  ([[TAS & movies|tas]]).
  Scripting ..... drive + read the editor from Lua
                  ([[Lua scripting & automation|scripting-gui]]).

The editor renders read-only in the always-on UI loop; edits + seeks are
queued and applied under the emulator lock, so determinism is intact.",
}];

const SCRIPTING_GUI_BODY: &str = "\
Lua scripting & automation
==========================

The Lua 5.4 engine (Tools -> Debug -> Lua Script) runs user scripts
against the running emulator. See the manual's [[Scripting]] topic for
the full API; this page covers the GUI + automation surface.

  Console ....... load / reload / stop a script + a log pane.
  Hooks ......... onFrame / onNmi / onIrq / setInput callbacks.
  Data .......... memory peek / poke / range reads (sized + by domain),
                  cart / system queries, joypad, save-state scripting,
                  breakpoint / symbol hooks ([[Symbols & source maps|dt-sym]]).
  IPC (comm.*) .. an OPT-IN, host-mediated bridge (off-by-default
                  `script-ipc` feature) for TCP / HTTP from scripts. It
                  is a NEW non-deterministic I/O source, so it is gated
                  exactly like the `emu.write` mutators and never used by
                  the deterministic core.
  Automation .... `client.*` verbs drive the frontend (load ROM, save
                  state, screenshot, ...) for headless / scripted runs.

Bundled example scripts ship with the build. Mutating verbs (emu.write,
poke, comm.*, client.*) are gated so a read-only script can never affect
determinism.";

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
                 rollback netplay, RetroAchievements, TAS movies + the
                 TAStudio editor, save-states, rewind, run-ahead, Lua
                 scripting + automation, HD packs, A/V recording.

Browse the [[Menus (GUI)|menus]], [[Debugger & devtools|devtools]],
[[Settings (GUI)|settings]], and [[TAS & movies|tas]] pages, or run
`rustynes help <topic>` in a terminal for the same manual on the
command line.";

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
    fn gui_topics_have_children_that_resolve() {
        // #53.2 — every GUI topic + every sub-page must be present.
        assert_eq!(GUI_TOPICS.len(), 5);
        assert!(GUI_TOPICS.iter().any(|t| t.title == "Menus (GUI)"));
        let devtools = GUI_TOPICS
            .iter()
            .find(|t| t.id == "devtools")
            .expect("devtools topic");
        assert!(
            devtools.children.len() >= 8,
            "devtools should expose its chip-inspector sub-pages"
        );
    }

    #[test]
    fn intra_doc_links_resolve() {
        // #53.4 — every link target id used in a body must resolve.
        assert!(matches!(resolve_link("about"), Some(DocSection::About)));
        assert!(matches!(
            resolve_link("changelog"),
            Some(DocSection::Changelog)
        ));
        assert!(matches!(resolve_link("devtools"), Some(DocSection::Gui(_))));
        assert!(matches!(
            resolve_link("dt-cpu"),
            Some(DocSection::GuiChild(_, _))
        ));
        // A shared CLI topic id (e.g. "mappers") resolves to a Topic.
        assert!(matches!(
            resolve_link("mappers"),
            Some(DocSection::Topic(_))
        ));
        // An unknown id resolves to nothing (no panic, no dead link nav).
        assert!(resolve_link("does-not-exist").is_none());
    }
}
