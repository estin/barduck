## MODIFIED Requirements

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
