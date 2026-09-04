# web-ui Specification

## Purpose

Serves a browser dashboard from the daemon, rendering configured layouts with Tailwind-styled components so users can check status from any device.

## Requirements

### Requirement: Web UI served by daemon
The daemon SHALL serve the web dashboard from the configured listen address, rendering the configured layout with each referenced source's latest value and health.

#### Scenario: Dashboard loads in browser
- **WHEN** the daemon is running and a browser opens the configured address
- **THEN** the configured layout renders with current values

### Requirement: Layout-driven rendering
Panels shown in the web UI MUST come from the same config-declared grid layouts used by the TUI, rendered with CSS grid columns and spans; sources not placed in a layout MUST NOT appear on the dashboard by default.

#### Scenario: Config change reflected after restart
- **WHEN** a panel is added to the config layout and the daemon restarts
- **THEN** the new panel appears in the web UI at its configured row/column position

#### Scenario: Grid arrangement honored
- **WHEN** a layout defines two rows — three sources in the first, `{ id = "weekly-report", title = "This week" }` plus a 2-column spacer in the second
- **THEN** the web UI shows the first row as three equal columns and the second with the report panel in column 1 spanning one column and nothing in columns 2–3, under the pane title "This week"

### Requirement: Health visible at a glance
The web UI SHALL visually distinguish healthy, failing, and stale sources, using a health-first coloring priority. While a source is failing or stale, its panel's border, background, and value text render in health's color (`failing`→red, `stale`→yellow) regardless of any threshold band — a banded source's own band reading does not override an active health problem, since a stale or failing fetch means that reading is no longer trustworthy. Only when a source is healthy does its threshold band's color apply; a healthy source with no threshold bands gets no accent color at all — its panel renders in the plain neutral style, not a default green. Whenever a source is failing or stale it SHALL also show a plain, uncolored status label next to its panel, alongside whatever color that status contributes.

#### Scenario: Failing source styled distinctly
- **WHEN** a source's last fetch failed
- **THEN** its panel is rendered with a distinct failing (red) style, health-derived — overriding any threshold band the source might declare

#### Scenario: Unbanded failing source shows a plain label instead of color
- **WHEN** a source with no threshold bands has a failed last fetch
- **THEN** its panel renders in the red style (health-derived, since it has no bands) and additionally shows an uncolored "failing" label next to it

#### Scenario: Unbanded stale source shows a plain label instead of color
- **WHEN** a source with no threshold bands has gone stale
- **THEN** its panel renders in the yellow style (health-derived) and additionally shows an uncolored "stale" label next to it

#### Scenario: Healthy unbanded source has no accent color
- **WHEN** a source with no threshold bands is currently healthy
- **THEN** its panel renders with no accent color at all — not green, not any color — just the plain neutral style

#### Scenario: Health overrides a stale threshold-band reading
- **WHEN** a threshold-banded source's last known value fell in a band, but the source is now failing or stale
- **THEN** its panel renders in health's color (red or yellow), not the band's color, alongside the plain status label

### Requirement: Panels show last update time
Each panel SHALL show when its latest value was collected, formatted as a single coarse time unit — seconds, minutes, hours, or days — rounded down to that unit's boundary (e.g. "updated 12s ago", "updated 10m ago", "updated 3h ago", "updated 2d ago").

#### Scenario: Age displayed
- **WHEN** the dashboard renders a source whose latest reading is 12 seconds old
- **THEN** its panel shows "updated 12s ago"

#### Scenario: Age rounds down to minutes
- **WHEN** the dashboard renders a source whose latest reading is 629 seconds old
- **THEN** its panel shows "updated 10m ago"

#### Scenario: Age rounds down to hours
- **WHEN** the dashboard renders a source whose latest reading is over an hour old
- **THEN** its panel shows the age in whole hours (e.g. "updated 3h ago")

#### Scenario: Age rounds down to days
- **WHEN** the dashboard renders a source whose latest reading is over a day old
- **THEN** its panel shows the age in whole days (e.g. "updated 2d ago")

### Requirement: App version visible
The web dashboard SHALL display the application version.

#### Scenario: Version rendered
- **WHEN** the dashboard loads
- **THEN** the page shows `barduck v<version>`

### Requirement: Threshold band coloring
When a source declares threshold bands and its latest value falls in a band, the panel SHALL use that band's color instead of the health-derived color.

#### Scenario: Band color wins
- **WHEN** a healthy source has bands 60→green, 85→yellow, 100→red and reports `92`
- **THEN** its panel renders with the red style

### Requirement: Per-source log view linked from panels
Each web panel's time-ago text SHALL be a link to `/logs/<source>` opening in a new tab; the daemon SHALL serve that page showing the source's recent fetch log entries (timestamp, outcome, duration, gathered value, error). Requests for unknown sources MUST return a client error naming the unknown source.

#### Scenario: Log view opens from panel
- **WHEN** the user clicks the time-ago text on the `bank-balance` panel
- **THEN** a new tab shows recent fetch log entries for `bank-balance`

#### Scenario: Unknown source log view rejected
- **WHEN** `/logs/nope` is requested
- **THEN** the response is a client error naming the unknown source

### Requirement: Panel retrospective history bar
A web UI panel for a source that declares threshold bands SHALL render a horizontal history bar at the bottom of the panel, unless that source declares `show_history = false` (spec: source-configuration — per-source history bar visibility), made of one colored segment per recent reading for that source, ordered oldest (left) to newest (right). Each segment's color SHALL be the threshold band level (`green`, `yellow`, or `red`) that reading falls into, computed the same way as the panel's own band coloring. A panel for a source with no threshold bands MUST NOT render a history bar. A reading with a non-numeric value (no band) SHALL render as a neutral/empty segment rather than being omitted, so the bar's segment count and left-to-right order stay stable.

#### Scenario: Bar shows recent readings colored by band
- **WHEN** a threshold-banded source's last 5 readings were `40`, `70`, `95`, `72`, `55` against bands 60→green, 85→yellow, 100→red
- **THEN** the panel renders a 5-segment bar reading green, yellow, red, yellow, green from left to right

#### Scenario: No bands, no bar
- **WHEN** a source declares no threshold bands
- **THEN** its panel renders with no history bar

#### Scenario: Non-numeric reading shown as neutral segment
- **WHEN** one of the recent readings for a banded source is non-numeric
- **THEN** its position in the bar renders as a neutral/empty segment rather than shifting the other segments

#### Scenario: Source opts out of its history bar
- **WHEN** a threshold-banded source declares `show_history = false`
- **THEN** its panel renders with no history bar, even though it declares bands

### Requirement: Global connection health indicator
The web dashboard SHALL show a single, always-visible connection health indicator near the app title and version at the top of the page, reflecting whether the browser can currently reach the daemon. The browser SHALL be the initiator: on an interval, the page itself sends a ping request to the daemon and reacts to whether a timely response arrives, independent of the panel-data refresh mechanism, so the indicator keeps working even if panel refresh stalls. Before the first ping resolves, the indicator MUST show a neutral "checking" state rather than claiming online or offline.

#### Scenario: Server reachable
- **WHEN** the browser's ping to the daemon succeeds
- **THEN** the indicator shows an online state

#### Scenario: Server unreachable
- **WHEN** the browser's ping to the daemon fails or does not respond within a timeout
- **THEN** the indicator shows an offline state

#### Scenario: Initial state before first ping
- **WHEN** the dashboard page has just loaded and no ping has completed yet
- **THEN** the indicator shows a neutral "checking" state, not online or offline

#### Scenario: Recovery detected
- **WHEN** the indicator is showing offline and a subsequent ping succeeds
- **THEN** the indicator returns to the online state

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

### Requirement: Source summary strip
The web dashboard SHALL show a single summary strip directly under the page title, containing one chip per source that appears in a configured layout, ordered the same way panels are laid out — layouts in declaration order, then each layout's rows top-to-bottom and cells left-to-right. Each chip's color SHALL match the color its panel currently renders with (threshold band color when configured, else health-derived color). Clicking a chip SHALL navigate to and visually highlight that source's panel. The strip SHALL update on the same refresh cycle as the panel grid, so its colors never lag behind the panels'.

#### Scenario: Chips reflect panel colors in layout order
- **WHEN** a layout places four sources whose panels currently render green, red, green, and yellow
- **THEN** the summary strip shows four chips in that same order and coloring

#### Scenario: Clicking a chip focuses its panel
- **WHEN** the user clicks a chip
- **THEN** the page scrolls to that source's panel and the panel is visually highlighted as the current focus target

#### Scenario: Strip stays in sync with panel refresh
- **WHEN** a source's health or value changes and the panel grid refreshes to reflect it
- **THEN** that source's chip color updates in the same refresh, without a manual page reload

### Requirement: Consistent token-based visual theme
The web dashboard SHALL present a single, consistent design-token-driven visual theme across the page — title, connection indicator, source summary strip, panels (including the history bar), and the per-source log view — instead of one-off, unrelated utility classes per element. Existing health/threshold status colors (green/yellow/red, per the "Threshold band coloring" and "Health visible at a glance" requirements) MUST render identically to before this change.

#### Scenario: Panel keeps its status color under the new theme
- **WHEN** a source's panel would have rendered with the red style before this change (failing health or a red threshold band)
- **THEN** it still renders with the red style after adopting the new component structure

#### Scenario: Log view matches the themed page
- **WHEN** the per-source log view is opened
- **THEN** it uses the same design tokens (borders, text, background) as the rest of the themed dashboard, not the previous unrelated slate-color classes

### Requirement: Light/dark theme toggle
The web dashboard SHALL provide a toggle control that switches the page between its existing light and dark design-token themes. On first visit, with no stored preference, the page SHALL apply the theme matching the browser's `prefers-color-scheme`. Toggling SHALL set an explicit preference, persisted across visits, that overrides `prefers-color-scheme` from then on. The chosen theme MUST be applied before the page's first paint, so no flash of the other theme is visible.

#### Scenario: First visit follows OS preference
- **WHEN** the browser's OS-level color scheme is dark and no theme preference has been stored yet
- **THEN** the dashboard renders in the dark theme on first load

#### Scenario: Toggle switches theme
- **WHEN** the user clicks the theme toggle
- **THEN** the dashboard immediately switches from its current theme to the other one

#### Scenario: Explicit choice persists across reloads
- **WHEN** the user has toggled to dark and then reloads the page
- **THEN** the dashboard renders in dark on reload, even if the OS-level preference is light

#### Scenario: Stored preference overrides OS preference
- **WHEN** a light preference is stored and the OS-level color scheme is dark
- **THEN** the dashboard renders in light, honoring the stored preference over the OS setting

#### Scenario: No flash of the wrong theme
- **WHEN** the stored (or OS-derived) theme is dark
- **THEN** the page's first rendered frame is already dark, not a light flash that then switches to dark

### Requirement: Panel title rendered on the card border
Each panel/pane card SHALL render its configured title embedded in the card's own top border, aligned to the top-left with padding on either side of the title text — the same convention the TUI already uses for a bordered panel's title — instead of as a separate header row above the panel's content. The border line SHALL pass through the vertical center of the title text (the way the TUI's own bordered-box title sits on its border line), not above or below it.

#### Scenario: Single-source panel title on the border
- **WHEN** a layout cell is a plain source reference or a `{ id, title }` override
- **THEN** its card's title renders on the card's own top border, top-left, not in a separate header row above the content

#### Scenario: Generalized pane title on the border
- **WHEN** a layout cell is a generalized pane combining `main`/`secondary`/`table` sections (spec: source-configuration — UI layouts are config-declared like sources)
- **THEN** its card's title renders the same way, on the card's own top border

#### Scenario: Untitled generalized pane shows no border title
- **WHEN** a generalized pane cell declares no `title` and no `main` (so it renders with no header text, spec: source-configuration — UI layouts are config-declared like sources)
- **THEN** its card's top border shows no title text, matching having no header text before this change

#### Scenario: Title vertically centered on the border line
- **WHEN** any panel/pane card renders its title on the border
- **THEN** the border line bisects the title text vertically, through its center, rather than sitting above or below it

### Requirement: Compact panel density
The panel grid's cards and rows, and the source summary strip's chips, SHALL use denser spacing and a smaller type scale than before this change, so more panels and chips fit on screen without scrolling, while every piece of information a panel showed before this change (value, unit, status label, "updated X ago" text, history bar) remains visible — this change reduces spacing only, it does not hide or remove any previously shown information.

#### Scenario: Summary strip chips are compact
- **WHEN** the source summary strip renders its chips
- **THEN** each chip uses a small pill shape with tight padding, noticeably more compact than before this change

#### Scenario: All previously shown information remains visible
- **WHEN** a panel that showed a value, unit, status label, age text, and history bar before this change is rendered after it
- **THEN** all of those same pieces of information are still shown, just in a denser layout

#### Scenario: More panels fit per screen
- **WHEN** the same layout is rendered before and after this change at the same viewport size
- **THEN** the panel grid after this change occupies less vertical space per panel than before

### Requirement: Responsive layout for small viewports
The web dashboard SHALL render usably on small (phone-width) viewports. The page SHALL declare a viewport meta tag so mobile browsers render it at device width instead of a zoomed-out desktop layout. The panel grid's column count SHALL reduce as the viewport narrows — down to a single column at phone widths — instead of forcing the layout's configured column count regardless of viewport width. No element SHALL force the page to scroll horizontally at any viewport width down to a small phone width (360px).

#### Scenario: Viewport meta tag present
- **WHEN** the dashboard page is loaded
- **THEN** its `<head>` declares a viewport meta tag setting the layout width to the device width

#### Scenario: Grid collapses to one column on a narrow viewport
- **WHEN** a 3-column layout is viewed at a phone-width viewport
- **THEN** the panel grid renders as a single column, each panel spanning the full available width

#### Scenario: Wide viewport keeps the configured column count
- **WHEN** the same 3-column layout is viewed at a desktop-width viewport
- **THEN** the panel grid renders with 3 columns, as configured

#### Scenario: No horizontal overflow at phone width
- **WHEN** the dashboard is viewed at a 360px-wide viewport
- **THEN** no element (grid, card, summary strip, header) causes the page to scroll horizontally
