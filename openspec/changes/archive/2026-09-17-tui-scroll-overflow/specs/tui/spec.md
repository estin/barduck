## ADDED Requirements

### Requirement: TUI scrolls when content overflows the terminal
The TUI SHALL compute each row's natural (content-driven) minimum height. When every row's natural height fits within the height available for the panel grid, rendering is unchanged from today — rows are stretched to fill the available height exactly as before. When the sum of natural row heights exceeds the available height, the TUI MUST NOT shrink rows below their natural height to force everything onto one screen; instead it SHALL show a contiguous, scrollable slice of rows at their natural height and let the user move that slice with the keyboard: `↑`/`k` and `↓`/`j` scroll by one row, `PgUp`/`PgDn` scroll by one full screen, and `Home`/`g` and `End`/`G` jump to the first and last row. Scrolling MUST NOT scroll past the first row or past the point where the last row is fully visible. While any content is scrolled out of view, the TUI SHALL show a visible scroll indicator (e.g. a scrollbar) reflecting the current position within the full content; when every row already fits on screen, no indicator is shown and the scroll keys have no effect.

#### Scenario: All rows fit — behavior unchanged
- **WHEN** the configured layouts produce rows whose natural heights together fit the current terminal height
- **THEN** rows stretch to fill the available height as before, no scroll indicator is shown, and the scroll keys have no visible effect

#### Scenario: More rows than fit — scrolled instead of squeezed
- **WHEN** the configured layouts produce more rows than fit the current terminal height
- **THEN** the TUI shows as many full-height rows as fit, starting from the top, rather than shrinking every row to force them all on screen

#### Scenario: Line scrolling moves by one row
- **WHEN** content overflows and the user presses `↓` (or `j`)
- **THEN** the visible slice moves down by one row, revealing the next row that was previously off-screen

#### Scenario: Page scrolling moves by one screen
- **WHEN** content overflows and the user presses `PgDn`
- **THEN** the visible slice advances by roughly one screen's worth of rows

#### Scenario: Scrolling clamps at the top
- **WHEN** the visible slice is already showing the first row and the user scrolls up
- **THEN** the visible slice does not move past the first row

#### Scenario: Scrolling clamps at the bottom
- **WHEN** the visible slice already shows the last row fully and the user scrolls down
- **THEN** the visible slice does not move further, leaving the last row fully visible with no trailing blank overscroll

#### Scenario: Jump to start and end
- **WHEN** content overflows and the user presses `End` (or `G`)
- **THEN** the view jumps directly to show the last row; pressing `Home` (or `g`) afterward jumps back to the first row

#### Scenario: Scroll indicator reflects position
- **WHEN** content overflows and the user has scrolled partway through
- **THEN** the TUI shows a scroll indicator whose position reflects how far through the content the current view is
