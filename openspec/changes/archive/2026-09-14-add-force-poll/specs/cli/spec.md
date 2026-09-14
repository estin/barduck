## ADDED Requirements

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
