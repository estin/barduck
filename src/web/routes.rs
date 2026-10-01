//! The two server-rendered pages: the dashboard itself and the per-source
//! log view.

// The tick-signal span's `raw!` interpolation only ever reads its captured
// binding from inside a JS string (`${_t}`), never as ordinary Rust code, so
// the binding is deliberately underscore-prefixed. Scoped to this file since
// it's specific to that one established pattern, not a blanket exception.
#![allow(clippy::no_effect_underscore_binding)]
// `#[component]` moves a function-level `#[allow(...)]` onto the generated
// props struct instead of the original fn body (it clears `item.attrs`
// before re-embedding `item`), so a length allow on `page_chrome` itself
// can't be scoped any tighter than the file. `page_chrome`'s length is the
// shared page shell's markup, not accumulating logic.
#![allow(clippy::too_many_lines)]

use super::{
    panels::{panels_grid, poll_button, text_style_for_color},
    theme::{theme_class, width_class},
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
    router::{page, path_param, request::uri},
    runtime::{shard, signal},
    tailwind,
    view::{Unescaped, View, attributes, component, view},
};

/// Declared once and reused by every page: two independent
/// `fontsource_font!` call sites would each register their own font route,
/// colliding on `.discover()` (`duplicate route registered for GET
/// /_topcoat/fonts/...`).
///
/// Only the weights and style the UI actually uses, and no italics: an
/// unqualified `fontsource_font!` pulls every face the family ships, and each
/// one becomes a `<link rel=preload>` on every page load — 18 woff2 files
/// for a dashboard that renders `font-normal`/`font-medium`/`font-semibold`/
/// `font-bold` and nothing else.
const GEIST: Font = fontsource_font!(GEIST, weight: [400, 500, 600, 700], style: Normal);

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

/// Wires the content-width toggle button: persists the chosen width via
/// `POST /api/width`, then reloads so the server renders both wrappers
/// with the new cap class from the first byte (spec: web-ui — header width
/// toggle). Same reload-instead-of-live-swap rationale as the theme
/// toggle: the server-rendered shell is the single source of truth, so a
/// client-side class swap could drift from what a fresh load renders.
const WIDTH_TOGGLE_SCRIPT: &str = r"(function () {
    var btn = document.getElementById('bd-width-toggle');
    if (!btn) return;
    // The wrappers carry the narrow cap iff any one of them does — both
    // are always rendered together by the server, so reading one is
    // enough and the toggle posts the width the page is about to switch
    // *to*, letting the server treat the request as authoritative.
    function isNarrow() {
        var wrap = document.querySelector('#bd-panel-wrapper');
        if (!wrap || !wrap.parentElement) return false;
        return wrap.parentElement.classList.contains('max-w-5xl');
    }
    function labelFor(narrow) {
        return narrow ? 'Switch to full-width layout' : 'Switch to narrow centered layout';
    }
    btn.addEventListener('click', function () {
        var next = isNarrow() ? 'wide' : 'narrow';
        fetch('/api/width', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ width: next })
        }).then(function () {
            location.reload();
        }).catch(function () {});
    });
    btn.setAttribute('aria-label', labelFor(isNarrow()));
})();";

/// Drives every poll control on the page (spec: web-ui — Panels can
/// force a poll). One delegated listener rather than per-button handlers:
/// the panel shard replaces the grid's DOM on every tick, so anything bound
/// to a particular button would stop working seconds later.
///
/// In-flight polls are tracked in a `Set` keyed by source, not by the
/// button's `disabled` attribute, for the same reason — a re-render would
/// otherwise hand the user a fresh, enabled button for a poll that is still
/// running. No separate progress/failure popup: the clicked button itself
/// enters a busy state for the request's own duration (immediate, since the
/// endpoint doesn't answer until the fetch is done) — disabled with
/// `aria-busy` and a spinning icon — and the outcome, success or failure,
/// shows up the same way it does for anyone else watching, through the
/// shared `polling`/health state every viewer's next tick re-renders from
/// (spec: web-ui — poll-in-progress is visible); the clicking page bumps
/// its own tick as soon as the request settles. The busy state never
/// rewrites the button's content: the control is icon markup, so a
/// `textContent` swap would destroy the icon and restore it as escaped text.
const POLL_SCRIPT: &str = r"(function () {
    var inFlight = new Set();
    document.addEventListener('click', function (ev) {
        var btn = ev.target.closest('[data-bd-poll]');
        if (!btn) return;
        // The control sits inside the panel's link area; a click on it must
        // never also follow the log-view link or submit anything.
        ev.preventDefault();
        ev.stopPropagation();
        var source = btn.dataset.bdPoll;
        if (inFlight.has(source)) return;
        inFlight.add(source);
        var icon = btn.querySelector('svg');
        btn.disabled = true;
        btn.setAttribute('aria-busy', 'true');
        if (icon) icon.classList.add('bd-spin');
        fetch('/api/sources/' + encodeURIComponent(source) + '/poll', { method: 'POST' })
            .catch(function () {})
            .finally(function () {
                inFlight.delete(source);
                // Re-render now rather than waiting for the SSE event, which
                // a dead stream would never deliver.
                if (globalThis.__bdRefresh) globalThis.__bdRefresh.bump();
                btn.disabled = false;
                btn.removeAttribute('aria-busy');
                if (icon) icon.classList.remove('bd-spin');
            });
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
pub(super) async fn page_chrome(
    cx: &Cx,
    title: String,
    view_source: Option<String>,
) -> Result<impl View> {
    let theme = theme_class(cx);
    let st = app_context::<AppState>(cx);
    // Cookie wins over the server default; both wrappers below share this
    // one value so header and content can never disagree (spec: web-ui —
    // per-browser width override).
    let width_cap = width_class(cx, &st.cfg.web_content_width);
    // Read once per log-view request and pass as fixed shard args (design
    // D7): the runtime re-invokes `log_rows` on every tick with the same
    // values, preserving page/filter state across the 5 s refresh. Topcoat
    // supports `f64`/`bool` shard args but not integer args; all valid offsets
    // here remain exact in `f64` (u32 page × 10,000 maximum rows/page).
    let q = if view_source.is_some() {
        LogViewQuery::parse(uri(cx).query().unwrap_or(""))
            .map_err(|e| topcoat::Error::from(topcoat::router::error::bad_request(e)))?
    } else {
        LogViewQuery {
            page: 1,
            error: false,
        }
    };
    let max_limit = u32::try_from(crate::db::MAX_LOGS_LIMIT).unwrap_or(u32::MAX);
    let limit = st.cfg.logs_per_page.clamp(1, max_limit);
    let offset = q.offset(limit);
    let errors_only = q.error;
    #[allow(clippy::cast_precision_loss)]
    let offset_f = offset as f64;
    let limit_f = f64::from(limit);
    let tick = signal(cx, || 0.0f64);
    #[allow(clippy::cast_possible_truncation)] // interval is validated > 0; millis fit easily
    let refresh_interval_ms = st.cfg.web_refresh_interval.as_millis() as u64;
    Ok(view! {
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
                    "[data-bd-poll] svg.bd-spin { animation: bd-spin 1s linear infinite; } @keyframes bd-spin { to { transform: rotate(360deg); } }
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
                <span :data-bd-tick=$({
                    let t = tick;
                    let interval_ms = refresh_interval_ms;
                    // SSE-driven immediate refresh (spec: web-ui — immediate
                    // panel refresh): each `refresh` event bumps the tick so
                    // shards re-render at once; the timer below is a fallback
                    // that fires every configured interval in which no event
                    // arrived (each event pushes it back), so panels keep
                    // refreshing while the stream is down or silently dead. A
                    // stream the browser gave up on (`CLOSED`, e.g. an error
                    // status during a daemon restart) is reopened. Every bump
                    // also dispatches `barduck:panels-updated` (spec: web-ui
                    // — panels-updated event for user scripts) so
                    // once-subscribed user scripts observe each refresh: the
                    // source list is tick-invariant (layout config is static
                    // per page load), so scanning the currently rendered
                    // `#panel-<id>` anchors names exactly the sources this
                    // refresh renders. Handlers needing post-morph DOM state
                    // must defer (e.g. `requestAnimationFrame`); the tick
                    // bump requests the re-render rather than awaiting it.
                    raw!("(globalThis.__bdRefresh ??= (() => { const bump = () => { ${t}.increment(); try { var els = document.querySelectorAll('#bd-panel-wrapper [id^=\"panel-\"]'); var sources = []; for (var i = 0; i < els.length; i++) { var id = els[i].getAttribute('id'); if (id && id.indexOf('panel-') === 0) sources.push(id.slice(6)); } document.dispatchEvent(new CustomEvent('barduck:panels-updated', { detail: { sources: sources } })); } catch (e) {} }; let timer = null; const arm = () => { clearTimeout(timer); timer = setTimeout(() => { bump(); arm(); }, ${interval_ms}); }; const connect = () => { const es = new EventSource('/api/refresh-events'); es.addEventListener('refresh', () => { bump(); arm(); }); es.onerror = () => { if (es.readyState === EventSource.CLOSED) setTimeout(connect, ${interval_ms}); }; }; arm(); connect(); return { bump }; })(), 'tick')", {
                        let _ = t.get();
                        let _ = interval_ms;
                        "tick"
                    })
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
                    <div class=(format!("{width_cap} mx-auto px-6 pt-6"))>
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
                                attrs: attributes! { id="bd-theme-toggle" type="button" aria-label=(if theme == "dark" { "Switch to light theme" } else { "Switch to dark theme" }) title=(if theme == "dark" { "Switch to light theme" } else { "Switch to dark theme" }) },
                                // Action-indicating icons (spec: web-ui — light/dark theme toggle):
                                // each icon depicts the theme clicking switches *to* — moon
                                // (dark) shown in light mode, sun (light) in dark mode.
                                <svg class="hidden dark:inline-block size-4" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
                                    <circle cx="12" cy="12" r="4"></circle>
                                    <path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M4.93 19.07l1.41-1.41M17.66 6.34l1.41-1.41"></path>
                                </svg>
                                <svg class="dark:hidden size-4" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor">
                                    <path d="M21 12.79A9 9 0 1111.21 3 7 7 0 0021 12.79z"></path>
                                </svg>
                            )
                            button(
                                variant: ButtonVariant::Ghost,
                                size: ButtonSize::Icon,
                                // Action-indicating icons (spec: web-ui —
                                // header width toggle): each icon depicts the
                                // width clicking switches *to* — a centered
                                // narrow column on a wide page, a full-width
                                // frame on a narrow page — the same pattern
                                // as the theme toggle's moon/sun swap. The
                                // server renders the right one per request
                                // (the toggle reloads the page), so no
                                // client-side swap is needed.
                                attrs: attributes! { id="bd-width-toggle" type="button" aria-label=(if width_cap.is_empty() { "Switch to narrow centered layout" } else { "Switch to full-width layout" }) title=(if width_cap.is_empty() { "Switch to narrow centered layout" } else { "Switch to full-width layout" }) },
                                if width_cap.is_empty() {
                                    <svg class="size-4" data-bd-width-icon="narrow" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                                        <rect x="8" y="3" width="8" height="18" rx="2"></rect>
                                    </svg>
                                } else {
                                    <svg class="size-4" data-bd-width-icon="wide" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                                        <rect x="2" y="6" width="20" height="12" rx="2"></rect>
                                    </svg>
                                }
                            )
                        </h1>
                    </div>
                </div>
                <div class=(format!("{width_cap} mx-auto px-6 pb-6"))>
                    <div id="bd-panel-wrapper">
                        if let Some(source) = &view_source {
                            <a href="/" class="text-xs opacity-60 hover:opacity-100 hover:underline">"← Back to dashboard"</a>
                            // The heading (and the poll control it carries) is
                            // rendered by the `log_rows` shard, not here:
                            // `page_chrome` is fixed once per real request, so
                            // a poll control placed here would keep showing
                            // the idle control for the whole duration of a
                            // running fetch and queue another one on every click,
                            // while the panels' own controls — inside the
                            // `panels_grid` shard — do reflect it (spec:
                            // web-ui — poll-in-progress is visible).
                            log_rows(
                                source: $(source.clone()),
                                tick: $(tick.get()),
                                limit: $(limit_f),
                                offset: $(offset_f),
                                errors_only: $(errors_only),
                            )
                        } else {
                            panels_grid(tick: $(tick.get()))
                        }
                    </div>
                    <footer class="sticky bottom-0 z-10 bg-background mt-6 py-3 text-xs text-muted-foreground">
                        "barduck v"(config::VERSION)
                    </footer>
                </div>
                <script>(Unescaped::new_unchecked(CONNECTION_SCRIPT))</script>
                <script>(Unescaped::new_unchecked(FAVICON_SCRIPT))</script>
                <script>(Unescaped::new_unchecked(THEME_TOGGLE_SCRIPT))</script>
                <script>(Unescaped::new_unchecked(WIDTH_TOGGLE_SCRIPT))</script>
                <script>(Unescaped::new_unchecked(POLL_SCRIPT))</script>
                // User scripts live in the static shell, outside the shards'
                // comment markers, so topcoat's `morph` never touches them:
                // they run exactly once per page load (spec: web-ui —
                // user-defined scripts injected into the web UI). Classic
                // scripts in document order — no async/defer/module — so
                // they execute in resolution order with globals shared.
                for script in &st.user_scripts {
                    <script src=(format!("/assets/user-js/{}", script.name))></script>
                }
            </body>
        </html>
    })
}

/// Web dashboard rendered from the same config layouts as the TUI (spec:
/// web-ui). Panels are served by a topcoat shard: a browser-side interval
/// bumps `tick`, and each change re-renders the grid on the server without a
/// full page reload.
#[page("/")]
pub async fn dashboard(cx: &Cx) -> Result<impl View> {
    let _ = cx;
    Ok(view! { page_chrome(title: "barduck".to_string(), view_source: None) })
}

path_param!(source_name: String);

/// `GET /logs/{source_name}`'s query string (spec: web-ui — Log view
/// error-only filter; Log view pagination).
///
/// Parsed from raw key/value pairs rather than deserialized into a struct —
/// `error=1` needs `""`/`1`-style leniency a plain `bool` field lacks —
/// following the same pair-loop shape as api.rs `LogsQuery::parse`, but
/// single-source (the source comes from the path) and with `page`/`error`.
#[derive(Debug, Default, PartialEq)]
struct LogViewQuery {
    /// 1-based page (default 1; 0 or unparseable falls back to 1).
    page: u32,
    /// `?error=1` — show only failed attempts.
    error: bool,
}

impl LogViewQuery {
    fn parse(query: &str) -> std::result::Result<Self, String> {
        let pairs: Vec<(String, String)> =
            serde_urlencoded::from_str(query).map_err(|e| format!("invalid query: {e}"))?;
        let mut out = Self {
            page: 1,
            error: false,
        };
        for (key, value) in pairs {
            match key.as_str() {
                "page" => {
                    // Later `page` params win; invalid values fall back to
                    // page 1 rather than rejecting the request.
                    out.page = value.parse().unwrap_or(1);
                }
                "error" => out.error = value == "1",
                // Unknown parameters stay ignored, matching api.rs.
                _ => {}
            }
        }
        out.page = out.page.max(1);
        Ok(out)
    }

    /// 0-based row offset for `limit`, saturating at the largest i64 offset.
    fn offset(&self, limit: u32) -> i64 {
        i64::from(self.page.saturating_sub(1)).saturating_mul(i64::from(limit))
    }
}

#[shard]
pub(super) async fn log_rows(
    cx: &Cx,
    source: String,
    tick: f64,
    limit: f64,
    offset: f64,
    errors_only: bool,
) -> Result<impl View> {
    let _ = tick; // refresh trigger only; data always re-read from the DB
    let st = app_context::<AppState>(cx);
    let Some(src) = st.cfg.sources.iter().find(|s| s.name() == source) else {
        return Err(topcoat::Error::from(topcoat::router::error::bad_request(
            format!("unknown source `{source}`"),
        )));
    };
    // Shard request bodies are caller-controlled. Sanitize numeric args
    // before SQL/pager use; Db clamps them again.
    let limit = clamp_shard_integer(limit, 1, crate::db::MAX_LOGS_LIMIT);
    let offset = clamp_shard_integer(offset, 0, i64::MAX);
    // Re-read on every tick, unlike in `page_chrome`: this is what makes the
    // heading's poll control reflect an in-flight fetch (spec: web-ui —
    // poll-in-progress is visible).
    let pollable = crate::collector::unpollable_reason(src).is_none();
    let polling = st.db.is_polling(&source);
    // A failed query is not "no entries": log it, so a dead reader task
    // doesn't look like a source that was never polled.
    let rows = match st
        .db
        .logs_filtered(Some(&source), limit, offset, errors_only)
        .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!("log view `{source}` rows: {e:#}");
            Vec::new()
        }
    };
    // One health lookup per render (spec: web-ui — log view relative
    // timestamps and threshold coloring): every row shares the same source,
    // so its live health only needs computing once, the same fallback
    // `build_panel` uses for a lookup failure.
    let health = health::compute(&st.db, &st.cfg, &source).await;
    let status = health.as_ref().map_or(health::Health::Stale, |h| h.status);
    // Owned: the view body captures values, not borrows of these locals.
    let bands = health
        .as_ref()
        .map_or(&[][..], |h| h.thresholds.as_slice())
        .to_vec();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64());

    // The source's own status color for the browser-tab favicon (spec:
    // web-ui — global connection health indicator: log view favicon
    // reflects the source's own status), computed from the true latest
    // reading rather than `rows[0]` — `rows` can be paginated or
    // error-filtered, so its first entry isn't reliably "the latest value"
    // (design.md — favicon marker decision).
    let latest = match st.db.logs_filtered(Some(&source), 1, 0, false).await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!("log view `{source}` latest row: {e:#}");
            Vec::new()
        }
    };
    let latest_row = latest.into_iter().next();
    let favicon_level = latest_row
        .as_ref()
        .filter(|_| !bands.is_empty())
        .and_then(|r| r.value.as_deref())
        .and_then(|v| config::level_for(&bands, v));
    let favicon_status = config::status_color(favicon_level, status);

    // Pager state from the returned page alone (design D3, no COUNT query).
    let current_page = u32::try_from((offset / limit).saturating_add(1)).unwrap_or(u32::MAX);
    let has_prev = current_page > 1;
    let has_next = rows.len() == usize::try_from(limit).unwrap_or(usize::MAX);
    let rows_empty = rows.is_empty();
    let row_count = rows.len();
    let showing = format!(
        "Showing {}–{}",
        offset.saturating_add(1),
        offset.saturating_add(i64::try_from(row_count).unwrap_or(i64::MAX))
    );
    let base_href = format!("/logs/{source}");
    let errors_href = pager_href(&source, 1, true);
    let prev_href = pager_href(&source, current_page.saturating_sub(1), errors_only);
    let next_href = pager_href(&source, current_page.saturating_add(1), errors_only);
    let source_unit = src.unit().unwrap_or("").to_string();

    Ok(view! {
        <span id="bd-status" data-status=(favicon_status.as_str()) style="display:none"></span>
        // Every panel links here, so a source with no control of its own on a
        // compact group card still has one a click away (spec: web-ui —
        // panels can force a poll). Rendered here rather than in
        // `page_chrome` so it re-reads `is_polling` on every tick like the
        // dashboard panels' own controls do.
        <h2 class="text-xl font-bold mt-2 mb-4 text-foreground flex items-center gap-3">
            "Fetch logs — "(source.clone())
            if pollable {
                <span class="text-xs font-normal">
                    poll_button(source: source.clone(), polling: polling)
                </span>
            }
        </h2>
        <div class="flex flex-wrap items-center justify-between gap-3 mb-4">
            if errors_only {
                <a href=(base_href) class="text-sm font-medium text-foreground underline underline-offset-4 hover:opacity-80" title="Showing only failed attempts — click to clear">
                    "Errors only — on"
                </a>
            } else {
                <a href=(errors_href) class="text-sm text-muted-foreground hover:underline" title="Show only failed attempts">
                    "Errors only"
                </a>
            }
            <div class="flex items-center gap-3 text-sm text-muted-foreground">
                <span>
                    if rows_empty {
                        "No entries"
                    } else {
                        (showing)
                    }
                </span>
                if has_prev {
                    <a href=(prev_href) class="rounded-md border border-border px-2 py-1 hover:bg-muted">
                        "Prev"
                    </a>
                } else {
                    <span class="rounded-md border border-border px-2 py-1 opacity-40">"Prev"</span>
                }
                if has_next {
                    <a href=(next_href) class="rounded-md border border-border px-2 py-1 hover:bg-muted">
                        "Next"
                    </a>
                } else {
                    <span class="rounded-md border border-border px-2 py-1 opacity-40">"Next"</span>
                }
            </div>
        </div>
        // Narrow row spacing so more entries fit without scrolling (spec:
        // web-ui — log view table density). Set on the table itself, in
        // descendant selectors, so the vendored component's own `p-3` /
        // `text-sm` defaults can't quietly win again.
        table(
            attrs: attributes! { class="bg-background rounded-xl shadow-sm text-xs [&_td]:p-1.5 [&_th]:h-7" },
            table_header(
                table_row(
                    table_head("TIME")
                    table_head("DURATION")
                    table_head("SOURCE")
                    table_head("VALUE")
                )
            )
            table_body(
                for l in rows {
                    table_row(
                        table_cell(
                            attrs: attributes! { class="font-mono" title=(l.ts.clone()) },
                            (age::ago_precise(now, l.ts_epoch).unwrap_or_else(|| "—".into()))
                        )
                        table_cell(attrs: attributes! { class="font-mono" }, (format!("{} ms", l.duration_ms)))
                        table_cell(attrs: attributes! { class="font-mono" }, (l.origin.to_string()))
                        if let Some(err) = &l.error {
                            table_cell(
                                attrs: attributes! {
                                    class="font-mono"
                                    // The `--status-*` token, not a fixed
                                    // Tailwind palette value: every other
                                    // status color in the app routes through
                                    // it, so the light-mode saturated red
                                    // doesn't leak into the dark theme
                                    // (spec: web-ui — consistent token-based
                                    // visual theme).
                                    style=(text_style_for_color(Some(config::Level::Red)))
                                },
                                <pre class="whitespace-pre-wrap break-all m-0">(err.clone())</pre>
                            )
                        } else {
                            table_cell(
                                attrs: attributes! {
                                    class="font-mono"
                                    style=(text_style_for_color(config::accent_color(
                                        l.value.as_deref().and_then(|v| config::level_for(&bands, v)),
                                        status,
                                    )))
                                },
                                <pre class="whitespace-pre-wrap break-all m-0">(value_with_unit(l.value.as_deref(), Some(source_unit.as_str())))</pre>
                            )
                        }
                    )
                }
                if rows_empty {
                    table_row(
                        table_cell(attrs: attributes! { class="text-center text-muted-foreground" },
                            if errors_only { "No failed attempts." } else { "No fetch attempts recorded yet." })
                    )
                }
            )
        )
    })
}

/// Convert an untrusted topcoat `f64` shard argument into a bounded integer.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn clamp_shard_integer(value: f64, min: i64, max: i64) -> i64 {
    if !value.is_finite() {
        return min;
    }
    value.round().clamp(min as f64, max as f64) as i64
}

/// Pager/error-toggle link target for `source` at 1-based `page`, preserving
/// the error filter (design D8): `?page=N&error=1` when filtered, plain
/// `?page=N` otherwise.
fn pager_href(source: &str, page: u32, errors_only: bool) -> String {
    if errors_only {
        format!("/logs/{source}?page={page}&error=1")
    } else {
        format!("/logs/{source}?page={page}")
    }
}

/// A log entry's value in the panel form (spec: web-ui — Per-source log
/// view linked from panels): `"{value} {unit}"`, or the bare value when
/// the source declares no unit or the entry carries none.
fn value_with_unit(value: Option<&str>, unit: Option<&str>) -> String {
    match (value, unit) {
        (Some(v), Some(u)) if !u.is_empty() => format!("{v} {u}"),
        (Some(v), _) => v.to_string(),
        (None, _) => "—".into(),
    }
}

/// Server-rendered per-source fetch log view, linked from each panel's
/// time-ago text (spec: web-ui — per-source log view). Renders through the
/// same `page_chrome` the dashboard uses, so both pages share one header,
/// footer, and set of scripts.
#[page("/logs/{source_name}")]
pub async fn source_logs(cx: &Cx) -> Result<impl View> {
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
    if !st.cfg.sources.iter().any(|s| s.name() == source) {
        return Err(topcoat::Error::from(topcoat::router::error::bad_request(
            format!("unknown source `{source}`"),
        )));
    }
    Ok(view! { page_chrome(title: format!("logs — {source}"), view_source: Some(source)) })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::LogViewQuery;

    #[test]
    fn log_view_query_defaults_to_page_one_without_filter() {
        let query = LogViewQuery::parse("").unwrap();
        assert_eq!(query.page, 1);
        assert!(!query.error);
    }

    #[test]
    fn log_view_query_invalid_or_zero_page_falls_back_to_one() {
        for query in ["page=0", "page=abc", "page=-1"] {
            assert_eq!(LogViewQuery::parse(query).unwrap().page, 1, "{query}");
        }
    }

    #[test]
    fn log_view_query_parses_page_and_error_filter() {
        let query = LogViewQuery::parse("page=3&error=1").unwrap();
        assert_eq!(query.page, 3);
        assert!(query.error);
        assert!(!LogViewQuery::parse("error=0").unwrap().error);
    }

    #[test]
    fn log_view_query_computes_paged_offset() {
        let query = LogViewQuery::parse("page=3").unwrap();
        assert_eq!(query.offset(50), 100);
    }
}
