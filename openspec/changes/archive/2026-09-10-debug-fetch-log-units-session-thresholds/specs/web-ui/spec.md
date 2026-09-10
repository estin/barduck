## MODIFIED Requirements

### Requirement: Per-source log view linked from panels
Each web panel's time-ago text SHALL be a link to `/logs/<source>`. The link SHALL open in the current tab, not a new tab. The daemon SHALL serve that page showing the source's recent fetch log entries: timestamp, duration, gathered value rendered together with the source's unit (when the source declares one), and error. The page SHALL include a link back to the dashboard. Requests for unknown sources MUST return a client error naming the unknown source.

#### Scenario: Log view opens from panel
- **WHEN** the user clicks the time-ago text on the `bank-balance` panel
- **THEN** the current tab shows recent fetch log entries for `bank-balance`

#### Scenario: Value renders with unit
- **WHEN** a source declaring `unit = "USD"` has a log entry with value `1480.42`
- **THEN** the entry's value cell shows the value together with `USD`, in the same form panels use

#### Scenario: Unitless value renders bare
- **WHEN** a source declaring no unit has a log entry
- **THEN** the entry's value cell shows the bare value, as before this change

#### Scenario: Back link returns to the dashboard
- **WHEN** the user clicks the back link on a source's log view
- **THEN** the browser goes back to the dashboard

#### Scenario: Unknown source log view rejected
- **WHEN** `/logs/nope` is requested
- **THEN** the response is a client error naming the unknown source
