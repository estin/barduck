# data-collection Specification

## Purpose

Runs source fetches on schedule and keeps every attempt logged and health-checked, so users can trust both current values and the system's own reliability.

## Requirements

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

### Requirement: Fetch attempts logged
Every fetch attempt SHALL be recorded with timestamp, duration, outcome (success/failure), and error detail on failure.

#### Scenario: Failure recorded with cause
- **WHEN** an http source times out
- **THEN** a fetch log entry exists with failure status and timeout detail

### Requirement: Health status derived from fetch outcomes
The system SHALL maintain per-source health (healthy / failing / stale) derived from recent fetch outcomes and last-success age.

#### Scenario: Consecutive failures flip health
- **WHEN** a healthy source fails its configured number of consecutive fetches
- **THEN** its health becomes failing until a fetch succeeds

#### Scenario: Stale source detected
- **WHEN** no successful fetch occurred within a configured staleness window
- **THEN** the source reports stale

### Requirement: Collector resilience
One source's failure MUST NOT stop collection of other sources or crash the process.

#### Scenario: Failing source does not block others
- **WHEN** one script source hangs until timeout while others are due
- **THEN** other sources are still fetched on their schedules

### Requirement: Setup gates first fetch
When a source declares a `setup` command, the collector SHALL run it before the source's first fetch attempt. Success enables normal scheduled fetching for the daemon's lifetime. Failure SHALL be recorded as a failed entry in the fetch log (with the command's error output), mark the source failing via the existing health derivation, and skip the fetch; on each subsequent schedule tick the collector SHALL retry the setup command instead of fetching until it succeeds.

#### Scenario: Successful setup enables collection
- **WHEN** a source's setup command exits zero
- **THEN** the source begins fetching on its schedule and produces readings

#### Scenario: Failed setup fails the source
- **WHEN** a source's setup command exits non-zero or times out
- **THEN** a failed fetch-log entry records the setup error, the source reports failing, and no reading is fetched

#### Scenario: Late-starting dependency recovers
- **WHEN** the setup target (a service or tunnel) becomes available after several failed setup retries
- **THEN** the next scheduled tick succeeds, the source turns healthy, and fetching begins without a daemon restart

#### Scenario: Setup never re-runs after success
- **WHEN** a source whose setup succeeded keeps collecting across many schedule ticks
- **THEN** the setup command does not execute again
