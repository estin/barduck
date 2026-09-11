## Purpose

Defines the `ingest` source type for barduck: a push-based source that receives data via HTTP and reports stale when no data arrives within a configured window.

## ADDED Requirements

### Requirement: Ingest source declaration
The system SHALL support sources of type `ingest`: a push-based source that receives data via HTTP POST to the daemon's ingest endpoint. An `ingest` source MUST declare `expected_interval` (a humantime duration) and MUST NOT declare `command`, `interval`, or `cron`.

#### Scenario: Valid ingest source loads
- **WHEN** the config file contains an `ingest` source with `name`, `expected_interval`, and optional `title`
- **THEN** the source is registered and eligible for staleness-based health reporting

#### Scenario: Missing expected_interval rejected
- **WHEN** an `ingest` source declares no `expected_interval`
- **THEN** startup fails naming the source and the missing field

#### Scenario: Command rejected for ingest
- **WHEN** an `ingest` source declares a `command` field
- **THEN** startup fails naming the source and stating that ingest sources have no command

#### Scenario: Interval/cron rejected for ingest
- **WHEN** an `ingest` source declares `interval` or `cron`
- **THEN** startup fails naming the source and stating that ingest sources are push-based and take only `expected_interval`

### Requirement: Ingest source staleness
An `ingest` source reports `stale` health when no push has arrived within its `expected_interval` window. Each received push counts as a successful reading (like `stream` lines), so `last_ok_age` measures the silence between pushes.

#### Scenario: Push arrives within interval
- **WHEN** an `ingest` source with `expected_interval = "1m"` receives a push every 30 seconds
- **THEN** the source reports healthy

#### Scenario: Silence beyond interval marks stale
- **WHEN** an `ingest` source with `expected_interval = "1m"` receives no push for over a minute
- **THEN** the source reports stale

#### Scenario: Ingest source has no scheduled fetch
- **WHEN** the collector runs on a schedule tick
- **THEN** `ingest` sources are skipped entirely; their data arrives only via HTTP push

### Requirement: Ingest source config fields
An `ingest` source accepts the same optional fields as `stream`: `title`, `format`, `thresholds`, `history_points`, `show_history`, `show_in`, `value_type`. These fields behave identically to their `stream` counterparts.

#### Scenario: Optional fields accepted
- **WHEN** an `ingest` source declares `title`, `format = "markdown"`, and `show_in = "web"`
- **THEN** those fields are applied to the source's display and rendering

### Requirement: Ingest source type in enum
The `SourceType` enum SHALL include `Ingest` as a third variant alongside `Query` and `Stream`. The `SourceCfg` tagged enum SHALL include an `Ingest` variant. `SourceKind` SHALL include `Ingest`.

#### Scenario: Type string serialization
- **WHEN** an `ingest` source's type is serialized (e.g., in logs or CLI output)
- **THEN** it appears as `"ingest"` (lowercase, via `#[serde(rename_all = "lowercase")]`)
