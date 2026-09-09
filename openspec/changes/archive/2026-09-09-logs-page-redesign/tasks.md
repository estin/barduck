## 1. Panel links

- [x] 1.1 Remove `target="_blank"` from the four `/logs/<source>` links in `src/web/panels.rs`. Verify with `rg -n 'target="_blank"' src/web/panels.rs` returning no matches.

## 2. Log page

- [x] 2.1 Add a back link to the dashboard at the top of `source_logs` in `src/web/routes.rs`. Verify a rendered page contains a link to `/`.
- [x] 2.2 Replace the raw `l.ts` timestamp with `age::ago(now(), l.ts_epoch)`. If it returns `None`, show a dash instead. Verify with a unit test asserting the TIME cell shows relative text like "2m ago", not a raw date string.
- [x] 2.3 Color the VALUE cell using `health::compute` and `config::level_for`/`accent_color`, the same way a panel colors its value. Use `accent_color`, not `status_color`: only `accent_color` gives a healthy, unbanded source no color at all. Verify with a unit test asserting a banded source's value cell carries the matching color style.
- [x] 2.4 Tighten the table's padding and font size for a compact layout. Verify by rendering the page and reading the row markup for the smaller classes.

## 3. Regression

- [x] 3.1 Update `tests/integration.rs`'s existing log-view test(s) for the new markup (timestamp format, color style, back link, no `target="_blank"`).
- [x] 3.2 Run `just ci` (clippy + nextest) and verify it passes with no regressions.
