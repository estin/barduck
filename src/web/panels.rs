//! Panel data model and the live panel-grid shard (spec: web-ui — current
//! values without manual reload; group panes; panel retrospective history
//! bar; source summary strip).
use std::collections::HashMap;
use super::markdown::formatted_content;
use crate::{
    AppState,
    components::card::{card, card_content, card_footer},
    config,
    config::{Level, ValueFormat},
    health::{self, Health},
};
use topcoat::{
    Result,
    context::{Cx, app_context},
    runtime::shard,
    view::{attributes, view},
};

pub(super) struct Panel {
    /// Source id (used for log links); `name` is the display title.
    source: String,
    name: String,
    value: String,
    unit: String,
    status: Health,
    level: Option<Level>,
    format: ValueFormat,
    ts_epoch: f64,
    /// Recent-readings history bar, oldest to newest; one entry per segment,
    /// `None` for a neutral/padding segment. Empty when the source has no
    /// threshold bands (spec: web-ui — panel retrospective history bar).
    history: Vec<Option<Level>>,
}

impl Panel {
    /// Normalized color for this panel's current state — threshold band
    /// level when configured, else health status — shared by the panel's
    /// own styling and the source summary strip's chips (spec: web-ui —
    /// health visible at a glance + threshold coloring; source summary strip).
    fn level_color(&self) -> Level {
        config::status_color(self.level, self.status)
    }

    /// Inline CSS for a single-source panel's card: border, background tint,
    /// and text color all carry the status. Deliberately `style`, not an
    /// appended Tailwind class: `card`'s own classes already set
    /// `border-border`/`bg-background`, and topcoat's components just
    /// concatenate an appended `class` string rather than replacing
    /// conflicting utilities — the two `border-*`/`bg-*` classes would then
    /// compete on Tailwind's generated stylesheet order, which isn't a
    /// reliable way to guarantee the status color wins. An inline `style`
    /// always wins over any class, regardless of generation order.
    ///
    /// `None` when healthy and unbanded: nothing meaningful to accent, so
    /// the card renders with no color at all rather than a default green.
    fn accent_color(&self) -> Option<Level> {
        config::accent_color(self.level, self.status)
    }

    fn status_style(&self) -> &'static str {
        full_style_for_color(self.accent_color())
    }

    /// Inline CSS for one row inside a group pane: only the text color
    /// carries the status — no border or background on the row itself, since
    /// the group card's own border already carries the worst member's color
    /// (spec: web-ui — group panes show multiple labeled, independently
    /// colored values).
    fn group_row_style(&self) -> &'static str {
        text_style_for_color(self.accent_color())
    }

    /// Plain, uncolored status label for a currently unhealthy panel/row —
    /// `None` when healthy, shown alongside whatever color that status
    /// contributes (spec: web-ui — health visible at a glance; group panes).
    fn plain_label(&self) -> Option<&'static str> {
        (self.status != Health::Healthy).then_some(self.status.as_str())
    }

    /// Inline CSS for a summary-strip chip in this panel's status color (same
    /// rationale as [`Panel::status_style`]).
    fn chip_style(&self) -> &'static str {
        match self.level_color() {
            Level::Red => {
                "background-color:var(--status-red-border);color:var(--status-red-chip-fg)"
            }
            Level::Yellow => {
                "background-color:var(--status-yellow-border);color:var(--status-yellow-chip-fg)"
            }
            Level::Green => {
                "background-color:var(--status-green-border);color:var(--status-green-chip-fg)"
            }
        }
    }

    /// Inline CSS for one history-bar segment, from the same `--status-*`
    /// tokens the panel border/chip use, so segments pick up the same
    /// dark-theme desaturation instead of being pinned to fixed Tailwind
    /// color classes regardless of theme (spec: web-ui — consistent
    /// token-based visual theme; dark theme uses moderated contrast and
    /// desaturated status colors).
    fn segment_style(level: Option<Level>) -> &'static str {
        match level {
            Some(Level::Red) => "background-color:var(--status-red-border)",
            Some(Level::Yellow) => "background-color:var(--status-yellow-border)",
            Some(Level::Green) => "background-color:var(--status-green-border)",
            None => "background-color:var(--border)",
        }
    }

    fn value_and_unit(&self) -> String {
        if self.unit.is_empty() {
            self.value.clone()
        } else {
            format!("{} {}", self.value, self.unit)
        }
    }

    /// Text shown for a `secondary` member: its value and unit, or the plain
    /// word `"FAILING"` when the fetch is currently failing — a currently
    /// failing fetch means any displayed value can't be trusted, and unlike
    /// `main`/`table`, `secondary` carries no separate status label to show
    /// alongside it (spec: web-ui — group panes combining main/secondary/
    /// table sections).
    fn secondary_text(&self) -> String {
        if self.status == Health::Failing {
            "FAILING".to_string()
        } else {
            self.value_and_unit()
        }
    }

    /// Human age of the latest reading, e.g. "12s ago" (spec: last update time).
    fn updated_ago(&self) -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(self.ts_epoch, |d| d.as_secs_f64());
        crate::age::ago(now, self.ts_epoch).unwrap_or_else(|| "never".into())
    }
}

/// A standalone static-text panel's content: no source, no health, no
/// history — just config-authored text rendered per `format` (spec: web-ui
/// — static-text panel rendering).
struct TextPanel {
    format: ValueFormat,
    text: String,
}

/// One grid slot: a spacer (`main`/`secondary`/`table`/`text` all empty), a
/// plain single-source panel (`main` only, `group_title` `None`), a
/// static-text panel (`text` only), or a generalized pane combining up to
/// three sections — `main` (regular-panel treatment), `secondary` (compact,
/// always-shown age, never a history bar), and `table` (today's label/value
/// rows, age only when stale) (spec: web-ui — group panes show multiple
/// labeled, independently colored values).
struct Slot {
    span: usize,
    /// 1-based column where this slot starts (grid rows are placed
    /// explicitly so a short row can't be auto-packed into the next).
    col_start: usize,
    /// Card header text for a grouped or static-text slot (the cell's
    /// `title`); `None` for a plain single-source slot, whose header comes
    /// from `main`'s own `name` instead, or a spacer.
    group_title: Option<String>,
    main: Option<Panel>,
    secondary: Vec<Panel>,
    table: Vec<Panel>,
    text: Option<TextPanel>,
    style: Option<HashMap<String, String>>,
}

impl Slot {
    /// DOM id anchor for this slot's card: `main`'s source when present,
    /// else the first `secondary` or `table` member's — shared by every
    /// member's chip so they all scroll to and highlight the same card.
    fn anchor(&self) -> Option<&str> {
        self.main
            .iter()
            .chain(&self.secondary)
            .chain(&self.table)
            .next()
            .map(|p| p.source.as_str())
    }

    /// A generalized pane card's own border — no background — is the worst
    /// color across every member in `main`, `secondary`, and `table`
    /// combined (spec: web-ui — group panes card border reflects the worst
    /// member across all sections).
    fn group_status_style(&self) -> &'static str {
        border_style_for_color(config::worst_color(
            self.main
                .iter()
                .chain(&self.secondary)
                .chain(&self.table)
                .map(Panel::level_color),
        ))
    }
}

/// Inline CSS for a single-source panel's card: border, tinted background,
/// and text color together. `None` renders as no override at all — the
/// card falls back to its default neutral classes instead of a forced green.
///
/// References the `--status-*` custom properties from `assets/styles.css`
/// (light values in `:root`, dark overrides in `.dark`/the OS-preference
/// media query) rather than literal hex, so these colors adapt to the
/// viewer's theme automatically — `var()` inside an inline `style` still
/// resolves against the live cascade, so this keeps the inline-style
/// approach (needed to reliably beat `card`'s own classes) while staying
/// theme-aware (spec: web-ui — light/dark theme toggle).
fn full_style_for_color(color: Option<Level>) -> &'static str {
    match color {
        Some(Level::Red) => {
            "border-color:var(--status-red-border);background-color:var(--status-red-bg);color:var(--status-red-fg)"
        }
        Some(Level::Yellow) => {
            "border-color:var(--status-yellow-border);background-color:var(--status-yellow-bg);color:var(--status-yellow-fg)"
        }
        Some(Level::Green) => {
            "border-color:var(--status-green-border);background-color:var(--status-green-bg);color:var(--status-green-fg)"
        }
        None => "",
    }
}

/// Inline CSS carrying only a border color — used by a group card's own
/// border, which reflects its worst member without tinting the card's
/// background.
fn border_style_for_color(color: Level) -> &'static str {
    match color {
        Level::Red => "border-color:var(--status-red-border)",
        Level::Yellow => "border-color:var(--status-yellow-border)",
        Level::Green => "border-color:var(--status-green-border)",
    }
}

/// Inline CSS carrying only a text color — used by a row inside a group
/// pane, which has no border or background of its own. `None` renders as no
/// override — the row's value inherits the default text color instead of a
/// forced green.
pub(super) fn text_style_for_color(color: Option<Level>) -> &'static str {
    match color {
        Some(Level::Red) => "color:var(--status-red-text)",
        Some(Level::Yellow) => "color:var(--status-yellow-text)",
        Some(Level::Green) => "color:var(--status-green-text)",
        None => "",
    }
}


/// Converts a config-authored style-override key to a CSS property name:
/// config keys follow this project's `snake_case` convention (matching
/// `history_points`, `show_history`, ...), but CSS properties are
/// hyphenated (`font-family`) — an unrecognized property name is silently
/// ignored by the browser, so without this conversion an override key like
/// `font_family` would parse fine but never actually take visual effect.
fn css_property_name(key: &str) -> String {
    key.replace('_', "-")
}

/// Convert an optional style override map to a CSS inline string. Each
/// key-value pair becomes `key: value;`, converting the config's
/// `snake_case` keys to hyphenated CSS property names.
fn style_override_to_css(style: Option<&HashMap<String, String>>) -> String {
    match style {
        Some(map) => map
            .iter()
            .map(|(k, v)| format!("{}: {v};", css_property_name(k)))
            .collect::<Vec<_>>()
            .join(" "),
        None => String::new(),
    }
}

/// Build a CSS inline style string from base styles and optional overrides.
/// Avoids leading/trailing semicolons and double semicolons.
fn style_string(base: &str, extra: &str) -> String {
    let base = base.trim().trim_start_matches(';').trim_end_matches(';').trim();
    if extra.is_empty() {
        base.to_string()
    } else {
        let extra = extra.trim().trim_start_matches(';').trim_end_matches(';').trim();
        format!("{base}; {extra}")
    }
}

struct Grid {
    title: String,
    style: Option<HashMap<String, String>>,
    columns: usize,
    rows: Vec<Vec<Slot>>,
}

/// Builds one panel's worth of value/status/threshold data for `name`. The
/// label is `title_override` if given (an explicit cell/group-entry title),
/// else the source's own declared `title`, else `name` itself.
///
/// Async — like every other data-path method here, it goes through `Db`'s
/// async API (`spawn_blocking`-backed), never the blocking sync path, so a
/// slow lock/disk wait never parks the Tokio worker thread rendering this
/// request.
async fn build_panel(
    st: &AppState,
    latest: &[crate::db::ReadingRow],
    name: &str,
    title_override: Option<&str>,
) -> Panel {
    let src = st.cfg.sources.iter().find(|s| s.name() == name);
    let label = title_override
        .or_else(|| src.and_then(config::SourceCfg::display_title))
        .unwrap_or(name);
    let row = latest.iter().find(|r| r.source == name);
    let health = health::compute(&st.db, &st.cfg, name).await;
    let status = health.as_ref().map_or(Health::Stale, |h| h.status);
    // Effective bands (declared, or the latest `jsonl` override) ride on
    // the health payload so overrides color every surface without extra
    // plumbing (spec: source-configuration — JSONL row schema).
    let bands: &[config::Threshold] = health.as_ref().map_or(&[], |h| &h.thresholds);
    let level = match row {
        Some(r) => (!bands.is_empty())
            .then(|| config::level_for(bands, &r.value))
            .flatten(),
        None => None,
    };
    let show_bar = !bands.is_empty() && src.is_some_and(|s| s.show_history().unwrap_or(true));
    let history = match src.filter(|_| show_bar) {
        Some(s) => {
            let n = s.history_points().unwrap_or(st.cfg.history_points);
            let recent = st
                .db
                .history(name, None, None, Some(i64::from(n)))
                .await
                .unwrap_or_default();
            let mut segments: Vec<Option<Level>> = recent
                .iter()
                .map(|r| config::level_for(bands, &r.value))
                .collect();
            let mut padded = vec![None; (n as usize).saturating_sub(segments.len())];
            padded.append(&mut segments);
            padded
        }
        None => Vec::new(),
    };
    Panel {
        source: name.to_string(),
        name: label.to_string(),
        value: row.map_or_else(|| "—".into(), |r| r.value.clone()),
        unit: row.and_then(|r| r.unit.clone()).unwrap_or_default(),
        status,
        level,
        format: src.and_then(config::SourceCfg::format).unwrap_or_default(),
        ts_epoch: row.map_or(0.0, |r| r.ts_epoch),
        history,
    }
}

/// One cell's built panel data and card metadata: `(main, secondary, table,
/// group_title, text_panel, style)`.
type CellParts = (
    Option<Panel>,
    Vec<Panel>,
    Vec<Panel>,
    Option<String>,
    Option<TextPanel>,
    Option<HashMap<String, String>>,
);

/// Builds one cell's panel data, dispatching on its config variant. A source
/// hidden from this view (spec: source-configuration — per-source view
/// visibility) is simply omitted here, so the cell falls through to the same
/// empty-grid-position rendering as an explicit `space` cell (spec: web-ui —
/// hidden sources render as space in the web dashboard).
async fn build_cell_parts(
    st: &AppState,
    latest: &[crate::db::ReadingRow],
    cell: &config::Cell,
) -> CellParts {
    match cell {
        config::Cell::Group {
            title,
            main,
            secondary,
            table: cell_table,
            style: group_style,
            ..
        } => {
            let main = match main.as_ref().filter(|item| {
                config::source_visible_in(&st.cfg, item.id(), config::View::Web)
            }) {
                Some(item) => Some(build_panel(st, latest, item.id(), item.explicit_label()).await),
                None => None,
            };
            let mut secondary_panels = Vec::new();
            for item in config::visible_items(&st.cfg, secondary, config::View::Web) {
                secondary_panels.push(build_panel(st, latest, item.id(), item.explicit_label()).await);
            }
            let mut table_out = Vec::new();
            for item in config::visible_items(&st.cfg, cell_table, config::View::Web) {
                table_out.push(build_panel(st, latest, item.id(), item.explicit_label()).await);
            }
            (main, secondary_panels, table_out, title.clone(), None, group_style.clone())
        }
        config::Cell::Source(name) => {
            let main = if config::source_visible_in(&st.cfg, name, config::View::Web) {
                Some(build_panel(st, latest, name, None).await)
            } else {
                None
            };
            (main, Vec::new(), Vec::new(), None, None, None)
        }
        config::Cell::Pane { id, title, style: pane_style } => {
            let main = if config::source_visible_in(&st.cfg, id, config::View::Web) {
                Some(build_panel(st, latest, id, title.as_deref()).await)
            } else {
                None
            };
            (main, Vec::new(), Vec::new(), None, None, pane_style.clone())
        }
        config::Cell::Space { .. } => (None, Vec::new(), Vec::new(), None, None, None),
        config::Cell::Text {
            title,
            format,
            text,
            style: text_style,
            ..
        } => (
            None,
            Vec::new(),
            Vec::new(),
            title.clone(),
            Some(TextPanel {
                format: format
                    .as_deref()
                    .and_then(|f| ValueFormat::parse(f).ok())
                    .unwrap_or_default(),
                text: text.clone(),
            }),
            text_style.clone(),
        ),
    }
}

async fn collect_grids(st: &AppState) -> Vec<Grid> {
    let Ok(latest) = st.db.latest_values().await else {
        return Vec::new();
    };
    let mut grids = Vec::new();
    for layout in &st.cfg.layouts {
        let mut rows = Vec::new();
        for cells in &layout.rows {
            let mut slots = Vec::new();
            let mut col = 1;
            for cell in cells {
                let (main, secondary, table_panels, group_title, text_panel, cell_style) =
                    build_cell_parts(st, &latest, cell).await;
                let span = cell.span();
                slots.push(Slot {
                    span,
                    col_start: col,
                    group_title,
                    main,
                    secondary,
                    table: table_panels,
                    text: text_panel,
                    style: cell_style,
                });
                col += span;
            }
            rows.push(slots);
        }
        grids.push(Grid {
            title: layout.title.clone(),
            style: layout.style.clone(),
            columns: layout.columns(),
            rows,
        });
    }
    grids
}

/// Live panel grid: re-renders on the server whenever `tick` changes
/// (spec: web-ui — current values without manual reload).
#[shard]
pub(super) async fn panels_grid(cx: &Cx, tick: f64) -> Result {
    let _ = tick; // refresh trigger only; data always re-read from the DB
    let st = app_context::<AppState>(cx);
    let grids = collect_grids(st).await;
    // Same panels, same order, as a flat list for the summary strip
    // (spec: web-ui — source summary strip). A group's members all point at
    // their shared card's anchor, not their own.
    let mut chips: Vec<(&Panel, &str)> = Vec::new();
    for grid in &grids {
        for row in &grid.rows {
            for slot in row {
                if let Some(anchor) = slot.anchor() {
                    for p in slot.main.iter().chain(&slot.secondary).chain(&slot.table) {
                        chips.push((p, anchor));
                    }
                }
            }
        }
    }
    // Worst color across every source currently on the dashboard (spec:
    // web-ui — health visible at a glance); read by the browser-tab favicon
    // script via this hidden marker's `data-status`, refreshed each tick.
    let worst = config::worst_color(chips.iter().map(|(p, _)| p.level_color()));
    view! {
        <span id="bd-status" data-status=(worst.as_str()) style="display:none"></span>
        if !chips.is_empty() {
            <div class="flex flex-wrap gap-1.5 mb-4">
                for (p, anchor) in &chips {
                    <a href=(format!("#panel-{}", anchor))
                        class="inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium leading-none hover:opacity-90"
                        style=(p.chip_style())
                    >
                        (p.name.clone())
                    </a>
                }
            </div>
        }
        for grid in &grids {
            <h2 class="text-lg font-semibold mb-3 text-foreground">(grid.title.clone())</h2>
            <div class=(format!("grid bd-panel-grid gap-3 mb-6 grid-cols-{}", grid.columns.min(6)))
                style=(style_string(&format!("--bd-cols: {}; grid-template-columns: repeat({}, minmax(0, 1fr));", grid.columns, grid.columns), &style_override_to_css(grid.style.as_ref())))
            >
                for (ri, row) in grid.rows.iter().enumerate() {
                    for slot in row {
                        if let Some(text) = &slot.text {
                            card(
                                attrs: attributes! {
                                    id=(format!("text-panel-{}-{}", ri, slot.col_start))
                                    class="relative bd-panel-cell"
                                    style=(style_string(&format!("grid-row: {}; grid-column: {} / span {}", ri + 1, slot.col_start, slot.span), &style_override_to_css(slot.style.as_ref())))
                                },
                                // No health/threshold color and no footer/log-link/history-bar —
                                // there's no source behind a static-text panel (spec: web-ui —
                                // static-text panel rendering).
                                if let Some(title) = &slot.group_title {
                                    <span class="absolute top-0 -translate-y-1/2 left-4 px-1.5 text-xs font-medium leading-none bg-background">
                                        (title.clone())
                                    </span>
                                }
                                card_content(
                                    formatted_content(format: text.format, value: text.text.clone(), unit: String::new())
                                )
                            )
                        } else if slot.secondary.is_empty() && slot.table.is_empty() && slot.main.is_some() {
                            if let Some(main) = &slot.main {
                                card(
                                attrs: attributes! {
                                    id=(format!("panel-{}", main.source))
                                    class="relative bd-panel-cell"
                                    style=(style_string(&format!("{}; grid-row: {}; grid-column: {} / span {}", main.status_style(), ri + 1, slot.col_start, slot.span), &style_override_to_css(slot.style.as_ref())))
                                },
                                    // Title on the card's own top border, top-left, padded — the
                                    // TUI's bordered-panel title convention (spec: web-ui — panel
                                    // title rendered on the card border). `top-0 -translate-y-1/2`
                                    // centers the span on the border line itself regardless of
                                    // font metrics, rather than a fixed `-top-*` offset that would
                                    // only line up for one particular line-height. Reuses the
                                    // card's own status style so the label's cutout background
                                    // matches whatever the card is currently tinted (or neutral).
                                    <span class="absolute top-0 -translate-y-1/2 left-4 px-1.5 text-xs font-medium leading-none bg-background" style=(main.status_style())>
                                        (slot.group_title.clone().unwrap_or_else(|| main.name.clone()))
                                    </span>
                                    card_content(
                                        formatted_content(format: main.format, value: main.value.clone(), unit: main.unit.clone())
                                        if !main.history.is_empty() {
                                            <div class="mt-1.5 flex h-1">
                                                for seg in &main.history {
                                                    <div class="flex-1" style=(Panel::segment_style(*seg))></div>
                                                }
                                            </div>
                                        }
                                    )
                                    card_footer(
                                        // `mt-auto` pins the footer to the card's bottom edge when the
                                        // card is stretched taller than its content by the grid row
                                        // (the row height matches the tallest sibling panel), instead
                                        // of the footer floating directly under the content.
                                        attrs: attributes! { class="mt-auto flex justify-between text-xs uppercase tracking-wide" },
                                        <span>(main.status.as_str())</span>
                                        <a
                                            href=(format!("/logs/{}", main.source))
                                            class="normal-case opacity-60 hover:opacity-100 hover:underline"
                                        >
                                            (format!("updated {}", main.updated_ago()))
                                        </a>
                                    )
                                )
                            }
                        } else if slot.main.is_some() || !slot.secondary.is_empty() || !slot.table.is_empty() {
                            card(
                                attrs: attributes! {
                                    id=(format!("panel-{}", slot.anchor().unwrap_or_default()))
                                    class="relative bd-panel-cell"
                                    style=(style_string(&format!("{}; grid-row: {}; grid-column: {} / span {}", slot.group_status_style(), ri + 1, slot.col_start, slot.span), &style_override_to_css(slot.style.as_ref())))
                                },
                                // A combined card's own background is always neutral (spec:
                                // web-ui — group panes card border reflects the worst member
                                // across all sections), so the title label needs no per-status
                                // override, just the same neutral background as the card.
                                if let Some(title) = &slot.group_title {
                                    <span class="absolute top-0 -translate-y-1/2 left-4 px-1.5 text-xs font-medium leading-none bg-background">
                                        (title.clone())
                                    </span>
                                }
                                card_content(
                                    if let Some(main) = &slot.main {
                                        <div class="mb-1.5">
                                            <div class="text-xl font-semibold" style=(main.group_row_style())>(main.value_and_unit())</div>
                                            <div class="flex items-center gap-1.5 text-xs mt-0.5">
                                                if let Some(label) = main.plain_label() {
                                                    <span class="opacity-60">(label)</span>
                                                }
                                                <a
                                                    href=(format!("/logs/{}", main.source))
                                                    class="opacity-60 hover:opacity-100 hover:underline"
                                                >
                                                    (format!("updated {}", main.updated_ago()))
                                                </a>
                                            </div>
                                            if !main.history.is_empty() {
                                                <div class="mt-1.5 flex h-1">
                                                    for seg in &main.history {
                                                        <div class="flex-1" style=(Panel::segment_style(*seg))></div>
                                                    }
                                                </div>
                                            }
                                        </div>
                                    }
                                    if !slot.secondary.is_empty() {
                                        <div class="flex flex-wrap items-center gap-2.5 mb-1.5">
                                            for p in &slot.secondary {
                                                <a
                                                    href=(format!("/logs/{}", p.source))
                                                    class="text-sm font-semibold hover:underline"
                                                    style=(p.group_row_style())
                                                >
                                                    (p.secondary_text())
                                                </a>
                                            }
                                        </div>
                                    }
                                    for p in &slot.table {
                                        <div class="px-1.5 py-0.5 mb-0.5">
                                            <div class="flex justify-between items-center gap-2">
                                                <a
                                                    href=(format!("/logs/{}", p.source))
                                                    class="text-xs tracking-wide opacity-70 hover:opacity-100 hover:underline"
                                                >
                                                    (p.name.clone())
                                                </a>
                                                <span class="flex items-center gap-1.5">
                                                    if let Some(label) = p.plain_label() {
                                                        <span class="text-xs normal-case opacity-60">(label)</span>
                                                    }
                                                    <span class="text-sm font-semibold" style=(p.group_row_style())>(p.value_and_unit())</span>
                                                </span>
                                            </div>
                                            if !p.history.is_empty() {
                                                <div class="mt-1 flex h-1">
                                                    for seg in &p.history {
                                                        <div class="flex-1" style=(Panel::segment_style(*seg))></div>
                                                    }
                                                </div>
                                            }
                                            if p.status == Health::Stale {
                                                <div class="mt-1 text-[10px] normal-case opacity-60">
                                                    (format!("updated {}", p.updated_ago()))
                                                </div>
                                            }
                                        </div>
                                    }
                                )
                            )
                        } else {
                            <div class="bd-panel-cell" style=(style_string(&format!("grid-row: {}; grid-column: {} / span {}", ri + 1, slot.col_start, slot.span), &style_override_to_css(slot.style.as_ref())))></div>
                        }
                    }
                }
            </div>
        }
        if grids.is_empty() {
            <div class="rounded-xl border-2 border-dashed border-border p-8 text-center text-muted-foreground">
                "No panels configured. Add sources and layouts to the config file."
            </div>
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_string_no_override() {
        let result = style_string("grid-row: 1; grid-column: 2", "");
        assert_eq!(result, "grid-row: 1; grid-column: 2");
    }

    #[test]
    fn style_string_with_override() {
        let result = style_string("grid-row: 1", "font-family: monospace");
        assert_eq!(result, "grid-row: 1; font-family: monospace");
    }

    #[test]
    fn style_string_leading_semicolon_stripped() {
        let result = style_string("; grid-row: 1; grid-column: 3", "font-family: monospace");
        assert_eq!(result, "grid-row: 1; grid-column: 3; font-family: monospace");
    }

    #[test]
    fn style_string_no_double_semicolons() {
        let result = style_string("grid-row: 1; ", "font-family: monospace;");
        assert!(!result.contains(";;"));
    }

    #[test]
    fn style_string_trailing_semicolon_on_extra_stripped() {
        // A trailing `;` on the override half must not survive into the
        // combined string, so it stays consistent regardless of whether the
        // config author's last declaration happened to end with one.
        let result = style_string("grid-row: 1", "font-family: monospace;");
        assert_eq!(result, "grid-row: 1; font-family: monospace");
    }

    /// A `snake_case` config key (this project's convention for every other
    /// config field) must become a hyphenated CSS property, or the browser
    /// silently ignores the whole declaration and the override has no
    /// visible effect at all.
    #[test]
    fn css_property_name_converts_underscores_to_hyphens() {
        assert_eq!(css_property_name("font_family"), "font-family");
        assert_eq!(css_property_name("grid-column"), "grid-column");
    }

    #[test]
    fn style_override_to_css_converts_keys_and_terminates_each_declaration() {
        let mut map = HashMap::new();
        map.insert("font_family".to_string(), "monospace".to_string());
        let css = style_override_to_css(Some(&map));
        assert_eq!(css, "font-family: monospace;");
    }

    #[test]
    fn style_override_to_css_empty_for_none_or_empty_map() {
        assert_eq!(style_override_to_css(None), "");
        assert_eq!(style_override_to_css(Some(&HashMap::new())), "");
    }

    /// End-to-end: a config-authored override lands in the final `style`
    /// attribute as valid, hyphenated CSS with no missing/doubled
    /// semicolons — the bug this covers rendered `font_family: monospace`
    /// (an unrecognized property the browser drops) instead of
    /// `font-family: monospace`.
    #[test]
    fn slot_style_combines_base_and_override_as_valid_css() {
        let mut map = HashMap::new();
        map.insert("font_family".to_string(), "monospace".to_string());
        let result = style_string(
            "grid-row: 1; grid-column: 3 / span 1",
            &style_override_to_css(Some(&map)),
        );
        assert_eq!(
            result,
            "grid-row: 1; grid-column: 3 / span 1; font-family: monospace"
        );
    }
}
