## MODIFIED Requirements

### Requirement: Health status derived from fetch outcomes
The system SHALL maintain per-source health (healthy / failing / stale) derived from recent fetch outcomes and last-success age, using a staleness window derived from each source's own schedule rather than a single global setting.

For an interval-scheduled source, the staleness window is `2 × effective_interval()`: it reports stale once no successful fetch has landed within that window — i.e. it has missed its second expected call. For a cron-scheduled source, there is no fixed period to derive a window from: it reports stale immediately whenever it has not yet completed any successful fetch, and is not re-evaluated for staleness once it has succeeded at least once.

#### Scenario: Consecutive failures flip health
- **WHEN** a healthy source fails its configured number of consecutive fetches
- **THEN** its health becomes failing until a fetch succeeds

#### Scenario: Stale source detected
- **WHEN** no successful fetch has occurred within the source's schedule-derived staleness window
- **THEN** the source reports stale

#### Scenario: Interval-scheduled source goes stale after its second missed call
- **WHEN** an interval-scheduled source with `interval = "5m"` has had no successful fetch for more than 10 minutes
- **THEN** the source reports stale

#### Scenario: Interval-scheduled source within its window stays healthy
- **WHEN** an interval-scheduled source with `interval = "5m"` last succeeded 6 minutes ago
- **THEN** the source does not report stale (it has missed at most one expected call)

#### Scenario: Cron-scheduled source stale before its first successful run
- **WHEN** a cron-scheduled source has not yet completed any successful fetch
- **THEN** the source reports stale

#### Scenario: Cron-scheduled source healthy after its first success
- **WHEN** a cron-scheduled source has completed at least one successful fetch, regardless of how long ago
- **THEN** the source does not report stale on that basis alone
