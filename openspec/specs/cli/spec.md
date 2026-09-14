# cli Specification

## Purpose

Shows latest values and fetch logs from the terminal in a scriptable, non-interactive form, usable both against the database file directly and against a running daemon.

## Requirements
### Requirement: Query commands
The CLI SHALL provide commands to print latest values per source and recent fetch logs. The `history` and `health` subcommands are removed: per-source history and health remain available in the web UI, the TUI, and the HTTP API. Output SHALL be human-readable tables by default and JSON with `--json`. The fetch-logs table SHALL include each entry's attempt origin (`push` for HTTP-ingested values, `poll` for scheduled fetches; spec: data-collection — Fetch attempts logged).

#### Scenario: Latest values printed
- **WHEN** the user runs the latest-values command
- **THEN** each configured source's most recent value is listed

#### Scenario: JSON output
- **WHEN** any data command runs with `--json`
- **THEN** output is valid JSON representing the same data

#### Scenario: Removed subcommands fail
- **WHEN** the user runs `history` or `health`
- **THEN** the CLI exits non-zero with an unknown-subcommand error

#### Scenario: Log origin rendered
- **WHEN** the user runs the fetch-logs command over entries of both origins
- **THEN** each row shows `push` or `poll` in its origin column
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
Data commands (`latest`, `logs`) SHALL accept repeatable `--source <name>` (`-s <name>`) filters applied identically in direct and daemon modes.

#### Scenario: Filter narrows output
- **WHEN** the user runs `latest --source a --source b`
- **THEN** only rows for sources `a` and `b` are printed
### Requirement: Source debug fetch command
The CLI SHALL provide a `fetch` command taking exactly one source via the required `--source <name>` (`-s <name>`) flag. It runs the source's command once and prints the result without writing anything to the database: no readings, fetch logs, health events, or threshold changes. For a `query` source it runs the command once; for a `stream` source it runs the command and prints the first parsed lines (up to 5), then kills the command. Execution honors the source's configured timeout. Output SHALL show the parsed result (extracted value, resolved timestamp, applied thresholds, value-type conversion) in a human-readable form by default and as JSON with `--json`. An unknown source name MUST fail naming the source.

#### Scenario: Query debug print
- **WHEN** the user runs `fetch --source cpu` for a `query` source printing `42%`
- **THEN** the extracted value `42%` is printed and the database is unchanged

#### Scenario: JSON debug output
- **WHEN** the fetch command runs with `--json`
- **THEN** output is valid JSON representing the same parsed result

#### Scenario: Stream debug prints first lines
- **WHEN** the user runs the fetch command for a `stream` source emitting rows continuously
- **THEN** the first parsed lines (up to 5) are printed, the command is killed, and the database is unchanged

#### Scenario: Unknown source rejected
- **WHEN** the fetch command names a source not in the config
- **THEN** it exits non-zero naming the unknown source

#### Scenario: Timeout honored
- **WHEN** the source's command hangs past its configured timeout
- **THEN** the fetch command reports the timeout and exits non-zero without writing to the database

#### Scenario: Missing source flag rejected
- **WHEN** the fetch command runs without `--source`
- **THEN** it exits non-zero reporting the missing required flag
### Requirement: Reset reports machine-readable result
The `reset` command SHALL accept `--json`: without it, it prints the human-readable confirmation; with it, it prints valid JSON describing the reset database instead.

#### Scenario: Reset confirmation printed
- **WHEN** the user confirms the reset without `--json`
- **THEN** a human-readable message names the reset database path

#### Scenario: Reset JSON output
- **WHEN** the user confirms the reset with `--json`
- **THEN** output is valid JSON representing the same reset result
### Requirement: Version shown in CLI output
Human-readable output SHALL begin with the application version.

#### Scenario: Version prefix
- **WHEN** any command producing human-readable output runs without `--json`
- **THEN** the first line contains `barduck v<version>`
### Requirement: Force poll command
The CLI SHALL provide a `poll` command that fetches named sources immediately, ignoring their schedules, and records the results exactly as a scheduled fetch does: a reading on success, a fetch-log entry with origin `poll` in either outcome, a health refresh, and any threshold override the fetched value carries.

Sources are named with the repeatable `--source <name>` (`-s <name>`) flag; at least one is required. Each named source is polled once, in the order given. Execution honors each source's configured timeout.

The command SHALL be distinct from `fetch`, which continues to write nothing (spec: cli — Source debug fetch command).

Output SHALL be human-readable by default and valid JSON with `--json`, reporting per source: the source name, whether the attempt succeeded, the stored value and timestamp on success, and the error on failure. The command SHALL exit non-zero if any named source's attempt failed, after attempting all of them.

An unknown source name MUST fail naming the source, before any source is polled.

#### Scenario: Forced poll stores a reading
- **WHEN** the user runs `poll -s cpu` for a `query` source printing `42`
- **THEN** a reading for `cpu` with value `42` is stored, a successful fetch-log entry with origin `poll` is recorded, and the command exits zero

#### Scenario: Failed fetch is recorded and reported
- **WHEN** the user runs `poll -s cpu` and the source's command exits non-zero
- **THEN** a failed fetch-log entry is recorded for `cpu`, the error is reported, and the command exits non-zero

#### Scenario: Multiple sources polled
- **WHEN** the user runs `poll -s a -s b`
- **THEN** both `a` and `b` are fetched and each source's outcome is reported

#### Scenario: JSON output
- **WHEN** the poll command runs with `--json`
- **THEN** output is valid JSON representing the same per-source outcomes

#### Scenario: Missing source flag rejected
- **WHEN** the poll command runs without `--source`
- **THEN** it exits non-zero reporting the missing required flag

#### Scenario: Unknown source rejected before polling
- **WHEN** the user runs `poll -s known -s nope` and `nope` is not in the config
- **THEN** the command exits non-zero naming `nope` and `known` is not polled

#### Scenario: Timeout honored
- **WHEN** a polled source's command hangs past its configured timeout
- **THEN** the attempt is recorded as a failed fetch naming the timeout and the command exits non-zero
### Requirement: Force poll works with and without a daemon
The `poll` command SHALL work whether or not a daemon is running, without the user choosing a mode.

With no daemon holding the database, `poll` SHALL run the fetch in the CLI process and write the results to the database file directly. When the database is already held by another process — a running daemon — `poll` SHALL instead route the request through that daemon's HTTP API, which performs the fetch in the daemon and returns its outcome, matching the fallback already applied to direct-mode queries (spec: cli — Direct mode by default). Passing the daemon-mode flag SHALL force the HTTP path without probing the database.

A daemon that cannot be reached MUST fail naming the connection failure, without recording anything.

#### Scenario: Poll without a daemon writes directly
- **WHEN** no daemon is running and the user runs `poll -s cpu`
- **THEN** the fetch runs in the CLI process and the reading is written to the database file

#### Scenario: Poll with a daemon running routes through it
- **WHEN** a daemon holds the database and the user runs `poll -s cpu` with no flags
- **THEN** the request is served by the running daemon and the stored reading is visible to it immediately

#### Scenario: Daemon unreachable fails clearly
- **WHEN** the daemon-mode flag is passed but the daemon is unreachable
- **THEN** the command exits non-zero naming the connection failure
### Requirement: Force poll rejects sources with nothing to fetch
The `poll` command MUST reject a source that has no fetch to force: an `ingest` source, which receives data only by HTTP push, and a `stream` source, which is a continuously running process rather than a per-tick fetch. The rejection SHALL name the source and why it cannot be polled, and MUST record nothing for it.

#### Scenario: Ingest source rejected
- **WHEN** the user runs `poll -s webhook` for an `ingest` source
- **THEN** the command exits non-zero explaining that ingest sources receive data via HTTP push, and nothing is recorded

#### Scenario: Stream source rejected
- **WHEN** the user runs `poll -s ticks` for a `stream` source
- **THEN** the command exits non-zero explaining that stream sources are collected continuously, and nothing is recorded
### Requirement: Poll and fetch accept composite roots and children
`barduck poll --source <name>` and `barduck fetch --source <name>` SHALL accept a composite source's own name, forcing/fetching the whole family, or one of its children's full `<parent>::<child>` name, with the same single-command fan-out described in data-collection (spec: data-collection — Force polling a composite source or its children). `fetch --source <name>` on a composite root prints the parsed array; on a child it prints only that child's resolved entry from the same single command run.

#### Scenario: poll on a composite root's own name
- **WHEN** `barduck poll --source load` is run against a composite source named `load`
- **THEN** the command runs once, every declared child is refreshed, and the printed outcome describes the root's command/parse result

#### Scenario: poll on a child's full name
- **WHEN** `barduck poll --source load::1m` is run
- **THEN** the parent's command runs once, every declared child is refreshed, and the printed outcome describes `load::1m`'s resulting value

#### Scenario: fetch on a composite root prints the whole array
- **WHEN** `barduck fetch --source load` is run
- **THEN** the parsed array of every child's entry is printed and nothing is written to the database

#### Scenario: fetch on a child prints just that child's entry
- **WHEN** `barduck fetch --source load::1m` is run
- **THEN** only `load::1m`'s resolved entry from the parsed array is printed and nothing is written to the database
