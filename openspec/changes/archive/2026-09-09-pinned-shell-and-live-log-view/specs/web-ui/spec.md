## MODIFIED Requirements

### Requirement: Per-source log view linked from panels
Each web panel's time-ago text SHALL be a link to `/logs/<source>`. The link SHALL open in the current tab, not a new tab. The daemon SHALL serve that page showing the source's recent fetch log entries: timestamp, duration, gathered value, and error. The page SHALL include a link back to the dashboard. Requests for unknown sources MUST return a client error naming the unknown source.

#### Scenario: Log view opens from panel
- **WHEN** the user clicks the time-ago text on the `bank-balance` panel
- **THEN** the current tab shows recent fetch log entries for `bank-balance`

#### Scenario: Back link returns to the dashboard
- **WHEN** the user clicks the back link on a source's log view
- **THEN** the browser goes back to the dashboard

#### Scenario: Unknown source log view rejected
- **WHEN** `/logs/nope` is requested
- **THEN** the response is a client error naming the unknown source

### Requirement: Log view relative timestamps and threshold coloring
The log view SHALL show each entry's timestamp as relative time. It SHALL use the same format panels already use for their own "updated X ago" text. It SHALL NOT show a raw date and time string. Each entry's TIME cell SHALL carry the full stored timestamp as a native hover tooltip, so the exact time stays available on demand.

The log view SHALL color each entry's value cell the same way panels do. If the source is failing or stale, the cell SHALL render in health's color, regardless of any threshold band. If the source is healthy, its threshold band color SHALL apply instead. A healthy source with no threshold bands, or a non-numeric value, SHALL render with no color.

The log view's table SHALL use narrow row spacing so more entries fit on screen without scrolling.

#### Scenario: Timestamp shows as relative time
- **WHEN** a fetch log entry was recorded 2 minutes ago
- **THEN** its row shows "2m ago" instead of a raw timestamp

#### Scenario: Hovering the relative time shows the exact timestamp
- **WHEN** the user hovers the TIME cell's relative-time text
- **THEN** the browser shows the full stored timestamp as a tooltip

#### Scenario: Value colored by threshold band
- **WHEN** a source has bands 60→green, 85→yellow, 100→red and an entry's value is `92`
- **THEN** that entry's value cell renders with the red color

#### Scenario: Unbanded or non-numeric value has no color when healthy
- **WHEN** a source declares no threshold bands, or an entry's value is not a number, and the source is currently healthy
- **THEN** that entry's value cell renders with no color

#### Scenario: Unbanded value colored by health when not healthy
- **WHEN** a source declares no threshold bands and its last fetch is currently failing
- **THEN** that entry's value cell renders in the failing (red) color

#### Scenario: Narrow rows fit more history on screen
- **WHEN** the log view renders many entries
- **THEN** its rows use narrower spacing than a standard table. More entries fit without scrolling

## ADDED Requirements

### Requirement: Log view live-refreshes without a full page reload
The log view SHALL re-render its table of fetch log entries on a periodic timer. This is the same way the dashboard's panel grid already refreshes, and it needs no full page reload. A new fetch attempt recorded after the page loads SHALL appear within one refresh cycle.

#### Scenario: New fetch attempt appears without reloading
- **WHEN** a source's fetch completes while its log view is open in a browser tab
- **THEN** the new entry appears in the table without the user reloading the page

### Requirement: Header and footer stay pinned across pages
The dashboard and the log view SHALL share one header (title, version, connection indicator, and theme toggle) and one footer. The header SHALL stay visible at the top of the viewport on both pages. It stays there as the page's own content scrolls beneath it. The footer SHALL stay visible at the bottom of the viewport the same way, on both pages. Neither SHALL permanently cover any content: each page SHALL have enough top and bottom spacing to clear both.

#### Scenario: Header stays visible while scrolling the dashboard
- **WHEN** the panel grid is taller than the viewport and the user scrolls down
- **THEN** the header stays visible at the top of the viewport

#### Scenario: Footer stays visible while scrolling the dashboard
- **WHEN** the panel grid is taller than the viewport and the user scrolls down
- **THEN** the footer stays visible at the bottom of the viewport

#### Scenario: Panel content is not hidden behind the pinned header or footer
- **WHEN** the dashboard loads with the pinned header and footer in place
- **THEN** every scrolled-into-view panel is fully visible, not covered by the header or footer

#### Scenario: Log view shares the same pinned header and footer
- **WHEN** a source's log view is open
- **THEN** the same header and footer as the dashboard are visible, pinned the same way
