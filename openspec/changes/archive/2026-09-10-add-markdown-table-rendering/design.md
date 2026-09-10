## Context

The web panel rendering pipeline uses `pulldown-cmark` to convert markdown to HTML. The dependency in `Cargo.toml` is configured as `pulldown-cmark = { version = "0.13.4", default-features = false, features = ["html"] }`, and the parser is constructed with `pulldown_cmark::Parser::new(value)` in `src/web/markdown.rs`. The `default-features = false` means the `tables` extension is not enabled, and `Parser::new()` uses default options without `Options::ENABLE_TABLES`.

See proposal.md - Why for the motivation.

## Goals / Non-Goals

**Goals:**
- Pipe-table syntax in markdown content renders as HTML `<table>` elements in web panels
- Minimal change to existing code — no new config fields, no new source types, no new dependencies
- Existing markdown rendering (lists, emphasis, links, paragraphs) behavior is unchanged

**Non-Goals:**
- TUI rendering of markdown tables (TUI uses its own rendering path)
- Markdown table rendering in CLI output or JSON API responses
- Any change to the markdown security posture (HTML escaping remains as-is)

## Decisions

**Add `tables` Cargo feature to `pulldown-cmark`**
Rationale: The `tables` feature is required for `Options::ENABLE_TABLES` to be recognized. Without it, the parser silently ignores the flag. The feature is optional and only adds parsing support — no new runtime cost for non-table content.

**Use `Parser::new_extended(value, Options::ENABLE_TABLES)` instead of `Parser::new(value)`**
Rationale: `new_extended` accepts explicit options. This is the idiomatic `pulldown-cmark` API for enabling extensions. The `Options` struct is in the same `pulldown_cmark` namespace.

**Security unchanged: continue rewriting `Html`/`InlineHtml` events to `Text`**
Rationale: The existing markdown-to-HTML pipeline already rewrites raw HTML events to literal text to prevent XSS. Table syntax is part of the CommonMark spec and is parsed as structured data, not raw HTML, so it passes through the same `push_html` rendering path and is subject to the same escaping. No additional security handling is needed.

## Risks / Trade-offs

- **Dependency feature change**: Adding `tables` to `pulldown-cmark` slightly increases the parser's code size and compilation time. This is negligible for a dependency of this size.
- **Table alignment syntax**: `pulldown-cmark`'s table extension supports alignment specifiers (`---:`, `:---`, `:---:`). These are rendered via CSS classes or inline styles — barduck does not customize table styling beyond the existing `prose` classes, so alignment may not be visually distinct.

## Migration Plan

No migration needed. This is a compile-time change to `Cargo.toml` and a one-line change to `src/web/markdown.rs`. Existing configs work unchanged.

## Open Questions

- Whether markdown table styling (borders, padding) needs CSS adjustments beyond the existing `prose` classes — likely not, as `prose-sm` already styles tables, but this is unverified.
