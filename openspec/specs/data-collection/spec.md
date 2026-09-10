# data-collection Specification

## Purpose

Runs source fetches on schedule and keeps every attempt logged and health-checked, so users can trust both current values and the system's own reliability.

## Requirements
### Requirement: Per-source schedules
Each query source SHALL be fetched according to its configured schedule: either a fixed interval or a cron expression. Schedules are independent per source. An interval-scheduled query source whose fetch fails SHALL retry after its `retry_interval` (spec: source-configuration — Per-source fetch retry interval) instead of waiting the full `interval`; it SHALL keep retrying at `retry_interval` for as long as fetches keep failing, and resume waiting the normal `interval` as soon as a fetch succeeds. A cron-scheduled query source is unaffected by fetch outcome: it is always fetched once per cron occurrence, never retried early on failure. A stream source has no schedule and is ingested continuously (spec: data-collection — Stream collection).

When the collector starts (daemon startup), a query source's first fetch SHALL be scheduled from the timestamp of its most recent fetch log entry (any outcome — success or failure), not from the last success alone: if the source has no fetch log entry, it is due and is fetched immediately; if the most recent entry is a success, the first fetch SHALL be deferred until the remaining freshness window (`interval` minus the age of that success) elapses, or fetched immediately when already overdue; if the most recent entry is a failure, the first fetch SHALL be scheduled `retry_interval` after that failure, or fetched immediately when that time has already passed. A cron-scheduled source's startup behavior is unchanged: it always waits for its next absolute cron occurrence, never fetching immediately on start. A one-shot collection request (not the daemon) SHALL continue to fetch every query source immediately regardless of fetch-log history; one-shot runs do not open stream sources.

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

#### Scenario: Fresh source is not re-fetched on daemon startup
- **WHEN** an interval-scheduled source with `interval = "1h"` has its most recent fetch log entry as a success 5 minutes ago and the daemon (re)starts
- **THEN** the collector does not fetch it immediately; its next fetch happens roughly 55 minutes later, when the interval elapses

#### Scenario: Stale source is fetched immediately on daemon startup
- **WHEN** an interval-scheduled source's most recent fetch log entry is overdue (a success older than its `interval`, or a failure older than its `retry_interval`), or it has no fetch log entry at all, and the daemon (re)starts
- **THEN** the collector fetches it immediately, same as today's behavior

#### Scenario: Recently failed run retries on retry_interval after daemon startup
- **WHEN** an interval-scheduled source with `interval = "1h"` and `retry_interval = "5m"` has its most recent fetch log entry as a failure 1 minute ago and the daemon (re)starts
- **THEN** the collector does not fetch it immediately; its next fetch happens roughly 4 minutes later, not 59 minutes later and not immediately

#### Scenario: Cron source startup behavior is unchanged
- **WHEN** a cron-scheduled source's daemon (re)starts partway between two cron occurrences
- **THEN** it waits for the next cron occurrence rather than fetching immediately, regardless of the timestamp or outcome of its last run

#### Scenario: One-shot collection ignores freshness
- **WHEN** a one-shot collection run (not the daemon) is invoked for a source whose last fetch log entry is still fresh
- **THEN** the source is fetched immediately, same as today's behavior

#### Scenario: Stream source has no schedule
- **WHEN** a `stream` source is configured alongside interval-scheduled `query` sources and the daemon (re)starts
- **THEN** the stream is opened immediately without consulting fetch-log freshness, while each `query` source follows its own freshness-deferred first tick
### Requirement: Stream collection
The collector SHALL run each stream source's command as a long-lived process and ingest its stdout line by line while the process lives: every well-formed `jsonl` row (spec: source-configuration — JSONL row schema) produces one reading stamped with the row's `ts` (or arrival time) and applies the row's `threshold` override when present. Threshold overrides live in daemon memory only: they are forgotten when the daemon stops, and a restarted daemon colors with config-declared bands until a new row overrides them. When the process ends for any reason (exit, signal, spawn failure), the collector SHALL record the outcome in the fetch log and reopen the command after the source's `retry_interval`; a spawn failure or immediate exit counts as a failed attempt. Shutdown stops reopening after the current wait, mirroring interval sources.

#### Scenario: Lines ingested continuously
- **WHEN** a stream command prints one `jsonl` row every second for a minute
- **THEN** roughly 60 readings are recorded without any schedule tick firing

#### Scenario: Exited stream reopens on retry_interval
- **WHEN** a stream command exits after printing one row and the source declares `retry_interval = "10s"`
- **THEN** a fetch log entry records the exit and the command is reopened roughly 10 seconds later

#### Scenario: Failing stream command retries
- **WHEN** a stream command exits non-zero immediately on every start
- **THEN** each restart is spaced by `retry_interval` and each exit is logged as failed, without affecting other sources

#### Scenario: Override forgotten on restart
- **WHEN** the daemon restarts after a stream row overrode a source's bands
- **THEN** no fetch-log or reading replay restores the override; the source uses config bands until a new row arrives
### Requirement: Fetch attempts logged
Every fetch attempt SHALL be recorded with timestamp, duration, and error detail on failure. A fetch attempt's outcome (success or failure) SHALL be derivable from whether that entry's error detail is present, not stored as a separate field: a failed attempt SHALL always carry error detail, and a successful attempt SHALL never carry error detail. Each successfully ingested `jsonl` line from a `stream` source counts as one successful attempt; a malformed line counts as one failed attempt carrying the parse error. Every entry SHALL record its origin: `push` for values arriving via `POST /api/ingest` (spec: http-api — HTTP ingest endpoint), `poll` for values gathered by scheduled fetching. Rows written before the origin column existed read as `poll`.

#### Scenario: Failure recorded with cause
- **WHEN** a query source's command times out
- **THEN** a fetch log entry exists with error detail naming the timeout

#### Scenario: Success recorded with no error detail
- **WHEN** a fetch succeeds
- **THEN** its fetch log entry carries no error detail, distinguishing it from a failure

#### Scenario: Malformed stream line logged as failure
- **WHEN** a `stream` source emits a line that is not a valid `jsonl` row
- **THEN** no reading is recorded and a fetch log entry exists with error detail naming the parse failure, while later valid lines are still ingested

#### Scenario: Ingest logged with push origin
- **WHEN** a value arrives via `POST /api/ingest`
- **THEN** its fetch log entry carries origin `push` and no error detail

#### Scenario: Scheduled fetch logged with poll origin
- **WHEN** a value is gathered by a scheduled fetch
- **THEN** its fetch log entry carries origin `poll`
### Requirement: Health status derived from fetch outcomes
The system SHALL maintain per-source health (healthy / failing / stale) derived from recent fetch outcomes and last-success age, using a staleness window derived from each source's own schedule rather than a single global setting.

For an interval-scheduled source, the staleness window is `2 × effective_interval()`: it reports stale once no successful fetch has landed within that window — i.e. it has missed its second expected call. For a cron-scheduled source, there is no fixed period to derive a window from: it reports stale immediately whenever it has not yet completed any successful fetch, and is not re-evaluated for staleness once it has succeeded at least once. For a stream source, the staleness window is its `expected_interval`: it reports stale whenever no value has arrived within that window, and recovers as soon as a value arrives.

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

#### Scenario: Silent stream goes stale after its expected interval
- **WHEN** a stream source with `expected_interval = "1m"` has received no value for more than a minute
- **THEN** the source reports stale

#### Scenario: Stream recovers on the next value
- **WHEN** a stale stream source emits a valid `jsonl` row
- **THEN** the source stops reporting stale on that basis
### Requirement: Collector resilience
One source's failure MUST NOT stop collection of other sources or crash the process.

#### Scenario: Failing source does not block others
- **WHEN** one query source hangs until timeout while others are due, or one stream source's command exits repeatedly
- **THEN** other sources are still fetched on their schedules, and the exiting stream is reopened on its own `retry_interval` without affecting the rest
### Requirement: Setup gates first fetch
When a source declares a `setup` command, the collector SHALL run it before the source's first fetch attempt — for a stream source, before opening the stream. Success enables normal scheduled fetching (or stream ingest) for the daemon's lifetime. Failure SHALL be recorded as a failed entry in the fetch log (with the command's error output), mark the source failing via the existing health derivation, and skip the fetch; on each subsequent schedule tick the collector SHALL retry the setup command instead of fetching until it succeeds. A stream source whose setup keeps failing SHALL retry the setup on its `retry_interval` instead of opening the stream.

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

#### Scenario: Stream setup gates opening
- **WHEN** a stream source's setup command exits non-zero
- **THEN** no stream process is opened, a failed fetch-log entry records the setup error, and the setup is retried on the source's `retry_interval`
### Requirement: Ingested values reset interval schedules
A successfully ingested value SHALL count as a successful attempt for scheduling: for an interval-scheduled source it resets the interval wait exactly like a successful fetch (next tick `interval` after the ingest, retry backoff cleared), and daemon-startup freshness treats the ingest's log row like any other success. A cron-scheduled source SHALL be unaffected: ingest stores the reading and log row but never alters the cron occurrence computation. A `stream` source has no schedule to reset.

#### Scenario: Ingest defers next interval tick
- **WHEN** an interval-scheduled source with `interval = "1h"` receives an ingest
- **THEN** its next scheduled fetch happens roughly 1 hour after the ingest, not on the pre-ingest cadence

#### Scenario: Ingest clears retry backoff
- **WHEN** an interval-scheduled source that was retrying after failures receives an ingest
- **THEN** its next fetch waits the full `interval`, not `retry_interval`

#### Scenario: Cron cadence untouched by ingest
- **WHEN** a cron-scheduled source receives an ingest between occurrences
- **THEN** its next fetch still happens at the cron expression's next occurrence
