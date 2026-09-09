//! The two server-rendered pages: the dashboard itself and the per-source
//! log view.

// The tick-signal span's `raw!` interpolation only ever reads its captured
// binding from inside a JS string (`${_t}`), never as ordinary Rust code, so
// the binding is deliberately underscore-prefixed. Scoped to this file since
// it's specific to that one established pattern, not a blanket exception.
#![allow(clippy::no_effect_underscore_binding)]

use super::{
    panels::{panels_grid, text_style_for_color},
    theme::theme_class,
};
use crate::{
    AppState, age,
    components::{
        badge::{BadgeVariant, badge},
        button::{ButtonSize, ButtonVariant, button},
        table::{table, table_body, table_cell, table_head, table_header, table_row},
    },
    config, health,
};
use topcoat::{
    Result,
    context::{Cx, app_context},
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

/// Frontend-initiated ping/pong: polls `/api/ping` and reflects reachability
/// in the connection indicator, independent of the panel-refresh shard
/// (spec: web-ui — global connection health indicator; design.md — plain
/// browser JS, not the shard mechanism, so failure is never silent).
const CONNECTION_SCRIPT: &str = r"(function () {
    var dot = document.getElementById('bd-conn-dot');
    var label = document.getElementById('bd-conn-label');
    // `var(--token)` (not a resolved hex/oklch string) so the dot's color
    // tracks whichever theme is active via the normal CSS cascade, the same
    // tokens the rest of the page uses (spec: web-ui — consistent
    // token-based visual theme; dark theme uses moderated contrast and
    // desaturated status colors).
    var colors = {
        checking: 'var(--muted-foreground)',
        online: 'var(--status-green-border)',
        offline: 'var(--status-red-border)'
    };
    var labels = { checking: 'checking…', online: 'online', offline: 'offline' };
    var banner = document.getElementById('bd-offline-banner');
    var panels = document.getElementById('bd-panel-wrapper');
    function setState(state) {
        dot.style.backgroundColor = colors[state];
        label.textContent = labels[state];
        // Shared with `FAVICON_SCRIPT`, which polls this on its own timer
        // rather than duplicating the ping loop (spec: web-ui — global
        // connection health indicator: favicon/banner/dim react to offline).
        document.body.dataset.bdConnection = state;
        var offline = state === 'offline';
        banner.hidden = !offline;
        panels.classList.toggle('opacity-50', offline);
        panels.classList.toggle('pointer-events-none', offline);
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
const FAVICON_SCRIPT: &str = r#"(function () {
    var link = document.getElementById('bd-favicon');
    // A data-URI SVG is its own standalone document with no access to the
    // host page's CSS custom properties, so (unlike the connection dot) the
    // actual computed color has to be read and baked into the SVG markup
    // here rather than referenced as `var(--token)` (spec: web-ui —
    // consistent token-based visual theme; dark theme uses moderated
    // contrast and desaturated status colors).
    var style = getComputedStyle(document.documentElement);
    var base = style.getPropertyValue('--muted-foreground').trim();
    var colors = {
        red: style.getPropertyValue('--status-red-border').trim(),
        yellow: style.getPropertyValue('--status-yellow-border').trim(),
        green: style.getPropertyValue('--status-green-border').trim()
    };
    function svgFor(hex) {
        return 'data:image/svg+xml,' + encodeURIComponent(
            '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">' +
            '<rect x="3" y="3" width="12" height="12" rx="2.5" fill="' + base + '"/>' +
            '<rect x="17" y="3" width="12" height="12" rx="2.5" fill="' + base + '"/>' +
            '<rect x="3" y="17" width="12" height="12" rx="2.5" fill="' + base + '"/>' +
            '<rect x="16.5" y="16.5" width="13" height="13" rx="2.5" fill="' + hex + '" transform="rotate(24 23 23)"/>' +
            '</svg>'
        );
    }
    function refresh() {
        // An offline connection overrides whatever health status was last
        // known — the favicon means "something needs your attention," and a
        // stale green tab during an outage would be actively misleading
        // (spec: web-ui — global connection health indicator: favicon turns
        // red when offline / resumes reflecting health after recovery).
        if (document.body.dataset.bdConnection === 'offline') {
            link.href = svgFor(colors.red);
            return;
        }
        var el = document.getElementById('bd-status');
        var status = (el && el.dataset.status) || 'green';
        link.href = svgFor(colors[status] || colors.green);
    }
    refresh();
    setInterval(refresh, 5000);
})();"#;

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

/// Shared shell for both pages (spec: web-ui — header and footer stay pinned
/// across pages): the pinned header/footer, the connection/favicon/theme
/// scripts, and the tick signal both content shards read. `view_source`
/// picks which one mounts: `None` for the dashboard's `panels_grid`, `Some`
/// for a source's `log_rows`. Deciding this with a plain `if let` (not a
/// `$(...)` expression) is why one shared shell can serve both routes: a
/// `#[component]` has no network endpoint of its own, so its parameters are
/// ordinary Rust values fixed once per real request, unlike a `#[shard]`'s.
#[component]
pub(super) async fn page_chrome(cx: &Cx, title: String, view_source: Option<String>) -> Result {
    let theme = theme_class(cx);
    view! {
        <!DOCTYPE html>
        <html class=(theme)>
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <title>(title)</title>
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
                    let _t = tick;
                    raw!("(globalThis.__bdTick ??= setInterval(() => ${_t}.increment(), 5000), 'tick')", "tick")
                }) style="display:none"></span>
                // The offline banner and the h1 header bar are pinned together
                // as one sticky block (spec: web-ui — header and footer stay
                // pinned across pages): a shared wrapper, not two
                // independently sticky elements, since two `top: 0` siblings
                // would stick at the same offset and overlap once the
                // banner's own show/hide toggles its height.
                <div class="sticky top-0 z-10 bg-background">
                    <div id="bd-offline-banner" hidden="" class="px-4 py-2 text-sm font-medium text-center" style="border-bottom:1px solid var(--status-red-border);background-color:var(--status-red-bg);color:var(--status-red-fg)">
                        "Connection lost — retrying…"
                    </div>
                    <div class="max-w-5xl mx-auto px-6 pt-6">
                        <h1 class="text-xl font-bold mb-4 text-foreground flex flex-wrap items-center gap-2">
                            <a href="/" class="text-foreground no-underline hover:underline">"barduck v"(config::VERSION)</a>
                            badge(
                                variant: BadgeVariant::Outline,
                                attrs: attributes! { class="gap-1.5 font-normal" },
                                <span id="bd-conn-dot" class="inline-block w-2 h-2 rounded-full" style="background-color:var(--muted-foreground)"></span>
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
                    </div>
                </div>
                <div class="max-w-5xl mx-auto px-6 pb-6">
                    <div id="bd-panel-wrapper">
                        if let Some(source) = &view_source {
                            <a href="/" class="text-xs opacity-60 hover:opacity-100 hover:underline">"← Back to dashboard"</a>
                            <h2 class="text-xl font-bold mt-2 mb-4 text-foreground">
                                "Fetch logs — "(source.clone())
                            </h2>
                            log_rows(source: $(source.clone()), tick: $(tick.get()))
                        } else {
                            panels_grid(tick: $(tick.get()))
                        }
                    </div>
                    <footer class="sticky bottom-0 z-10 bg-background mt-6 py-3 text-xs text-muted-foreground">
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

/// Web dashboard rendered from the same config layouts as the TUI (spec:
/// web-ui). Panels are served by a topcoat shard: a browser-side interval
/// bumps `tick`, and each change re-renders the grid on the server without a
/// full page reload.
#[page("/")]
pub async fn dashboard(cx: &Cx) -> Result {
    let _st = app_context::<AppState>(cx);
    view! { page_chrome(title: "barduck".to_string(), view_source: None) }
}

path_param!(source_name: String);

/// Live log table: re-renders on the server whenever `tick` changes (spec:
/// web-ui — log view live-refreshes without a full page reload), the same
/// way `panels_grid` refreshes the dashboard. A shard has its own endpoint
/// and no guard runs automatically for it (its arguments must not be
/// trusted), so `source` is re-validated here independently of
/// `source_logs`'s own check on the initial page render.
#[shard]
pub(super) async fn log_rows(cx: &Cx, source: String, tick: f64) -> Result {
    let _ = tick; // refresh trigger only; data always re-read from the DB
    let st = app_context::<AppState>(cx);
    let Some(src) = st.cfg.sources.iter().find(|s| s.name == source) else {
        return Err(topcoat::Error::from(topcoat::router::error::bad_request(
            format!("unknown source `{source}`"),
        )));
    };
    let rows = st.db.logs(Some(&source), 50).await.unwrap_or_default();
    // One health lookup per render (spec: web-ui — log view relative
    // timestamps and threshold coloring): every row shares the same source,
    // so its live health only needs computing once, the same fallback
    // `build_panel` uses for a lookup failure.
    let status = health::compute(&st.db, &st.cfg, &source)
        .await
        .map_or(health::Health::Stale, |h| h.status);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64());
    view! {
        table(
            attrs: attributes! { class="bg-background rounded-xl shadow-sm" },
            table_header(
                table_row(
                    table_head("TIME")
                    table_head("DURATION")
                    table_head("VALUE")
                    table_head("ERROR")
                )
            )
            table_body(
                for l in &rows {
                    table_row(
                        table_cell(
                            attrs: attributes! { class="font-mono" title=(l.ts.clone()) },
                            (age::ago(now, l.ts_epoch).unwrap_or_else(|| "—".into()))
                        )
                        table_cell(attrs: attributes! { class="font-mono" }, (format!("{} ms", l.duration_ms)))
                        table_cell(
                            attrs: attributes! {
                                class="font-mono"
                                style=(text_style_for_color(config::accent_color(
                                    l.value.as_deref().and_then(|v| config::level_for(&src.thresholds, v)),
                                    status,
                                )))
                            },
                            <pre class="whitespace-pre-wrap break-all m-0">(l.value.clone().unwrap_or_else(|| "—".into()))</pre>
                        )
                        table_cell(
                            attrs: attributes! { class="text-red-500 font-mono" },
                            <pre class="whitespace-pre-wrap break-all m-0">(l.error.clone().unwrap_or_default())</pre>
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
    }
}

/// Server-rendered per-source fetch log view, linked from each panel's
/// time-ago text (spec: web-ui — per-source log view). Renders through the
/// same `page_chrome` the dashboard uses, so both pages share one header,
/// footer, and set of scripts.
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
    if !st.cfg.sources.iter().any(|s| s.name == source) {
        return Err(topcoat::Error::from(topcoat::router::error::bad_request(
            format!("unknown source `{source}`"),
        )));
    }
    view! { page_chrome(title: format!("logs — {source}"), view_source: Some(source)) }
}
