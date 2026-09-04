#![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use crate::{config::{Config, TuiWidth, VERSION}, query::Backend};
use anyhow::Result;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use std::time::Duration;

/// Renders config-declared layouts as panels (spec: tui). Refreshes
/// periodically; shows an error banner instead of crashing when the data
/// path is unavailable (spec: tui — graceful degradation).
pub fn run(backend: &Backend, cfg: &Config) -> Result<()> {
    let mut terminal = init()?;
    let res = event_loop(&mut terminal, backend, cfg);
    restore(&mut terminal)?;
    res
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    backend: &Backend,
    cfg: &Config,
) -> Result<()> {
    let mut state = UiState {
        error: None,
        rows: Vec::new(),
        tick: 0,
    };
    // Fetches run on a detached thread so a slow daemon/DB round trip never
    // blocks the keyboard poll below — otherwise `q` has to wait for the
    // in-flight fetch to finish before it's even read.
    let (tx, rx) = std::sync::mpsc::channel();
    let mut refresh_in_flight = false;
    loop {
        if crossterm::event::poll(Duration::from_millis(250))?
            && let crossterm::event::Event::Key(key) = crossterm::event::read()?
            && matches!(key.kind, crossterm::event::KeyEventKind::Press)
            && matches!(key.code, crossterm::event::KeyCode::Char('q') | crossterm::event::KeyCode::Esc)
        {
            return Ok(());
        }
        // Refresh data every ~2 seconds.
        state.tick += 1;
        if !refresh_in_flight && state.tick % 8 == 1 {
            refresh_in_flight = true;
            spawn_refresh(backend.clone(), cfg.clone(), tx.clone());
        }
        if let Ok(outcome) = rx.try_recv() {
            refresh_in_flight = false;
            apply_outcome(cfg, outcome, &mut state);
        }
        terminal.draw(|f| draw(f, &state, &cfg.tui_width))?;
    }
}

struct Panel {
    name: String,
    value: String,
    unit: String,
    status: &'static str,
    /// Threshold band level for the current value, when the source has
    /// thresholds configured (spec: tui — threshold band coloring).
    level: Option<String>,
    ts_epoch: f64,
}

/// One grid slot: a spacer (`main` and `secondary`/`table` all empty), a
/// single-source panel (`main` only, `group_title` `None`), or a generalized
/// pane combining up to three sections — `main` (regular-panel treatment),
/// `secondary` (compact, always-shown age), and `table` (label/value rows,
/// age only when stale) (spec: tui — group panes show multiple labeled,
/// independently colored values).
struct Slot {
    span: usize,
    /// Card title for a group slot; unused for a plain single-source slot,
    /// whose title lives on its `main` panel's `name` instead.
    group_title: Option<String>,
    main: Option<Panel>,
    secondary: Vec<Panel>,
    table: Vec<Panel>,
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
    let src = cfg.sources.iter().find(|s| s.name == name);
    let label = title_override
        .or_else(|| src.and_then(crate::config::SourceCfg::display_title))
        .unwrap_or(name);
    let level = row.and_then(|r| {
        src.filter(|s| !s.thresholds.is_empty())
            .and_then(|s| crate::config::level_for(&s.thresholds, &r.value))
    });
    Panel {
        name: label.to_string(),
        value: row.map_or_else(|| "—".into(), |r| r.value.clone()),
        unit: row.and_then(|r| r.unit.clone()).unwrap_or_default(),
        status: healths.iter().find(|h| h.source == name).map_or("stale", |h| h.status.as_str()),
        level,
        ts_epoch: row.map_or(0.0, |r| r.ts_epoch),
    }
}

struct UiState {
    error: Option<String>,
    rows: Vec<Vec<Slot>>,
    tick: u32,
}

enum Outcome {
    Data(Vec<crate::db::ReadingRow>, Vec<crate::health::SourceHealth>),
    Failed(String),
}

fn spawn_refresh(backend: Backend, cfg: Config, tx: std::sync::mpsc::Sender<Outcome>) {
    std::thread::spawn(move || {
        let fut = async {
            match (backend.latest().await, backend.health(&cfg).await) {
                (Ok(latest), Ok(healths)) => Outcome::Data(latest, healths),
                (Err(e), _) | (_, Err(e)) => Outcome::Failed(format!("{e:#}")),
            }
        };
        // Dedicated single-thread runtime; this thread runs outside any
        // ambient tokio context so `Handle::block_on` would be unsafe here.
        let outcome = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt.block_on(fut),
            Err(e) => Outcome::Failed(format!("internal error: {e}")),
        };
        let _ = tx.send(outcome);
    });
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
                        let (main, secondary, table, group_title) = match cell {
                            crate::config::Cell::Group { title, main, secondary, table } => (
                                main.as_ref().map(|item| {
                                    build_panel(cfg, &latest, &healths, item.id(), item.explicit_label())
                                }),
                                secondary
                                    .iter()
                                    .map(|item| {
                                        build_panel(cfg, &latest, &healths, item.id(), item.explicit_label())
                                    })
                                    .collect(),
                                table
                                    .iter()
                                    .map(|item| {
                                        build_panel(cfg, &latest, &healths, item.id(), item.explicit_label())
                                    })
                                    .collect(),
                                title.clone(),
                            ),
                            crate::config::Cell::Source(name) => {
                                (Some(build_panel(cfg, &latest, &healths, name, None)), Vec::new(), Vec::new(), None)
                            }
                            crate::config::Cell::Pane { id, title } => (
                                Some(build_panel(cfg, &latest, &healths, id, title.as_deref())),
                                Vec::new(),
                                Vec::new(),
                                None,
                            ),
                            crate::config::Cell::Space { .. } => (None, Vec::new(), Vec::new(), None),
                        };
                        slots.push(Slot { span: cell.span(), group_title, main, secondary, table });
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
fn status_style(level: Option<&str>, status: &str) -> Style {
    match crate::config::accent_color(level, status) {
        Some(color) => color_style(color),
        None => Style::default(),
    }
}

/// Plain `[failing]`/`[stale]` suffix for a currently unhealthy value —
/// empty when healthy, shown alongside whatever color that status
/// contributes (spec: tui — threshold band coloring; group panes).
fn plain_label(status: &str) -> String {
    if status == "healthy" {
        String::new()
    } else {
        format!(" [{status}]")
    }
}

/// Comfortable width (terminal columns) for one column of panels in
/// `"auto"` sizing — enough for a value, unit, and "updated Xs ago".
const AUTO_COLUMN_WIDTH: u16 = 30;

/// Centers and caps the horizontal extent of `area` per `tui_width`, sized
/// against the widest row's column count in `"auto"` mode. Leaves `area`
/// unchanged when there's nothing to size around (`max_columns == 0`)
/// (spec: tui — configurable TUI content width).
fn content_rect(area: ratatui::layout::Rect, tui_width: &TuiWidth, max_columns: usize) -> ratatui::layout::Rect {
    if max_columns == 0 {
        return area;
    }
    let desired = match tui_width {
        TuiWidth::Fixed(cols) => *cols,
        TuiWidth::Named(_) => (max_columns as u16).saturating_mul(AUTO_COLUMN_WIDTH),
    };
    let width = desired.min(area.width);
    let x = area.x + (area.width - width) / 2;
    ratatui::layout::Rect { x, y: area.y, width, height: area.height }
}

fn draw(f: &mut ratatui::Frame, state: &UiState, tui_width: &TuiWidth) {
    let area = f.area();

    // Grid rows stacked vertically (equal height each), columns within a row
    // proportional to cell spans.
    let rows: Vec<&Vec<Slot>> =
        state.rows.iter().filter(|row| row.iter().map(|c| c.span).sum::<usize>() > 0).collect();
    let max_columns = rows.iter().map(|r| r.iter().map(|c| c.span).sum::<usize>()).max().unwrap_or(0);
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
        let col_areas =
            Layout::horizontal(row.iter().map(|c| Constraint::Fill(c.span as u16)).collect::<Vec<_>>())
                .split(*rect);
        for (cell, crect) in row.iter().zip(col_areas.iter()) {
            if cell.main.is_some() || !cell.secondary.is_empty() || !cell.table.is_empty() {
                f.render_widget(panel_widget(cell), *crect);
            }
        }
    }
}

fn header_line(f: &mut ratatui::Frame, area: ratatui::layout::Rect, y: u16, text: String, style: Style) {
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(text, style))),
        ratatui::layout::Rect { x: area.x, y, width: area.width, height: 1 },
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
    if slot.secondary.is_empty()
        && slot.table.is_empty()
        && let Some(p) = &slot.main
    {
        let style = status_style(p.level.as_deref(), p.status);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(p.ts_epoch, |d| d.as_secs_f64());
        let updated = crate::age::ago(now, p.ts_epoch).map_or_else(String::new, |s| format!(" - {s}"));
        let text = Line::from(vec![
            Span::styled(p.value.clone(), style.add_modifier(Modifier::BOLD)),
            Span::raw(if p.unit.is_empty() { String::new() } else { format!(" {}", p.unit) }),
            Span::styled(updated, Style::default().add_modifier(Modifier::DIM)),
        ]);
        let title = slot.group_title.as_deref().unwrap_or(&p.name);
        Paragraph::new(text).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(style)
                .title(Span::styled(format!(" {title} [{}] ", p.status), style)),
        )
    } else {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64());
        let colors: Vec<&str> = slot
            .main
            .iter()
            .chain(&slot.secondary)
            .chain(&slot.table)
            .map(|p| crate::config::status_color(p.level.as_deref(), p.status))
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
            let style = status_style(p.level.as_deref(), p.status);
            // Only show the age when the value is lagging (stale) — a
            // table line otherwise omits it to stay compact.
            let updated = if p.status == "stale" {
                crate::age::ago(now, p.ts_epoch).map_or_else(String::new, |s| format!(" - {s}"))
            } else {
                String::new()
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{}: ", p.name), Style::default().add_modifier(Modifier::DIM)),
                Span::styled(p.value.clone(), style.add_modifier(Modifier::BOLD)),
                Span::raw(if p.unit.is_empty() { String::new() } else { format!(" {}", p.unit) }),
                Span::styled(plain_label(p.status), Style::default().add_modifier(Modifier::DIM)),
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
    let style = status_style(p.level.as_deref(), p.status);
    let updated = crate::age::ago(now, p.ts_epoch).map_or_else(String::new, |s| format!(" - {s}"));
    Line::from(vec![
        Span::styled(p.value.clone(), style.add_modifier(value_modifier)),
        Span::raw(if p.unit.is_empty() { String::new() } else { format!(" {}", p.unit) }),
        Span::styled(plain_label(p.status), Style::default().add_modifier(Modifier::DIM)),
        Span::styled(updated, Style::default().add_modifier(Modifier::DIM)),
    ])
}

/// Style for a normalized `"red"`/`"yellow"`/`"green"` color, shared by a
/// group panel's own border and (via [`status_style`]) each member line.
fn color_style(color: &str) -> Style {
    match color {
        "red" => Style::default().fg(Color::Red),
        "yellow" => Style::default().fg(Color::Yellow),
        _ => Style::default().fg(Color::Green),
    }
}

fn init() -> Result<Terminal<CrosstermBackend<std::io::Stdout>>> {
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn restore(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<()> {
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )?;
    crossterm::terminal::disable_raw_mode()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

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
                            status: "healthy",
                            level: None,
                            ts_epoch: 0.0,
                        }),
                        secondary: Vec::new(),
                        table: Vec::new(),
                    },
                    Slot {
                        span: 1,
                        group_title: None,
                        main: Some(Panel {
                            name: "Balance".into(),
                            value: "7.25".into(),
                            unit: "USD".into(),
                            status: "healthy",
                            level: None,
                            ts_epoch: 0.0,
                        }),
                        secondary: Vec::new(),
                        table: Vec::new(),
                    },
                ],
                vec![
                    Slot { span: 2, group_title: None, main: None, secondary: Vec::new(), table: Vec::new() },
                    Slot {
                        span: 1,
                        group_title: None,
                        main: Some(Panel {
                            name: "dead-service".into(),
                            value: "\u{2014}".into(),
                            unit: String::new(),
                            status: "failing",
                            level: None,
                            ts_epoch: 0.0,
                        }),
                        secondary: Vec::new(),
                        table: Vec::new(),
                    },
                ],
            ],
            tick: 0,
        };
        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &state, &TuiWidth::Named("auto".into()))).unwrap();
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
                        status: "stale",
                        level: Some("red".into()),
                        ts_epoch: now - 629.0,
                    },
                    Panel {
                        name: "balance".into(),
                        value: "90".into(),
                        unit: "USD".into(),
                        status: "healthy",
                        level: Some("green".into()),
                        ts_epoch: now,
                    },
                ],
            }]],
            tick: 0,
        };
        // Fixed, not "auto": the line now carries a plain "[stale]" label
        // alongside its age suffix, needing more than auto-sizing's
        // one-column default width to render without truncating.
        let backend = ratatui::backend::TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &state, &TuiWidth::Fixed(38))).unwrap();
        let buf = terminal.backend().buffer().clone();
        let text: String = (0..buf.area().height)
            .map(|y| (0..buf.area().width).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect();
        assert!(text.contains("ihor"), "group title missing");
        assert!(text.contains("days left"), "first member label missing");
        assert!(text.contains('5'), "first member value missing");
        assert!(text.contains("balance"), "second member label missing");
        assert!(text.contains("90"), "second member value missing");
        assert!(text.contains("10m ago"), "stale first member's update age missing");

        // A member that isn't lagging shows no age at all.
        let rows: Vec<String> = (0..buf.area().height)
            .map(|y| (0..buf.area().width).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect();
        let balance_row = rows.iter().find(|r| r.contains("balance")).expect("balance line missing");
        assert!(!balance_row.contains("ago"), "fresh member should show no age: {balance_row:?}");

        // The two value cells carry their own colors, not one shared color
        // for the whole panel — and "days left"'s stale health overrides its
        // red band entirely (health takes priority over a threshold
        // reading, since a stale fetch means that reading is no longer
        // trustworthy) (spec: tui — threshold band coloring).
        let find_fg = |needle: &str| -> Option<Color> {
            for y in 0..buf.area().height {
                for x in 0..buf.area().width {
                    if buf[(x, y)].symbol() == &needle[..1] {
                        let row: String =
                            (x..buf.area().width).map(|xi| buf[(xi, y)].symbol()).collect();
                        if row.starts_with(needle) {
                            return Some(buf[(x, y)].fg);
                        }
                    }
                }
            }
            None
        };
        assert_eq!(find_fg("5"), Some(Color::Yellow), "stale health should override the red band");
        assert_eq!(find_fg("90"), Some(Color::Green));

        // The panel's own border is yellow — the worst of its two members
        // (days-left's health-overridden yellow beats balance's green).
        let border_fg = (0..buf.area().height)
            .flat_map(|y| (0..buf.area().width).map(move |x| (x, y)))
            .find(|&(x, y)| buf[(x, y)].symbol() == "─")
            .map(|(x, y)| buf[(x, y)].fg);
        assert_eq!(border_fg, Some(Color::Yellow), "panel border should reflect the worst member");
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
                        status: "healthy",
                        level: Some("green".into()),
                        ts_epoch: 0.0,
                    },
                    Panel {
                        name: "flaky".into(),
                        value: "9".into(),
                        unit: String::new(),
                        status: "failing",
                        level: None,
                        ts_epoch: 0.0,
                    },
                ],
            }]],
            tick: 0,
        };
        let backend = ratatui::backend::TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &state, &TuiWidth::Named("auto".into()))).unwrap();
        let buf = terminal.backend().buffer().clone();
        let rows: Vec<String> = (0..buf.area().height)
            .map(|y| (0..buf.area().width).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect();
        let (y, flaky_row) =
            rows.iter().enumerate().find(|(_, r)| r.contains("flaky")).expect("flaky line missing");
        assert!(flaky_row.contains("[failing]"), "plain failing label expected: {flaky_row:?}");

        // The value itself now renders red — health-derived, alongside the
        // label. Use a char (column) index, not a byte index: the border's
        // box-drawing characters are multi-byte, so `str::find` would
        // misalign with the buffer's column coordinates.
        let value_x = flaky_row.chars().position(|c| c == '9').expect("flaky value missing");
        assert_eq!(buf[(value_x as u16, y as u16)].fg, Color::Red, "unbanded failing value should render red");

        // The panel's own border still reflects the failing unbanded member.
        let border_fg = (0..buf.area().height)
            .flat_map(|y| (0..buf.area().width).map(move |x| (x, y)))
            .find(|&(x, y)| buf[(x, y)].symbol() == "─")
            .map(|(x, y)| buf[(x, y)].fg);
        assert_eq!(border_fg, Some(Color::Red), "border should still flag the failing unbanded member");
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
                    status: "healthy",
                    level: Some("yellow".into()),
                    ts_epoch: 0.0,
                }),
                secondary: Vec::new(),
                table: Vec::new(),
            }]],
            tick: 0,
        };
        let backend = ratatui::backend::TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &state, &TuiWidth::Named("auto".into()))).unwrap();
        let buf = terminal.backend().buffer().clone();
        let text: String = (0..buf.area().height)
            .map(|y| (0..buf.area().width).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect();
        assert!(text.contains("CPU"), "cell title should be used, not main's own label");
        assert!(!text.contains("cpu-load-ignored"), "main's own label should not appear");
        assert!(text.contains("42"), "main value missing");
        let border_fg = (0..buf.area().height)
            .flat_map(|y| (0..buf.area().width).map(move |x| (x, y)))
            .find(|&(x, y)| buf[(x, y)].symbol() == "─")
            .map(|(x, y)| buf[(x, y)].fg);
        assert_eq!(border_fg, Some(Color::Yellow), "border should reflect main's own band color");
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
                    status: "healthy",
                    level: Some("green".into()),
                    ts_epoch: 0.0,
                }),
                secondary: vec![Panel {
                    name: "mem-used".into(),
                    value: "80".into(),
                    unit: "%".into(),
                    status: "failing",
                    level: None,
                    ts_epoch: 0.0,
                }],
                table: vec![Panel {
                    name: "days left".into(),
                    value: "5".into(),
                    unit: "d".into(),
                    status: "healthy",
                    level: Some("green".into()),
                    ts_epoch: 0.0,
                }],
            }]],
            tick: 0,
        };
        let backend = ratatui::backend::TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &state, &TuiWidth::Named("auto".into()))).unwrap();
        let buf = terminal.backend().buffer().clone();
        let text: String = (0..buf.area().height)
            .map(|y| (0..buf.area().width).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect();
        assert!(text.contains("Server"), "cell title missing");
        assert!(text.contains("42"), "main value missing");
        assert!(text.contains("80"), "secondary value missing");
        assert!(text.contains("mem-used") || text.contains('%'), "secondary content missing");
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

    /// A banded value colors by its band when healthy (spec: tui —
    /// threshold band coloring).
    #[test]
    fn banded_value_colors_by_band_when_healthy() {
        assert_eq!(status_style(Some("red"), "healthy").fg, Some(Color::Red));
        assert_eq!(status_style(Some("yellow"), "healthy").fg, Some(Color::Yellow));
    }

    /// Health status overrides a stale band reading — even a source with
    /// bands shows red/yellow while failing/stale, not its last band color
    /// (spec: tui — threshold band coloring).
    #[test]
    fn health_overrides_a_stale_band_reading() {
        assert_eq!(status_style(Some("green"), "failing").fg, Some(Color::Red));
        assert_eq!(status_style(Some("green"), "stale").fg, Some(Color::Yellow));
    }

    /// An unbanded, healthy source has no accent color at all — not even
    /// green — while failing/stale still colors it (spec: tui — threshold
    /// band coloring).
    #[test]
    fn unbanded_healthy_source_has_no_accent_color() {
        assert_eq!(status_style(None, "healthy").fg, None);
        assert_eq!(status_style(None, "failing").fg, Some(Color::Red));
        assert_eq!(status_style(None, "stale").fg, Some(Color::Yellow));
    }

    /// The plain status label appears for any currently unhealthy value,
    /// banded or not (spec: tui — threshold band coloring; group panes).
    #[test]
    fn plain_label_only_for_non_healthy() {
        assert_eq!(plain_label("healthy"), "");
        assert_eq!(plain_label("failing"), " [failing]");
        assert_eq!(plain_label("stale"), " [stale]");
    }

    /// (spec: tui — configurable TUI content width)
    #[test]
    fn auto_width_stays_narrow_for_few_columns() {
        let area = ratatui::layout::Rect { x: 0, y: 0, width: 220, height: 40 };
        let rect = content_rect(area, &TuiWidth::Named("auto".into()), 2);
        assert!(rect.width < area.width, "2 columns should not fill a 220-wide terminal");
        assert_eq!(rect.width, 60);
    }

    #[test]
    fn auto_width_grows_with_more_columns_but_stays_capped() {
        let area = ratatui::layout::Rect { x: 0, y: 0, width: 220, height: 40 };
        let narrow = content_rect(area, &TuiWidth::Named("auto".into()), 2);
        let wide = content_rect(area, &TuiWidth::Named("auto".into()), 6);
        assert!(wide.width > narrow.width, "more columns should use more space");
        assert!(wide.width <= area.width, "auto width must never exceed the terminal");
    }

    #[test]
    fn fixed_width_caps_at_terminal_width() {
        let area = ratatui::layout::Rect { x: 0, y: 0, width: 220, height: 40 };
        let rect = content_rect(area, &TuiWidth::Fixed(300), 2);
        assert_eq!(rect.width, 220, "fixed width larger than the terminal should be capped");
    }

    #[test]
    fn empty_rows_keep_full_width() {
        let area = ratatui::layout::Rect { x: 0, y: 0, width: 220, height: 40 };
        let rect = content_rect(area, &TuiWidth::Named("auto".into()), 0);
        assert_eq!(rect, area, "nothing to size around should leave the area unchanged");
    }
}
