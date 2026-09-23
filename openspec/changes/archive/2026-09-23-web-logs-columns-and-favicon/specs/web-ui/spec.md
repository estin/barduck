# Spec Delta

## MODIFIED Requirements

### Requirement: Per-source log view linked from panels
Each web panel's time-ago text SHALL be a link to `/logs/<source>`. The link SHALL open in the current tab, not a new tab. The daemon SHALL serve that page showing the source's recent fetch log entries as a table with columns, in this order: TIME, DURATION, SOURCE, VALUE. The SOURCE column SHALL show the attempt's origin (`push` for HTTP-ingested values, `poll` for scheduled fetches; spec: data-collection — Fetch attempts logged). The VALUE column SHALL show the gathered value rendered together with the source's unit (when the source declares one); for an entry that recorded an error, the VALUE cell SHALL instead show that error's text, wrapped, in place of a value — there is no separate error column. The page SHALL include a link back to the dashboard. Requests for unknown sources MUST return a client error naming the unknown source.

#### Scenario: Log view opens from panel
- **WHEN** the user clicks the time-ago text on the `bank-balance` panel
- **THEN** the current tab shows recent fetch log entries for `bank-balance`

#### Scenario: Value renders with unit
- **WHEN** a source declaring `unit = "USD"` has a log entry with value `1480.42`
- **THEN** the entry's VALUE cell shows the value together with `USD`, in the same form panels use

#### Scenario: Unitless value renders bare
- **WHEN** a source declaring no unit has a log entry
- **THEN** the entry's VALUE cell shows the bare value, as before this change

#### Scenario: Origin rendered
- **WHEN** the log view renders entries of both origins
- **THEN** each entry's SOURCE cell shows `push` or `poll`

#### Scenario: Error entry shows error text in the VALUE cell
- **WHEN** a fetch log entry recorded an error
- **THEN** its VALUE cell shows the wrapped error text instead of a value, and the row has no separate error cell

#### Scenario: Back link returns to the dashboard
- **WHEN** the user clicks the back link on a source's log view
- **THEN** the browser goes back to the dashboard

#### Scenario: Unknown source log view rejected
- **WHEN** `/logs/nope` is requested
- **THEN** the response is a client error naming the unknown source

### Requirement: Log view relative timestamps and threshold coloring
The log view SHALL show each entry's timestamp as relative time, at up to two units of precision, coarser unit first (e.g. `1d 6h`, `1h 12m`, `12m 3s`, `45s`). The second (finer) unit SHALL be omitted when it would be zero — e.g. an age of exactly one hour shows `1h`, not `1h 0m`. An age under one minute SHALL show seconds alone, since there is no finer unit. This is more precise than the single-unit format panels use for their own "updated X ago" text, and is otherwise unrelated to it. It SHALL NOT show a raw date and time string. Each entry's TIME cell SHALL carry the full stored timestamp as a native hover tooltip, so the exact time stays available on demand.

The log view SHALL color each entry's VALUE cell the same way panels do. If the source is failing or stale, the cell SHALL render in health's color, regardless of any threshold band. If the source is healthy, its threshold band color SHALL apply instead. A healthy source with no threshold bands, or a non-numeric value, SHALL render with no color. When an entry's VALUE cell shows error text instead of a value (spec: web-ui — Per-source log view linked from panels), it SHALL always render in the failing (red) color, regardless of the source's health or threshold bands.

The log view's table SHALL use narrow row spacing so more entries fit on screen without scrolling.

#### Scenario: Timestamp shows as relative time
- **WHEN** a fetch log entry was recorded exactly 2 minutes ago
- **THEN** its row's TIME cell shows `2m`

#### Scenario: Two units shown when both are non-zero
- **WHEN** a fetch log entry was recorded 1 day and 6 hours ago
- **THEN** its row's TIME cell shows `1d 6h`

#### Scenario: Zero-valued finer unit is omitted
- **WHEN** a fetch log entry was recorded exactly 1 hour ago
- **THEN** its row's TIME cell shows `1h`, not `1h 0m`

#### Scenario: Sub-minute age shows seconds only
- **WHEN** a fetch log entry was recorded 45 seconds ago
- **THEN** its row's TIME cell shows `45s`

#### Scenario: Hovering the relative time shows the exact timestamp
- **WHEN** the user hovers the TIME cell's relative-time text
- **THEN** the browser shows the full stored timestamp as a tooltip

#### Scenario: Value colored by threshold band
- **WHEN** a source has bands 60→green, 85→yellow, 100→red and an entry's value is `92`
- **THEN** that entry's VALUE cell renders with the red color

#### Scenario: Unbanded or non-numeric value has no color when healthy
- **WHEN** a source declares no threshold bands, or an entry's value is not a number, and the source is currently healthy
- **THEN** that entry's VALUE cell renders with no color

#### Scenario: Unbanded value colored by health when not healthy
- **WHEN** a source declares no threshold bands and its last fetch is currently failing
- **THEN** that entry's VALUE cell renders in the failing (red) color

#### Scenario: Error entry renders in red regardless of health or band
- **WHEN** an entry recorded an error, and its source is currently healthy with a green threshold band
- **THEN** that entry's VALUE cell renders the error text in the failing (red) color, not green

#### Scenario: Narrow rows fit more history on screen
- **WHEN** the log view renders many entries
- **THEN** its rows use narrower spacing than a standard table. More entries fit without scrolling

### Requirement: Global connection health indicator
The web dashboard SHALL show a single, always-visible connection health indicator near the app title and version at the top of the page, reflecting whether the browser can currently reach the daemon. The browser SHALL be the initiator: on an interval, the page itself sends a ping request to the daemon and reacts to whether a timely response arrives, independent of the panel-data refresh mechanism, so the indicator keeps working even if panel refresh stalls. Before the first ping resolves, the indicator MUST show a neutral "checking" state rather than claiming online or offline.

While the connection is offline, the dashboard SHALL additionally: render the browser-tab favicon in its red status color, overriding whatever health-derived color it would otherwise show; display a persistent, fixed-position banner stating the connection is lost; and visibly dim the main panel content to signal it may be stale. All three SHALL clear automatically, with no page reload, the instant the connection recovers.

On the dashboard, the favicon's health-derived color SHALL reflect the worst status across every source shown on the page. On a source's log view (`/logs/<source>`), the favicon SHALL instead reflect that one source's own status color — its threshold band color when healthy, or the failing/stale health color when it is not — falling back to green when the source is healthy and declares no threshold bands. The offline override still applies on the log view: while the connection is offline, its favicon SHALL also turn red, the same as the dashboard's.

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

#### Scenario: Favicon turns red when offline
- **WHEN** the connection goes offline
- **THEN** the browser-tab favicon renders in its red status color, regardless of the dashboard's last known health status

#### Scenario: Favicon resumes reflecting health after recovery
- **WHEN** the connection recovers after having been offline
- **THEN** the favicon returns to reflecting the dashboard's current health-derived color, not staying red

#### Scenario: Offline banner and dim shown
- **WHEN** the connection goes offline
- **THEN** a persistent banner stating the connection is lost appears, and the main panel content is visibly dimmed

#### Scenario: Offline banner and dim clear on recovery
- **WHEN** the connection recovers after having been offline
- **THEN** the banner disappears and the panel content returns to its normal appearance, without a page reload

#### Scenario: No banner or dim while checking or online
- **WHEN** the connection is in the initial "checking" state or is online
- **THEN** no offline banner is shown and the panel content is not dimmed

#### Scenario: Log view favicon reflects the source's own status
- **WHEN** a source's log view is open and that source is currently `failing`
- **THEN** the browser-tab favicon renders in the failing (red) color, regardless of any other source's status on the dashboard

#### Scenario: Log view favicon reflects a healthy banded source's color
- **WHEN** a source's log view is open, the source is healthy, and its latest value falls in its yellow threshold band
- **THEN** the browser-tab favicon renders in the yellow color

#### Scenario: Log view favicon falls back to green for an unbanded healthy source
- **WHEN** a source's log view is open, the source is healthy, and it declares no threshold bands
- **THEN** the browser-tab favicon renders in the green color

#### Scenario: Log view favicon still turns red when offline
- **WHEN** a source's log view is open and the connection goes offline
- **THEN** the browser-tab favicon renders in its red status color, overriding the source's own status color
