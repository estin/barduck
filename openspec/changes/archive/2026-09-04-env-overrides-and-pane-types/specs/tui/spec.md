## MODIFIED Requirements

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
