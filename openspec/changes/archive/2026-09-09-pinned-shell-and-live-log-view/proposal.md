## Why

The just-shipped per-source log view and dashboard shell have four rough edges. The fetch log stores a redundant `ok` boolean. It duplicates what "an error is present" already tells you. The log view needs a manual browser reload to see new fetch attempts, even though the dashboard already has a live-refresh mechanism. The dashboard's header and footer scroll away with the panel grid instead of staying in place. The log view's relative timestamps ("2m ago") hide the exact time some readers want.

## What Changes

- **BREAKING**: drop the `fetch_logs.ok` column. Every `insert_log` call already sets `error: Some(_)` on a failed fetch and `error: None` on a successful one. Outcome is fully derivable from whether `error` is present. Storing it separately was redundant. No migration is provided. This matches this project's established no-migration precedent for schema changes. An existing database file must be recreated.
- Remove the OUTCOME column from the web log view's table. It already showed the same "ok"/"failed" text the ERROR column's presence or absence already conveys. Remove the OK column from the CLI's `barduck logs` output too.
- Convert the log view's table into a topcoat shard. It polls on the same kind of browser-side timer the dashboard's panel grid already uses. A new fetch attempt then appears without a manual page reload.
- Give the log view's TIME cell a native tooltip: the browser's built-in hover title, showing the full stored timestamp. The exact time stays available on demand, even though the cell itself shows relative text.
- Pin the header (title, version, connection indicator, theme toggle) to the top of the viewport, and the footer to the bottom. This applies to both the dashboard and the log view. Both pages now share one page-chrome component. The log view gains the same header and footer the dashboard already had, instead of its own bare shell.
- The log view's VALUE cell keeps its existing threshold-band coloring from the prior change, unchanged. This change only touches how the table refreshes and what columns it has.
- Navigating between the dashboard and a log view stays a real page load, same as before this change. A client-side switch was investigated and rejected. topcoat's signal system has no supported way for a click inside `panels_grid` (its own independently-refreshed shard) to reach a signal declared in an enclosing page. Reaching that outcome needs restructuring `panels.rs`'s existing, tested rendering across four separate call sites. This dashboard's page loads are already near-instant on a local network, so that cost is not worth paying.

## Capabilities

### New Capabilities
(none)

### Modified Capabilities
- `data-collection`: "Fetch attempts logged" no longer requires a separately recorded outcome flag. Outcome is derived from whether the error field is present.
- `web-ui`: "Per-source log view linked from panels" drops outcome from its listed columns. "Log view relative timestamps and threshold coloring" gains a full-timestamp hover tooltip on the TIME cell. New requirements cover the log view's shard-based auto-refresh and the dashboard's pinned header/footer.

## Impact

- `src/db.rs`: drop the `ok` column from `create_schema`'s `fetch_logs` table, `WriteCmd::InsertLog`, `LogRow`, `Db::insert_log`'s signature, and every SQL statement referencing it. `Db::last_success` derives success from `error IS NULL` instead of `ok`.
- `src/health.rs`: derive the consecutive-failure count from `error.is_some()` instead of `!l.ok`.
- `src/web/routes.rs`: extract the log table into a new `#[shard]` function taking the source name and a tick signal. Drop the OUTCOME column. Add a `title` attribute to the TIME cell. Extract a shared `page_chrome` component: sticky header, sticky footer, the connection/favicon/theme scripts. Both `dashboard()` and `source_logs()` render through it, picking which content shard to show with a plain server-side parameter.
- `src/cli_report.rs`: drop the OK column from `print_logs`'s table output.
- `tests/integration.rs`, `src/collector.rs`, `src/db.rs` (unit tests): drop the `ok` argument from every `insert_log` call site and any assertion reading `LogRow.ok`.
