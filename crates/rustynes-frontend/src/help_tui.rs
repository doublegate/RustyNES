//! v1.4.0 "Fidelity" Workstream H3 — interactive terminal help browser.
//!
//! A full-screen ratatui + crossterm browser over the `cli::HELP_TOPICS`
//! registry (the same content the static `rustynes help <topic>` page renders,
//! so the two can't drift). Launched by `rustynes help` on a TTY, or by
//! `rustynes help --interactive`.
//!
//! **Gated** behind the default-on `help-tui` cargo feature *and*
//! `cfg(not(target_arch = "wasm32"))`. When the feature is off, or stdout is not
//! a terminal, the caller (`main.rs`) falls back to the static styled page — so
//! `rustynes help mappers | less` and CI never block on a TUI.
//!
//! Layout: a left topic list, a scrollable colored content pane on the right,
//! and a bottom status/search line. Nav: Up/Down + Tab move topics, PgUp/PgDn +
//! Home/End scroll, `/` opens incremental search, `q`/`Esc` quit.

use std::io;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Wrap,
};
use ratatui::{DefaultTerminal, Frame};

use crate::cli::{HELP_TOPICS, HelpTopic};

/// Interactive-help UI state.
struct HelpApp {
    /// Selected topic index into `HELP_TOPICS`.
    selected: usize,
    /// Vertical scroll offset (in wrapped lines) of the content pane.
    scroll: u16,
    /// `Some(query)` while the search input is active.
    search: Option<String>,
    list_state: ListState,
}

impl HelpApp {
    fn new(start: usize) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(start));
        Self {
            selected: start,
            scroll: 0,
            search: None,
            list_state,
        }
    }

    fn current(&self) -> &'static HelpTopic {
        &HELP_TOPICS[self.selected]
    }

    fn select(&mut self, idx: usize) {
        self.selected = idx.min(HELP_TOPICS.len().saturating_sub(1));
        self.scroll = 0;
        self.list_state.select(Some(self.selected));
    }

    fn next_topic(&mut self) {
        let next = (self.selected + 1) % HELP_TOPICS.len();
        self.select(next);
    }

    fn prev_topic(&mut self) {
        let prev = if self.selected == 0 {
            HELP_TOPICS.len() - 1
        } else {
            self.selected - 1
        };
        self.select(prev);
    }

    /// Jump to the first topic whose title or body contains `query`
    /// (case-insensitive). No-op if nothing matches.
    fn run_search(&mut self) {
        let Some(q) = self.search.as_deref() else {
            return;
        };
        if q.is_empty() {
            return;
        }
        let q = q.to_ascii_lowercase();
        if let Some(idx) = HELP_TOPICS.iter().position(|t| {
            t.title.to_ascii_lowercase().contains(&q) || t.body.to_ascii_lowercase().contains(&q)
        }) {
            self.select(idx);
        }
    }
}

/// Run the interactive help browser, starting on `start_topic` (or topic 0).
///
/// Initialises the terminal (raw mode + alternate screen), runs the event loop,
/// and always restores the terminal on exit (ratatui installs a panic hook that
/// restores too).
///
/// # Errors
/// Propagates any terminal I/O error from the draw / event loop.
pub fn run(start_topic: Option<&str>) -> io::Result<()> {
    let start = start_topic
        .and_then(|id| HELP_TOPICS.iter().position(|t| t.id == id))
        .unwrap_or(0);

    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, start);
    ratatui::restore();
    result
}

fn event_loop(terminal: &mut DefaultTerminal, start: usize) -> io::Result<()> {
    let mut app = HelpApp::new(start);
    loop {
        terminal.draw(|frame| draw(frame, &mut app))?;

        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        // Search-input mode captures most keys.
        if let Some(query) = app.search.as_mut() {
            match key.code {
                KeyCode::Esc => app.search = None,
                KeyCode::Enter => {
                    app.run_search();
                    app.search = None;
                }
                KeyCode::Backspace => {
                    query.pop();
                }
                KeyCode::Char(c) => query.push(c),
                _ => {}
            }
            continue;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => break,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
            KeyCode::Char('/') => app.search = Some(String::new()),
            KeyCode::Down | KeyCode::Char('j') => app.scroll = app.scroll.saturating_add(1),
            KeyCode::Up | KeyCode::Char('k') => app.scroll = app.scroll.saturating_sub(1),
            KeyCode::PageDown => app.scroll = app.scroll.saturating_add(10),
            KeyCode::PageUp => app.scroll = app.scroll.saturating_sub(10),
            KeyCode::Home => app.scroll = 0,
            KeyCode::End => app.scroll = u16::MAX / 2,
            KeyCode::Tab | KeyCode::Right => app.next_topic(),
            KeyCode::BackTab | KeyCode::Left => app.prev_topic(),
            // Number keys 1-9 jump to a topic.
            KeyCode::Char(c @ '1'..='9') => {
                let idx = (c as usize) - ('1' as usize);
                if idx < HELP_TOPICS.len() {
                    app.select(idx);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn draw(frame: &mut Frame, app: &mut HelpApp) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Fill(1), Constraint::Length(1)])
        .split(frame.area());
    let body = chunks[0];
    let status = chunks[1];

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(18), Constraint::Fill(1)])
        .split(body);

    draw_topic_list(frame, cols[0], app);
    draw_content(frame, cols[1], app);
    draw_status(frame, status, app);
}

fn draw_topic_list(frame: &mut Frame, area: Rect, app: &mut HelpApp) {
    let items: Vec<ListItem> = HELP_TOPICS
        .iter()
        .enumerate()
        .map(|(i, t)| {
            ListItem::new(Line::from(vec![
                Span::styled(format!("{} ", i + 1), Style::default().fg(Color::DarkGray)),
                Span::raw(t.title),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" RustyNES help ")
                .title_style(
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(list, area, &mut app.list_state);
}

fn draw_content(frame: &mut Frame, area: Rect, app: &HelpApp) {
    let topic = app.current();
    let lines: Vec<Line> = topic.body.lines().map(style_help_line).collect();
    let total = u16::try_from(lines.len()).unwrap_or(u16::MAX);

    // Clamp scroll so End / large jumps don't run off the bottom.
    let inner_h = area.height.saturating_sub(2);
    let max_scroll = total.saturating_sub(inner_h);
    let scroll = app.scroll.min(max_scroll);

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", topic.title))
                .title_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(para, area);

    if total > inner_h {
        let mut sb_state = ScrollbarState::new(max_scroll as usize).position(scroll as usize);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area,
            &mut sb_state,
        );
    }
}

/// Heuristic colouring: an all-caps / `===`-underlined heading is green+bold;
/// everything else is plain.
fn style_help_line(line: &str) -> Line<'_> {
    let trimmed = line.trim_end();
    if trimmed.chars().filter(|c| !c.is_whitespace()).count() > 0
        && trimmed
            .chars()
            .all(|c| c == '=' || c == '-' || c.is_whitespace())
    {
        return Line::from(Span::styled(line, Style::default().fg(Color::DarkGray)));
    }
    // A heading is a short line followed (visually) by `====`; cheaply approximate
    // by bolding lines with no leading whitespace that contain no `.` ruler dots.
    if !line.starts_with(' ') && !line.contains("..") && !line.is_empty() {
        return Line::from(Span::styled(
            line,
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
    }
    Line::from(line)
}

fn draw_status(frame: &mut Frame, area: Rect, app: &HelpApp) {
    let text = app.search.as_ref().map_or_else(
        || "q/Esc quit   Tab/Up-Down topic   PgUp/PgDn scroll   / search   1-9 jump".to_string(),
        |query| format!("/{query}"),
    );
    let style = if app.search.is_some() {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    frame.render_widget(Paragraph::new(text).style(style), area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_starts_on_requested_topic() {
        let app = HelpApp::new(2);
        assert_eq!(app.selected, 2);
        assert_eq!(app.list_state.selected(), Some(2));
    }

    #[test]
    fn topic_navigation_wraps() {
        let mut app = HelpApp::new(0);
        app.prev_topic();
        assert_eq!(app.selected, HELP_TOPICS.len() - 1);
        app.next_topic();
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn search_jumps_to_matching_topic() {
        let mut app = HelpApp::new(0);
        app.search = Some("netplay".to_string());
        app.run_search();
        assert_eq!(app.current().id, "netplay");
    }

    #[test]
    fn heading_line_is_bolded() {
        let line = style_help_line("About RustyNES");
        // The styled span should carry the BOLD modifier.
        assert!(
            line.spans
                .iter()
                .any(|s| s.style.add_modifier.contains(Modifier::BOLD))
        );
    }
}
