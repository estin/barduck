# Spec Delta

## MODIFIED Requirements

### Requirement: Web UI served by daemon
The daemon SHALL serve the web dashboard from the configured listen address, rendering the configured layout with each referenced source's latest value and health. The panel grid SHALL re-render immediately whenever a new value is stored for any source shown on the page — whether the value arrived via a scheduled poll, a forced poll, or HTTP-ingested push — delivered over a server-sent refresh-event stream, without waiting for any timer and without a full page reload. A periodic fallback refresh SHALL also exist and fire only when no stream update arrived within its interval, so nothing goes stale on a dropped stream, and so age text and health states still update. The fallback interval SHALL be configurable (default 5 seconds).

#### Scenario: Dashboard loads in browser
- **WHEN** the daemon is running and a browser opens the configured address
- **THEN** the configured layout renders with current values

#### Scenario: Pushed value appears without waiting for the fallback
- **WHEN** an HTTP-ingested value is stored for a source while its dashboard is open in a browser tab
- **THEN** the new value appears in its panel without the user reloading and without waiting for the next fallback refresh

#### Scenario: Polled value appears without waiting for the fallback
- **WHEN** a scheduled or forced poll stores a new value while the dashboard is open
- **THEN** the new value appears in its panel without waiting for the next fallback refresh

#### Scenario: Fallback stays quiet while the stream is live
- **WHEN** stream updates keep arriving within the fallback interval
- **THEN** no extra periodic re-render fires for the fallback (it resets on each stream update)

#### Scenario: Fallback catches up after a dropped stream
- **WHEN** the refresh stream drops and a value is stored while it is down
- **THEN** the next fallback refresh still picks up the stored value, as before

#### Scenario: Fallback interval is configurable
- **WHEN** the operator sets the web refresh interval to a non-default value
- **THEN** the browser's fallback refresh uses that interval instead of the default

### Requirement: Log view live-refreshes without a full page reload
The log view SHALL re-render its table of fetch log entries through the same stream-driven immediate-refresh mechanism the dashboard's panel grid uses, plus the same fallback-only periodic timer. It needs no full page reload.

#### Scenario: New fetch attempt appears without reloading
- **WHEN** a source's fetch completes while its log view is open in a browser tab
- **THEN** the new entry appears in the table without the user reloading the page and without waiting for the next fallback refresh
