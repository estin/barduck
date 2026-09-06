## ADDED Requirements

### Requirement: Hidden sources render as space in the web dashboard
When a layout cell (a bare source reference or `{ id, title }` cell) names a source whose `show_in` (spec: source-configuration — Per-source view visibility) excludes `"web"`, the web UI SHALL render that grid position as an empty cell of the same column span instead of the source's card, rather than failing startup. When a generalized pane cell's `main`, `secondary`, or `table` member names a source excluded from `"web"`, the web UI SHALL omit that member from the pane's rendering; if omitting excluded members leaves the cell with none of `main`, `secondary`, or `table` populated for the web UI, the whole cell SHALL render as an empty grid position. This does not change the layout's column count or row geometry, and the hidden source's chip MUST NOT appear in the source summary strip for this view.

#### Scenario: Cell hidden from the web UI renders empty
- **WHEN** a layout cell references a source declaring `show_in = "tui"`
- **THEN** the web dashboard renders that grid position empty, occupying the same span the card would have used, and no chip for it appears in the summary strip

#### Scenario: Cell visible in the web UI renders normally
- **WHEN** a layout cell references a source declaring `show_in = "web"` (or `"all"`, or no `show_in`)
- **THEN** the web dashboard renders that source's card as usual, including its chip in the summary strip

#### Scenario: Hidden generalized pane member is omitted
- **WHEN** a generalized pane cell's `secondary` list includes a member whose source declares `show_in = "tui"`, alongside other members visible in the web UI
- **THEN** the web dashboard renders the pane without that member, showing the remaining members normally

#### Scenario: Generalized pane cell with every member hidden renders empty
- **WHEN** a generalized pane cell's only members (across `main`, `secondary`, `table`) all declare `show_in = "tui"`
- **THEN** the web dashboard renders that cell as an empty grid position

#### Scenario: Same layout renders differently per view
- **WHEN** a layout cell references a source declaring `show_in = "tui"`
- **THEN** the web dashboard shows that grid position empty while the TUI shows the source's panel, from the same layout config
