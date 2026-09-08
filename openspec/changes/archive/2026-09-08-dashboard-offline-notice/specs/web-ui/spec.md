## MODIFIED Requirements

### Requirement: Global connection health indicator
The web dashboard SHALL show a single, always-visible connection health indicator near the app title and version at the top of the page, reflecting whether the browser can currently reach the daemon. The browser SHALL be the initiator: on an interval, the page itself sends a ping request to the daemon and reacts to whether a timely response arrives, independent of the panel-data refresh mechanism, so the indicator keeps working even if panel refresh stalls. Before the first ping resolves, the indicator MUST show a neutral "checking" state rather than claiming online or offline.

While the connection is offline, the dashboard SHALL additionally: render the browser-tab favicon in its red status color, overriding whatever health-derived color it would otherwise show; display a persistent, fixed-position banner stating the connection is lost; and visibly dim the main panel content to signal it may be stale. All three SHALL clear automatically, with no page reload, the instant the connection recovers.

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
