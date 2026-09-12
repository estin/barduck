#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use crate::{
    config::{Config, Level, TuiWidth, VERSION, View},
    health::Health,
    query::Backend,
};
use anyhow::Result;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use std::time::{Duration, Instant};

/// How often the TUI re-reads data.
const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

/// How long each keyboard poll waits before the loop comes back around: the
/// upper bound on how long a keypress, a completed refresh, or a resize can
/// sit unnoticed. Independent of [`REFRESH_INTERVAL`], which is measured
/// against the wall clock — the two used to be the same knob (refresh "every
/// 8th poll"), so a burst of keypresses, each cutting a poll short, silently
/// sped refreshes up.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Renders config-declared layouts as panels (spec: tui). Refreshes
/// periodically; shows an error banner instead of crashing when the data
/// path is unavailable (spec: tui — graceful degradation).
pub fn run(backend: &Backend, cfg: &Config) -> Result<()> {
    let mut terminal = TerminalGuard::enter()?;
    event_loop(&mut terminal, backend, cfg)
}

fn event_loop(terminal: &mut TerminalGuard, backend: &Backend, cfg: &Config) -> Result<()> {
    let mut state = UiState {
        error: None,
        rows: Vec::new(),
    };
    let mut refresher = Refresher::start(backend.clone(), cfg.clone());
    // Due immediately, so the first frame isn't an empty dashboard.
    let mut next_refresh = Instant::now();
    loop {
        if refresher.is_idle() && Instant::now() >= next_refresh {
            if !refresher.request() {
                state.error = Some("refresh worker stopped".into());
            }
            next_refresh = Instant::now() + REFRESH_INTERVAL;
        }
        if let Some(outcome) = refresher.take_latest() {
            apply_outcome(cfg, outcome, &mut state);
        }
        terminal.draw(|f| draw(f, &state, &cfg.tui_width))?;
        if crossterm::event::poll(POLL_INTERVAL)? && quit_requested(&crossterm::event::read()?) {
            return Ok(());
        }
    }
}

/// Whether `event` asks the TUI to exit: `q`, Esc, or Ctrl+C. Ctrl+C has to
/// be handled here rather than left to a signal handler — raw mode is
/// exactly the mode in which the terminal stops turning it into `SIGINT`, so
/// without this the TUI cannot be interrupted the way every other terminal
/// program can.
fn quit_requested(event: &crossterm::event::Event) -> bool {
    use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
    let Event::Key(key) = event else {
        return false;
    };
    if key.kind != KeyEventKind::Press {
        return false;
    }
    matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
}

struct Panel {
    name: String,
    value: String,
    unit: String,
    status: Health,
    /// Threshold band level for the current value, when the source has
    /// thresholds configured (spec: tui — threshold band coloring).
    level: Option<Level>,
    ts_epoch: f64,
}

/// One grid slot: a spacer (`main`/`secondary`/`table`/`text` all empty), a
/// single-source panel (`main` only, `group_title` `None`), a static-text
/// panel (`text` only), or a generalized pane combining up to three sections
/// — `main` (regular-panel treatment), `secondary` (compact, always-shown
/// age), and `table` (label/value rows, age only when stale) (spec: tui —
/// group panes show multiple labeled, independently colored values).
struct Slot {
    span: usize,
    /// Card title for a group or static-text slot; unused for a plain
    /// single-source slot, whose title lives on its `main` panel's `name`
    /// instead.
    group_title: Option<String>,
    main: Option<Panel>,
    secondary: Vec<Panel>,
    table: Vec<Panel>,
    /// A static-text panel's literal content, shown as-is — the TUI never
    /// interprets `markdown`/`json` formatting (spec: tui — static-text
    /// panel rendering).
    text: Option<String>,
}

/// Builds one panel's value/status/threshold data for `name`, labeled
/// `label` (the pane title for a single-source cell, or a group member's own
/// label).
fn build_panel(
    cfg: &Config,
    latest: &[crate::db::ReadingRow],
    healths: &[crate::health::SourceHealth],
    name: &str,
    title_override: Option<&str>,
) -> Panel {
    let row = latest.iter().find(|r| r.source == name);
    let src = cfg.sources.iter().find(|s| s.name() == name);
    let label = title_override
        .or_else(|| src.and_then(crate::config::SourceCfg::display_title))
        .unwrap_or(name);
    // Effective bands ride on the health payload (declared, or the latest
    // `jsonl` override) so the sync renderer needs no DB access itself.
    let bands: &[crate::config::Threshold] = healths
        .iter()
        .find(|h| h.source == name)
        .map_or(&[], |h| &h.thresholds);
    let level = row.and_then(|r| {
        (!bands.is_empty())
            .then(|| crate::config::level_for(bands, &r.value))
            .flatten()
    });
    Panel {
        name: label.to_string(),
        value: row.map_or_else(|| "—".into(), |r| r.value.clone()),
        unit: row.and_then(|r| r.unit.clone()).unwrap_or_default(),
        status: healths
            .iter()
            .find(|h| h.source == name)
            .map_or(Health::Stale, |h| h.status),
        level,
        ts_epoch: row.map_or(0.0, |r| r.ts_epoch),
    }
}

struct UiState {
    error: Option<String>,
    rows: Vec<Vec<Slot>>,
}

enum Outcome {
    Data(Vec<crate::db::ReadingRow>, Vec<crate::health::SourceHealth>),
    Failed(String),
}

/// The TUI's background refresh worker: one OS thread owning one Tokio
/// runtime for the whole session, driven by a request channel.
///
/// Refreshes stay off the UI thread so a slow daemon/DB round trip never
/// blocks the keyboard poll — otherwise `q` waits for the in-flight fetch
/// before it is even read. What changed is the cost of that: each refresh
/// used to spawn a fresh thread *and* build a fresh current-thread runtime,
/// every two seconds, for as long as the TUI stayed open. Both are now
/// created once.
///
/// The worker is deliberately not joined on exit. Dropping the request
/// sender ends its loop, but a fetch already in flight can still be sitting
/// on the daemon client's 10-second timeout, and quitting a TUI must be
/// immediate.
struct Refresher {
    requests: std::sync::mpsc::Sender<()>,
    outcomes: std::sync::mpsc::Receiver<Outcome>,
    in_flight: bool,
}

impl Refresher {
    fn start(backend: Backend, cfg: Config) -> Self {
        let (requests, work) = std::sync::mpsc::channel::<()>();
        let (results, outcomes) = std::sync::mpsc::channel::<Outcome>();
        std::thread::spawn(move || {
            // This thread runs outside any ambient Tokio context, so it owns
            // a runtime rather than borrowing a `Handle`.
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = results.send(Outcome::Failed(format!("internal error: {e}")));
                    return;
                }
            };
            while work.recv().is_ok() {
                if results.send(rt.block_on(fetch(&backend, &cfg))).is_err() {
                    return; // the UI is gone
                }
            }
        });
        Self {
            requests,
            outcomes,
            in_flight: false,
        }
    }

    /// Whether a refresh can be started — false while one is still running,
    /// so a slow backend can never queue up a backlog of them.
    fn is_idle(&self) -> bool {
        !self.in_flight
    }

    /// Starts a refresh. False means the worker thread is gone (its runtime
    /// failed to build), which the caller surfaces rather than retrying
    /// silently forever.
    fn request(&mut self) -> bool {
        self.in_flight = self.requests.send(()).is_ok();
        self.in_flight
    }

    /// The newest completed refresh, if any finished since the last call.
    /// Drains rather than taking one per frame, so the UI always renders the
    /// freshest result instead of working through a backlog of stale ones.
    fn take_latest(&mut self) -> Option<Outcome> {
        let mut latest = None;
        while let Ok(outcome) = self.outcomes.try_recv() {
            self.in_flight = false;
            latest = Some(outcome);
        }
        latest
    }
}

/// One refresh: the dashboard's values and every source's health.
///
/// Sequential, not `join!`ed. In direct mode both sides take the database's
/// advisory lock, so running them concurrently just makes one back off and
/// sleep; and each is now a fixed two or three queries whatever the source
/// count (see `health::compute_all`), which is what actually made refreshes
/// cheap.
async fn fetch(backend: &Backend, cfg: &Config) -> Outcome {
    match (backend.latest().await, backend.health(cfg).await) {
        (Ok(latest), Ok(healths)) => Outcome::Data(latest, healths),
        (Err(e), _) | (_, Err(e)) => Outcome::Failed(format!("{e:#}")),
    }
}

fn apply_outcome(cfg: &Config, outcome: Outcome, state: &mut UiState) {
    match outcome {
        Outcome::Data(latest, healths) => {
            state.error = None;
            let mut rows = Vec::new();
            for layout in &cfg.layouts {
                for row_cells in &layout.rows {
                    let mut slots = Vec::new();
                    for cell in row_cells {
                        // A source hidden from this view (spec: source-configuration —
                        // per-source view visibility) is simply omitted here, so the
                        // cell falls through to the same empty-slot rendering as an
                        // explicit `space` cell (spec: tui — hidden sources render as
                        // space in the TUI).
                        let (main, secondary, table, group_title, text) = match cell {
                            crate::config::Cell::Group {
                                title,
                                main,
                                secondary,
                                table,
                                ..
                            } => (
                                main.as_ref()
                                    .filter(|item| {
                                        crate::config::source_visible_in(cfg, item.id(), View::Tui)
                                    })
                                    .map(|item| {
                                        build_panel(
                                            cfg,
                                            &latest,
                                            &healths,
                                            item.id(),
                                            item.explicit_label(),
                                        )
                                    }),
                                crate::config::visible_items(cfg, secondary, View::Tui)
                                    .iter()
                                    .map(|item| {
                                        build_panel(
                                            cfg,
                                            &latest,
                                            &healths,
                                            item.id(),
                                            item.explicit_label(),
                                        )
                                    })
                                    .collect(),
                                crate::config::visible_items(cfg, table, View::Tui)
                                    .iter()
                                    .map(|item| {
                                        build_panel(
                                            cfg,
                                            &latest,
                                            &healths,
                                            item.id(),
                                            item.explicit_label(),
                                        )
                                    })
                                    .collect(),
                                title.clone(),
                                None,
                            ),
                            crate::config::Cell::Source(name) => (
                                crate::config::source_visible_in(cfg, name, View::Tui)
                                    .then(|| build_panel(cfg, &latest, &healths, name, None)),
                                Vec::new(),
                                Vec::new(),
                                None,
                                None,
                            ),
                            crate::config::Cell::Pane { id, title, .. } => (
                                crate::config::source_visible_in(cfg, id, View::Tui).then(|| {
                                    build_panel(cfg, &latest, &healths, id, title.as_deref())
                                }),
                                Vec::new(),
                                Vec::new(),
                                None,
                                None,
                            ),
                            crate::config::Cell::Space { .. } => {
                                (None, Vec::new(), Vec::new(), None, None)
                            }
                            crate::config::Cell::Text { title, text, .. } => (
                                None,
                                Vec::new(),
                                Vec::new(),
                                title.clone(),
                                Some(text.clone()),
                            ),
                        };
                        slots.push(Slot {
                            span: cell.span(),
                            group_title,
                            main,
                            secondary,
                            table,
                            text,
                        });
                    }
                    rows.push(slots);
                }
            }
            state.rows = rows;
        }
        Outcome::Failed(msg) => state.error = Some(msg),
    }
}

/// Health-priority accent color when there is one (`None` when healthy and
/// unbanded — nothing meaningful to accent, so the panel renders with the
/// terminal's default color instead of a forced green) (spec: tui —
/// threshold band coloring).
fn status_style(level: Option<Level>, status: Health) -> Style {
    match crate::config::accent_color(level, status) {
        Some(color) => color_style(color),
        None => Style::default(),
    }
}

/// Plain `[failing]`/`[stale]` suffix for a currently unhealthy value —
/// empty when healthy, shown alongside whatever color that status
/// contributes (spec: tui — threshold band coloring; group panes).
fn plain_label(status: Health) -> String {
    if status == Health::Healthy {
        String::new()
    } else {
        format!(" [{}]", status.as_str())
    }
}

/// Comfortable width (terminal columns) for one column of panels in
/// `"auto"` sizing — enough for a value, unit, and "updated Xs ago".
const AUTO_COLUMN_WIDTH: u16 = 30;

/// Centers and caps the horizontal extent of `area` per `tui_width`, sized
/// against the widest row's column count in `"auto"` mode. Leaves `area`
/// unchanged when there's nothing to size around (`max_columns == 0`)
/// (spec: tui — configurable TUI content width).
fn content_rect(
    area: ratatui::layout::Rect,
    tui_width: &TuiWidth,
    max_columns: usize,
) -> ratatui::layout::Rect {
    if max_columns == 0 {
        return area;
    }
    let desired = match tui_width {
        TuiWidth::Fixed(cols) => *cols,
        TuiWidth::Named(_) => (max_columns as u16).saturating_mul(AUTO_COLUMN_WIDTH),
    };
    let width = desired.min(area.width);
    let x = area.x + (area.width - width) / 2;
    ratatui::layout::Rect {
        x,
        y: area.y,
        width,
        height: area.height,
    }
}

fn draw(f: &mut ratatui::Frame, state: &UiState, tui_width: &TuiWidth) {
    let area = f.area();

    // Grid rows stacked vertically (equal height each), columns within a row
    // proportional to cell spans.
    let rows: Vec<&Vec<Slot>> = state
        .rows
        .iter()
        .filter(|row| row.iter().map(|c| c.span).sum::<usize>() > 0)
        .collect();
    let max_columns = rows
        .iter()
        .map(|r| r.iter().map(|c| c.span).sum::<usize>())
        .max()
        .unwrap_or(0);
    let content = content_rect(area, tui_width, max_columns);

    let mut idx = 0usize;
    let next_line = |idx: &mut usize| -> u16 {
        let y = content.y + *idx as u16;
        *idx += 1;
        y
    };

    header_line(
        f,
        content,
        next_line(&mut idx),
        format!(" barduck v{VERSION} -- [q] quit "),
        Style::default().fg(Color::Black).bg(Color::Cyan),
    );
    if let Some(err) = &state.error {
        header_line(
            f,
            content,
            next_line(&mut idx),
            format!(" ERROR: {err} "),
            Style::default().fg(Color::Black).bg(Color::Red),
        );
    }

    if rows.is_empty() {
        return;
    }
    let body = ratatui::layout::Rect {
        x: content.x,
        y: content.y + idx as u16,
        width: content.width,
        height: content.height.saturating_sub(idx as u16),
    };
    let row_areas = Layout::vertical(vec![Constraint::Fill(1); rows.len()]).split(body);
    for (row, rect) in rows.iter().zip(row_areas.iter()) {
        let col_areas = Layout::horizontal(
            row.iter()
                .map(|c| Constraint::Fill(c.span as u16))
                .collect::<Vec<_>>(),
        )
        .split(*rect);
        for (cell, crect) in row.iter().zip(col_areas.iter()) {
            if cell.main.is_some()
                || !cell.secondary.is_empty()
                || !cell.table.is_empty()
                || cell.text.is_some()
            {
                f.render_widget(panel_widget(cell), *crect);
            }
        }
    }
}

fn header_line(
    f: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    y: u16,
    text: String,
    style: Style,
) {
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(text, style))),
        ratatui::layout::Rect {
            x: area.x,
            y,
            width: area.width,
            height: 1,
        },
    );
}

/// Renders a slot's panel(s). When `main` is the slot's only content (a
/// plain single-source cell, or a generalized pane with only `main` set), it
/// renders exactly as a single-source panel always has: one styled line,
/// value shown directly, age always shown. Otherwise it's a generalized
/// pane combining up to three sections in one bordered panel — `main`
/// (value shown directly, always-shown age, emphasized style), `secondary`
/// (same treatment, non-emphasized style), and `table` (today's "label:
/// value" lines, age only when stale) — with the panel's own border colored
/// by the worst member across all three sections (spec: tui — group panes
/// show multiple labeled, independently colored values).
fn panel_widget(slot: &Slot) -> Paragraph<'_> {
    if let Some(text) = &slot.text {
        // Shown as-is: the TUI never interprets `markdown`/`json` formatting
        // (spec: tui — static-text panel rendering). No age suffix and no
        // health/threshold color — there's no source behind this panel.
        // `text`'s own newlines must become separate `Line`s — a single
        // `Line` never breaks on embedded `\n`, it would just run everything
        // together — and `Wrap` lets a long line (a link, a list item) wrap
        // instead of being cut off at the panel's width.
        let lines: Vec<Line> = text.lines().map(Line::from).collect();
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", slot.group_title.as_deref().unwrap_or(""))),
        )
    } else if slot.secondary.is_empty()
        && slot.table.is_empty()
        && let Some(p) = &slot.main
    {
        let style = status_style(p.level, p.status);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(p.ts_epoch, |d| d.as_secs_f64());
        let updated =
            crate::age::ago(now, p.ts_epoch).map_or_else(String::new, |s| format!(" - {s}"));
        // A value can itself span multiple lines (e.g. a markdown-format
        // source's fetched content) — each of its lines becomes its own
        // `Line` so they stack instead of being joined into one, with the
        // unit/age suffix trailing the last line.
        let value_lines: Vec<&str> = {
            let l = p.value.lines().collect::<Vec<_>>();
            if l.is_empty() { vec![""] } else { l }
        };
        let mut lines: Vec<Line> = value_lines
            .into_iter()
            .map(|l| {
                Line::from(Span::styled(
                    l.to_string(),
                    style.add_modifier(Modifier::BOLD),
                ))
            })
            .collect();
        if let Some(last) = lines.last_mut() {
            if !p.unit.is_empty() {
                last.spans.push(Span::raw(format!(" {}", p.unit)));
            }
            last.spans.push(Span::styled(
                updated,
                Style::default().add_modifier(Modifier::DIM),
            ));
        }
        let title = slot.group_title.as_deref().unwrap_or(&p.name);
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(style)
                .title(Span::styled(
                    format!(" {title} [{}] ", p.status.as_str()),
                    style,
                )),
        )
    } else {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64());
        let colors: Vec<Level> = slot
            .main
            .iter()
            .chain(&slot.secondary)
            .chain(&slot.table)
            .map(|p| crate::config::status_color(p.level, p.status))
            .collect();
        let border_style = color_style(crate::config::worst_color(colors));
        let mut lines: Vec<Line> = Vec::new();
        if let Some(p) = &slot.main {
            lines.push(main_or_secondary_line(p, now, Modifier::BOLD));
        }
        for p in &slot.secondary {
            lines.push(main_or_secondary_line(p, now, Modifier::empty()));
        }
        for p in &slot.table {
            let style = status_style(p.level, p.status);
            // Only show the age when the value is lagging (stale) — a
            // table line otherwise omits it to stay compact.
            let updated = if p.status == Health::Stale {
                crate::age::ago(now, p.ts_epoch).map_or_else(String::new, |s| format!(" - {s}"))
            } else {
                String::new()
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{}: ", p.name),
                    Style::default().add_modifier(Modifier::DIM),
                ),
                Span::styled(p.value.clone(), style.add_modifier(Modifier::BOLD)),
                Span::raw(if p.unit.is_empty() {
                    String::new()
                } else {
                    format!(" {}", p.unit)
                }),
                Span::styled(
                    plain_label(p.status),
                    Style::default().add_modifier(Modifier::DIM),
                ),
                Span::styled(updated, Style::default().add_modifier(Modifier::DIM)),
            ]));
        }
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(format!(" {} ", slot.group_title.as_deref().unwrap_or(""))),
        )
    }
}

/// One `main`/`secondary` line within a generalized pane: value shown
/// directly (no label), age always shown, `value_modifier` distinguishing
/// `main` (bold) from `secondary` (plain) (spec: tui — group panes show
/// multiple labeled, independently colored values).
fn main_or_secondary_line(p: &Panel, now: f64, value_modifier: Modifier) -> Line<'_> {
    let style = status_style(p.level, p.status);
    let updated = crate::age::ago(now, p.ts_epoch).map_or_else(String::new, |s| format!(" - {s}"));
    Line::from(vec![
        Span::styled(p.value.clone(), style.add_modifier(value_modifier)),
        Span::raw(if p.unit.is_empty() {
            String::new()
        } else {
            format!(" {}", p.unit)
        }),
        Span::styled(
            plain_label(p.status),
            Style::default().add_modifier(Modifier::DIM),
        ),
        Span::styled(updated, Style::default().add_modifier(Modifier::DIM)),
    ])
}

/// Style for a color level, shared by a group panel's own border and (via
/// [`status_style`]) each member line.
fn color_style(color: Level) -> Style {
    match color {
        Level::Red => Style::default().fg(Color::Red),
        Level::Yellow => Style::default().fg(Color::Yellow),
        Level::Green => Style::default().fg(Color::Green),
    }
}

/// Owns the terminal's raw-mode and alternate-screen state for the TUI's
/// lifetime, restoring both on drop.
///
/// A `Drop` impl rather than a `restore()` call at the end of `run`: the
/// call is skipped when the stack unwinds, so any panic inside the render
/// loop used to leave the user's shell in raw mode with no echo and no
/// visible prompt — recoverable only by blindly typing `reset`. Drop runs on
/// that path too.
struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<std::io::Stdout>>,
}

impl TerminalGuard {
    fn enter() -> Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        // No mouse capture: nothing here reads mouse events, and enabling it
        // costs the user their terminal's own click-to-select and copy.
        if let Err(e) = crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen) {
            let _ = crossterm::terminal::disable_raw_mode();
            return Err(e.into());
        }
        match Terminal::new(CrosstermBackend::new(stdout)) {
            Ok(terminal) => Ok(Self { terminal }),
            Err(e) => {
                let _ = crossterm::execute!(
                    std::io::stdout(),
                    crossterm::terminal::LeaveAlternateScreen
                );
                let _ = crossterm::terminal::disable_raw_mode();
                Err(e.into())
            }
        }
    }
}

impl std::ops::Deref for TerminalGuard {
    type Target = Terminal<CrosstermBackend<std::io::Stdout>>;
    fn deref(&self) -> &Self::Target {
        &self.terminal
    }
}

impl std::ops::DerefMut for TerminalGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.terminal
    }
}

impl Drop for TerminalGuard {
    /// Best-effort: nothing useful can be done if restoring the terminal
    /// fails, and a `Drop` running during an unwind must not panic.
    fn drop(&mut self) {
        let _ = crossterm::execute!(
            self.terminal.backend_mut(),
            crossterm::terminal::LeaveAlternateScreen
        );
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = self.terminal.show_cursor();
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn key(code: crossterm::event::KeyCode) -> crossterm::event::Event {
        crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
            code,
            crossterm::event::KeyModifiers::NONE,
        ))
    }

    /// `q` and Esc quit, and so does Ctrl+C: raw mode stops the terminal
    /// turning it into `SIGINT`, so without handling it here the TUI can't
    /// be interrupted the way every other terminal program can (spec: tui).
    #[test]
    fn quit_keys_are_recognized() {
        use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
        assert!(quit_requested(&key(KeyCode::Char('q'))));
        assert!(quit_requested(&key(KeyCode::Esc)));
        assert!(quit_requested(&Event::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL
        ))));

        // Anything else keeps the TUI running.
        assert!(!quit_requested(&key(KeyCode::Char('c'))));
        assert!(!quit_requested(&key(KeyCode::Char('x'))));
        assert!(!quit_requested(&key(KeyCode::Enter)));
        assert!(!quit_requested(&Event::Resize(10, 10)));

        // A key *release* must not quit — otherwise letting go of an
        // unrelated key would exit on terminals that report both edges.
        let mut release = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;
        assert!(!quit_requested(&Event::Key(release)));
    }

    fn wait_for_outcome(r: &mut Refresher) -> Outcome {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(outcome) = r.take_latest() {
                return outcome;
            }
            assert!(Instant::now() < deadline, "refresh never completed");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// One worker serves every refresh for the TUI's lifetime — the whole
    /// point of the request channel, replacing a thread *and* a Tokio
    /// runtime built per refresh. A worker that only handled the first
    /// request would hang here on the second.
    #[test]
    fn refresher_serves_many_refreshes_from_one_worker() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.duckdb");
        drop(crate::db::Db::open_rw(&path).unwrap());
        let cfg: Config =
            toml::from_str(&format!("database_path = \"{}\"\n", path.display())).unwrap();
        let backend = Backend::new(&cfg, false).unwrap();

        let mut refresher = Refresher::start(backend, cfg);
        assert!(refresher.is_idle(), "nothing running before the first request");
        for round in 0..3 {
            assert!(refresher.request(), "request {round} should dispatch");
            assert!(
                !refresher.is_idle(),
                "a running refresh must block a second one"
            );
            assert!(
                matches!(wait_for_outcome(&mut refresher), Outcome::Data(..)),
                "refresh {round} should have returned data"
            );
            assert!(refresher.is_idle(), "finishing frees the worker again");
        }
    }

    /// Several results queued between frames collapse to the newest, so a
    /// slow UI renders current data instead of working through stale ones.
    #[test]
    fn take_latest_drains_to_the_newest_outcome() {
        let (requests, _work) = std::sync::mpsc::channel();
        let (results, outcomes) = std::sync::mpsc::channel();
        let mut refresher = Refresher {
            requests,
            outcomes,
            in_flight: true,
        };
        results.send(Outcome::Failed("stale".into())).unwrap();
        results.send(Outcome::Data(Vec::new(), Vec::new())).unwrap();

        assert!(matches!(
            refresher.take_latest(),
            Some(Outcome::Data(..))
        ));
        assert!(refresher.is_idle());
        assert!(refresher.take_latest().is_none(), "queue is drained");
    }

    /// A worker that never started (its runtime failed to build) must be
    /// reported, not retried silently forever.
    #[test]
    fn request_reports_a_dead_worker() {
        let (requests, work) = std::sync::mpsc::channel();
        drop(work);
        let (_results, outcomes) = std::sync::mpsc::channel();
        let mut refresher = Refresher {
            requests,
            outcomes,
            in_flight: false,
        };
        assert!(!refresher.request());
        assert!(
            refresher.is_idle(),
            "a failed dispatch must not leave the UI thinking a refresh is running"
        );
    }

    /// Renders one frame offscreen; fails if drawing panics or drops panels.
    #[test]
    fn draw_renders_panels_and_error_banner() {
        let state = UiState {
            error: Some("daemon unreachable".into()),
            // Two rows: [echo | balance] and [spacer(span2) | dead].
            rows: vec![
                vec![
                    Slot {
                        span: 1,
                        group_title: None,
                        main: Some(Panel {
                            name: "echo".into(),
                            value: "42".into(),
                            unit: String::new(),
                            status: Health::Healthy,
                            level: None,
                            ts_epoch: 0.0,
                        }),
                        secondary: Vec::new(),
                        table: Vec::new(),
                        text: None,
                    },
                    Slot {
                        span: 1,
                        group_title: None,
                        main: Some(Panel {
                            name: "Balance".into(),
                            value: "7.25".into(),
                            unit: "USD".into(),
                            status: Health::Healthy,
                            level: None,
                            ts_epoch: 0.0,
                        }),
                        secondary: Vec::new(),
                        table: Vec::new(),
                        text: None,
                    },
                ],
                vec![
                    Slot {
                        span: 2,
                        group_title: None,
                        main: None,
                        secondary: Vec::new(),
                        table: Vec::new(),
                        text: None,
                    },
                    Slot {
                        span: 1,
                        group_title: None,
                        main: Some(Panel {
                            name: "dead-service".into(),
                            value: "\u{2014}".into(),
                            unit: String::new(),
                            status: Health::Failing,
                            level: None,
                            ts_epoch: 0.0,
                        }),
                        secondary: Vec::new(),
                        table: Vec::new(),
                        text: None,
                    },
                ],
            ],
        };
        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| draw(f, &state, &TuiWidth::Named("auto".into())))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let text: String = (0..buf.area().height)
            .map(|y| {
                (0..buf.area().width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect();
        assert!(text.contains("echo"), "panel title missing");
        assert!(text.contains("42"), "panel value missing");
        assert!(text.contains("Balance"), "second-row panel title missing");
        assert!(text.contains("dead-service"), "third panel title missing");
        assert!(text.contains("daemon unreachable"), "error banner missing");
    }

    /// One bordered panel titled with the group's title, one line per
    /// member, each colored independently (health-derived color overriding
    /// a stale band reading), the panel's own border colored by the worst
    /// member, and each line showing its own update age only when that
    /// member is lagging (stale) (spec: tui — group panes show multiple
    /// labeled, independently colored values).
    #[test]
    fn group_panel_renders_title_and_labeled_lines() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        let state = UiState {
            error: None,
            rows: vec![vec![Slot {
                span: 1,
                group_title: Some("ihor".into()),
                main: None,
                secondary: Vec::new(),
                table: vec![
                    Panel {
                        name: "days left".into(),
                        value: "5".into(),
                        unit: "d".into(),
                        status: Health::Stale,
                        level: Some(Level::Red),
                        ts_epoch: now - 629.0,
                    },
                    Panel {
                        name: "balance".into(),
                        value: "90".into(),
                        unit: "USD".into(),
                        status: Health::Healthy,
                        level: Some(Level::Green),
                        ts_epoch: now,
                    },
                ],
                text: None,
            }]],
        };
        // Fixed, not "auto": the line now carries a plain "[stale]" label
        // alongside its age suffix, needing more than auto-sizing's
        // one-column default width to render without truncating.
        let backend = ratatui::backend::TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| draw(f, &state, &TuiWidth::Fixed(38)))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let text: String = (0..buf.area().height)
            .map(|y| {
                (0..buf.area().width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect();
        assert!(text.contains("ihor"), "group title missing");
        assert!(text.contains("days left"), "first member label missing");
        assert!(text.contains('5'), "first member value missing");
        assert!(text.contains("balance"), "second member label missing");
        assert!(text.contains("90"), "second member value missing");
        assert!(
            text.contains("10m ago"),
            "stale first member's update age missing"
        );

        // A member that isn't lagging shows no age at all.
        let rows: Vec<String> = (0..buf.area().height)
            .map(|y| {
                (0..buf.area().width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect();
        let balance_row = rows
            .iter()
            .find(|r| r.contains("balance"))
            .expect("balance line missing");
        assert!(
            !balance_row.contains("ago"),
            "fresh member should show no age: {balance_row:?}"
        );

        // The two value cells carry their own colors, not one shared color
        // for the whole panel — and "days left"'s stale health overrides its
        // red band entirely (health takes priority over a threshold
        // reading, since a stale fetch means that reading is no longer
        // trustworthy) (spec: tui — threshold band coloring).
        let find_fg = |needle: &str| -> Option<Color> {
            for y in 0..buf.area().height {
                for x in 0..buf.area().width {
                    if buf[(x, y)].symbol() == &needle[..1] {
                        let row: String = (x..buf.area().width)
                            .map(|xi| buf[(xi, y)].symbol())
                            .collect();
                        if row.starts_with(needle) {
                            return Some(buf[(x, y)].fg);
                        }
                    }
                }
            }
            None
        };
        assert_eq!(
            find_fg("5"),
            Some(Color::Yellow),
            "stale health should override the red band"
        );
        assert_eq!(find_fg("90"), Some(Color::Green));

        // The panel's own border is yellow — the worst of its two members
        // (days-left's health-overridden yellow beats balance's green).
        let border_fg = (0..buf.area().height)
            .flat_map(|y| (0..buf.area().width).map(move |x| (x, y)))
            .find(|&(x, y)| buf[(x, y)].symbol() == "─")
            .map(|(x, y)| buf[(x, y)].fg);
        assert_eq!(
            border_fg,
            Some(Color::Yellow),
            "panel border should reflect the worst member"
        );
    }

    /// An unbanded, failing group member renders its value in red
    /// (health-derived, since it has no bands) and additionally shows a
    /// plain "[failing]" label alongside it, while the panel's own border
    /// still reflects that member as the worst (spec: tui — group panes show
    /// multiple labeled, independently colored values; threshold band
    /// coloring).
    #[test]
    fn group_panel_unbanded_member_colors_red_with_plain_label() {
        let state = UiState {
            error: None,
            rows: vec![vec![Slot {
                span: 1,
                group_title: Some("grp".into()),
                main: None,
                secondary: Vec::new(),
                table: vec![
                    Panel {
                        name: "days left".into(),
                        value: "5".into(),
                        unit: "d".into(),
                        status: Health::Healthy,
                        level: Some(Level::Green),
                        ts_epoch: 0.0,
                    },
                    Panel {
                        name: "flaky".into(),
                        value: "9".into(),
                        unit: String::new(),
                        status: Health::Failing,
                        level: None,
                        ts_epoch: 0.0,
                    },
                ],
                text: None,
            }]],
        };
        let backend = ratatui::backend::TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| draw(f, &state, &TuiWidth::Named("auto".into())))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let rows: Vec<String> = (0..buf.area().height)
            .map(|y| {
                (0..buf.area().width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect();
        let (y, flaky_row) = rows
            .iter()
            .enumerate()
            .find(|(_, r)| r.contains("flaky"))
            .expect("flaky line missing");
        assert!(
            flaky_row.contains("[failing]"),
            "plain failing label expected: {flaky_row:?}"
        );

        // The value itself now renders red — health-derived, alongside the
        // label. Use a char (column) index, not a byte index: the border's
        // box-drawing characters are multi-byte, so `str::find` would
        // misalign with the buffer's column coordinates.
        let value_x = flaky_row
            .chars()
            .position(|c| c == '9')
            .expect("flaky value missing");
        assert_eq!(
            buf[(value_x as u16, y as u16)].fg,
            Color::Red,
            "unbanded failing value should render red"
        );

        // The panel's own border still reflects the failing unbanded member.
        let border_fg = (0..buf.area().height)
            .flat_map(|y| (0..buf.area().width).map(move |x| (x, y)))
            .find(|&(x, y)| buf[(x, y)].symbol() == "─")
            .map(|(x, y)| buf[(x, y)].fg);
        assert_eq!(
            border_fg,
            Some(Color::Red),
            "border should still flag the failing unbanded member"
        );
    }

    /// A generalized pane with only `main` set renders exactly like a
    /// single-source panel, titled with the cell's own title rather than
    /// `main`'s own label (spec: tui — group panes show multiple labeled,
    /// independently colored values).
    #[test]
    fn group_panel_with_only_main_renders_like_single_source_panel() {
        let state = UiState {
            error: None,
            rows: vec![vec![Slot {
                span: 1,
                group_title: Some("CPU".into()),
                main: Some(Panel {
                    name: "cpu-load-ignored".into(),
                    value: "42".into(),
                    unit: "%".into(),
                    status: Health::Healthy,
                    level: Some(Level::Yellow),
                    ts_epoch: 0.0,
                }),
                secondary: Vec::new(),
                table: Vec::new(),
                text: None,
            }]],
        };
        let backend = ratatui::backend::TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| draw(f, &state, &TuiWidth::Named("auto".into())))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let text: String = (0..buf.area().height)
            .map(|y| {
                (0..buf.area().width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect();
        assert!(
            text.contains("CPU"),
            "cell title should be used, not main's own label"
        );
        assert!(
            !text.contains("cpu-load-ignored"),
            "main's own label should not appear"
        );
        assert!(text.contains("42"), "main value missing");
        let border_fg = (0..buf.area().height)
            .flat_map(|y| (0..buf.area().width).map(move |x| (x, y)))
            .find(|&(x, y)| buf[(x, y)].symbol() == "─")
            .map(|(x, y)| buf[(x, y)].fg);
        assert_eq!(
            border_fg,
            Some(Color::Yellow),
            "border should reflect main's own band color"
        );
    }

    /// A generalized pane combining all three sections renders `main`,
    /// `secondary`, and `table` members together in one bordered panel, with
    /// the border reflecting the worst member across every section — even
    /// an unbanded, failing `secondary` member (spec: tui — group panes show
    /// multiple labeled, independently colored values).
    #[test]
    fn group_panel_combines_main_secondary_and_table_sections() {
        let state = UiState {
            error: None,
            rows: vec![vec![Slot {
                span: 1,
                group_title: Some("Server".into()),
                main: Some(Panel {
                    name: "cpu-load".into(),
                    value: "42".into(),
                    unit: "%".into(),
                    status: Health::Healthy,
                    level: Some(Level::Green),
                    ts_epoch: 0.0,
                }),
                secondary: vec![Panel {
                    name: "mem-used".into(),
                    value: "80".into(),
                    unit: "%".into(),
                    status: Health::Failing,
                    level: None,
                    ts_epoch: 0.0,
                }],
                table: vec![Panel {
                    name: "days left".into(),
                    value: "5".into(),
                    unit: "d".into(),
                    status: Health::Healthy,
                    level: Some(Level::Green),
                    ts_epoch: 0.0,
                }],
                text: None,
            }]],
        };
        let backend = ratatui::backend::TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| draw(f, &state, &TuiWidth::Named("auto".into())))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let text: String = (0..buf.area().height)
            .map(|y| {
                (0..buf.area().width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect();
        assert!(text.contains("Server"), "cell title missing");
        assert!(text.contains("42"), "main value missing");
        assert!(text.contains("80"), "secondary value missing");
        assert!(
            text.contains("mem-used") || text.contains('%'),
            "secondary content missing"
        );
        assert!(text.contains("days left"), "table label missing");
        assert!(text.contains('5'), "table value missing");
        let border_fg = (0..buf.area().height)
            .flat_map(|y| (0..buf.area().width).map(move |x| (x, y)))
            .find(|&(x, y)| buf[(x, y)].symbol() == "─")
            .map(|(x, y)| buf[(x, y)].fg);
        assert_eq!(
            border_fg,
            Some(Color::Red),
            "border should reflect the failing unbanded secondary member, not just main/table"
        );
    }

    /// A static-text panel renders its raw content in a bordered panel,
    /// titled with the cell's `title` (spec: tui — static-text panel
    /// rendering).
    #[test]
    fn text_panel_renders_titled_content() {
        let state = UiState {
            error: None,
            rows: vec![vec![Slot {
                span: 1,
                group_title: Some("Links".into()),
                main: None,
                secondary: Vec::new(),
                table: Vec::new(),
                text: Some("github.com".into()),
            }]],
        };
        let backend = ratatui::backend::TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| draw(f, &state, &TuiWidth::Named("auto".into())))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let text: String = (0..buf.area().height)
            .map(|y| {
                (0..buf.area().width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect();
        assert!(text.contains("Links"), "panel title missing");
        assert!(text.contains("github.com"), "panel content missing");
        let border_fg = (0..buf.area().height)
            .flat_map(|y| (0..buf.area().width).map(move |x| (x, y)))
            .find(|&(x, y)| buf[(x, y)].symbol() == "─")
            .map(|(x, y)| buf[(x, y)].fg);
        assert_eq!(
            border_fg,
            Some(Color::Reset),
            "a text panel's border should be the terminal's default, unaccented style"
        );
    }

    /// An untitled static-text panel renders no title (spec: tui —
    /// static-text panel rendering).
    #[test]
    fn text_panel_without_title_renders_no_title() {
        let state = UiState {
            error: None,
            rows: vec![vec![Slot {
                span: 1,
                group_title: None,
                main: None,
                secondary: Vec::new(),
                table: Vec::new(),
                text: Some("Just a note.".into()),
            }]],
        };
        let backend = ratatui::backend::TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| draw(f, &state, &TuiWidth::Named("auto".into())))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let text: String = (0..buf.area().height)
            .map(|y| {
                (0..buf.area().width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect();
        assert!(text.contains("Just a note."), "panel content missing");
    }

    /// A `markdown`-format static-text panel still shows the literal,
    /// uninterpreted text — the TUI never parses `markdown`/`json` (spec:
    /// tui — static-text panel rendering).
    #[test]
    fn text_panel_shows_markdown_as_is() {
        let state = UiState {
            error: None,
            rows: vec![vec![Slot {
                span: 1,
                group_title: Some("Note".into()),
                main: None,
                secondary: Vec::new(),
                table: Vec::new(),
                text: Some("# Heading".into()),
            }]],
        };
        let backend = ratatui::backend::TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| draw(f, &state, &TuiWidth::Named("auto".into())))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let text: String = (0..buf.area().height)
            .map(|y| {
                (0..buf.area().width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect();
        assert!(
            text.contains("# Heading"),
            "literal markdown text should appear uninterpreted"
        );
    }

    /// A static-text panel's embedded newlines become separate lines in the
    /// rendered panel, not one run-together line (spec: tui — static-text
    /// panel rendering).
    #[test]
    fn text_panel_splits_on_newlines() {
        let state = UiState {
            error: None,
            rows: vec![vec![Slot {
                span: 1,
                group_title: Some("Links".into()),
                main: None,
                secondary: Vec::new(),
                table: Vec::new(),
                text: Some("first line\nsecond line\nthird line".into()),
            }]],
        };
        let backend = ratatui::backend::TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| draw(f, &state, &TuiWidth::Named("auto".into())))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let rows: Vec<String> = (0..buf.area().height)
            .map(|y| {
                (0..buf.area().width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect();
        let y_of = |needle: &str| rows.iter().position(|r| r.contains(needle));
        let (y1, y2, y3) = (
            y_of("first line").expect("first line missing"),
            y_of("second line").expect("second line missing"),
            y_of("third line").expect("third line missing"),
        );
        assert!(
            y1 < y2 && y2 < y3,
            "each line should render on its own row, in order: {y1}, {y2}, {y3}"
        );
    }

    /// A single-source panel's value can itself be multi-line (e.g. a
    /// markdown-format source's fetched content) — it must render as
    /// stacked lines, not collapse onto one (spec: tui — Main section
    /// renders like a single-source panel).
    #[test]
    fn single_source_panel_splits_value_on_newlines() {
        let state = UiState {
            error: None,
            rows: vec![vec![Slot {
                span: 1,
                group_title: None,
                main: Some(Panel {
                    name: "weekly-report".into(),
                    value: "first line\nsecond line\nthird line".into(),
                    unit: String::new(),
                    status: Health::Healthy,
                    level: None,
                    ts_epoch: 0.0,
                }),
                secondary: Vec::new(),
                table: Vec::new(),
                text: None,
            }]],
        };
        let backend = ratatui::backend::TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| draw(f, &state, &TuiWidth::Named("auto".into())))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let rows: Vec<String> = (0..buf.area().height)
            .map(|y| {
                (0..buf.area().width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect();
        let y_of = |needle: &str| rows.iter().position(|r| r.contains(needle));
        let (y1, y2, y3) = (
            y_of("first line").expect("first line missing"),
            y_of("second line").expect("second line missing"),
            y_of("third line").expect("third line missing"),
        );
        assert!(
            y1 < y2 && y2 < y3,
            "each line should render on its own row, in order: {y1}, {y2}, {y3}"
        );
    }

    /// A banded value colors by its band when healthy (spec: tui —
    /// threshold band coloring).
    #[test]
    fn banded_value_colors_by_band_when_healthy() {
        assert_eq!(
            status_style(Some(Level::Red), Health::Healthy).fg,
            Some(Color::Red)
        );
        assert_eq!(
            status_style(Some(Level::Yellow), Health::Healthy).fg,
            Some(Color::Yellow)
        );
    }

    /// Health status overrides a stale band reading — even a source with
    /// bands shows red/yellow while failing/stale, not its last band color
    /// (spec: tui — threshold band coloring).
    #[test]
    fn health_overrides_a_stale_band_reading() {
        assert_eq!(
            status_style(Some(Level::Green), Health::Failing).fg,
            Some(Color::Red)
        );
        assert_eq!(
            status_style(Some(Level::Green), Health::Stale).fg,
            Some(Color::Yellow)
        );
    }

    /// An unbanded, healthy source has no accent color at all — not even
    /// green — while failing/stale still colors it (spec: tui — threshold
    /// band coloring).
    #[test]
    fn unbanded_healthy_source_has_no_accent_color() {
        assert_eq!(status_style(None, Health::Healthy).fg, None);
        assert_eq!(status_style(None, Health::Failing).fg, Some(Color::Red));
        assert_eq!(status_style(None, Health::Stale).fg, Some(Color::Yellow));
    }

    /// The plain status label appears for any currently unhealthy value,
    /// banded or not (spec: tui — threshold band coloring; group panes).
    #[test]
    fn plain_label_only_for_non_healthy() {
        assert_eq!(plain_label(Health::Healthy), "");
        assert_eq!(plain_label(Health::Failing), " [failing]");
        assert_eq!(plain_label(Health::Stale), " [stale]");
    }

    /// (spec: tui — configurable TUI content width)
    #[test]
    fn auto_width_stays_narrow_for_few_columns() {
        let area = ratatui::layout::Rect {
            x: 0,
            y: 0,
            width: 220,
            height: 40,
        };
        let rect = content_rect(area, &TuiWidth::Named("auto".into()), 2);
        assert!(
            rect.width < area.width,
            "2 columns should not fill a 220-wide terminal"
        );
        assert_eq!(rect.width, 60);
    }

    #[test]
    fn auto_width_grows_with_more_columns_but_stays_capped() {
        let area = ratatui::layout::Rect {
            x: 0,
            y: 0,
            width: 220,
            height: 40,
        };
        let narrow = content_rect(area, &TuiWidth::Named("auto".into()), 2);
        let wide = content_rect(area, &TuiWidth::Named("auto".into()), 6);
        assert!(
            wide.width > narrow.width,
            "more columns should use more space"
        );
        assert!(
            wide.width <= area.width,
            "auto width must never exceed the terminal"
        );
    }

    #[test]
    fn fixed_width_caps_at_terminal_width() {
        let area = ratatui::layout::Rect {
            x: 0,
            y: 0,
            width: 220,
            height: 40,
        };
        let rect = content_rect(area, &TuiWidth::Fixed(300), 2);
        assert_eq!(
            rect.width, 220,
            "fixed width larger than the terminal should be capped"
        );
    }

    #[test]
    fn empty_rows_keep_full_width() {
        let area = ratatui::layout::Rect {
            x: 0,
            y: 0,
            width: 220,
            height: 40,
        };
        let rect = content_rect(area, &TuiWidth::Named("auto".into()), 0);
        assert_eq!(
            rect, area,
            "nothing to size around should leave the area unchanged"
        );
    }

    fn cfg_from(toml: &str) -> Config {
        let cfg: Config = toml::from_str(toml).unwrap();
        crate::config::validate(&cfg).unwrap();
        cfg
    }

    fn only_slot(cfg: &Config) -> Slot {
        let mut state = UiState {
            error: None,
            rows: Vec::new(),
        };
        apply_outcome(cfg, Outcome::Data(Vec::new(), Vec::new()), &mut state);
        state.rows.remove(0).remove(0)
    }

    /// A layout cell for a source hidden from the TUI renders as an empty
    /// space, not the source's panel (spec: tui — hidden sources render as
    /// space in the TUI).
    #[test]
    fn hidden_source_cell_renders_as_space() {
        let cfg = cfg_from(
            "[[sources]]\nname = \"a\"\ntype = \"query\"\ncommand = \"echo 0\"\nshow_in = \"web\"\n\n[[layouts]]\ntitle = \"L\"\nrows = [[\"a\"]]\n",
        );
        let slot = only_slot(&cfg);
        assert!(
            slot.main.is_none()
                && slot.secondary.is_empty()
                && slot.table.is_empty()
                && slot.text.is_none()
        );
    }

    /// A layout cell for a source visible in the TUI (`show_in = "tui"` or
    /// `"all"`, or unset) renders normally (spec: tui — hidden sources render
    /// as space in the TUI).
    #[test]
    fn visible_source_cell_renders_normally() {
        for show_in in ["tui", "all"] {
            let cfg = cfg_from(&format!(
                "[[sources]]\nname = \"a\"\ntype = \"query\"\ncommand = \"echo 0\"\nshow_in = \"{show_in}\"\n\n[[layouts]]\ntitle = \"L\"\nrows = [[\"a\"]]\n"
            ));
            let slot = only_slot(&cfg);
            assert!(
                slot.main.is_some(),
                "show_in = {show_in:?} should still render in the TUI"
            );
        }
        let cfg = cfg_from(
            "[[sources]]\nname = \"a\"\ntype = \"query\"\ncommand = \"echo 0\"\n\n[[layouts]]\ntitle = \"L\"\nrows = [[\"a\"]]\n",
        );
        assert!(
            only_slot(&cfg).main.is_some(),
            "no show_in should still render in the TUI"
        );
    }

    /// A generalized pane's `secondary` member hidden from the TUI is
    /// omitted; the remaining members still render (spec: tui — hidden
    /// sources render as space in the TUI).
    #[test]
    fn hidden_pane_member_is_omitted() {
        let cfg = cfg_from(
            "[[sources]]\nname = \"a\"\ntype = \"query\"\ncommand = \"echo 0\"\nshow_in = \"web\"\n\n[[sources]]\nname = \"b\"\ntype = \"query\"\ncommand = \"echo 0\"\n\n[[layouts]]\ntitle = \"L\"\nrows = [[{ title = \"grp\", secondary = [\"a\", \"b\"] }]]\n",
        );
        let slot = only_slot(&cfg);
        assert_eq!(
            slot.secondary.len(),
            1,
            "the hidden member should be omitted, not just left empty"
        );
    }

    /// A generalized pane cell whose every member is hidden from the TUI
    /// renders as an empty space (spec: tui — hidden sources render as space
    /// in the TUI).
    #[test]
    fn pane_with_every_member_hidden_renders_as_space() {
        let cfg = cfg_from(
            "[[sources]]\nname = \"a\"\ntype = \"query\"\ncommand = \"echo 0\"\nshow_in = \"web\"\n\n[[layouts]]\ntitle = \"L\"\nrows = [[{ title = \"grp\", secondary = [\"a\"] }]]\n",
        );
        let slot = only_slot(&cfg);
        assert!(
            slot.main.is_none()
                && slot.secondary.is_empty()
                && slot.table.is_empty()
                && slot.text.is_none()
        );
    }

    /// Hiding a source from the TUI does not change the layout's column
    /// count or row geometry — only its content (spec: tui — hidden sources
    /// render as space in the TUI).
    #[test]
    fn hiding_a_source_does_not_change_grid_geometry() {
        let visible = cfg_from(
            "[[sources]]\nname = \"a\"\ntype = \"query\"\ncommand = \"echo 0\"\n\n[[layouts]]\ntitle = \"L\"\nrows = [[\"a\", { kind = \"space\", colspan = 2 }]]\n",
        );
        let hidden = cfg_from(
            "[[sources]]\nname = \"a\"\ntype = \"query\"\ncommand = \"echo 0\"\nshow_in = \"web\"\n\n[[layouts]]\ntitle = \"L\"\nrows = [[\"a\", { kind = \"space\", colspan = 2 }]]\n",
        );
        let mut visible_state = UiState {
            error: None,
            rows: Vec::new(),
        };
        apply_outcome(
            &visible,
            Outcome::Data(Vec::new(), Vec::new()),
            &mut visible_state,
        );
        let mut hidden_state = UiState {
            error: None,
            rows: Vec::new(),
        };
        apply_outcome(
            &hidden,
            Outcome::Data(Vec::new(), Vec::new()),
            &mut hidden_state,
        );
        let visible_spans: Vec<usize> = visible_state.rows[0].iter().map(|s| s.span).collect();
        let hidden_spans: Vec<usize> = hidden_state.rows[0].iter().map(|s| s.span).collect();
        assert_eq!(
            visible_spans, hidden_spans,
            "column spans must not change when a source is hidden"
        );
    }

    /// From the same shared layout, a source restricted to the web view
    /// renders as space in the TUI (the mirror web-ui behavior is proven in
    /// `tests/integration.rs`) (spec: tui — hidden sources render as space in
    /// the TUI — same layout renders differently per view).
    #[test]
    fn same_layout_renders_differently_per_view() {
        let cfg = cfg_from(
            "[[sources]]\nname = \"a\"\ntype = \"query\"\ncommand = \"echo 0\"\nshow_in = \"web\"\n\n[[layouts]]\ntitle = \"L\"\nrows = [[\"a\"]]\n",
        );
        let slot = only_slot(&cfg);
        assert!(
            slot.main.is_none(),
            "a web-only source should render as space in the TUI"
        );
    }
}
