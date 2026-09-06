## Why

Today a source's presence in the TUI and web dashboard is all-or-nothing: it's either placed in the shared config-declared layout (and shows in both UIs) or left out entirely (and shows in neither). Users want to keep a source in one view but hide it from the other — for example, a chatty debug source that's useful in the TUI during troubleshooting but clutters the web dashboard shown to other people, without duplicating layouts or removing the source's history from one view.

## What Changes

- Add a per-source `show_in` config field with three modes: `"all"` (default, unchanged behavior), `"tui"` (visible only in the TUI), or `"web"` (visible only in the web dashboard). Any other value is rejected at startup, naming the source and the invalid value.
- The source continues to be collected normally regardless of `show_in` — this only controls display placement, not data collection.
- When a layout cell (a bare source reference or `{ id, title }` cell) names a source that's hidden from the view currently rendering, that view renders the cell as an empty space of the same span instead of the source's panel. The *other* view (where the source isn't hidden) still renders it normally from the same layout — no startup failure, no separate layout needed.
- The same rule applies inside a generalized pane's `main`/`secondary`/`table` members: a hidden member is simply omitted from that view's rendering of the pane. If omitting hidden members leaves a generalized pane cell with none of `main`, `secondary`, or `table` populated for that view, the whole cell renders as space in that view.
- Static-text panel cells are unaffected (they have no backing source).

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `source-configuration`: adds the `show_in` field and its startup validation.
- `tui`: layout rendering must treat a cell/member hidden from the TUI as space/omitted.
- `web-ui`: layout rendering must treat a cell/member hidden from the web UI as space/omitted.

## Impact

- Config schema: new optional per-source `show_in` field (`"all" | "tui" | "web"`, default `"all"`).
- TUI and web layout-rendering code: resolve `show_in` per view when building the rendered grid, independent of the shared layout config.
- No changes to collection/scheduling, storage, or the HTTP API.
