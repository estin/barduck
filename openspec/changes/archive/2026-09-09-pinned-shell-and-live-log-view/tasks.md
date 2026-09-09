## 1. Remove the outcome field

- [x] 1.1 Drop the `ok` column from the `fetch_logs` table in `create_schema` (`src/db.rs`). Verify with a test asserting `DESCRIBE fetch_logs` has no `ok` column.
- [x] 1.2 Drop the `ok` parameter from `WriteCmd::InsertLog`, `LogRow`, and `Db::insert_log`'s signature, and drop it from every SQL statement referencing it. Verify the crate builds.
- [x] 1.3 Update `health::compute` to count consecutive failures from `l.error.is_some()` instead of `!l.ok`. Verify the existing health tests still pass unchanged.
- [x] 1.4 Update `Db::last_success`'s query to `WHERE source = ? AND error IS NULL`. Verify with a unit test that a logged failure is not returned as the last success.
- [x] 1.5 Remove the OUTCOME column from the log view's table (`src/web/routes.rs`) and the OK column from `print_logs` (`src/cli_report.rs`). Verify by rendering the log view and running `barduck logs`, verifying neither column appears.
- [x] 1.6 Update every `insert_log` call site in `tests/integration.rs`, `src/collector.rs`, and `src/db.rs`'s own tests. Drop the removed argument at each site, and update any assertion that read `LogRow.ok`. Verify the crate builds.

## 2. Shard-ify the log view

- [x] 2.1 Extract the log table's row-rendering into a new `#[shard] async fn log_rows(cx: &Cx, source: String, tick: f64) -> Result` in `src/web/routes.rs`. Keep `source_logs` as the page shell: back link, page head, calling the shard. No `#[shard]`/`#[page]` function in this codebase is ever called directly outside a real request. Verify through the real router instead, with the existing HTTP-level test (`log_view_shows_relative_time_threshold_color_and_back_link`).
- [x] 2.2 Wire `source_logs` to declare its own `signal tick` and the same kind of browser-side interval script `dashboard()` uses, calling `log_rows(source: $(source), tick: $(tick.get()))`. Add the `/assets/bd-runtime.js` script tag to the log view's `<head>` too. `source_logs` was missing it, and without it no client-side runtime loads to drive the tick signal. Verify by rendering the page and reading the `data-bd-tick` markup and the script tag.
- [x] 2.3 Verify the log table is really wired as a shard, not just plain content. Verify the page carries the shard's reactive-scope marker (`::topcoat::scope::`), the same marker the dashboard's own shard-wiring test looks for. A live exchange with the shard's own endpoint needs an internal argument encoding this test suite does not reverse-engineer. This is the same kind of browser-only gap already flagged for the offline banner and dark theme in earlier changes.

## 3. Pin the dashboard header and footer

- [x] 3.1 Wrap the offline banner and the `<h1>` header bar in one sticky top container (`position: sticky; top: 0`, opaque background) in `dashboard()`. Verify by rendering the dashboard and reading the sticky classes on that wrapper.
- [x] 3.2 Make the dashboard's `<footer>` sticky to the bottom (`position: sticky; bottom: 0`, opaque background). Verify the same way.

## 4. Hover tooltip on the log view's TIME cell

- [x] 4.1 Add a `title` attribute carrying the full stored timestamp (`l.ts`) to the TIME cell in `log_rows`. Verify with a unit test asserting the rendered cell's `title` attribute equals the raw timestamp.

## 5. Regression

- [x] 5.1 Verify the VALUE cell's threshold-based coloring still works after the shard extraction. If the existing `log_view_shows_relative_time_threshold_color_and_back_link` test needs a different way to fetch the page's rows, adjust it for the new shard boundary. (The test's raw-timestamp assertion also needed updating for task 4.1's new tooltip: the raw timestamp is now expected, in `title="..."`, not absent.)
- [x] 5.2 Run `just ci` (clippy + nextest) and verify it passes with no regressions.

## 6. Shared page chrome (manual-testing follow-up)

Manual testing after section 1-5 landed found two real gaps: the log view had no header or footer, and navigating between the dashboard and a log view still did a full page load. See proposal.md and design.md's Context/Non-Goals for why the second point stays a real page load. The fix for both is one shared `page_chrome` component.

- [x] 6.1 Extract a `#[component] async fn page_chrome(cx: &Cx, title: String, view_source: Option<String>) -> Result` in `src/web/routes.rs` from `dashboard()`'s existing markup: the sticky header (offline banner + h1 with badge/theme toggle), the sticky footer, and the `CONNECTION_SCRIPT`/`FAVICON_SCRIPT`/`THEME_TOGGLE_SCRIPT`/`bd-runtime.js` tags. If `view_source` is `Some(source)`, its content area renders `log_rows(source: $(source.clone()), tick: $(tick.get()))` behind a back link and heading. Otherwise it renders `panels_grid(tick: $(tick.get()))`. Verify the crate builds. (`#[component]` parameters take plain Rust values with no `$(...)`/paren wrapping at all. The compiler flagged the first attempt's parens as unnecessary, proving this. It is simpler than a `#[shard]`'s parameters, which always need `$(...)`.)
- [x] 6.2 Reduce `dashboard()` to `view! { page_chrome(title: "barduck".to_string(), view_source: None) }`. Verify the existing dashboard tests still pass with no changes to their assertions.
- [x] 6.3 Reduce `source_logs()` to keep its own source-name validation (unknown source still a client error), then `view! { page_chrome(title: format!("logs — {source}"), view_source: Some(source)) }`. Verify the crate builds.
- [x] 6.4 Add an integration test asserting the log view's rendered page contains the theme toggle button and the sticky footer. These are the same markers the dashboard's own structural test looks for.
- [x] 6.5 Run `just ci` (clippy + nextest) and verify it passes with no regressions.
