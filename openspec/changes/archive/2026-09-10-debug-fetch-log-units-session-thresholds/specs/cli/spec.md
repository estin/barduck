## ADDED Requirements

### Requirement: Source debug fetch command
The CLI SHALL provide a `fetch` command taking one source name that runs the source's command once and prints the result without writing anything to the database: no readings, fetch logs, health events, or threshold changes. For a `query` source it runs the command once; for a `stream` source it runs the command and prints the first parsed lines (up to 5), then kills the command. Execution honors the source's configured timeout. Output SHALL show the parsed result (extracted value, resolved timestamp, applied thresholds, value-type conversion) in a human-readable form by default and as JSON with `--json`. An unknown source name MUST fail naming the source.

#### Scenario: Query debug print
- **WHEN** the user runs the fetch command for a `query` source printing `42%`
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
