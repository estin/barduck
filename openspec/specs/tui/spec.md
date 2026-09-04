# tui Specification

## Purpose

Renders source values and health in the terminal as a live dashboard, using the same config-declared layouts as the web UI.

## Requirements

### Requirement: TUI renders configured layouts
The TUI SHALL display panels arranged according to the configured grid: each configured row renders as a horizontal strip of panels sized by their column spans relative to the layout's column count, spacer cells render as empty gaps spanning their columns, and panels show their custom pane title when one is configured. Panels show each referenced source's latest value and health status and refresh periodically.

#### Scenario: Panel shows latest value
- **WHEN** the TUI is open with a panel referencing `disk-usage-root`
- **THEN** that panel shows the latest recorded value for the source

#### Scenario: Grid arrangement honored
- **WHEN** a layout defines two rows — three sources in the first, one wide panel plus a 2-column spacer in the second
- **THEN** the TUI shows the first row split into three equal strips and the second row with the panel occupying one third and the remainder empty

#### Scenario: Custom title shown
- **WHEN** a cell configures `title = "Status"` for source `status-json`
- **THEN** the TUI panel header reads "Status"

### Requirement: TUI dual query modes
The TUI SHALL query the database directly by default and route queries through the daemon's HTTP API when daemon mode is enabled via flag or config.

#### Scenario: Daemon mode routes over HTTP
- **WHEN** the TUI is started with daemon mode enabled
- **THEN** its data comes from the daemon's HTTP API, not direct database access

### Requirement: Unreachable data source degrades gracefully
When the chosen data path is unavailable (database file missing, daemon down), the TUI MUST show an error state instead of crashing.

#### Scenario: Daemon down shows error panel
- **WHEN** daemon mode is enabled but the daemon is not running
- **THEN** the TUI displays a connection error and remains interactive

### Requirement: TUI shows app version and last update time
The TUI SHALL show the application version in a persistent header, and each panel SHALL show the age of its latest reading, formatted as a single coarse time unit — seconds, minutes, hours, or days — rounded down to that unit's boundary, matching the web UI's formatting.

#### Scenario: Header and age visible
- **WHEN** the TUI renders panels
- **THEN** the version appears in the header and each panel shows its value's update age

#### Scenario: Age rounds down to a coarser unit
- **WHEN** a panel's latest reading is 629 seconds old
- **THEN** the panel shows "10m ago" instead of a raw seconds count

### Requirement: Threshold band coloring
The TUI SHALL color a panel — border, title, and value — using a health-first priority. While a source is failing or stale, its panel renders in health's color (`failing`→red, `stale`→yellow) regardless of any threshold band — a banded source's own band reading does not override an active health problem, since a stale or failing fetch means that reading is no longer trustworthy. Only when a source is healthy does its threshold band's color (green, yellow, or red) apply; a healthy source with no threshold bands gets no accent color at all, rendering in the terminal's default color rather than a forced green. Whenever a source is failing or stale it SHALL also show a plain, uncolored status label next to the panel, alongside whatever color that status contributes.

#### Scenario: Band color wins over health
- **WHEN** a healthy source has bands 60→green, 85→yellow, 100→red and reports `92`
- **THEN** its panel (border, title, and value) renders in the red style

#### Scenario: No thresholds keeps health coloring
- **WHEN** a source declares no threshold bands and is currently healthy
- **THEN** its panel renders with no accent color at all — the terminal's default, not green

#### Scenario: No thresholds shows a plain label instead of color when failing
- **WHEN** a source declares no threshold bands and is currently `failing`
- **THEN** its panel renders in the red style (health-derived) and additionally shows an uncolored "failing" label

#### Scenario: No thresholds shows a plain label instead of color when stale
- **WHEN** a source declares no threshold bands and is currently `stale`
- **THEN** its panel renders in the yellow style (health-derived) and additionally shows an uncolored "stale" label

#### Scenario: Health overrides a stale threshold-band reading
- **WHEN** a threshold-banded source's last known value fell in a band, but the source is now failing or stale
- **THEN** its panel renders in health's color (red or yellow), not the band's color, alongside the plain status label

### Requirement: Group panes show multiple labeled, independently colored values
A layout cell using the generalized pane shape (`{ title, main?, secondary?, table? }`, spec: source-configuration — UI layouts are config-declared like sources) SHALL render as a single bordered panel titled with the cell's configured title, combining up to three sections in this order: `main`, then `secondary`, then `table`. The TUI has no history bars, so this distinction does not apply to any section.

**Main section**: when present, its single member renders exactly as a single-source panel would for that source — the value shown directly (no label prefix), health-first colored, with its age always shown as an inline suffix (spec: tui — TUI shows app version and last update time), and a plain status label when failing or stale.

**Secondary section**: when present, each of its members renders the same way as the main section — value shown directly, health-first colored, age always shown — but in the terminal's normal (non-emphasized) style rather than the main section's emphasized style, so it reads as visually secondary to `main`.

**Table section**: when present, each of its members renders one line exactly as today's group panel line: that value's configured label and its latest value and unit, colored independently — the same health-first coloring rule single-source panels use, applied per line — so one line can be red while another is green. A line's coloring is its value's text color only. A line that is currently failing or stale SHALL also show a plain, uncolored status label next to it. A line for a healthy source with no threshold bands gets no accent color at all. A line SHALL show the age of its latest reading as an inline suffix only when that source is lagging (its health status is `stale`); a line that is not stale shows no age suffix.

The panel's own border SHALL be colored by the worst color across every member in `main`, `secondary`, and `table` combined, ranked red > yellow > green, using the same health-first rule as each member's own color: a member failing or stale contributes red or yellow regardless of its band; a healthy member contributes its band color, or green if unbanded.

#### Scenario: Group panel renders one line per value
- **WHEN** a cell is `{ title = "ihor", table = [{ id = "vds-base1", label = "days left" }, { id = "vds-base1-balance", label = "balance" }] }`
- **THEN** the TUI renders one bordered panel titled with the cell's title, containing a "days left" line and a "balance" line, each showing that source's latest value and unit

#### Scenario: Lines colored independently
- **WHEN** a cell's `table` "days left" value is healthy and inside its red threshold band, and its "balance" value is healthy and inside its green threshold band
- **THEN** the "days left" line renders in the red style and the "balance" line renders in the green style, within the same panel

#### Scenario: Panel border reflects the worst line
- **WHEN** a cell's `table` lines currently render green, yellow, and red
- **THEN** the panel's own border renders in the red style

#### Scenario: Unbanded member's health counts toward the worst color
- **WHEN** a cell's `table` has one threshold-banded line currently green (healthy) and one unbanded line whose source is `failing`
- **THEN** the panel's own border renders in the red style, matching the failing member

#### Scenario: Unbanded line shows a plain label instead of color
- **WHEN** a cell's `table` "balance" line is for a source with no threshold bands and that source is currently `failing`
- **THEN** the "balance" line renders in the red style (health-derived) and additionally shows an uncolored "failing" label, matching the panel's own border which also renders red for that member

#### Scenario: Age shown for a lagging line
- **WHEN** a cell's `table` "balance" line is for a `stale` source whose latest reading is 629 seconds old
- **THEN** that line shows "10m ago" alongside its label and value

#### Scenario: Age hidden for a line that isn't lagging
- **WHEN** a cell's `table` "days left" line is for a source that is not `stale`
- **THEN** that line shows no age suffix, even though its reading has some age

#### Scenario: Healthy unbanded line has no accent color
- **WHEN** a cell's `table` "balance" line is for a source with no threshold bands and that source is currently healthy
- **THEN** that line renders with no accent color at all — the terminal's default, not green

#### Scenario: Health overrides a stale band reading in a group line
- **WHEN** a cell's `table` "days left" line is for a threshold-banded source whose last known value fell in its red band, but the source is now `stale`
- **THEN** that line renders in yellow (health-derived), not red, alongside the plain "stale" label

#### Scenario: Main section renders like a single-source panel
- **WHEN** a cell is `{ title = "Server", main = "cpu-load" }` and `cpu-load` is healthy and inside its yellow threshold band
- **THEN** the panel renders `cpu-load`'s value directly (no label) in the yellow style, with its age always shown

#### Scenario: Secondary member shown in normal style
- **WHEN** a cell is `{ title = "Server", main = "cpu-load", secondary = ["mem-used"] }`
- **THEN** `mem-used` renders below `cpu-load` in the terminal's normal (non-emphasized) style, value shown directly with its age always shown

#### Scenario: Combined panel renders all three sections
- **WHEN** a cell is `{ title = "Server", main = "cpu-load", secondary = ["mem-used", "disk-free"], table = [{ id = "vds-base1", label = "days left" }] }`
- **THEN** the panel renders `cpu-load` as the main section, `mem-used` and `disk-free` as secondary lines below it, and `vds-base1` as a "days left" table line below that, all within the same panel

#### Scenario: Unbanded failing member counts toward the worst color across sections
- **WHEN** a cell's main section is healthy (green) and a secondary member has no threshold bands but is currently `failing`
- **THEN** the panel's own border renders in the red style, matching the failing member

### Requirement: Configurable TUI content width
The system SHALL support a `tui_width` config setting controlling how wide the TUI's content area (header, error banner, and panel grid) is, instead of always stretching to the full terminal width. `tui_width` MUST be either the string `"auto"` (the default) or a positive integer number of terminal columns; any other value MUST be rejected at startup. In `"auto"` mode, the content width SHALL scale with the widest row's column count at a fixed comfortable width per column. A fixed integer SHALL cap the content width at that many columns. In both modes, the content width MUST NOT exceed the terminal's actual width, and the content area SHALL be horizontally centered when narrower than the terminal.

#### Scenario: Auto width stays narrow for few panels
- **WHEN** `tui_width` is `"auto"` (or unset) and the widest row has 2 columns, on a 220-column-wide terminal
- **THEN** the content area is narrower than the terminal and centered, not stretched to fill it

#### Scenario: Auto width uses more space for more panels
- **WHEN** `tui_width` is `"auto"` and the widest row has 6 columns, on the same terminal
- **THEN** the content area is wider than in the 2-column case, still capped at the terminal's width

#### Scenario: Fixed width caps the content area
- **WHEN** `tui_width = 100` on a 220-column-wide terminal
- **THEN** the content area is 100 columns wide and centered

#### Scenario: Fixed width never exceeds the terminal
- **WHEN** `tui_width = 300` on a 220-column-wide terminal
- **THEN** the content area is capped at 220 columns, not 300

#### Scenario: Invalid value rejected
- **WHEN** `tui_width` is `"wide"` or `0`
- **THEN** startup fails naming the invalid value
