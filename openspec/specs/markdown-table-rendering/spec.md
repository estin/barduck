## Purpose

Enable markdown table (pipe table) rendering in barduck web panels so that source output or static-text content formatted as markdown tables displays as HTML `<table>` elements instead of raw pipe syntax.

## ADDED Requirements

### Requirement: Markdown tables render as HTML tables in web panels

When `pulldown-cmark` parses markdown content containing pipe-table syntax (`| header | value |`), the parser SHALL recognize table rows and produce HTML `<table>`, `<thead>`, `<tbody>`, `<tr>`, `<th>`, and `<td>` elements via `pulldown_cmark::html::push_html`.

#### Scenario: Source output with markdown table renders correctly
- **WHEN** a source has `format = "markdown"` and its output contains a pipe table like `| Metric | Value |\n|---|---|\n| CPU | 42% |`
- **THEN** the web panel renders an HTML `<table>` with the headers and rows, not raw pipe syntax

#### Scenario: Plain text continues to render as plain text
- **WHEN** a source has `format = "markdown"` and its output contains no table syntax
- **THEN** the web panel renders the content as plain text (existing behavior unchanged)

#### Scenario: Markdown list and emphasis continue to render
- **WHEN** a source has `format = "markdown"` and its output contains `- item` or `*emphasis*`
- **THEN** the web panel renders the list and emphasis correctly (existing behavior unchanged)

#### Scenario: HTML blocks in markdown continue to be escaped
- **WHEN** a source has `format = "markdown"` and its output contains `<script>alert(1)</script>`
- **THEN** the script tags are rendered as literal text, not executable HTML (security posture unchanged)
