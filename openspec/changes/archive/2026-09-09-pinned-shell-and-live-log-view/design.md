## Context

`fetch_logs.ok BOOLEAN NOT NULL` (`src/db.rs`) is written by every `insert_log` call. Every success call passes `error: None, value: Some(_)`. Every failure call (`record_failure` in `src/collector.rs`) passes `error: Some(_), value: None`. `ok` never disagrees with `error.is_some()`. `health::compute` reads it (`logs.iter().take_while(|l| !l.ok)`). `Db::last_success` filters on it (`WHERE source = ? AND ok`).

`src/web/panels.rs`'s `panels_grid` is a `#[shard]`. A browser-side `setInterval` bumps a `tick` signal. Each change re-renders the shard's content on the server and swaps it into the DOM. The rest of the page stays untouched. See proposal.md for why the log view needs the same behavior.

The dashboard's header (`<h1>` with title, connection badge, theme toggle) and `<footer>` currently scroll with the rest of `src/web/routes.rs`'s `dashboard()` page content. An offline banner sits above the header, in normal flow, toggled by `CONNECTION_SCRIPT`. `source_logs()` has no equivalent header, footer, or scripts at all: it is a separate, much simpler page shell.

Manual testing after this change's first pass through `/opsx:apply` found two gaps. The log view has no header or footer. Navigating between the dashboard and a log view is still a full page load. The second point was investigated in depth, tracing `topcoat-runtime`'s actual browser source rather than just its docs, before writing the design below.

A `#[shard]`'s arguments cross the network as a one-way JSON snapshot. Its own codegen wraps every argument in `Expr<T>`. `topcoat-runtime`'s `ReactiveScope.fetchAndReplace` confirms this. A click handler rendered inside a shard's body can only mutate a signal declared in that same shard's own `view!` block. It cannot reach a signal declared by whatever page or component called it. `panels_grid`'s own per-source/per-member links render deep inside its own shard body, across four separate call sites. None of them can reach a signal declared in an enclosing page. The browser-side runtime confirms there is no back door either. `browser/src/index.ts` is just `new Runtime().start(document)`. The instance is never exposed on `window`. No hand-written script can reach the signal registry from outside.

## Goals / Non-Goals

**Goals:**
- Remove `ok` as a stored column and as a `Db`/`WriteCmd` parameter. Derive outcome from `error` everywhere it is read.
- Reuse the exact shard mechanism `panels_grid` already uses for the log view's table, instead of a second live-refresh approach.
- Pin the header and footer with no JavaScript changes, so the offline banner's existing show/hide logic needs no adjustment. Share one page shell between the dashboard and the log view, so both get the same chrome for free.

**Non-Goals:**
- Changing the log view's route, its data, or the unknown-source error behavior.
- Any migration for a database file created before this change. None is provided, matching the project's established precedent.
- Redesigning the offline banner itself.
- A client-side, no-reload switch between the dashboard and a log view. The Context section above explains why this needs restructuring `panels.rs`'s own rendering to work. The user chose to accept a real page load instead of that cost.

## Decisions

**Drop `ok`, derive outcome from `error`.** `WriteCmd::InsertLog` and `Db::insert_log` drop the `ok: bool` parameter entirely, not just the column. It was write-only redundant state. Keeping an unused parameter risks the two drifting apart again later. `health::compute` reads `l.error.is_some()` instead of `!l.ok`. `Db::last_success`'s query becomes `WHERE source = ? AND error IS NULL`.

**Log view table becomes a shard, `panels_grid`-style.** The `<table>` block, and the health lookup that colors it, both move into a new function: `#[shard] async fn log_rows(cx: &Cx, source: String, tick: f64) -> Result`. A shard's every argument, even a non-reactive one like `source`, still goes through `$(...)`, the same expression mechanism `tick` uses.

**Header/footer pinning uses `position: sticky`, not `fixed`.** A `fixed` header removes itself from flow. That needs matching top/bottom padding on the scrollable content, to avoid overlap. That padding also needs to change whenever the offline banner shows or hides, recomputed via JavaScript. `position: sticky` needs none of that. The header and footer stay in normal flow, so the rest of the page is never covered. The browser handles the "stick while scrolling past" behavior on its own. The offline banner moves inside the same sticky header block as the `<h1>` bar. Its height change is then handled by ordinary flow, not by anything this change adds.

**One shared `page_chrome` component owns the header, footer, and scripts.** `dashboard()` and `source_logs()` both reduce to a `view! { page_chrome(title: (...), view_source: (...)) }` call. `page_chrome` declares `signal tick` once and owns the sticky header/footer, the `CONNECTION_SCRIPT`/`FAVICON_SCRIPT`/`THEME_TOGGLE_SCRIPT` tags, and the `/assets/bd-runtime.js` script tag. Its content area picks between the two with a plain `if let Some(source) = &view_source`, evaluated once per real request. When `source` is set, it renders `log_rows(source: $(source.clone()), tick: $(tick.get()))`, with the "back to dashboard" link and a `"Fetch logs — X"` heading above it. Otherwise it renders `panels_grid(tick: $(tick.get()))`. `view_source` is a plain `Option<String>` component parameter, not wrapped in `$(...)`. `#[component]` parameters are ordinary Rust values inlined at render time, unlike `#[shard]` parameters. A component has no network endpoint of its own to serialize arguments across. This is why `page_chrome` can safely decide once, server-side, which content shard to mount, while `tick` still flows live into whichever shard is chosen.

**Log view's TIME cell tooltip uses a plain `title` attribute.** No JavaScript: the browser's native tooltip already does this. `l.ts`, the full stored timestamp, is already available at render time.

## Risks / Trade-offs

[Dropping `ok` is a schema-breaking change with no migration] → Matches the project's existing precedent, the same choice made for `typed-source-values`. A pre-existing database file must be recreated.

[A `sticky` header/footer needs an opaque background, or scrolled panel content shows through underneath it] → Both use the existing `--background` token. It is the same one `card`/`table` already rely on for an opaque surface.

[A shard changes what `source_logs` itself renders] → The page still renders its own back link and head. Only the row-rendering loop, and the health lookup it needs, move into the shard. The split mirrors `panels_grid`'s own relationship to `dashboard()`.

[Navigating dashboard-to-log-view stays a real page load] → Accepted, per the Non-Goals above. The alternative's real cost only became clear once traced through the actual runtime source, not just assumed. `page_chrome` makes the two page loads share identical chrome: header, footer, scripts. The transition looks and feels consistent even though it is a real navigation.
