## MODIFIED Requirements

### Requirement: Query commands
The CLI SHALL provide commands to print latest values per source and recent fetch logs. The `history` and `health` subcommands are removed: per-source history and health remain available in the web UI, the TUI, and the HTTP API. Output SHALL be human-readable tables by default and JSON with `--json`.

#### Scenario: Latest values printed
- **WHEN** the user runs the latest-values command
- **THEN** each configured source's most recent value is listed

#### Scenario: JSON output
- **WHEN** any data command runs with `--json`
- **THEN** output is valid JSON representing the same data

#### Scenario: Removed subcommands fail
- **WHEN** the user runs `history` or `health`
- **THEN** the CLI exits non-zero with an unknown-subcommand error

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

### Requirement: Version shown in CLI output
Human-readable output SHALL begin with the application version.

#### Scenario: Version prefix
- **WHEN** any command producing human-readable output runs without `--json`
- **THEN** the first line contains `barduck v<version>`

## ADDED Requirements

### Requirement: Reset reports machine-readable result
The `reset` command SHALL accept `--json`: without it, it prints the human-readable confirmation; with it, it prints valid JSON describing the reset database instead.

#### Scenario: Reset confirmation printed
- **WHEN** the user confirms the reset without `--json`
- **THEN** a human-readable message names the reset database path

#### Scenario: Reset JSON output
- **WHEN** the user confirms the reset with `--json`
- **THEN** output is valid JSON representing the same reset result
