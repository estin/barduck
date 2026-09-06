## Context

`Cell` (`src/config.rs`) is an untagged enum matched top-to-bottom by serde. After a prior change, `Cell::Group`'s fields are *all* optional (`title`, `main`, `secondary`, `table` all default), so it now matches almost any table-shaped cell that isn't `Pane`/`Space` — anything added after it in the enum needs a field `Group` doesn't have, tried *before* `Group` gets a chance. `format` (text/markdown/json) already exists on `SourceCfg` and is already rendered by both UIs: `src/web.rs`'s `Panel` renders markdown via `pulldown_cmark` and JSON pretty-printed; `src/tui.rs`'s `Panel` has no `format` field at all and always shows the raw value — "as-is" is already its only behavior there, so the TUI needs no new format-handling logic, only new rendering for the cell itself. See proposal.md for motivation.

## Goals / Non-Goals

**Goals:**
- Keep the new text cell genuinely source-free: no DB row, no health, no log view, no summary-strip chip — it is config content rendered directly, nothing more.
- Reuse the existing format-handling logic (markdown → HTML, json → pretty-printed, else plain text) rather than duplicating it for the new cell.

**Non-Goals:**
- No nesting a text cell's literal content inside a generalized pane's `main`/`secondary`/`table` (those slots take source references, not literal text) — a text cell is always a standalone top-level cell.
- No markdown interpretation in the TUI — explicitly out of scope per the request ("for tui it must be rendered as is"); a text cell's raw `text` renders identically to how the TUI already shows any source's raw value today, regardless of format.
- No source, collector, health, or database involvement of any kind for this cell type — that's the whole point of "static."

## Decisions

**`Cell::Text` must be tried before `Cell::Group` in the untagged enum.** Since `Group`'s fields are all optional (see Context), a `{ text = "...", format = "markdown" }` table would otherwise deserialize as an empty `Group` (extra fields are simply ignored — no variant in this enum uses `#[serde(deny_unknown_fields)]`) before ever reaching `Text`, then fail validation with a confusing "group with none of main/secondary/table" instead of rendering as text. `Text`'s `text` field is required, so trying it first is unambiguous: a cell without `text` cleanly falls through to `Group` as before. This is the same category of ordering constraint that already governs `Pane` needing to precede `Group`, worth calling out explicitly since it's easy to get backwards when adding a variant to an untagged enum.

**Web/TUI rendering: a lighter-weight sibling of the single-source panel, not a reuse of `Panel`.** `web.rs`/`tui.rs` build a minimal struct (title, format, text) for a `Text` cell rather than routing it through `build_panel`/`Panel` (which carries fields — `source`, `status`, `level`, `history`, `ts_epoch` — that don't apply and would all need to be faked). The card/panel markup itself *is* shared: the same border-title span (web) and bordered-`Block` (TUI) as every other panel, just with the footer/age/log-link/history-bar pieces omitted and the border always neutral (never health/threshold-colored, since there's no health to reflect).

**Format handling is factored so both the new cell and existing sources share it, not duplicated.** The web UI's markdown-to-HTML / JSON-pretty-print / plain-text branching already exists for a `Panel`'s value; the text cell needs the identical three-way branch over the identical `ValueFormat` enum, just fed `text` instead of a fetched value. The implementation extracts that branching (or the small pieces of it needed) into something callable from both places rather than copy-pasting the `if format == Markdown {...} else if == Json {...} else {...}` block a third time (it already appears twice, once per panel-rendering branch in `web.rs`).

## Risks / Trade-offs

- [`Cell` variant ordering is a latent footgun] Anyone adding a future `Cell` variant with all-optional fields, or adding one after `Group`, could reintroduce an ordering bug like the one `Text` has to route around → called out explicitly in a doc comment on the `Cell` enum itself, not just in this design doc, so it's visible at the point someone would add a new variant.
- [Duplicated format-handling logic if the extraction is skipped] It would be easy to just copy the three-way format branch a third time for the text cell instead of factoring it out → flagged in tasks.md as part of the implementation task, not left implicit.

## Migration Plan

1. `src/config.rs`: add `Cell::Text { title: Option<String>, format: Option<String>, text: String }` (before `Cell::Group`), update `source_names()`/`pane_title()`, and add validation (non-empty `text`, valid `format`).
2. `src/web.rs`: render `Cell::Text`, factoring out the shared format-rendering logic used by the existing panel branches.
3. `src/tui.rs`: render `Cell::Text` (raw text, no format interpretation).
4. `demo/config.toml`: add a static-text example (e.g. a quick-links panel) so the demo exercises the new capability end to end.
5. Verification: unit tests for the new validation rules and the `Cell` ordering concern (a `{ text, format }` cell must not be misparsed as an empty `Group`); web/TUI tests asserting a text cell renders its content with no footer/age/log-link/history-bar and no color, both titled and untitled, and (TUI only) that a `markdown`-format text cell still shows literal, uninterpreted text. `cargo build`/`clippy`/`nextest` clean beyond the pre-existing sandbox TCP failures.

No rollback complexity: purely additive config/rendering, no migration of existing data or configs.
