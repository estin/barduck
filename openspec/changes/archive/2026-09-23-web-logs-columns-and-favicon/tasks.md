# Tasks

## 1. Two-unit relative time formatter

- [x] 1.1 Add `age::ago_precise(now, ts_epoch) -> Option<String>` beside `age::ago` in `src/age.rs`: coarsest unit first (d/h/m/s), the next-finer unit's remainder appended unless it's zero, seconds-only when under a minute. Verify with unit tests covering `1d 6h`, `1h 12m`, `12m 3s`, `45s`, `1h` (zero-minute case), and `1d` (zero-hour case).
- [x] 1.2 Verify `age::ago` itself and its existing tests (panel "updated X ago" text) are untouched.

## 2. Log table columns

- [x] 2.1 In `src/web/routes.rs::log_rows`, switch the TIME cell to `age::ago_precise` and reorder/relabel the table header to TIME, DURATION, SOURCE, VALUE (drop the ERROR header). Verify by re-reading the rendered `<thead>` markup.
- [x] 2.2 Move the ORIGIN cell (`l.origin.to_string()`) to the third column position, unchanged otherwise.
- [x] 2.3 Merge the VALUE and ERROR cells into one: when `l.error` is `Some`, render its wrapped text in `text-red-500`; otherwise render the existing value-with-unit, colored via `text_style_for_color`/`config::accent_color` as before. Verify with a test asserting an error row's cell contains the error text and `text-red-500`, and a value row's cell is unchanged from before this change.

## 3. Source-scoped favicon on the log view

- [x] 3.1 In `log_rows`, fetch the source's true latest reading via `st.db.logs_filtered(Some(&source), 1, 0, false)` (unfiltered, unpaginated), independent of the page's own `errors_only`/pagination args.
- [x] 3.2 Compute that source's level the same way `build_panel` does (`bands` non-empty → `config::level_for(bands, latest.value)`, else `None`), reusing the `status`/`bands` already computed in `log_rows`, then `config::status_color(level, status)` for the always-colored favicon value.
- [x] 3.3 Render `<span id="bd-status" data-status=(...)>` in `log_rows`'s output (mirroring `panels_grid`'s marker), so `FAVICON_SCRIPT` picks it up unchanged. Verify `page_chrome` never renders both `panels_grid` and `log_rows` on the same page load, so there is always exactly one `#bd-status` element.
- [x] 3.4 Verify end-to-end: a `failing` source's log view favicon renders red, a healthy yellow-banded source's renders yellow, a healthy unbanded source's renders green, and going offline while a log view is open still overrides to red (existing `FAVICON_SCRIPT` behavior, unmodified).

## 4. Validation

- [x] 4.1 Run `just ci` (`cargo clippy --all-targets -- -D warnings` and `cargo nextest run`) and confirm no regressions.
- [x] 4.2 Re-read the diff against `openspec/changes/web-logs-columns-and-favicon/specs/web-ui/spec.md` scenario-by-scenario and confirm each one is satisfied.
