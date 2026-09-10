## MODIFIED Requirements

### Requirement: Static-text panel rendering
A layout cell that is a static-text panel (`{ title?, format?, text }`, spec: source-configuration — UI layouts are config-declared like sources) SHALL render as its own bordered panel, titled with the cell's configured `title`, or with no title when omitted. The panel's content SHALL show `text` as-is (the TUI does not interpret `markdown` formatting — a `format` of `markdown` renders the same raw text a `text`-format value would). Since there is no backing source, the panel MUST NOT show an age suffix, and its border MUST always render in the terminal's default (unaccented) style, never a health or threshold color.

#### Scenario: Static-text panel renders titled content
- **WHEN** a cell is `{ title = "Links", text = "github.com" }`
- **THEN** the TUI renders a bordered panel titled "Links" containing "github.com" as plain text

#### Scenario: Static-text panel with no title shows no title
- **WHEN** a cell is `{ text = "Just a note." }` with no `title`
- **THEN** the TUI renders the panel with no title, containing the note

#### Scenario: Static-text panel content shown as-is regardless of format
- **WHEN** a cell is `{ text = "# Heading", format = "markdown" }`
- **THEN** the TUI shows the literal text `"# Heading"`, not an interpreted heading

#### Scenario: Static-text panel has no age suffix or accent color
- **WHEN** a static-text panel cell is rendered
- **THEN** its panel shows no age suffix and its border renders in the terminal's default style, never a health or threshold color
