## Why

The TUI's `draw()` (`src/tui.rs`) splits the body area into `state.rows.len()` equal `Constraint::Fill(1)` vertical slices, so when a config declares more rows (or taller group panes) than fit the current terminal height, every row is squeezed into whatever sliver of height remains — panels render clipped, overlapping, or unreadably short — instead of the user being able to see all of them by scrolling or paging. There is currently no scroll state and no keybinding beyond quit (`q`/Esc/Ctrl+C); every other key is silently ignored.

## What Changes

- The TUI gives each row a natural (content-driven) minimum height instead of dividing the body evenly across however many rows exist.
- When the sum of row heights exceeds the visible body height, the TUI enters a scrollable viewport: only a contiguous slice of rows renders at once, and the user can move the viewport with the keyboard instead of every row being squashed to fit.
- New keybindings while content overflows: line-scroll (`↑`/`↓` or `k`/`j`), page-scroll (`PgUp`/`PgDn`), and jump-to-start/end (`Home`/`g` and `End`/`G`). These keys are inert (existing no-op behavior) when everything already fits on screen.
- A visible scroll affordance (a vertical scrollbar, per design.md) shows the user's current position and that more content exists off-screen, only when the content overflows.
- No change to the web UI, to how layouts/panels are computed, or to any config schema — this is purely how the existing TUI grid is laid out and navigated on screen.

## Capabilities

### New Capabilities
(none)

### Modified Capabilities
- `tui`: adds a new requirement that the TUI must let the user reach every row via scrolling/paging when the configured grid doesn't fit the terminal, instead of silently shrinking every row to fit.

## Impact

- `src/tui.rs`: `draw()`'s row layout (`Layout::vertical(vec![Constraint::Fill(1); rows.len()])`) changes to content-sized row heights plus a scroll offset; `UiState` gains scroll-position state; `event_loop`'s key handling gains scroll keybindings alongside the existing quit check.
- No change to `src/web/*`, `src/config/*`, or any other capability's spec.
