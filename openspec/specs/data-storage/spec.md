# data-storage Specification

## Purpose

Persists readings, fetch logs, and health events in a simple embedded OLAP database so history survives restarts and stays queryable with plain SQL.

## Requirements
### Requirement: Embedded single-file storage
All persistent data SHALL live in a single DuckDB database file whose location comes from config. No external database service may be required.

#### Scenario: Restart preserves history
- **WHEN** the daemon is stopped and started again
- **THEN** previously stored readings, fetch logs, and health events remain queryable

#### Scenario: Direct SQL access works while stopped
- **WHEN** no daemon is running and a user opens the database file with any DuckDB client
- **THEN** tables for readings, fetch logs, and health events can be queried directly
### Requirement: Readings persisted with provenance
Each stored reading MUST include source name, value, unit if declared, and collection timestamp. The timestamp is the moment the value was collected, unless the value arrived as a `jsonl` row carrying a valid `ts` (spec: source-configuration — JSONL row schema), in which case the row's `ts` is stored instead.

#### Scenario: Reading queryable by source and time range
- **WHEN** readings exist for multiple sources over several days
- **THEN** they can be queried filtered by source name and time range

#### Scenario: Row timestamp preserved
- **WHEN** a `jsonl` row carries `ts = "2026-09-09T12:00:00Z"`
- **THEN** the stored reading's timestamp equals that instant, not the arrival time
### Requirement: Fetch logs and health events persisted
Fetch log entries and health-status transitions SHALL be stored with timestamps, queryable through the same database.

#### Scenario: Health transition traceable
- **WHEN** a source flips from healthy to failing and back
- **THEN** both transitions are recorded with timestamps
### Requirement: Concurrent access safety
Direct CLI/TUI access and daemon writes SHALL not corrupt the database; concurrent readers must not block the daemon's writes indefinitely.

#### Scenario: Query during collection
- **WHEN** a direct-mode CLI query runs while the daemon writes new readings
- **THEN** both operations complete without corruption errors
### Requirement: Typed value columns
The `readings` table SHALL include nullable `value_bigint` (BIGINT), `value_double` (DOUBLE), and `value_json` (JSON) columns in addition to the existing `value` (VARCHAR) column. When a source declares a `value_type` (spec: source-configuration — Configurable stored value type) other than the default `string`, each of its readings SHALL have the matching typed column populated with that reading's typed representation, so it can be queried and used in SQL math/analytics directly, without re-parsing the string column. The `value` column SHALL still be populated for every reading regardless of `value_type`, so existing consumers of the string column are unaffected. No migration path is provided for a database file created before this change; such a file MUST be recreated before use with the new schema.

#### Scenario: Typed column populated alongside the string column
- **WHEN** a source with `value_type = "double"` records a reading
- **THEN** the row's `value` column holds the string form and its `value_double` column holds the parsed double, with `value_bigint` and `value_json` left `NULL`

#### Scenario: Default-typed source leaves new columns null
- **WHEN** a source with no `value_type` (or `value_type = "string"`) records a reading
- **THEN** the row's `value` column is populated as before and `value_bigint`, `value_double`, and `value_json` are all `NULL`
