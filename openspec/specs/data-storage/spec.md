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
Each stored reading MUST include source name, value, unit if declared, and collection timestamp.

#### Scenario: Reading queryable by source and time range
- **WHEN** readings exist for multiple sources over several days
- **THEN** they can be queried filtered by source name and time range

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
