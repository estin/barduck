#![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use crate::{
    AppState, config,
    config::ValueFormat,
    components::{
        badge::{BadgeVariant, badge},
        button::{ButtonSize, ButtonVariant, button_variants},
        card::{card, card_content, card_footer, card_header, card_title},
        table::{table, table_body, table_cell, table_head, table_header, table_row},
    },
    health,
};
use topcoat::{
    Result,
    context::{Cx, app_context},
    font::{Font, fontsource::fontsource_font},
    router::{page, path_param},
    runtime::shard,
    tailwind,
    view::{Unescaped, attributes, view},
};

/// Declared once and reused by every page: two independent
/// `fontsource_font!(GEIST)` call sites would each register their own font
/// route, colliding on `.discover()` (`duplicate route registered for GET
/// /_topcoat/fonts/...`).
const GEIST: Font = fontsource_font!(GEIST);

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
            "red" => "background-color:#ef4444;color:#ffffff",
            "yellow" => "background-color:#fbbf24;color:#451a03",
            _ => "background-color:#10b981;color:#ffffff",
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

    /// Human age of the latest reading, e.g. "12s ago" (spec: last update time).
    fn updated_ago(&self) -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(self.ts_epoch, |d| d.as_secs_f64());
        crate::age::ago(now, self.ts_epoch).unwrap_or_else(|| "never".into())
    }

    /// Pretty JSON when parseable; raw text otherwise.
    fn json_pretty(&self) -> String {
        serde_json::from_str::<serde_json::Value>(&self.value)
            .and_then(|v| serde_json::to_string_pretty(&v))
            .unwrap_or_else(|_| self.value.clone())
    }

    /// Markdown rendered to HTML (trusted: comes from user-owned config).
    fn markdown_html(&self) -> Unescaped<String> {
        let mut html = String::new();
        pulldown_cmark::html::push_html(&mut html, pulldown_cmark::Parser::new(&self.value));
        Unescaped::new_unchecked(html)
    }
}

struct Slot {
    span: usize,
    /// 1-based column where this slot starts (grid rows are placed
    /// explicitly so a short row can't be auto-packed into the next).
    col_start: usize,
    /// Card header text for a grouped slot (the cell's `title`); `None` for
    /// a single-source slot, whose header comes from that panel's own
    /// `name` instead, or a spacer.
    group_title: Option<String>,
    /// Empty for a spacer, one entry for a single-source cell, N for a
    /// group cell — one independently colored row per member.
    panels: Vec<Panel>,
}

impl Slot {
    /// DOM id anchor for this slot's card: its one panel's source, or (for a
    /// group) its first member's — shared by every member's chip so they all
    /// scroll to and highlight the same card.
    fn anchor(&self) -> Option<&str> {
        self.panels.first().map(|p| p.source.as_str())
    }

    /// A group card's own border — no background — is the worst color among
    /// its panels (spec: web-ui — group panes card border reflects the worst
    /// row).
    fn group_status_style(&self) -> &'static str {
        border_style_for_color(config::worst_color(self.panels.iter().map(Panel::level_color)))
    }
}

/// Inline CSS for a single-source panel's card: border, tinted background,
/// and text color together. `None` renders as no override at all — the
/// card falls back to its default neutral classes instead of a forced green.
fn full_style_for_color(color: Option<&str>) -> &'static str {
    match color {
        Some("red") => "border-color:#ef4444;background-color:#fef2f2;color:#7f1d1d",
        Some("yellow") => "border-color:#fbbf24;background-color:#fffbeb;color:#78350f",
        Some("green") => "border-color:#10b981;background-color:#ffffff;color:#0f172a",
        _ => "",
    }
}

/// Inline CSS carrying only a border color — used by a group card's own
/// border, which reflects its worst member without tinting the card's
/// background.
fn border_style_for_color(color: &str) -> &'static str {
    match color {
        "red" => "border-color:#ef4444",
        "yellow" => "border-color:#fbbf24",
        _ => "border-color:#10b981",
    }
}

/// Inline CSS carrying only a text color — used by a row inside a group
/// pane, which has no border or background of its own. `None` renders as no
/// override — the row's value inherits the default text color instead of a
/// forced green.
fn text_style_for_color(color: Option<&str>) -> &'static str {
    match color {
        Some("red") => "color:#ef4444",
        Some("yellow") => "color:#d97706",
        Some("green") => "color:#059669",
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
                let (panels, group_title) = match cell {
                    config::Cell::Group { title, ids } => (
                        ids.iter()
                            .map(|item| build_panel(st, &latest, item.id(), item.explicit_label()))
                            .collect(),
                        Some(title.clone()),
                    ),
                    config::Cell::Source(name) => (vec![build_panel(st, &latest, name, None)], None),
                    config::Cell::Pane { id, title } => {
                        (vec![build_panel(st, &latest, id, title.as_deref())], None)
                    }
                    config::Cell::Space { .. } => (Vec::new(), None),
                };
                let span = cell.span();
                slots.push(Slot { span, col_start: col, group_title, panels });
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
                    for p in &slot.panels {
                        chips.push((p, anchor));
                    }
                }
            }
        }
    }
    view! {
        if !chips.is_empty() {
            <div class="flex flex-wrap gap-2 mb-6">
                for (p, anchor) in &chips {
                    <a href=(format!("#panel-{}", anchor))
                        class=(button_variants(ButtonVariant::Outline, ButtonSize::Sm))
                        style=(p.chip_style())
                    >
                        (p.name.clone())
                    </a>
                }
            </div>
        }
        for grid in &grids {
            <h2 class="text-lg font-semibold mb-3 text-foreground">(grid.title.clone())</h2>
            <div class=(format!("grid gap-4 mb-8 grid-cols-{}", grid.columns.min(6)))
                style=(format!("--bd-cols: {}; grid-template-columns: repeat({}, minmax(0, 1fr));", grid.columns, grid.columns))
            >
                for (ri, row) in grid.rows.iter().enumerate() {
                    for slot in row {
                        if slot.panels.len() == 1 {
                            card(
                                attrs: attributes! {
                                    id=(format!("panel-{}", slot.panels[0].source))
                                    style=(format!(
                                        "{}; grid-row: {}; grid-column: {} / span {};",
                                        slot.panels[0].status_style(), ri + 1, slot.col_start, slot.span
                                    ))
                                },
                                card_header(card_title(attrs: attributes! { class="text-sm font-medium opacity-70" }, (slot.panels[0].name.clone())))
                                card_content(
                                    if slot.panels[0].format == ValueFormat::Markdown {
                                        <div class="prose prose-sm max-w-none">(slot.panels[0].markdown_html())</div>
                                    } else if slot.panels[0].format == ValueFormat::Json {
                                        <div class="text-3xl font-semibold">(slot.panels[0].value_and_unit())</div>
                                        <pre class="mt-2 text-xs font-mono whitespace-pre-wrap break-all max-h-40 overflow-y-auto">(slot.panels[0].json_pretty())</pre>
                                    } else {
                                        <div class="text-3xl font-semibold">(slot.panels[0].value_and_unit())</div>
                                    }
                                    if !slot.panels[0].history.is_empty() {
                                        <div class="mt-2 flex gap-0.5 h-2">
                                            for seg in &slot.panels[0].history {
                                                <div class=(format!("flex-1 rounded-sm {}", Panel::segment_class(seg.as_deref())))></div>
                                            }
                                        </div>
                                    }
                                )
                                card_footer(
                                    attrs: attributes! { class="flex justify-between text-xs uppercase tracking-wide" },
                                    <span>(slot.panels[0].status)</span>
                                    <a
                                        href=(format!("/logs/{}", slot.panels[0].source))
                                        target="_blank"
                                        class="normal-case opacity-60 hover:opacity-100 hover:underline"
                                    >
                                        (format!("updated {}", slot.panels[0].updated_ago()))
                                    </a>
                                )
                            )
                        } else if !slot.panels.is_empty() {
                            card(
                                attrs: attributes! {
                                    id=(format!("panel-{}", slot.panels[0].source))
                                    style=(format!(
                                        "{}; grid-row: {}; grid-column: {} / span {};",
                                        slot.group_status_style(), ri + 1, slot.col_start, slot.span
                                    ))
                                },
                                card_header(card_title(attrs: attributes! { class="text-sm font-medium opacity-70" }, (slot.group_title.clone().unwrap_or_default())))
                                card_content(
                                    for p in &slot.panels {
                                        <div class="px-2 py-1 mb-1">
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
                            <div style=(format!("grid-row: {}; grid-column: {} / span {};", ri + 1, slot.col_start, slot.span))></div>
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
    view! {
        <!DOCTYPE html>
        <html>
            <head>
                <meta charset="utf-8" />
                <title>"barduck"</title>
                <script type="module" src="/assets/bd-runtime.js"></script>
                topcoat::font::link(font: GEIST)
                <link rel="stylesheet" href=(tailwind::stylesheet!())>
                <style>"[id^='panel-']:target { outline: 3px solid #6366f1; outline-offset: 2px; }"</style>
            </head>
            <body>
                signal tick = 0.0;
                <span :data-bd-tick=$({
                        #[allow(clippy::no_effect_underscore_binding)] // name is interpolated into the JS
                    let _t = tick;
                    raw!("(globalThis.__bdTick ??= setInterval(() => ${_t}.increment(), 5000), 'tick')", "tick")
                }) style="display:none"></span>
                <div class="max-w-5xl mx-auto p-6">
                    <h1 class="text-xl font-bold mb-4 text-foreground flex items-center gap-2">
                        <span>"barduck v"(config::VERSION)</span>
                        badge(
                            variant: BadgeVariant::Outline,
                            attrs: attributes! { class="gap-1.5 font-normal" },
                            <span id="bd-conn-dot" class="inline-block w-2 h-2 rounded-full" style="background-color:#94a3b8"></span>
                            <span id="bd-conn-label">"checking…"</span>
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
    view! {
        <!DOCTYPE html>
        <html>
            <head>
                <meta charset="utf-8" />
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
