## ADDED Requirements

### Requirement: A composite root renders as a table panel

A layout cell that names a composite source directly (a bare source reference, `{ id, title }`, or a generalized pane's `main` member) SHALL render as a table panel listing every declared child's current value, in declared order — the same shape the generalized pane's `table` section already renders for a manually-listed group of sources (spec: tui — generalized pane rendering). The panel's title is the composite source's own `title`, or its name when none is declared. Each row's label, health coloring, and threshold-band coloring follow that child's own declared fields exactly as an individually-referenced child source would. A composite source's children remain individually referenceable in any existing layout cell position, unaffected by this auto-rendering.

#### Scenario: Bare layout cell naming a composite root

- **WHEN** a layout row contains the bare cell `"load"` for a composite source named `load` with children `1m`, `5m`, `15m`
- **THEN** the rendered panel is a table with one row per child, in declared order, each colored by that child's own health and thresholds

#### Scenario: Composite root referenced as a pane's main member

- **WHEN** a generalized pane cell sets `main = "load"` for a composite source
- **THEN** the panel renders the children table in place of a single scalar value

#### Scenario: A child referenced individually elsewhere still renders normally

- **WHEN** a layout also references `load::1m` directly in its own cell
- **THEN** that cell renders `load::1m` as an ordinary single-source panel, unaffected by the root's auto-rendered table appearing elsewhere
