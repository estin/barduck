## MODIFIED Requirements

### Requirement: UI layouts are config-declared like sources
The system SHALL let users declare TUI and web dashboard layouts in the same config file as a row/column grid: each layout has a `title` and `rows`, where a row is a list of cells and a cell is one of
- a string naming a source (shorthand), or
- a table `{ id = "<source>", title = "<pane title>" }` overriding the default pane title (the source's declared `title`, or its name if it has none), or
- a table `{ kind = "space", colspan = <columns> }` rendering an empty spacer, or
- a table `{ title?, format?, text }` rendering a standalone static-text panel with no backing source: `text` is the literal content (required), `format` is optional (`text` default or `markdown`, same values as a source's `format`), and `title` is an optional pane header (omitted renders no header text, since there's no source to fall back to a label from), or
- a table `{ title?, main?, secondary?, table? }` rendering one generalized pane, where `title` is an optional pane header, `main` is an optional single member, and `secondary` and `table` are each an optional list of members. A "member" is either a bare source-id string (its label defaults to that source's declared `title`, or its id if it has none) or a table `{ id = "<source>", label = "<value label>" }` overriding the label. At least one of `main`, `secondary`, or `table` MUST be present; `title` MAY be omitted entirely — the rendered pane then falls back to `main`'s own label when `main` is set, else shows no header text.

The layout's column count SHALL be the maximum number of columns spanned by any row; shorter rows are implicitly padded. Validation MUST reject rows referencing unknown sources (including any source id inside a generalized pane cell's `main`, `secondary`, or `table`), cells that are neither a valid source reference, space, static-text panel, nor generalized pane, `space` cells without `colspan`, colspans smaller than 1, static-text panel cells with an empty `text`, generalized pane cells with an explicitly empty `title` (`title = ""`), and generalized pane cells with `main`, `secondary`, and `table` all absent. The legacy flat `sources = [...]` layout key is removed (**BREAKING**). The prior `{ title, ids = [...] }` shape is replaced by `table` (**BREAKING**). A static-text cell declaring `format = "json"` MUST be rejected at startup like any other unknown format (**BREAKING**).

#### Scenario: Layout references existing source
- **WHEN** a layout row contains cell `"domain-expiry"`
- **THEN** the TUI and web dashboards render that source's latest value in that position

#### Scenario: Custom pane title
- **WHEN** a cell is `{ id = "status-json", title = "Status" }`
- **THEN** both UIs render the pane titled "Status"

#### Scenario: Spacer spans columns
- **WHEN** a row is `[{ kind = "space", colspan = 2 }, "work-hours"]` in a 3-column layout
- **THEN** the first two column slots are empty and `work-hours` occupies the third

#### Scenario: Unknown source in grid rejected
- **WHEN** a layout row references source `nope`
- **THEN** startup fails naming the layout and the unknown source

#### Scenario: Group pane with labeled values
- **WHEN** a cell is `{ title = "ihor", table = [{ id = "vds-base1", label = "days left" }, { id = "vds-base1-balance", label = "balance" }] }`
- **THEN** both UIs render one pane titled "ihor" containing both values as table rows, each shown under its configured label

#### Scenario: Group pane with a bare id falls back to the id as its label
- **WHEN** a generalized pane cell's `table` contains the bare string `"vds-base1"` (no `label`) and that source declares no `title`
- **THEN** that value's row is labeled `"vds-base1"`

#### Scenario: Unknown source inside a group rejected
- **WHEN** a generalized pane cell's `main`, `secondary`, or `table` references source `nope`
- **THEN** startup fails naming the layout and the unknown source

#### Scenario: Group cell with empty ids rejected
- **WHEN** a generalized pane cell declares none of `main`, `secondary`, or `table`
- **THEN** startup fails naming the layout and the empty pane

#### Scenario: Pane combines main, secondary, and table sections
- **WHEN** a cell is `{ title = "Server", main = "cpu-load", secondary = ["mem-used", "disk-free"], table = [{ id = "vds-base1", label = "days left" }] }`
- **THEN** both UIs render one pane titled "Server" containing `cpu-load` as the main section, `mem-used` and `disk-free` as secondary members, and `vds-base1` as a table row, all inside the same pane

#### Scenario: Pane with only a main section
- **WHEN** a cell is `{ title = "CPU", main = "cpu-load" }`
- **THEN** both UIs render one pane titled "CPU" containing only `cpu-load`'s main-section rendering

#### Scenario: Title omitted falls back to main's own label
- **WHEN** a cell is `{ main = "cpu-load" }` with no `title`
- **THEN** both UIs render the pane titled with `cpu-load`'s own declared `title` (or its id if it has none), the same as a bare `"cpu-load"` cell

#### Scenario: Title omitted with no main renders no header text
- **WHEN** a cell is `{ secondary = ["disk-home", "disk-root"] }` with no `title` and no `main`
- **THEN** both UIs render the pane with no header text, containing the two secondary members

#### Scenario: Explicitly empty title rejected
- **WHEN** a generalized pane cell is `{ title = "", main = "cpu-load" }`
- **THEN** startup fails naming the layout and the empty title

#### Scenario: Static-text panel renders standalone content
- **WHEN** a cell is `{ title = "Links", format = "markdown", text = "- [GitHub](https://github.com)" }`
- **THEN** both UIs render one panel titled "Links" containing that content, with no backing source

#### Scenario: Static-text panel with no title renders no header text
- **WHEN** a cell is `{ text = "Just a note." }` with no `title`
- **THEN** both UIs render the panel with no header text, containing the note

#### Scenario: Static-text panel defaults to plain text format
- **WHEN** a cell is `{ text = "Plain note" }` with no `format`
- **THEN** both UIs render the content as plain text, the same default as a source with no declared `format`

#### Scenario: Empty static-text content rejected
- **WHEN** a cell is `{ title = "Links", text = "" }`
- **THEN** startup fails naming the layout and the empty text panel

#### Scenario: Static-text json format rejected
- **WHEN** a cell declares `format = "json"`
- **THEN** startup fails naming the unknown format

### Requirement: Value formats
A source MAY declare a `format` of `text` (default) or `markdown`; other values — including the removed `json` — MUST be rejected at startup (**BREAKING**: configs using `format = "json"` fail to load and must switch to `text` or `markdown`). UIs SHALL render markdown as HTML.

#### Scenario: Unknown format rejected
- **WHEN** a source declares `format = "rst"`
- **THEN** startup fails naming the unknown format

#### Scenario: Removed json format rejected
- **WHEN** a source declares `format = "json"`
- **THEN** startup fails naming the unknown format

#### Scenario: Markdown rendered as markup
- **WHEN** a markdown-format source yields a reading containing `# Heading`
- **THEN** the web UI renders it as an HTML heading
