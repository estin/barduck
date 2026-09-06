## ADDED Requirements

### Requirement: Hidden sources render as space in the TUI
When a layout cell (a bare source reference or `{ id, title }` cell) names a source whose `show_in` (spec: source-configuration — Per-source view visibility) excludes `"tui"`, the TUI SHALL render that cell as an empty space of the same column span instead of the source's panel, rather than failing startup. When a generalized pane cell's `main`, `secondary`, or `table` member names a source excluded from `"tui"`, the TUI SHALL omit that member from the pane's rendering; if omitting excluded members leaves the cell with none of `main`, `secondary`, or `table` populated for the TUI, the whole cell SHALL render as space. This does not change the layout's column count or row geometry.

#### Scenario: Cell hidden from the TUI renders as space
- **WHEN** a layout cell references a source declaring `show_in = "web"`
- **THEN** the TUI renders that grid position as an empty space, occupying the same span the panel would have used

#### Scenario: Cell visible in the TUI renders normally
- **WHEN** a layout cell references a source declaring `show_in = "tui"` (or `"all"`, or no `show_in`)
- **THEN** the TUI renders that source's panel as usual

#### Scenario: Hidden generalized pane member is omitted
- **WHEN** a generalized pane cell's `secondary` list includes a member whose source declares `show_in = "web"`, alongside other members visible in the TUI
- **THEN** the TUI renders the pane without that member, showing the remaining members normally

#### Scenario: Generalized pane cell with every member hidden renders as space
- **WHEN** a generalized pane cell's only members (across `main`, `secondary`, `table`) all declare `show_in = "web"`
- **THEN** the TUI renders that cell as an empty space

#### Scenario: Same layout renders differently per view
- **WHEN** a layout cell references a source declaring `show_in = "web"`
- **THEN** the TUI shows an empty space for that cell while the web dashboard shows the source's panel, from the same layout config
