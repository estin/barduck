## ADDED Requirements

### Requirement: Forced polls are serialized with a source's schedule

A forced poll of a source (spec: http-api — Force poll endpoint; cli — Force poll command) performed by a running daemon SHALL be carried out by that source's own collector task, so it can never overlap with that source's scheduled fetch. While a fetch for a source is in flight, a forced poll for it SHALL wait rather than starting a second concurrent fetch of the same source.

A forced poll of one source MUST NOT delay any other source's collection.

After a forced poll of an interval-scheduled source completes, its next scheduled fetch SHALL be a full interval away from the forced attempt — `interval` after a successful one and `retry_interval` after a failed one — rather than firing at the time the pre-existing schedule had planned. A cron-scheduled source's next occurrence SHALL be unaffected by a forced poll, matching how cron schedules already ignore fetch outcome.

#### Scenario: Forced poll does not overlap a scheduled fetch

- **WHEN** a source's scheduled fetch is in flight and a forced poll for that source arrives
- **THEN** only one fetch of that source runs at a time, and the forced poll is carried out after the in-flight one finishes

#### Scenario: Forced poll leaves other sources alone

- **WHEN** one source is forced to poll while another source's fetch is in flight
- **THEN** the other source's collection is unaffected

#### Scenario: Interval schedule restarts from the forced poll

- **WHEN** an interval source with `interval = "1h"` is force-polled successfully 5 minutes before its next scheduled fetch
- **THEN** its next scheduled fetch is one hour after the forced poll, not 5 minutes later

#### Scenario: Failed forced poll uses the retry interval

- **WHEN** an interval source's forced poll fails
- **THEN** its next scheduled attempt is `retry_interval` after the forced poll

#### Scenario: Cron schedule unaffected

- **WHEN** a cron-scheduled source is force-polled
- **THEN** its next fetch still occurs at the next cron occurrence

### Requirement: Poll-in-progress is visible

The system SHALL expose, per source, whether a fetch for it is currently running — covering both a scheduled tick and a forced poll, since both run the same command the same way. This is a live signal about an attempt in progress, not an outcome: it is independent of the source's last known health status (healthy, failing, or stale), and it carries no meaning once no fetch is running for that source. It SHALL be visible through the same query surface health is already read from (spec: http-api — Query endpoints), so every consumer — the web dashboard, the TUI, a direct daemon-mode query — sees the same signal without a separate endpoint.

#### Scenario: Visible during a scheduled fetch

- **WHEN** a source's regularly scheduled fetch is running
- **THEN** querying that source's status reports it as currently polling

#### Scenario: Visible during a forced poll

- **WHEN** a forced poll for a source is running
- **THEN** querying that source's status reports it as currently polling, the same as a scheduled fetch would

#### Scenario: Cleared once the fetch finishes

- **WHEN** a source's fetch — scheduled or forced — finishes, successfully or not
- **THEN** querying that source's status no longer reports it as polling

#### Scenario: Independent of health status

- **WHEN** a source with a healthy, failing, or stale last-known status is currently mid-fetch
- **THEN** it is reported as polling in every case, without its health status being reported as changed by that alone
