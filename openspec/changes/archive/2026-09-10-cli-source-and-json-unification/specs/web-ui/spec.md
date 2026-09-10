## MODIFIED Requirements

### Requirement: Static-text panel rendering
A layout cell that is a static-text panel (`{ title?, format?, text }`, spec: source-configuration — UI layouts are config-declared like sources) SHALL render as its own card, titled on the card's own top border the same way any other panel is (spec: web-ui — panel title rendered on the card border), or with no title text when `title` is omitted. The card's content SHALL render `text` the same way a source's value renders for that `format` — markdown as HTML, otherwise as plain text (spec: web-ui — health visible at a glance covers the same format handling for a source's value). Since there is no backing source, the card MUST NOT show an "updated X ago" footer, a per-source log link, a history bar, or any health/threshold-derived styling.

#### Scenario: Static-text panel renders titled content
- **WHEN** a cell is `{ title = "Links", format = "markdown", text = "- [GitHub](https://github.com)" }`
- **THEN** the web UI renders a card titled "Links" on its top border, containing that markdown rendered as HTML

#### Scenario: Static-text panel with no title shows no header text
- **WHEN** a cell is `{ text = "Just a note." }` with no `title`
- **THEN** the web UI renders the card with no title on its border, containing the note as plain text

#### Scenario: Static-text panel has no footer, log link, or history bar
- **WHEN** a static-text panel cell is rendered
- **THEN** its card shows no "updated X ago" text, no link to a log view, and no history bar

#### Scenario: Static-text panel is never colored
- **WHEN** a static-text panel cell is rendered
- **THEN** its card's border and background render in the plain neutral style, never a health or threshold color
