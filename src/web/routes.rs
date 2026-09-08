//! The two server-rendered pages: the dashboard itself and the per-source
//! log view.

use super::{panels::panels_grid, theme::theme_class};
use crate::{
    AppState,
    components::{
        badge::{BadgeVariant, badge},
        button::{ButtonSize, ButtonVariant, button},
        table::{table, table_body, table_cell, table_head, table_header, table_row},
    },
    config,
};
use topcoat::{
    Result,
    context::{Cx, app_context},
    font::{Font, fontsource::fontsource_font},
    router::{page, path_param},
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
        return Err(topcoat::Error::from(topcoat::router::error::bad_request(
            format!("unknown source `{source}`"),
        )));
    };
    let rows = st.db.logs(Some(&source), 50).await.unwrap_or_default();
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
