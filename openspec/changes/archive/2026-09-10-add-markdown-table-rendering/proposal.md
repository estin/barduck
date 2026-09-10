## Why

Markdown tables (pipe tables) currently render as plain text in barduck web panels. When a source has `format = "markdown"` and its output contains a table like `| header | value |`, the `pulldown-cmark` parser is created without the `tables` option, so table syntax is treated as plain text instead of being rendered as an HTML `<table>`.

## What Changes

- Enable `tables` support in `pulldown-cmark` so pipe-table syntax renders as HTML `<table>` elements in web panels
- Add the `tables` Cargo feature to `pulldown-cmark` in `Cargo.toml`
- Pass `Options::ENABLE_TABLES` when constructing the parser in `src/web/markdown.rs`
- **No spec-level behavior change** — existing `format = "markdown"` sources continue to work; table syntax within their output now renders correctly

## Capabilities

### New Capabilities
- `markdown-table-rendering`: Enable `pulldown-cmark` table extension so markdown tables render as HTML `<table>` elements in web panels

### Modified Capabilities
- (none)

## Non-goals

- No changes to the TUI, CLI, or JSON API rendering paths
- No new config fields or source types
- No changes to markdown security posture (HTML blocks continue to be rewritten to literal text)
