//! In-app Documentation browser (v1.5.0 "Lens" Workstream I10; overhauled in
//! v1.7.0 "Forge" beta.5, #53; deep-tree + wrap rework in v1.7.1, #docs).
//!
//! A searchable, egui-native manual that reuses the SAME structured help-topic
//! registry as the `rustynes help` CLI / ratatui TUI ([`crate::cli::HELP_TOPICS`])
//! so the terminal help and the GUI manual can never drift. On top of the shared
//! CLI topics it builds a **multi-level documentation tree** that descends to the
//! baseline features available in every menu, Settings tab, and debugger panel.
//!
//! The whole manual is a single recursive [`DocNode`] tree (`title` + `body` +
//! `children`), built once via a `OnceLock`. The same tree drives:
//!
//! - the **left sidebar**, rendered as a real expandable/collapsible tree
//!   (egui [`egui::CollapsingHeader`] per node, nested). In the default initial
//!   view only the top level is shown — every node below it starts collapsed;
//! - the **content pane** (with working word-wrap and clickable intra-doc links).
//!
//! v1.7.1 fixes (this pass) on top of the v1.7.0 #53 work:
//!
//! 1. **Word-wrap at any UI scale.** The content pane is a *vertical*
//!    `ScrollArea` (the old `ScrollArea::both` handed the body an unbounded
//!    horizontal width, so `Label::wrap` had nothing to wrap against — text only
//!    flowed after a manual resize at x4 scale). Bodies now wrap to the pane's
//!    real `available_width` at x1-x4 and any pane size.
//! 2. **Changelog dropdown order.** Released versions list newest-first as
//!    before, but the `[Unreleased]` section is moved to the *bottom* of the
//!    selector ([`changelog_releases`] keeps file order; the combo reorders).
//! 3. **Deep collapsible tree.** The doc tree descends past one sub-level into
//!    the real File / Emulation / View / Tools / Debug / Help menu items, the
//!    Settings tabs and their controls, and the debugger panels, with intra-doc
//!    `[[id]]` / `[[label|id]]` hyperlinks throughout.
//!
//! A `/`-style search box filters the tree (matching a node title OR body, at
//! any depth — a branch is shown when itself or any descendant matches).
//! Native-only: the shared topic registry lives in the native-only `cli` module
//! (a browser tab has no terminal), so this whole panel is gated to
//! `cfg(not(target_arch = "wasm32"))` in the module tree.

use crate::cli::HELP_TOPICS;

/// The embedded changelog (split per release at render time). Small + factual,
/// already shipped in the repo root.
const CHANGELOG: &str = include_str!("../../../../CHANGELOG.md");

/// One node of the documentation tree. A node is addressed by its stable `id`
/// (also the intra-doc link target). Nodes with `children` render as a
/// collapsible branch in the sidebar; leaves render as a selectable label.
struct DocNode {
    /// Stable link id (matched by `[[id]]` tokens, case-insensitive). Unique.
    id: &'static str,
    /// Sidebar + heading title.
    title: &'static str,
    /// Body text (colorized + wrapped at render time). May be empty for a pure
    /// branch node (then a child index is shown instead).
    body: &'static str,
    /// Child nodes (empty for a leaf).
    children: &'static [Self],
    /// When true this node renders the special changelog browser instead of a
    /// plain body (it carries the release selector + combo).
    is_changelog: bool,
}

impl DocNode {
    const fn leaf(id: &'static str, title: &'static str, body: &'static str) -> Self {
        Self {
            id,
            title,
            body,
            children: &[],
            is_changelog: false,
        }
    }

    const fn branch(
        id: &'static str,
        title: &'static str,
        body: &'static str,
        children: &'static [Self],
    ) -> Self {
        Self {
            id,
            title,
            body,
            children,
            is_changelog: false,
        }
    }
}

/// Persistent state of the Documentation window.
pub struct DocPanelState {
    /// The id of the currently-selected node.
    selected: String,
    /// The `/`-search filter text (matches node title OR body at any depth).
    filter: String,
    /// The selected changelog release index (into the parsed list).
    changelog_idx: usize,
}

impl Default for DocPanelState {
    fn default() -> Self {
        Self {
            // Land on the first shared topic (Controls), like the CLI.
            selected: HELP_TOPICS
                .first()
                .map(|t| t.id.to_string())
                .unwrap_or_default(),
            filter: String::new(),
            changelog_idx: 0,
        }
    }
}

/// Build the full documentation tree once. Roots, in display order:
/// the shared `rustynes help` topics (Controls, Hotkeys, ... About), then the
/// GUI-only branches (Menus, Settings, Debugger, TAS, Scripting), then About +
/// Changelog. Reuses the CLI topic bodies verbatim so the GUI + terminal manual
/// never drift; the GUI branches are authored here (terminal help is
/// terminal-scoped).
fn doc_tree() -> &'static [DocNode] {
    use std::sync::OnceLock;
    static TREE: OnceLock<Vec<DocNode>> = OnceLock::new();
    TREE.get_or_init(|| {
        let mut roots: Vec<DocNode> = Vec::new();

        // Shared CLI help topics first (leaves; same bodies as `rustynes help`).
        for t in HELP_TOPICS {
            roots.push(DocNode::leaf(t.id, t.title, t.body));
        }

        // GUI-only branches (authored below). These descend into the real menus,
        // Settings tabs, and debugger panels.
        roots.push(DocNode::branch(
            "menus",
            "Menus (GUI)",
            MENUS_BODY,
            MENUS_CHILDREN,
        ));
        roots.push(DocNode::branch(
            "settings",
            "Settings (GUI)",
            SETTINGS_BODY,
            SETTINGS_CHILDREN,
        ));
        roots.push(DocNode::branch(
            "devtools",
            "Debugger & devtools",
            DEVTOOLS_BODY,
            DEVTOOLS_CHILDREN,
        ));
        roots.push(DocNode::branch(
            "tas",
            "TAS & movies",
            TAS_BODY,
            TAS_CHILDREN,
        ));
        roots.push(DocNode::leaf(
            "scripting-gui",
            "Lua scripting & automation",
            SCRIPTING_GUI_BODY,
        ));

        // Reference: About + the changelog browser. The card uses a distinct
        // `about-gui` id so it does not collide with the shared CLI `about`
        // help topic (both are "about" content; only the id must be unique).
        roots.push(DocNode::leaf("about-gui", "About", ABOUT_GUI_BODY));
        roots.push(DocNode {
            id: "changelog",
            title: "Changelog",
            body: "",
            children: &[],
            is_changelog: true,
        });

        roots
    })
}

/// Find a node by id (case-insensitive), returning a reference. Searches the
/// whole tree depth-first.
fn find_node(id: &str) -> Option<&'static DocNode> {
    fn walk(nodes: &'static [DocNode], id: &str) -> Option<&'static DocNode> {
        for n in nodes {
            if n.id.eq_ignore_ascii_case(id) {
                return Some(n);
            }
            if let Some(found) = walk(n.children, id) {
                return Some(found);
            }
        }
        None
    }
    walk(doc_tree(), id.trim())
}

/// Split the embedded `CHANGELOG.md` into `(heading, body)` per `## [version]`
/// release section, in file order (newest first, with `[Unreleased]` heading the
/// file). Built lazily. The combo reorders for display (#2).
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

/// The display order of [`changelog_releases`]: released versions newest-first
/// (their natural file order) with the `[Unreleased]` section moved LAST (#2).
/// Returns indices into [`changelog_releases`].
fn changelog_display_order() -> Vec<usize> {
    let releases = changelog_releases();
    let mut released: Vec<usize> = Vec::with_capacity(releases.len());
    let mut unreleased: Vec<usize> = Vec::new();
    for (i, (head, _)) in releases.iter().enumerate() {
        if is_unreleased_heading(head) {
            unreleased.push(i);
        } else {
            released.push(i);
        }
    }
    released.extend(unreleased);
    released
}

/// Whether a changelog heading is the `[Unreleased]` section.
fn is_unreleased_heading(head: &str) -> bool {
    head.to_ascii_lowercase().contains("unreleased")
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
    // Two-pane layout: a left collapsible topic tree (with the search filter)
    // and the remaining area as a scrollable content pane.
    egui::Panel::left("doc_topics")
        .resizable(true)
        .default_size(230.0)
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
            // The sidebar may itself overflow vertically *and* horizontally on
            // deep nesting at high UI scale, so allow both axes here (this is the
            // tree, not the prose pane — wrapping does not apply to tree rows).
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let nav = topic_tree(ui, state);
                    if let Some(target) = nav {
                        state.selected = target;
                    }
                });
        });

    // Link clicks return a navigation target applied AFTER the render pass (so
    // the borrow on `state.selected` is released first).
    //
    // #1 — the content pane is a *vertical-only* `ScrollArea`. `ScrollArea::both`
    // gives the inner UI an unbounded horizontal width, which defeats text
    // wrapping (`Label::wrap` wraps to `available_width`, which would then be
    // infinite). A vertical scroll area constrains the width to the pane, so
    // bodies wrap correctly at every UI scale (x1-x4) and any pane size.
    let mut nav: Option<String> = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Pin the content column to the pane width so child layouts (the
            // `In this section` index, breadcrumbs, etc.) also wrap to the pane
            // rather than growing it.
            ui.set_max_width(ui.available_width());
            nav = content(ui, state);
        });
    if let Some(target) = nav {
        state.selected = target;
    }
}

/// Whether `needle` (already lowercased) matches `haystack`. Allocation-free
/// case-insensitive substring test.
fn contains_ci(haystack: &str, needle: &str) -> bool {
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    n.len() <= h.len() && h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
}

/// Whether `node` or any descendant matches the (already-lowercased) `needle`.
/// An empty needle matches everything.
fn node_matches(node: &DocNode, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    contains_ci(node.title, needle)
        || contains_ci(node.body, needle)
        || node.children.iter().any(|c| node_matches(c, needle))
}

/// Render the sidebar tree. Returns a navigation target id if a node was
/// clicked. Top-level nodes are shown expanded as collapsible headers that start
/// **collapsed** (so the default initial view shows only the top level); deeper
/// nodes nest recursively.
fn topic_tree(ui: &mut egui::Ui, state: &DocPanelState) -> Option<String> {
    let needle = state.filter.trim().to_ascii_lowercase();
    let mut nav: Option<String> = None;
    // When a filter is active, force-open the matching branches so hits are
    // visible without manual expansion.
    let force_open = !needle.is_empty();
    for node in doc_tree() {
        if node_matches(node, &needle) {
            render_tree_node(ui, node, &state.selected, &needle, force_open, &mut nav);
        }
    }
    nav
}

/// Render one tree node (recursively). A leaf is a selectable label; a branch is
/// a `CollapsingHeader` whose header row is itself selectable (clicking the
/// title navigates; clicking the triangle expands).
fn render_tree_node(
    ui: &mut egui::Ui,
    node: &'static DocNode,
    selected: &str,
    needle: &str,
    force_open: bool,
    nav: &mut Option<String>,
) {
    let is_sel = selected.eq_ignore_ascii_case(node.id);
    if node.children.is_empty() {
        if ui.selectable_label(is_sel, node.title).clicked() {
            *nav = Some(node.id.to_string());
        }
        return;
    }

    // A branch: a collapsing header. Default-collapsed (so the initial view is
    // top-level only); force-open while filtering so matches are visible.
    let mut header = egui::CollapsingHeader::new(node.title)
        .id_salt(("doc-tree", node.id))
        .default_open(false);
    if force_open {
        header = header.open(Some(true));
    }
    let resp = header.show(ui, |ui| {
        // A selectable "(open this page)" row so the branch's own body is
        // reachable distinctly from expand/collapse.
        if ui.selectable_label(is_sel, "\u{2022} overview").clicked() {
            *nav = Some(node.id.to_string());
        }
        for child in node.children {
            if node_matches(child, needle) {
                render_tree_node(ui, child, selected, needle, force_open, nav);
            }
        }
    });
    // Clicking the header text itself also navigates to the branch body.
    if resp.header_response.clicked() {
        *nav = Some(node.id.to_string());
    }
}

/// Render the content pane for the current selection. Returns a navigation
/// target id if the user clicked an intra-doc link, applied by the caller.
fn content(ui: &mut egui::Ui, state: &mut DocPanelState) -> Option<String> {
    let Some(node) = find_node(&state.selected) else {
        // Stale selection (shouldn't happen) — recover to the first topic.
        ui.label("Select a topic from the tree on the left.");
        return None;
    };

    if node.is_changelog {
        changelog_view(ui, state);
        return None;
    }

    if node.id.eq_ignore_ascii_case("about-gui") {
        about_card(ui);
        return None;
    }

    ui.heading(node.title);
    ui.add_space(4.0);
    let mut nav = render_body(ui, node.body);

    // For a branch node, append a small inline index of its sub-pages as links
    // (mirrors the sidebar tree; #3 navigation aid).
    if !node.children.is_empty() {
        ui.add_space(6.0);
        ui.separator();
        ui.label(egui::RichText::new("In this section").strong());
        for child in node.children {
            if ui.link(format!("\u{2192} {}", child.title)).clicked() {
                nav = nav.or_else(|| Some(child.id.to_string()));
            }
        }
    }
    nav
}

/// Resolve an intra-doc link id (from a `[[id]]` token) to a node id, if it
/// names a real node. (We just validate + normalize; the caller navigates by
/// id.)
fn resolve_link(id: &str) -> Option<String> {
    find_node(id).map(|n| n.id.to_string())
}

/// Colorize + word-wrap a plain-text doc body, rendering `[[id]]` /
/// `[[label|id]]` tokens as clickable intra-doc links. Returns a navigation
/// target id when a link is clicked.
///
/// Recognised line shapes (heuristic, matching the CLI body style):
/// - a heading line immediately followed by a line of `===` or `---` -> a
///   colored heading (the underline line is consumed);
/// - a line beginning with two-or-more spaces -> an indented "code"/detail line
///   (dimmer monospace);
/// - everything else -> a wrapped paragraph line.
fn render_body(ui: &mut egui::Ui, body: &str) -> Option<String> {
    const HEADING: egui::Color32 = egui::Color32::from_rgb(0x6C, 0xB4, 0xF0);
    const CODE: egui::Color32 = egui::Color32::from_rgb(0xC0, 0xA8, 0x70);
    const BULLET: egui::Color32 = egui::Color32::from_rgb(0x9C, 0xD0, 0x9C);

    let mut nav: Option<String> = None;
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
            // Indented detail / code line -> dimmer monospace, but still scan it
            // for links so cross-references in tables work.
            if let Some(target) = render_line_with_links(ui, line, CODE, true) {
                nav = nav.or(Some(target));
            }
        } else if line.trim_start().starts_with("- ") || line.trim_start().starts_with("* ") {
            // A bullet line -> tint the marker.
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
/// to the pane width. Returns a navigation target id if a link was clicked.
fn render_line_with_links(
    ui: &mut egui::Ui,
    line: &str,
    color: egui::Color32,
    monospace: bool,
) -> Option<String> {
    // Fast path: no link token -> one wrapped label (cheapest + wraps cleanly).
    if !line.contains("[[") {
        let mut rt = egui::RichText::new(line).color(color);
        if monospace {
            rt = rt.monospace();
        }
        ui.add(egui::Label::new(rt).wrap());
        return None;
    }

    let mut nav: Option<String> = None;
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
                // Wrap text segments so a long run between links cannot overflow
                // the pane horizontally.
                ui.add(egui::Label::new(rt).wrap());
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
                let mut rt = egui::RichText::new(rest).color(color);
                if monospace {
                    rt = rt.monospace();
                }
                ui.add(egui::Label::new(rt).wrap());
                rest = "";
                break;
            }
        }
        if !rest.is_empty() {
            let mut rt = egui::RichText::new(rest).color(color);
            if monospace {
                rt = rt.monospace();
            }
            // Wrap the trailing text segment after the last link.
            ui.add(egui::Label::new(rt).wrap());
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
    // #2 — display order is newest-first with [Unreleased] LAST.
    let order = changelog_display_order();
    state.changelog_idx = state.changelog_idx.min(releases.len() - 1);
    ui.heading("Changelog");
    ui.horizontal(|ui| {
        ui.label("Release:");
        egui::ComboBox::from_id_salt("doc-changelog-release")
            .selected_text(releases[state.changelog_idx].0.clone())
            .show_ui(ui, |ui| {
                for &i in &order {
                    let (head, _) = &releases[i];
                    ui.selectable_value(&mut state.changelog_idx, i, head.clone());
                }
            });
    });
    ui.separator();
    render_body(ui, &releases[state.changelog_idx].1);
}

// ===========================================================================
// GUI-only topic bodies (authored here; the CLI registry is terminal-scoped).
// `[[id]]` / `[[label|id]]` tokens are intra-doc links.
// ===========================================================================

const MENUS_BODY: &str = "\
Menu bar (File / Emulation / View / Tools / Debug / Help)
=========================================================

The always-on egui shell carries a native menu bar. Every entry has a
Font-Awesome icon, shows its bound accelerator key, and is enabled or
disabled in context (e.g. Frame Advance only while paused; Vs. Insert
Coin only for Vs. System games). Tool windows open as floating panels
without forcing the debugger overlay on. Toggle the bar with M.

Expand a menu in the sidebar tree, or follow the links below:

  - [[File menu|menu-file]]
  - [[Emulation menu|menu-emulation]]
  - [[View menu|menu-view]]
  - [[Tools menu|menu-tools]]
  - [[Debug menu|menu-debug]]
  - [[Help menu|menu-help]]

Note (v1.7.0): the debugger toolbar HUD was removed; the backtick (`)
key now toggles the status-bar RetroAchievements read-out between its
compact and long-form variants.

See also: [[Settings (GUI)|settings]], [[Debugger & devtools|devtools]],
[[TAS & movies|tas]], [[Hotkeys|hotkeys]].";

const MENUS_CHILDREN: &[DocNode] = &[
    DocNode::leaf(
        "menu-file",
        "File menu",
        "\
File menu
=========

  Open ROM... (F12) ... load a .nes / .fds / .zip / .nsf (native).
  Open Recent ......... a submenu of recently-opened ROMs + Clear
                        Recent.
  Close ROM ........... unload the current cartridge.
  Save States ......... a submenu:
                          - Save State / Load State (current slot),
                          - Active Slot (1-8),
                          - Save to Slot / Load from Slot (1-8),
                          - Manage States... (the save-state browser).
  Take Screenshot ..... write a PNG (native).
  Copy Screenshot to Clipboard (native).
  Quit ................ exit RustyNES.

Save-state slots persist on disk; see [[Config|config]] for where they
live. The slot hotkeys (F1 save / F4 load) are listed under
[[Hotkeys|hotkeys]].",
    ),
    DocNode::leaf(
        "menu-emulation",
        "Emulation menu",
        "\
Emulation menu
==============

  Pause / Resume ...... halt or continue emulation.
  Reset (F2) .......... warm reset.
  Power Cycle (F3) .... cold boot (re-randomizes power-on RAM via the
                        seeded PRNG; the determinism contract holds).
  Frame Advance ....... step exactly one frame (only while paused).
  Fast Forward (hold) . run unthrottled while held.
  Run-Ahead ........... 0-3 frames of latency-hiding speculation; the
                        snapshot/restore lives in the frontend so the
                        core stays bit-identical (see [[Latency|set-emulation]]).
  Speed ............... 25%-300% presets (locked to 100% in netplay).
  Region .............. read-out of NTSC / PAL / Dendy (set from the
                        header / per-game DB, not a build fork).
  Vs. Insert Coin (F10) for Vs. System arcade boards.
  Swap Disk Side (F9) . for Famicom Disk System games
                        ([[TAS & movies|tas]] notes the FDS workflow).",
    ),
    DocNode::leaf(
        "menu-view",
        "View menu",
        "\
View menu
=========

  Settings... ......... open the tabbed [[Settings (GUI)|settings]]
                        window.
  Theme ............... Light / Dark / System + the high-contrast and
                        Okabe-Ito colorblind accessibility themes.
  8:7 Pixel Aspect .... correct the NES pixel aspect ratio.
  Hide Overscan ....... crop the top + bottom 8 scanlines.
  Fullscreen (F11) .... toggle fullscreen (native).
  Window Size ......... 1x / 2x / 3x / 4x integer scales (native).
  Show FPS ............ overlay the frame rate.
  Show Lag Frames ..... overlay the lag-frame counter.
  Pause When Unfocused  auto-pause on focus loss.
  Show Menu Bar (M) ... toggle this menu bar.

The video + accessibility controls also live, in more depth, under
[[Settings: Video|set-video]].",
    ),
    DocNode::leaf(
        "menu-tools",
        "Tools menu",
        "\
Tools menu
==========

  Cheats... ........... Game Genie codes + raw-RAM cheats.
  Movies (TAS) ........ Record / Play / Branch an .rnm movie, plus
                        Import / Export FCEUX .fm2 and BizHawk .bk2 and
                        subtitle (.srt) export ([[TAS & movies|tas]]).
  Record A/V... ....... capture video + audio to a file.
  Netplay... .......... host / join GGPO-style rollback netplay
                        ([[Netplay|netplay]]).
  RetroAchievements... opt-in cheevos (native, feature-gated).
  Input Display ....... the consolidated controller HUD
                        ([[Visualizers|dt-vis]]).
  NSF Player .......... NSF / NSFe music playback + a scope.
  Replay / TAS ........ movie playback + seek + device topology.
  TAStudio ............ the piano-roll TAS editor
                        ([[TAStudio editor|tas-studio]]).
  Export Last 30s (.rnm) dump the trailing 30 s of live play.
  ROM Database ........ the in-app per-game DB editor.
  HD Pack ............. Load / Unload, the Pixel Inspector, and the
                        HD-Pack Builder (record).",
    ),
    DocNode::leaf(
        "menu-debug",
        "Debug menu",
        "\
Debug menu
==========

  Show Debugger ....... toggle the deep chip-inspector overlay.
  Performance Monitor . frame-time + GPU-pass timing panel.
  CPU / PPU / APU ..... the chip inspectors
                        ([[Debugger & devtools|devtools]]).
  Memory / Memory Compare / OAM (memory tools, see
                        [[Memory & search|dt-mem]]).
  Mapper .............. the [[Mapper inspector|dt-mapper]].
  Trace Logger / Watch / Breakpoints / Event Viewer
                        ([[Trace, Watch & breakpoints|dt-trace]],
                        [[Event viewer|dt-events]]).
  Lua Script .......... the scripting console
                        ([[Lua scripting & automation|scripting-gui]]).
  Cartridge Info / Header Editor... edit iNES / NES 2.0 headers.
  Load Symbols / Clear Symbols (.sym / .mlb / .nl;
                        [[Symbols & source maps|dt-sym]]).",
    ),
    DocNode::leaf(
        "menu-help",
        "Help menu",
        "\
Help menu
=========

  Documentation... .... this searchable in-app manual (native).
  Keyboard Shortcuts .. the full hotkey grid, with a player selector.
  About ............... version, license, author, and links
                        ([[About|about-gui]]).

The same manual is available on the command line: run
`rustynes help <topic>` in a terminal.",
    ),
];

const SETTINGS_BODY: &str = "\
Settings (View -> Settings)
===========================

A tabbed window; every control auto-saves to config.toml on change
(no separate Save step). Tabs:

  - [[Video|set-video]]
  - [[Shaders|set-shaders]]
  - [[Audio|set-audio]]
  - [[Input|set-input]]
  - [[Emulation|set-emulation]]

Settings live in the OS config directory under RustyNES/config.toml.
See also: [[Config|config]] for the file format, [[Hotkeys|hotkeys]] for
the defaults.";

const SETTINGS_CHILDREN: &[DocNode] = &[
    DocNode::leaf(
        "set-video",
        "Video",
        "\
Settings: Video (Graphics)
==========================

  Present mode ....... Mailbox / Fifo (restart to apply).
  Pacing ............. auto (display-sync) / display (vsync) / vrr
                       (G-Sync/FreeSync) / wallclock.
  Max frame latency .. 1-2 (1 = lowest latency; restart to apply).
  NTSC filter ........ off / composite / rgb / composite-rt; the
                       composite-rt mode adds live Contrast /
                       Saturation / Brightness / Hue knobs.
  Hide overscan ...... crop the top + bottom 8 scanlines.
  Overscan (per-side)  WYSIWYG Top / Bottom / Left / Right sliders.
  CRT / scanlines .... a built-in scanline pass + intensity (the full
                       stack is under [[Shaders|set-shaders]]).
  Palette ............ built-in or a saved named palette; Load / Clear
                       a legacy .pal; the Palette editor (a 64-swatch
                       grid + save / import).

Theme + UI scaling + the high-contrast / colorblind accessibility
themes are also reachable from the [[View menu|menu-view]].",
    ),
    DocNode::leaf(
        "set-shaders",
        "Shaders",
        "\
Settings: Shaders
=================

  Shader stack ....... a composable list of passes (CRT / scanline /
                       NTSC), run top to bottom; toggle, reorder, and
                       tune per-pass parameters.
  Presets ............ a CRT preset bank: add the built-ins, save / load
                       / delete named presets.
  Import ............. a constrained RetroArch .slangp / .cgp import.

The LMP88959 NTSC/PAL and hqNx / xBRZ filters are part of this stack.",
    ),
    DocNode::leaf(
        "set-audio",
        "Audio",
        "\
Settings: Audio
===============

  Volume / Mute ...... master output level.
  Channels ........... per-channel enable: Pulse 1/2, Triangle, Noise,
                       DMC, and Mapper (expansion) audio.
  Channel volume ..... per-channel gain sliders (a g==1.0 fast path is
                       byte-identical; see [[APU & audio|dt-apu]]).
  Graphic EQ ......... a 5-band (or 20-band) graphic equaliser.
  Stereo ............. per-channel pan, reverb, and headphone crossfeed.
  Context volume ..... Master / Game / Menu mix levels.
  Output device ...... pick the cpal output (restart to apply).
  Sample rate ........ 44100 / 48000 or custom (restart to apply).
  Audio latency ...... output buffer in ms (restart to apply).
  Dynamic rate control +/-0.5% drift compensation (the resampler lives
                       in the frontend, so the core stays deterministic).",
    ),
    DocNode::leaf(
        "set-input",
        "Input",
        "\
Settings: Input
===============

  Controller bindings  rebind every Player 1-4 button
                       ([[Controls|controls]]).
  System hotkeys ..... rebind the [input.system] accelerators
                       ([[Hotkeys|hotkeys]]).
  Devices ............ Four Score multitap, the port-2 device picker
                       (Zapper / Arkanoid / SNES mouse / Power Pad /
                       keyboards / Hyper Shot; [[Gamepad|gamepad]]).
  Tuning ............. gamepad deadzone and turbo / autofire.
  Export config... ... write a copy of the bindings to a .toml file.",
    ),
    DocNode::leaf(
        "set-emulation",
        "Emulation (Latency / Rewind)",
        "\
Settings: Emulation
===================

  Run-ahead .......... 0-3 frames; removes the game's internal input
                       lag (1 fits most games).
  Rewind ............. enable + the window length (seconds) + keyframe
                       period (restart to resize the buffer).
  Enhancements ....... the non-accuracy group: disable the
                       8-sprite-per-scanline limit and an overclock
                       (extra scanlines). Both are STAGED-but-INERT
                       pending the v2.0 master-clock work, so they do
                       not yet change emulation or AccuracyCoin.

Run-ahead + rewind snapshot/restore live in the frontend, never in the
synthesis core, so the determinism contract is preserved.",
    ),
];

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

const DEVTOOLS_CHILDREN: &[DocNode] = &[
    DocNode::leaf(
        "dt-cpu",
        "CPU & disassembly",
        "\
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
    ),
    DocNode::leaf(
        "dt-ppu",
        "PPU & video viewers",
        "\
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
    ),
    DocNode::leaf(
        "dt-apu",
        "APU & audio",
        "\
APU inspector (Debug -> APU)
============================

  Channel scopes . per-channel waveform scopes (pulse 1/2, triangle,
                   noise, DMC) + any mapper expansion-audio channels.
  Volume meters .. live per-channel output levels.
  Register dump .. the raw $4000-$4017 APU register state.

Per-channel mute + gain and the 5-band EQ live in
[[Settings: Audio|set-audio]]. NSF/NSFe playback has its own player
under Tools -> NSF Player ([[Visualizers|dt-vis]]).",
    ),
    DocNode::leaf(
        "dt-mem",
        "Memory & search",
        "\
Memory tools (Debug menu)
=========================

  Memory ......... a CPU + PPU bus hex viewer with go-to-address; the
                   access-counter heatmap tints each byte by read /
                   write / execute frequency (v1.7.0 Workstream C2).
  Memory Compare . snapshot RAM and diff two captures to hunt cheats
                   (equal / less / greater / changed filters).
  OAM ............ the sprite list + a visual sprite grid.

In RetroAchievements hardcore mode the Memory viewer is disabled (it is
a RAM-watch surface); see [[Features|features]] in the manual.",
    ),
    DocNode::leaf(
        "dt-mapper",
        "Mapper inspector",
        "\
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
    ),
    DocNode::leaf(
        "dt-trace",
        "Trace, Watch & breakpoints",
        "\
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
    ),
    DocNode::leaf(
        "dt-events",
        "Event viewer",
        "\
Event viewer (Debug -> Event Viewer)
====================================

A 341x312 per-dot read/write heatmap of the current frame: each PPU dot
(x = dot, y = scanline) is tinted by the register accesses that landed
on it, so mid-scanline $2005/$2006 writes, sprite-zero hits, and mapper
IRQ points are visible in their exact timing position.

Pairs naturally with the [[PPU & video viewers|dt-ppu]] scroll trace.",
    ),
    DocNode::leaf(
        "dt-vis",
        "Visualizers",
        "\
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
                   the deeper editor is [[TAStudio editor|tas-studio]].",
    ),
    DocNode::leaf(
        "dt-sym",
        "Symbols & source maps",
        "\
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
    ),
];

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

const TAS_CHILDREN: &[DocNode] = &[DocNode::leaf(
    "tas-studio",
    "TAStudio editor",
    "\
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
)];

const SCRIPTING_GUI_BODY: &str = "\
Lua scripting & automation
==========================

The Lua 5.4 engine (Tools / Debug -> Lua Script) runs user scripts
against the running emulator. See the manual's [[Scripting|scripting]]
topic for the full API; this page covers the GUI + automation surface.

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
  Features ..... 150 mapper families, FDS, Vs. System / PlayChoice-10,
                 rollback netplay, RetroAchievements, TAS movies + the
                 TAStudio editor, save-states, rewind, run-ahead, Lua
                 scripting + automation, HD packs, A/V recording.

Browse the [[Menus (GUI)|menus]], [[Settings (GUI)|settings]],
[[Debugger & devtools|devtools]], and [[TAS & movies|tas]] pages, or run
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
    fn changelog_display_order_puts_unreleased_last() {
        // #2 — the display order lists released versions first (newest-first,
        // file order) with [Unreleased] at the very bottom.
        let releases = changelog_releases();
        let order = changelog_display_order();
        assert_eq!(
            order.len(),
            releases.len(),
            "the display order must cover every release exactly once"
        );
        // Every index appears exactly once.
        let mut seen = std::collections::HashSet::new();
        for &i in &order {
            assert!(seen.insert(i), "duplicate index in display order");
        }
        // The LAST display entry is the [Unreleased] section...
        let last = *order.last().expect("non-empty changelog");
        assert!(
            is_unreleased_heading(&releases[last].0),
            "[Unreleased] must be displayed last, got {:?}",
            releases[last].0
        );
        // ...and no earlier display entry is the Unreleased section.
        for &i in &order[..order.len() - 1] {
            assert!(
                !is_unreleased_heading(&releases[i].0),
                "only the final entry may be the Unreleased section"
            );
        }
        // Sanity: the first displayed release is a real (non-Unreleased) one.
        let first = order[0];
        assert!(!is_unreleased_heading(&releases[first].0));
    }

    #[test]
    fn filter_matches_title_and_body_at_depth() {
        let menus = find_node("menus").expect("menus branch");
        // Empty needle matches everything.
        assert!(node_matches(menus, ""));
        // Title substring.
        assert!(node_matches(menus, "menu"));
        // A DESCENDANT body hit surfaces the branch (e.g. "Power Cycle" lives in
        // the Emulation submenu body, not the Menus overview body).
        assert!(node_matches(menus, "power cycle"));
        // No hit anywhere.
        assert!(!node_matches(menus, "xyzzy-not-present"));
    }

    #[test]
    fn tree_is_deep_and_top_level_has_children() {
        // #3 — the tree descends past one sub-level. The Settings branch must
        // expose its five real tabs; the Debugger branch its chip inspectors.
        let settings = find_node("settings").expect("settings branch");
        assert!(
            settings.children.len() >= 5,
            "Settings should expose its tab sub-pages"
        );
        let devtools = find_node("devtools").expect("devtools branch");
        assert!(
            devtools.children.len() >= 8,
            "Debugger should expose its chip-inspector sub-pages"
        );
        // The Menus branch descends into the six real menus, and each is itself
        // a resolvable node (one level deeper than the old flat list).
        let menus = find_node("menus").expect("menus branch");
        assert_eq!(menus.children.len(), 6);
        for m in [
            "menu-file",
            "menu-emulation",
            "menu-view",
            "menu-tools",
            "menu-debug",
            "menu-help",
        ] {
            assert!(find_node(m).is_some(), "missing menu node: {m}");
        }
    }

    #[test]
    fn node_ids_are_unique() {
        // Stable + unique ids are required for both link resolution and the
        // sidebar CollapsingHeader id_salts.
        let mut seen = std::collections::HashSet::new();
        fn walk(nodes: &'static [DocNode], seen: &mut std::collections::HashSet<&'static str>) {
            for n in nodes {
                assert!(seen.insert(n.id), "duplicate doc node id: {}", n.id);
                walk(n.children, seen);
            }
        }
        walk(doc_tree(), &mut seen);
    }

    #[test]
    fn intra_doc_links_resolve() {
        // Every link target id used in a body must resolve to a real node.
        assert_eq!(resolve_link("about").as_deref(), Some("about"));
        assert_eq!(resolve_link("about-gui").as_deref(), Some("about-gui"));
        assert_eq!(resolve_link("changelog").as_deref(), Some("changelog"));
        assert_eq!(resolve_link("devtools").as_deref(), Some("devtools"));
        assert_eq!(resolve_link("dt-cpu").as_deref(), Some("dt-cpu"));
        // A deep settings/menu node resolves.
        assert_eq!(resolve_link("set-audio").as_deref(), Some("set-audio"));
        assert_eq!(resolve_link("menu-file").as_deref(), Some("menu-file"));
        // A shared CLI topic id (e.g. "mappers") resolves to its node.
        assert_eq!(resolve_link("mappers").as_deref(), Some("mappers"));
        // Case-insensitive.
        assert_eq!(resolve_link("DT-PPU").as_deref(), Some("dt-ppu"));
        // An unknown id resolves to nothing (no panic, no dead link nav).
        assert!(resolve_link("does-not-exist").is_none());
    }

    #[test]
    fn all_body_links_point_at_real_nodes() {
        // Walk every node body, extract its [[...]] link tokens, and assert each
        // target id resolves — guards against a typo'd cross-reference (#3).
        fn check(nodes: &'static [DocNode]) {
            for n in nodes {
                let mut rest = n.body;
                while let Some(start) = rest.find("[[") {
                    let after = &rest[start + 2..];
                    if let Some(end) = after.find("]]") {
                        let token = &after[..end];
                        let (_label, id) = token.split_once('|').unwrap_or((token, token));
                        assert!(
                            resolve_link(id).is_some(),
                            "doc node '{}' links to unknown id '{}'",
                            n.id,
                            id.trim()
                        );
                        rest = &after[end + 2..];
                    } else {
                        break;
                    }
                }
                check(n.children);
            }
        }
        check(doc_tree());
    }

    #[test]
    fn default_selection_lands_on_first_topic() {
        let st = DocPanelState::default();
        assert_eq!(st.selected, HELP_TOPICS[0].id);
        assert!(find_node(&st.selected).is_some());
    }
}
