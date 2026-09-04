## MODIFIED Requirements

### Requirement: Group panes show multiple labeled, independently colored values
A layout cell using the generalized pane shape (`{ title, main?, secondary?, table? }`, spec: source-configuration — UI layouts are config-declared like sources) SHALL render as a single card titled with the cell's configured title, combining up to three sections in this order: `main`, then `secondary`, then `table`.

**Main section**: when present, its single member renders exactly as a single-source panel would for that source — full-size value, health-first colored border, background, and value text (spec: web-ui — health visible at a glance, threshold band coloring), an always-shown "updated X ago" age, a plain status label when failing or stale, and its own history bar when its source declares threshold bands and doesn't opt out via `show_history` (spec: web-ui — panel retrospective history bar).

**Secondary section**: when present, its members render together in one shared row, each as a plain colored value and unit linked to `/logs/<source>` for that member's source — no separate label, no age, and no history bar regardless of the source's threshold bands or `show_history` setting. A member's coloring follows the same health-first rule as any other section. When a secondary member's source is currently `failing`, its link's text is the plain word `FAILING` instead of the (untrustworthy) fetched value, still colored and still linking to that source's log view.

**Table section**: when present, each of its members renders one row: that value's configured label and its latest value and unit, colored independently — the same health-first coloring rule single-source panels use, applied per row — so one row can be red while another is green. A row's coloring is its value's text color only — the row's label is not colored, and a table row carries no border or background color of its own. A row that is currently failing or stale SHALL also show a plain, uncolored status label next to it, alongside its color. A row for a healthy source with no threshold bands gets no accent color at all. A row's label SHALL be a link to `/logs/<source>` for that row's source. A row SHALL show its "updated X ago" age text only when that source is lagging (its health status is `stale`); a row that is not stale shows no age text at all, keeping fresh rows compact. A row whose source declares threshold bands SHALL render its own history-bar preview below the row, honoring `show_history`; a row whose source declares no threshold bands renders no history bar.

The card's own border — not its background — SHALL be colored by the worst color across every member in `main`, `secondary`, and `table` combined, ranked red > yellow > green, using the same health-first rule as each member's own color: a member failing or stale contributes red or yellow regardless of its band; a healthy member contributes its band color, or green if unbanded. The card's background stays neutral regardless of its members' colors.

#### Scenario: Group pane renders one row per value
- **WHEN** a cell is `{ title = "ihor", table = [{ id = "vds-base1", label = "days left" }, { id = "vds-base1-balance", label = "balance" }] }`
- **THEN** the web UI renders one card titled with the cell's title, containing a "days left" row and a "balance" row, each showing that source's latest value and unit

#### Scenario: Rows colored independently
- **WHEN** a cell's `table` "days left" value is healthy and inside its red threshold band, and its "balance" value is healthy and inside its green threshold band
- **THEN** the "days left" row renders in the red style and the "balance" row renders in the green style, within the same card

#### Scenario: Card border reflects the worst row
- **WHEN** a cell's `table` rows currently render green, yellow, and red
- **THEN** the card's own border renders in the red style, and its background stays neutral

#### Scenario: Unbanded member's health counts toward the worst color
- **WHEN** a cell's `table` has one threshold-banded row currently green (healthy) and one unbanded row whose source is `failing`
- **THEN** the card's own border renders in the red style, matching the failing member

#### Scenario: Only the row's value carries color
- **WHEN** a cell's `table` "days left" row is healthy and inside its red threshold band
- **THEN** that row's value renders in red text, its label renders uncolored, and neither carries a border or background color

#### Scenario: Unbanded row shows a plain label instead of color
- **WHEN** a cell's `table` "balance" row is for a source with no threshold bands and that source is currently `failing`
- **THEN** the "balance" row's value renders in the red style (health-derived) and additionally shows an uncolored "failing" label, matching the card's own border which also renders red for that member

#### Scenario: Row label links to its own source's log view
- **WHEN** a cell's `table` "balance" row is for source `vds-base1-balance`
- **THEN** that row's "balance" label links to `/logs/vds-base1-balance`

#### Scenario: Age hidden for a row that isn't lagging
- **WHEN** a cell's `table` "balance" row was just collected (not `stale`)
- **THEN** that row shows no "updated X ago" text

#### Scenario: Age shown for a lagging row
- **WHEN** a cell's `table` "days left" row's source has gone `stale`
- **THEN** that row shows its "updated X ago" age

#### Scenario: Row shows its own history bar
- **WHEN** a cell's `table` "days left" row is healthy, for a threshold-banded source with recent readings
- **THEN** that row renders its own colored history bar beneath it, and a row for an unbanded member in the same pane renders no bar

#### Scenario: Row honors its own history-bar opt-out
- **WHEN** a cell's `table` "days left" row is healthy, for a threshold-banded source that declares `show_history = false`
- **THEN** that row renders no history bar, even though its source declares bands

#### Scenario: Healthy unbanded row has no accent color
- **WHEN** a cell's `table` "balance" row is for a source with no threshold bands and that source is currently healthy
- **THEN** that row's value renders with no accent color at all — not green, not any color

#### Scenario: Health overrides a stale band reading in a group row
- **WHEN** a cell's `table` "days left" row is for a threshold-banded source whose last known value fell in its red band, but the source is now `stale`
- **THEN** that row renders in yellow (health-derived), not red, alongside the plain "stale" label

#### Scenario: Main section renders like a single-source panel
- **WHEN** a cell is `{ title = "Server", main = "cpu-load" }` and `cpu-load` is healthy and inside its yellow threshold band
- **THEN** the card renders `cpu-load`'s value in the yellow style, with a colored border and background, its always-shown "updated X ago" age, and its own history bar if it declares threshold bands

#### Scenario: Secondary member renders as a plain colored value linked to its log view
- **WHEN** a cell is `{ title = "Server", main = "cpu-load", secondary = ["mem-used"] }` and `mem-used` is healthy and inside its yellow threshold band
- **THEN** `mem-used` renders as its value and unit, colored yellow, linking to `/logs/mem-used` — no separate label, no age, and no history bar even though it declares threshold bands

#### Scenario: Multiple secondary members render in one row
- **WHEN** a cell's `secondary` lists two or more members
- **THEN** all of them render together inside one shared row, not stacked as separate table-style rows

#### Scenario: Failing secondary member shows FAILING text instead of its value
- **WHEN** a cell's `secondary` includes a member whose source is currently `failing`
- **THEN** that member's link shows the plain text `FAILING`, colored red, still linking to that source's `/logs/<source>` view

#### Scenario: Combined pane renders all three sections
- **WHEN** a cell is `{ title = "Server", main = "cpu-load", secondary = ["mem-used", "disk-free"], table = [{ id = "vds-base1", label = "days left" }] }`
- **THEN** the card renders `cpu-load` as the main section, `mem-used` and `disk-free` together in the secondary row below it, and `vds-base1` as a table row below that, all within the same card

#### Scenario: Unbanded failing member counts toward the worst color across sections
- **WHEN** a cell's main section is healthy (green) and a secondary member has no threshold bands but is currently `failing`
- **THEN** the card's own border renders in the red style, matching the failing member
