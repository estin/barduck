## ADDED Requirements

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

## MODIFIED Requirements

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
