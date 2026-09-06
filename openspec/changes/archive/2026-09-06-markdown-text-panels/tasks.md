## 1. Config schema: static-text layout cell

- [x] 1.1 Add `Cell::Text { title: Option<String>, format: Option<String>, text: String }` to `src/config.rs`, positioned **before** `Cell::Group` in the enum, with a doc comment on `Cell` itself explaining why variant order matters (a variant with all-optional fields, like `Group`, must come after every variant whose required field should be tried first)
- [x] 1.2 Update `Cell::source_names()` (returns none for `Text`, since it has no source), `Cell::pane_title()` (returns `Text`'s `title`), and `Cell::span()` (unaffected, still 1) — with unit tests
- [x] 1.3 Update `validate_cell` to reject an empty `text` and an invalid `format`, and to accept a valid minimal `{ text = "..." }` cell — with unit tests, including one proving a `{ text = "...", format = "..." }` cell does NOT get misparsed as an empty `Group` (the ordering concern from design.md)

## 2. Web UI rendering

- [x] 2.1 Factor the existing format-rendering branch (markdown → HTML via `pulldown_cmark`, json → pretty-printed, else plain text) in `src/web.rs` into something callable from both the existing panel-rendering branches and the new text-cell branch, rather than copy-pasting it a third time
- [x] 2.2 Render `Cell::Text` in `panels_grid`: a card titled on its top border (or untitled) using the same border-title span as other panels, content rendered via the factored format logic, no footer/age/log-link/history-bar, border/background always neutral
- [x] 2.3 Add integration tests: a titled markdown text cell renders its HTML content with a border-title span; an untitled text cell renders no title span; a text cell shows no "updated" text, no `/logs/` link, and no history-bar wrapper class; its card style carries no health/threshold color

## 3. TUI rendering

- [x] 3.1 Render `Cell::Text` in `src/tui.rs`: a bordered panel titled with `title` (or untitled), content shown as-is (the raw `text`, ignoring `format`), no age suffix, border always the terminal's default (unaccented) style
- [x] 3.2 Add tests: a titled text cell renders its raw content in a bordered panel; an untitled one renders no title; a `format = "markdown"` text cell still shows the literal, uninterpreted text; no age suffix or accent color ever appears for a text cell

## 4. Docs and demo config

- [x] 4.1 Add a static-text panel example to `demo/config.toml` (e.g. a "Quick Links" panel) and update `demo/README.md`'s feature list to mention it
- [x] 4.2 Run `cargo build`, `cargo clippy --all-targets`, and `cargo nextest run`; confirm no new failures beyond the pre-existing sandbox-only TCP test failures
