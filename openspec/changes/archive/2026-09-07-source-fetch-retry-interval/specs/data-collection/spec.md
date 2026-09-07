## MODIFIED Requirements

### Requirement: Per-source schedules
Each source SHALL be fetched according to its configured schedule: either a fixed interval or a cron expression. Schedules are independent per source. An interval-scheduled source whose fetch fails SHALL retry after its `retry_interval` (spec: source-configuration — Per-source fetch retry interval) instead of waiting the full `interval`; it SHALL keep retrying at `retry_interval` for as long as fetches keep failing, and resume waiting the normal `interval` as soon as a fetch succeeds. A cron-scheduled source is unaffected by fetch outcome: it is always fetched once per cron occurrence, never retried early on failure.

#### Scenario: Interval respected
- **WHEN** a source has a 5-minute interval and the collector runs for 15 minutes
- **THEN** that source is fetched approximately 3 times (±1)

#### Scenario: Cron schedule respected
- **WHEN** a source declares `cron = "0 */10 * * * *"` (every 10 minutes) and the collector runs for 30 minutes
- **THEN** that source is fetched at each 10-minute cron occurrence, approximately 3 times (±1)

#### Scenario: Failed fetch retries sooner than the full interval
- **WHEN** an interval-scheduled source with `interval = "30m"` and `retry_interval = "10s"` fails a fetch
- **THEN** its next fetch attempt happens 10 seconds later, not 30 minutes later

#### Scenario: Repeated failures keep retrying at retry_interval
- **WHEN** an interval-scheduled source keeps failing across several consecutive attempts
- **THEN** each attempt after the first failure is spaced by `retry_interval`, not the full `interval`

#### Scenario: Recovery resumes the normal interval
- **WHEN** an interval-scheduled source that was retrying after failures has a fetch succeed
- **THEN** its next fetch attempt waits the full `interval` again, not `retry_interval`

#### Scenario: Cron-scheduled source ignores fetch outcome
- **WHEN** a cron-scheduled source's fetch fails
- **THEN** its next fetch attempt still happens at the cron expression's next occurrence, not sooner

#### Scenario: Setup-command retries are unaffected
- **WHEN** a source's setup command fails
- **THEN** the collector retries the setup command on the source's normal schedule tick, not `retry_interval` (spec: data-collection — Setup gates first fetch)
