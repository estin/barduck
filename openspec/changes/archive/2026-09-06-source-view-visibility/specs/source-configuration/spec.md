## ADDED Requirements

### Requirement: Per-source view visibility
A source MAY declare a `show_in` field controlling which UI(s) are allowed to display it: `"all"` (default, unchanged behavior), `"tui"` (visible only in the TUI), or `"web"` (visible only in the web dashboard). Any other value MUST be rejected at startup, naming the source and the invalid value. `show_in` SHALL have no effect on data collection: the source is fetched on its configured schedule regardless of its value.

#### Scenario: Default is visible everywhere
- **WHEN** a source declares no `show_in` field
- **THEN** it is eligible to display in both the TUI and the web dashboard, as before this change

#### Scenario: Restricting to one view hides it from the other
- **WHEN** a source declares `show_in = "tui"` and is referenced by a layout cell
- **THEN** the source's panel appears in the TUI but not on the web dashboard, even though both UIs share the same layout config

#### Scenario: Invalid value rejected
- **WHEN** a source declares `show_in = "cli"`
- **THEN** startup fails naming the source and the invalid value

#### Scenario: Collection unaffected by view restriction
- **WHEN** a source declares `show_in = "web"`
- **THEN** the source is still fetched on its configured schedule, even though it never displays in the TUI
