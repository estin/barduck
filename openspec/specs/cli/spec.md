# cli Specification

## Purpose

Shows results and health from the terminal in a scriptable, non-interactive form, usable both against the database file directly and against a running daemon.

## Requirements

### Requirement: Query commands
The CLI SHALL provide commands to print latest values per source, reading history for a source with time-range filters, per-source health, and recent fetch logs. Output SHALL be human-readable tables by default and JSON with `--json`.

#### Scenario: Latest values printed
- **WHEN** the user runs the latest-values command
- **THEN** each configured source's most recent value is listed

#### Scenario: JSON output
- **WHEN** any query command runs with `--json`
- **THEN** output is valid JSON representing the same data

### Requirement: Direct mode by default
By default CLI queries SHALL read the DuckDB database file directly; no daemon needs to be running.

#### Scenario: Works without daemon
- **WHEN** no daemon is running and the user runs a query command
- **THEN** results are read directly from the database file

### Requirement: Daemon-backed mode
CLI queries SHALL route through the daemon's HTTP API when daemon mode is selected via flag or config.

#### Scenario: Daemon flag routes to API
- **WHEN** the user passes the daemon-mode flag while the daemon is running
- **THEN** query data comes from the HTTP API

#### Scenario: Daemon unreachable fails clearly
- **WHEN** daemon mode is selected but the daemon is unreachable
- **THEN** the command exits non-zero with an error naming the connection failure

### Requirement: Filter query output by source
Query commands (`latest`, `health`, `logs`) SHALL accept repeatable `--source <name>` filters applied identically in direct and daemon modes.

#### Scenario: Filter narrows output
- **WHEN** the user runs `latest --source a --source b`
- **THEN** only rows for sources `a` and `b` are printed

### Requirement: Version shown in CLI output
Human-readable output SHALL begin with the application version.

#### Scenario: Version prefix
- **WHEN** any query command runs without `--json`
- **THEN** the first line contains `barduck v<version>`
