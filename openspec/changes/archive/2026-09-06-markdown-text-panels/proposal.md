## Why

Dynamic markdown content already works today (`type = "script"` + `format = "markdown"` — a script's stdout is rendered as HTML in the web UI, already exercised by `demo/config.toml`'s `weekly-report` source). What's missing is a way to put purely static text on the dashboard — a quick-links list, a note, anything that's just authored once and never changes — without declaring a fake `[[sources]]` entry that has a schedule, a health status, a fetch log, and a summary-strip chip for content that was never going to change or need any of that machinery.

## What Changes

- Add a new layout cell kind, `{ title?, format?, text }`, for a standalone static-text panel with no backing source at all — no `[[sources]]` entry, no database row, no health, no log view, no summary-strip chip. `text` is the literal content, authored directly in the layout.
- The cell renders through the existing format-aware pipeline: the web UI renders `format = "markdown"` content as HTML (the same rendering a markdown-format source already gets), and the TUI renders it as-is (raw text), matching how the TUI already treats any source's value today regardless of format.

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `source-configuration`: adds the `{ title?, format?, text }` layout cell.
- `web-ui`: renders the new text cell (title-on-border, format-aware content, no footer/history-bar/log-link since there's no source behind it).
- `tui`: renders the new text cell (bordered panel, as-is text content, no age suffix since there's no source behind it).

## Impact

- `src/config.rs`: a new `Cell::Text { title: Option<String>, format: Option<String>, text: String }` variant, ordered **before** `Cell::Group` in the untagged enum (`Group`'s fields are all optional after a prior change, so an unrelated cell could otherwise match it by accident — `Text`'s required `text` field avoids that ambiguity only if tried first); validation for the new cell.
- `src/web.rs`, `src/tui.rs`: render the new `Cell::Text` (a lighter-weight sibling of the existing single-source panel — no age, log link, or history bar, since there's no source).
- No changes to the collector, health derivation, database schema, CLI, or HTTP API — this is a config/layout-only, source-free addition.
