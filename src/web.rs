#![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use crate::{
    AppState, config,
    config::ValueFormat,
    components::{
        badge::{BadgeVariant, badge},
        button::{ButtonSize, ButtonVariant, button},
        card::{card, card_content, card_footer},
        table::{table, table_body, table_cell, table_head, table_header, table_row},
    },
    health,
};
use topcoat::{
    Result,
    context::{Cx, app_context},
    cookie::{Cookies, cookies},
    font::{Font, fontsource::fontsource_font},
    router::{page, path_param},
    runtime::shard,
    tailwind,
    view::{Unescaped, attributes, component, view},
};

/// Declared once and reused by every page: two independent
/// `fontsource_font!(GEIST)` call sites would each register their own font
/// route, colliding on `.discover()` (`duplicate route registered for GET
/// /_topcoat/fonts/...`).
const GEIST: Font = fontsource_font!(GEIST);

/// Name of the cookie persisting the browser's explicit light/dark choice
/// (spec: web-ui — light/dark theme toggle). Set by `POST /api/theme`
/// (`src/api.rs`); read here so every server-rendered page applies the same
/// class the toggle last chose, with no client-side bootstrap step needed.
pub(crate) const THEME_COOKIE: &str = "bd_theme";

/// `"dark"`/`"light"` when the browser has made an explicit choice
/// (`bd_theme` cookie), else empty — an empty class lets the CSS
/// `prefers-color-scheme` media query (`assets/styles.css`) decide, so a
/// first-time visitor still gets their OS preference (spec: web-ui —
/// light/dark theme toggle).
fn theme_class(cx: &Cx) -> &'static str {
    match cookies(cx).get(THEME_COOKIE).as_ref().map(topcoat::cookie::Cookie::value) {
        Some("dark") => "dark",
        Some("light") => "light",
        _ => "",
    }
}

/// Frontend-initiated ping/pong: polls `/api/ping` and reflects reachability
/// in the connection indicator, independent of the panel-refresh shard
/// (spec: web-ui — global connection health indicator; design.md — plain
/// browser JS, not the shard mechanism, so failure is never silent).
const CONNECTION_SCRIPT: &str = r"(function () {
    var dot = document.getElementById('bd-conn-dot');
    var label = document.getElementById('bd-conn-label');
    var colors = { checking: '#94a3b8', online: '#10b981', offline: '#ef4444' };
    var labels = { checking: 'checking…', online: 'online', offline: 'offline' };
    function setState(state) {
        dot.style.backgroundColor = colors[state];
        label.textContent = labels[state];
    }
    function ping() {
        fetch('/api/ping', { signal: AbortSignal.timeout(3000) })
            .then(function (r) {
                if (!r.ok) throw new Error('bad status');
                return r.json();
            })
            .then(function () { setState('online'); })
            .catch(function () { setState('offline'); });
    }
    ping();
    setInterval(ping, 5000);
})();";

/// Reflects the dashboard's worst current status (spec: web-ui — health
/// visible at a glance) in the browser tab: a "crooked tile" mark, gray on
/// gray until something needs attention, then red/yellow/green matching the
/// summary strip's own colors. Polls the hidden status marker `panels_grid`
/// renders on the same interval as the connection ping, rather than
/// depending on exactly how the shard patches the DOM.
const FAVICON_SCRIPT: &str = r##"(function () {
    var link = document.getElementById('bd-favicon');
    var colors = { red: '#ef4444', yellow: '#fbbf24', green: '#10b981' };
    function svgFor(hex) {
        return 'data:image/svg+xml,' + encodeURIComponent(
            '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">' +
            '<rect x="3" y="3" width="12" height="12" rx="2.5" fill="#767d78"/>' +
            '<rect x="17" y="3" width="12" height="12" rx="2.5" fill="#767d78"/>' +
            '<rect x="3" y="17" width="12" height="12" rx="2.5" fill="#767d78"/>' +
            '<rect x="16.5" y="16.5" width="13" height="13" rx="2.5" fill="' + hex + '" transform="rotate(24 23 23)"/>' +
            '</svg>'
        );
    }
    function refresh() {
        var el = document.getElementById('bd-status');
        var status = (el && el.dataset.status) || 'green';
        link.href = svgFor(colors[status] || colors.green);
    }
    refresh();
    setInterval(refresh, 5000);
})();"##;

/// Wires the theme toggle button: persists the chosen theme via `POST
/// /api/theme`, then reloads the page so the server renders it with the new
/// `dark`/`light` class from the very first byte (spec: web-ui — light/dark
/// theme toggle). Reloading rather than only flipping the class client-side
/// means the toggle can't drift from what the server would render — nothing
/// on the page needs its own separate "does this react live to a class
/// change" story, including the parts of `panels_grid` a shard re-render
/// might otherwise leave stale until its next tick.
const THEME_TOGGLE_SCRIPT: &str = r"(function () {
    var btn = document.getElementById('bd-theme-toggle');
    if (!btn) return;
    btn.addEventListener('click', function () {
        var next = document.documentElement.classList.contains('dark') ? 'light' : 'dark';
        fetch('/api/theme', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ theme: next })
        }).then(function () {
            location.reload();
        }).catch(function () {});
    });
})();";

struct Panel {
    /// Source id (used for log links); `name` is the display title.
    source: String,
    name: String,
    value: String,
    unit: String,
    status: &'static str,
    level: Option<String>,
    format: ValueFormat,
    ts_epoch: f64,
    /// Recent-readings history bar, oldest to newest; one entry per segment,
    /// `None` for a neutral/padding segment. Empty when the source has no
    /// threshold bands (spec: web-ui — panel retrospective history bar).
    history: Vec<Option<String>>,
}

impl Panel {
    /// Normalized color for this panel's current state — threshold band
    /// level when configured, else health status — shared by the panel's
    /// own styling and the source summary strip's chips (spec: web-ui —
    /// health visible at a glance + threshold coloring; source summary strip).
    fn level_color(&self) -> &'static str {
        config::status_color(self.level.as_deref(), self.status)
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
    fn accent_color(&self) -> Option<&'static str> {
        config::accent_color(self.level.as_deref(), self.status)
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
        (self.status != "healthy").then_some(self.status)
    }

    /// Inline CSS for a summary-strip chip in this panel's status color (same
    /// rationale as [`Panel::status_style`]).
    fn chip_style(&self) -> &'static str {
        match self.level_color() {
            "red" => "background-color:var(--status-red-border);color:var(--status-red-chip-fg)",
            "yellow" => "background-color:var(--status-yellow-border);color:var(--status-yellow-chip-fg)",
            _ => "background-color:var(--status-green-border);color:var(--status-green-chip-fg)",
        }
    }

    /// Tailwind class for one history-bar segment. No component wraps these,
    /// so a plain utility class is unambiguous (no competing base class).
    fn segment_class(level: Option<&str>) -> &'static str {
        match level {
            Some("red") => "bg-red-500",
            Some("yellow") => "bg-amber-400",
            Some("green") => "bg-emerald-500",
            _ => "bg-slate-200",
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
        if self.status == "failing" { "FAILING".to_string() } else { self.value_and_unit() }
    }

    /// Human age of the latest reading, e.g. "12s ago" (spec: last update time).
    fn updated_ago(&self) -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(self.ts_epoch, |d| d.as_secs_f64());
        crate::age::ago(now, self.ts_epoch).unwrap_or_else(|| "never".into())
    }

}

/// Pretty JSON when parseable; raw text otherwise. Shared by any
/// format-aware content — a source's own value and a static-text cell's
/// literal text (spec: web-ui — static-text panel rendering).
fn json_pretty(value: &str) -> String {
    serde_json::from_str::<serde_json::Value>(value)
        .and_then(|v| serde_json::to_string_pretty(&v))
        .unwrap_or_else(|_| value.to_string())
}

/// Markdown rendered to HTML (trusted: config-authored content, either a
/// source's fetched value or a static-text cell's literal text).
fn markdown_to_html(value: &str) -> Unescaped<String> {
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, pulldown_cmark::Parser::new(value));
    Unescaped::new_unchecked(html)
}

/// Renders `value` (with `unit` appended when non-empty, except for
/// markdown, which never gets a unit suffix) according to `format`: markdown
/// as HTML, JSON pretty-printed below the raw value, otherwise plain text.
/// Shared by the single-source panel's content and the static-text cell's
/// content, so the three-way format branch isn't copy-pasted a third time
/// (design.md — factor format-rendering logic).
#[component]
async fn formatted_content(format: ValueFormat, value: String, unit: String) -> Result {
    let value_and_unit = if unit.is_empty() { value.clone() } else { format!("{value} {unit}") };
    view! {
        if format == ValueFormat::Markdown {
            <div class="prose prose-sm max-w-none">(markdown_to_html(&value))</div>
        } else if format == ValueFormat::Json {
            <div class="text-2xl font-semibold">(value_and_unit)</div>
            <pre class="mt-1.5 text-xs font-mono whitespace-pre-wrap break-all max-h-40 overflow-y-auto">(json_pretty(&value))</pre>
        } else {
            <div class="text-2xl font-semibold">(value_and_unit)</div>
        }
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
            self.main.iter().chain(&self.secondary).chain(&self.table).map(Panel::level_color),
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
fn full_style_for_color(color: Option<&str>) -> &'static str {
    match color {
        Some("red") => "border-color:var(--status-red-border);background-color:var(--status-red-bg);color:var(--status-red-fg)",
        Some("yellow") => "border-color:var(--status-yellow-border);background-color:var(--status-yellow-bg);color:var(--status-yellow-fg)",
        Some("green") => "border-color:var(--status-green-border);background-color:var(--status-green-bg);color:var(--status-green-fg)",
        _ => "",
    }
}

/// Inline CSS carrying only a border color — used by a group card's own
/// border, which reflects its worst member without tinting the card's
/// background.
fn border_style_for_color(color: &str) -> &'static str {
    match color {
        "red" => "border-color:var(--status-red-border)",
        "yellow" => "border-color:var(--status-yellow-border)",
        _ => "border-color:var(--status-green-border)",
    }
}

/// Inline CSS carrying only a text color — used by a row inside a group
/// pane, which has no border or background of its own. `None` renders as no
/// override — the row's value inherits the default text color instead of a
/// forced green.
fn text_style_for_color(color: Option<&str>) -> &'static str {
    match color {
        Some("red") => "color:var(--status-red-text)",
        Some("yellow") => "color:var(--status-yellow-text)",
        Some("green") => "color:var(--status-green-text)",
        _ => "",
    }
}

struct Grid {
    title: String,
    columns: usize,
    rows: Vec<Vec<Slot>>,
}

/// Builds one panel's worth of value/status/threshold data for `name`. The
/// label is `title_override` if given (an explicit cell/group-entry title),
/// else the source's own declared `title`, else `name` itself.
fn build_panel(
    st: &AppState,
    latest: &[crate::db::ReadingRow],
    name: &str,
    title_override: Option<&str>,
) -> Panel {
    let src = st.cfg.sources.iter().find(|s| s.name == name);
    let label = title_override
        .or_else(|| src.and_then(config::SourceCfg::display_title))
        .unwrap_or(name);
    let row = latest.iter().find(|r| r.source == name);
    let status = health::compute(&st.db, &st.cfg, name).map_or("stale", |h| h.status.as_str());
    let level = match row {
        Some(r) => src
            .filter(|s| !s.thresholds.is_empty())
            .and_then(|s| config::level_for(&s.thresholds, &r.value)),
        None => None,
    };
    let history = src
        .filter(|s| !s.thresholds.is_empty() && s.show_history.unwrap_or(true))
        .map(|s| {
            let n = s.history_points.unwrap_or(st.cfg.history_points);
            let mut segments: Vec<Option<String>> = st
                .db
                .history_sync(name, i64::from(n))
                .unwrap_or_default()
                .iter()
                .map(|r| config::level_for(&s.thresholds, &r.value))
                .collect();
            let mut padded = vec![None; (n as usize).saturating_sub(segments.len())];
            padded.append(&mut segments);
            padded
        })
        .unwrap_or_default();
    Panel {
        source: name.to_string(),
        name: label.to_string(),
        value: row.map_or_else(|| "—".into(), |r| r.value.clone()),
        unit: row.and_then(|r| r.unit.clone()).unwrap_or_default(),
        status,
        level,
        format: src
            .and_then(|s| s.format.as_deref())
            .and_then(|f| ValueFormat::parse(f).ok())
            .unwrap_or_default(),
        ts_epoch: row.map_or(0.0, |r| r.ts_epoch),
        history,
    }
}

fn collect_grids(st: &AppState) -> Vec<Grid> {
    let Ok(latest) = st.db.latest_values_sync() else {
        return Vec::new();
    };
    let mut grids = Vec::new();
    for layout in &st.cfg.layouts {
        let mut rows = Vec::new();
        for cells in &layout.rows {
            let mut slots = Vec::new();
            let mut col = 1;
            for cell in cells {
                // A source hidden from this view (spec: source-configuration —
                // per-source view visibility) is simply omitted here, so the cell
                // falls through to the same empty-grid-position rendering as an
                // explicit `space` cell (spec: web-ui — hidden sources render as
                // space in the web dashboard).
                let (main, secondary, table_panels, group_title, text_panel) = match cell {
                    config::Cell::Group { title, main, secondary, table: cell_table } => (
                        main.as_ref()
                            .filter(|item| config::source_visible_in(&st.cfg, item.id(), "web"))
                            .map(|item| build_panel(st, &latest, item.id(), item.explicit_label())),
                        config::visible_items(&st.cfg, secondary, "web")
                            .iter()
                            .map(|item| build_panel(st, &latest, item.id(), item.explicit_label()))
                            .collect(),
                        config::visible_items(&st.cfg, cell_table, "web")
                            .iter()
                            .map(|item| build_panel(st, &latest, item.id(), item.explicit_label()))
                            .collect(),
                        title.clone(),
                        None,
                    ),
                    config::Cell::Source(name) => (
                        config::source_visible_in(&st.cfg, name, "web")
                            .then(|| build_panel(st, &latest, name, None)),
                        Vec::new(),
                        Vec::new(),
                        None,
                        None,
                    ),
                    config::Cell::Pane { id, title } => (
                        config::source_visible_in(&st.cfg, id, "web")
                            .then(|| build_panel(st, &latest, id, title.as_deref())),
                        Vec::new(),
                        Vec::new(),
                        None,
                        None,
                    ),
                    config::Cell::Space { .. } => (None, Vec::new(), Vec::new(), None, None),
                    config::Cell::Text { title, format, text } => (
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
                    ),
                };
                let span = cell.span();
                slots.push(Slot {
                    span,
                    col_start: col,
                    group_title,
                    main,
                    secondary,
                    table: table_panels,
                    text: text_panel,
                });
                col += span;
            }
            rows.push(slots);
        }
        grids.push(Grid {
            title: layout.title.clone(),
            columns: layout.columns(),
            rows,
        });
    }
    grids
}

/// Live panel grid: re-renders on the server whenever `tick` changes
/// (spec: web-ui — current values without manual reload).
#[shard]
async fn panels_grid(cx: &Cx, tick: f64) -> Result {
    let _ = tick; // refresh trigger only; data always re-read from the DB
    let st = app_context::<AppState>(cx);
    let grids = collect_grids(st);
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
        <span id="bd-status" data-status=(worst) style="display:none"></span>
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
                style=(format!("--bd-cols: {}; grid-template-columns: repeat({}, minmax(0, 1fr));", grid.columns, grid.columns))
            >
                for (ri, row) in grid.rows.iter().enumerate() {
                    for slot in row {
                        if let Some(text) = &slot.text {
                            card(
                                attrs: attributes! {
                                    id=(format!("text-panel-{}-{}", ri, slot.col_start))
                                    class="relative bd-panel-cell"
                                    style=(format!("grid-row: {}; grid-column: {} / span {};", ri + 1, slot.col_start, slot.span))
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
                                        style=(format!(
                                            "{}; grid-row: {}; grid-column: {} / span {};",
                                            main.status_style(), ri + 1, slot.col_start, slot.span
                                        ))
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
                                            <div class="mt-1.5 flex gap-0.5 h-2">
                                                for seg in &main.history {
                                                    <div class=(format!("flex-1 rounded-sm {}", Panel::segment_class(seg.as_deref())))></div>
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
                                        <span>(main.status)</span>
                                        <a
                                            href=(format!("/logs/{}", main.source))
                                            target="_blank"
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
                                    style=(format!(
                                        "{}; grid-row: {}; grid-column: {} / span {};",
                                        slot.group_status_style(), ri + 1, slot.col_start, slot.span
                                    ))
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
                                                    target="_blank"
                                                    class="opacity-60 hover:opacity-100 hover:underline"
                                                >
                                                    (format!("updated {}", main.updated_ago()))
                                                </a>
                                            </div>
                                            if !main.history.is_empty() {
                                                <div class="mt-1.5 flex gap-0.5 h-2">
                                                    for seg in &main.history {
                                                        <div class=(format!("flex-1 rounded-sm {}", Panel::segment_class(seg.as_deref())))></div>
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
                                                    target="_blank"
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
                                                    target="_blank"
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
                                                <div class="mt-1 flex gap-0.5 h-1.5">
                                                    for seg in &p.history {
                                                        <div class=(format!("flex-1 rounded-sm {}", Panel::segment_class(seg.as_deref())))></div>
                                                    }
                                                </div>
                                            }
                                            if p.status == "stale" {
                                                <div class="mt-1 text-[10px] normal-case opacity-60">
                                                    (format!("updated {}", p.updated_ago()))
                                                </div>
                                            }
                                        </div>
                                    }
                                )
                            )
                        } else {
                            <div class="bd-panel-cell" style=(format!("grid-row: {}; grid-column: {} / span {};", ri + 1, slot.col_start, slot.span))></div>
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

/// Web dashboard rendered from the same config layouts as the TUI (spec:
/// web-ui). Panels are served by a topcoat shard: a browser-side interval
/// bumps `tick`, and each change re-renders the grid on the server without a
/// full page reload.
#[page("/")]
#[allow(clippy::no_effect_underscore_binding)] // `_t` is interpolated into the raw JS
pub async fn dashboard(cx: &Cx) -> Result {
    let _st = app_context::<AppState>(cx);
    let theme = theme_class(cx);
    view! {
        <!DOCTYPE html>
        <html class=(theme)>
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <title>"barduck"</title>
                <link rel="icon" id="bd-favicon">
                <script type="module" src="/assets/bd-runtime.js"></script>
                topcoat::font::link(font: GEIST)
                <link rel="stylesheet" href=(tailwind::stylesheet!())>
                <style>
                    "[id^='panel-']:target { outline: 3px solid #6366f1; outline-offset: 2px; }
                    /* Below phone width, the grid's dynamic per-layout inline
                       styles (a fixed column count and each cell's explicit
                       grid-row/grid-column) can't vary by viewport on their
                       own, and an inline `style` attribute otherwise always
                       beats a stylesheet rule regardless of source order — so
                       overriding them here for small screens needs
                       `!important` (spec: web-ui — responsive layout for
                       small viewports). Cells simply stack in DOM order once
                       explicit placement is reset, which already matches the
                       layout's own row-major declaration order. */
                    @media (max-width: 640px) {
                        .bd-panel-grid { grid-template-columns: 1fr !important; }
                        .bd-panel-cell { grid-column: auto !important; grid-row: auto !important; }
                    }"
                </style>
            </head>
            <body>
                signal tick = 0.0;
                <span :data-bd-tick=$({
                        #[allow(clippy::no_effect_underscore_binding)] // name is interpolated into the JS
                    let _t = tick;
                    raw!("(globalThis.__bdTick ??= setInterval(() => ${_t}.increment(), 5000), 'tick')", "tick")
                }) style="display:none"></span>
                <div class="max-w-5xl mx-auto p-6">
                    <h1 class="text-xl font-bold mb-4 text-foreground flex flex-wrap items-center gap-2">
                        <span>"barduck v"(config::VERSION)</span>
                        badge(
                            variant: BadgeVariant::Outline,
                            attrs: attributes! { class="gap-1.5 font-normal" },
                            <span id="bd-conn-dot" class="inline-block w-2 h-2 rounded-full" style="background-color:#94a3b8"></span>
                            <span id="bd-conn-label">"checking…"</span>
                        )
                        button(
                            variant: ButtonVariant::Ghost,
                            size: ButtonSize::Icon,
                            attrs: attributes! { id="bd-theme-toggle" type="button" aria-label="Toggle light/dark theme" },
                            <svg class="dark:hidden size-4" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
                                <circle cx="12" cy="12" r="4"></circle>
                                <path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M4.93 19.07l1.41-1.41M17.66 6.34l1.41-1.41"></path>
                            </svg>
                            <svg class="hidden dark:inline-block size-4" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor">
                                <path d="M21 12.79A9 9 0 1111.21 3 7 7 0 0021 12.79z"></path>
                            </svg>
                        )
                    </h1>
                    <div>
                        panels_grid(tick: $(tick.get()))
                    </div>
                    <footer class="mt-6 text-xs text-muted-foreground">
                        "barduck v"(config::VERSION)
                    </footer>
                </div>
                <script>(Unescaped::new_unchecked(CONNECTION_SCRIPT.to_string()))</script>
                <script>(Unescaped::new_unchecked(FAVICON_SCRIPT.to_string()))</script>
                <script>(Unescaped::new_unchecked(THEME_TOGGLE_SCRIPT.to_string()))</script>
            </body>
        </html>
    }
}

path_param!(source_name: String);

/// Server-rendered per-source fetch log view, linked from each panel's
/// time-ago text (spec: web-ui — per-source log view).
#[page("/logs/{source_name}")]
pub async fn source_logs(cx: &Cx) -> Result {
    let st = app_context::<AppState>(cx);
    // Unparsed String params cannot fail to parse, but map the error to owned
    // so no cx-borrowed data escapes the handler.
    let source = match path_param::<SourceName>(cx) {
        Ok(s) => s.clone(),
        Err(_) => {
            return Err(topcoat::Error::from(topcoat::router::error::bad_request(
                "invalid source name",
            )));
        }
    };
    let Some(src) = st.cfg.sources.iter().find(|s| s.name == source) else {
        return Err(topcoat::Error::from(topcoat::router::error::bad_request(format!(
            "unknown source `{source}`"
        ))));
    };
    let rows = st.db.logs_sync(Some(&source), 50).unwrap_or_default();
    let theme = theme_class(cx);
    view! {
        <!DOCTYPE html>
        <html class=(theme)>
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <title>(format!("logs — {}", src.name))</title>
                topcoat::font::link(font: GEIST)
                <link rel="stylesheet" href=(tailwind::stylesheet!())>
            </head>
            <body>
                <div class="p-6">
                    <h1 class="text-xl font-bold mb-4 text-foreground">
                        "Fetch logs — "(src.name.clone())
                    </h1>
                    table(
                        attrs: attributes! { class="bg-background rounded-xl shadow-sm" },
                        table_header(
                            table_row(
                                table_head("TIME")
                                table_head("OUTCOME")
                                table_head("DURATION")
                                table_head("VALUE")
                                table_head("ERROR")
                            )
                        )
                        table_body(
                            for l in &rows {
                                table_row(
                                    table_cell(attrs: attributes! { class="font-mono text-xs" }, (l.ts.clone()))
                                    if l.ok {
                                        table_cell(attrs: attributes! { class="text-emerald-600" }, "ok")
                                    } else {
                                        table_cell(attrs: attributes! { class="text-red-600" }, "failed")
                                    }
                                    table_cell(attrs: attributes! { class="font-mono text-xs" }, (format!("{} ms", l.duration_ms)))
                                    table_cell(
                                        attrs: attributes! { class="font-mono text-xs" },
                                        <pre class="whitespace-pre-wrap break-all m-0 max-w-xs">(l.value.clone().unwrap_or_else(|| "—".into()))</pre>
                                    )
                                    table_cell(
                                        attrs: attributes! { class="text-red-500 font-mono text-xs" },
                                        <pre class="whitespace-pre-wrap break-all m-0 max-w-xs">(l.error.clone().unwrap_or_default())</pre>
                                    )
                                )
                            }
                            if rows.is_empty() {
                                table_row(
                                    table_cell(attrs: attributes! { class="text-center text-muted-foreground" }, "No fetch attempts recorded yet.")
                                )
                            }
                        )
                    )
                </div>
            </body>
        </html>
    }
}
