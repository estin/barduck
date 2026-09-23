# Design

## Context

`src/web/routes.rs` renders both web pages through one shared `page_chrome` component and shard scripts (see proposal.md - Why). Relevant existing pieces:

- `log_rows` (`#[shard]`) renders the `/logs/<source>` table: currently TIME, DURATION, VALUE, ORIGIN, ERROR, one `table_cell` each, reading `LogRow { ts_epoch, duration_ms, value, error, origin, .. }`.
- `age::ago(now, ts_epoch) -> Option<String>` formats a single-unit "Xs/Xm/Xh/Xd ago" string, shared by panels and the log view's TIME cell. Panels' own "updated X ago" text must keep this exact format — it is not part of this change.
- `FAVICON_SCRIPT` (inline JS in `page_chrome`) polls `#bd-status[data-status]` every 5s and paints the tab favicon from `--status-{status}-border`. `panels_grid` is the only place that renders `#bd-status` today, with `data-status` set to `config::worst_color(...)` across every panel on the dashboard. `log_rows` renders no such marker, so on `/logs/<source>` the script's `document.getElementById('bd-status')` is `null` and it falls back to green unconditionally — the bug this change fixes.
- `config::status_color(level, status) -> Level` (never `None`, falls back to green) is the exact "always-colored" function `Panel::level_color()` uses for the dashboard-wide worst-color computation. `config::accent_color(level, status) -> Option<Level>` is the "no color when healthy+unbanded" variant used for the VALUE cell's own text color.

## Goals / Non-Goals

**Goals:**
- Reorder/relabel the log table's columns and merge VALUE/ERROR into one cell, per the spec delta.
- Add a two-unit relative-time formatter for the log view's TIME cell only.
- Give `/logs/<source>` its own `#bd-status`-style marker so `FAVICON_SCRIPT` (unchanged) colors the tab for that one source.

**Non-Goals:**
- Changing panels' own "updated X ago" text or `age::ago`'s single-unit behavior.
- Changing `/api/logs`, the CLI `logs` command, or any database schema.
- Changing how the dashboard's own (multi-source, worst-color) favicon behavior works.

## Decisions

- **Two-unit formatter lives beside `age::ago` in `src/age.rs`, not inside it.** `age::ago` is a shared single-unit primitive already reused by panels and (today) the log view; changing its output shape would ripple into panel text the proposal explicitly leaves alone. A second function (e.g. `age::ago_precise`) computes the same `(now - ts_epoch)` duration and additionally reports the next-finer unit's remainder, dropping it when zero. Alternative considered: pull in the `humantime` crate's `format_duration` — rejected, since its output pluralizes units and has no "at most two units, coarsest first, seconds-only under a minute" shaping without post-processing anyway, so a purpose-built function is no larger and stays consistent with `age::ago`'s existing style.
- **The merged VALUE/ERROR cell picks error vs. value in `log_rows`, not further down.** `LogRow.error: Option<String>` already indicates which to show; the cell becomes `if let Some(err) = &l.error { <pre class="... text-red-500">(err)</pre> } else { <pre style=(text_style_for_color(...))>(value_with_unit(...))</pre> }`, replacing today's two separate `table_cell`s with one. This keeps the existing per-value threshold/health coloring path untouched for the non-error case, matching the spec's carried-over scenarios.
- **SOURCE column reuses the existing ORIGIN cell verbatim, only the header text and column position change.** No data-model change; `l.origin.to_string()` moves from column 4 to column 3, header text `"ORIGIN"` → `"SOURCE"`.
- **Favicon marker: fetch the source's true latest reading with a dedicated 1-row query, not `rows[0]`.** `log_rows`'s `rows` is paginated and can be error-filtered, so `rows.first()` is not reliably "the latest reading" (page 2, or `errors_only=true` while the latest attempt succeeded, would both give the wrong value). Instead, `log_rows` issues one extra `st.db.logs_filtered(Some(&source), 1, 0, false)` call (unfiltered, unpaginated, `LIMIT 1`) to get the true latest row, then computes `config::status_color(level, status)` exactly as `build_panel` does (`level = level_for(bands, latest.value)` when `bands` is non-empty, else `None`), reusing the `status`/`bands` already computed in `log_rows` for the VALUE column's own coloring. Alternative considered: extend `health::compute`/`SourceHealth` to carry the latest value — rejected as a wider, cross-cutting data-model change for one page's favicon marker.
- **Marker element is a second `id="bd-status"` span rendered only inside `log_rows`, never alongside `panels_grid`'s.** The two shards are mutually exclusive per `page_chrome`'s `if let Some(source) = &view_source { log_rows(...) } else { panels_grid(...) }`, so exactly one `#bd-status` exists in the DOM on any given page load — `FAVICON_SCRIPT` needs no changes at all, it already just reads whichever one is present. This is why the favicon logic itself required no touch: the fix is entirely in what `log_rows` renders.

## Risks / Trade-offs

- [The extra `logs_filtered(..., 1, 0, false)` query adds one more DB round trip per `log_rows` tick (every 5s while a log view tab is open)] → Negligible: it's a `LIMIT 1` indexed-by-recency read, the same cost class as the `health::compute` call `log_rows` already makes once per render.
- [Dropping the ERROR column narrows what's visible without hovering/selecting text] → Acceptable per the proposal's explicit ask; the VALUE cell already wraps (`whitespace-pre-wrap break-all`) so the full error text remains visible, just relocated.

## Migration Plan

Pure rendering-path change behind existing routes; no data migration, no config or API changes. Ships as a normal code change. Rollback is reverting the commit.
