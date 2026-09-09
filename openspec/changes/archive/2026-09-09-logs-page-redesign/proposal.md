## Why

The per-source log view opens in a new browser tab. It shows raw timestamps and plain, uncolored values. This does not match the rest of the dashboard. A user loses their place in the main tab and has to do mental math to read a timestamp.

## What Changes

- Remove the new-tab link target from every panel link to the log view in `src/web/panels.rs`. Links open in the current tab instead.
- Add a "Back to dashboard" link at the top of the log view. The browser's own back button also works now, since the page no longer opens in a new tab.
- Replace the raw timestamp column with the same relative time format panels already use (`src/age.rs`, for example "2m ago").
- Color the VALUE column by the source's threshold bands. Use the same coloring rule panels already use (`config::level_for` and `status_color`).
- Make the table more compact. Reduce row height and padding so more history fits on screen without scrolling.
- This change is not breaking. The route, its data, and the unknown-source error stay the same. Only the page layout and navigation change.

## Capabilities

### New Capabilities
(none)

### Modified Capabilities
- `web-ui`: the "Per-source log view linked from panels" requirement changes. The log view opens in the current tab, not a new tab, and includes a back link. A new requirement covers the compact layout, relative timestamps, and threshold-based value coloring.

## Impact

- `src/web/panels.rs`: remove `target="_blank"` from the four links to `/logs/<source>`.
- `src/web/routes.rs`: in `source_logs`, add a back link. Replace the raw timestamp with `age::ago`. Color the VALUE cell using `config::level_for` and `status_color` against the source's thresholds and current health. Reduce table spacing.
- `src/api.rs` and `Db::logs` stay the same. The JSON API is not part of this change.
