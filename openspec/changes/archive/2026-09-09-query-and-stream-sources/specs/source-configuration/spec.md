## MODIFIED Requirements

### Requirement: Config file defines sources
The system SHALL read all source definitions from a single user config file at startup. Each source MUST have a unique name, a type (`query` or `stream`), type-specific parameters, and a schedule selector: a `query` source is scheduled by `interval` or `cron`, while a `stream` source declares `expected_interval` instead of a schedule.

#### Scenario: Valid config loads
- **WHEN** the config file contains one `query` source and one `stream` source with valid parameters
- **THEN** both sources are registered and eligible for collection

#### Scenario: Invalid source rejected
- **WHEN** a source definition is missing required fields or has an unknown type or duplicate name
- **THEN** startup fails with an error naming the offending source and field

### Requirement: Threshold bands
A source MAY declare threshold bands as `{bound, level}` pairs where `level` is `green`, `yellow`, or `red`. The reading's numeric value selects the first band whose bound is greater or equal (bands sorted by bound); levels may be ordered green→red or red→green. Non-numeric readings get no band color. Invalid levels or a single-band list MUST be rejected at startup. A `jsonl` row carrying a `threshold` field persistently replaces the source's bands for that and all later readings; the replacement bands are validated with the same rules, and an invalid replacement MUST be rejected the same way as an invalid declaration (the reading is still recorded, but the source's bands are left unchanged).

#### Scenario: High value turns red
- **WHEN** a source has bands 60→green, 85→yellow, 100→red and reports `92`
- **THEN** its panel renders with the red band style

#### Scenario: Row threshold override persists
- **WHEN** a source declaring no bands emits a `jsonl` row with `threshold = [{bound=100.0, level="red"}]` (plus a second valid band) and a later plain reading of `92`
- **THEN** both the row's reading and the later reading are colored with the override bands

#### Scenario: Invalid row threshold rejected
- **WHEN** a `jsonl` row carries `threshold = [{bound=50.0, level="blue"}]`
- **THEN** the row's reading is still recorded, but the source keeps its previous bands

### Requirement: Source setup commands
A source MAY declare a `setup` field containing a shell command run by the collector before that source's first fetch (for example to start a service or open a tunnel). The command runs under the same shell semantics as `query` sources and honors the source's timeout.

#### Scenario: Setup declared and validated
- **WHEN** a config declares `setup = "systemctl start my-exporter"` on a source
- **THEN** the source loads normally and the command runs before its first fetch

#### Scenario: Setup applies to any source type
- **WHEN** either a `query` or a `stream` source declares a `setup` command
- **THEN** both gate their fetching behind it

### Requirement: Config-relative working directory
At startup, before loading the config file or running any command, the system SHALL resolve the config file path to an absolute path and change its working directory to that file's parent directory. Every relative path inside the config file (such as `database_path`) and every command the system executes (a `query` or `stream` source's `command`, a source's `setup` command) SHALL be resolved relative to the config file's directory, not the directory the process was launched from.

#### Scenario: Relative database path resolves against config directory
- **WHEN** `database_path` is a relative path and the config file lives in `/srv/dashboard/config.toml`
- **THEN** the database file is opened at `/srv/dashboard/<database_path>` regardless of the process's launch directory

#### Scenario: Source commands resolve against config directory
- **WHEN** a `query` source's `command`, a `stream` source's `command`, or a source's `setup` command references a relative script path
- **THEN** the command runs with its working directory set to the config file's directory, so the relative path resolves there

#### Scenario: Same config runs identically from any launch directory
- **WHEN** the same config file is launched via `--config /srv/dashboard/config.toml` from two different starting directories
- **THEN** both runs resolve all relative config paths and commands identically

### Requirement: Per-source fetch retry interval
A source MAY declare a `retry_interval` field (humantime duration) controlling how soon an interval-scheduled `query` source retries after a failed fetch, instead of waiting the full `interval`. It defaults to `30s` when not declared. `retry_interval` has no effect on a source's setup-command retries (those always wait the normal schedule tick) and no effect on a cron-scheduled source. A `stream` source reuses `retry_interval` as the delay before reopening its command after the process ends. A source MUST NOT declare both `cron` and `retry_interval`; declaring one with the other MUST be rejected at startup, naming the source.

#### Scenario: Default retry interval applies without configuration
- **WHEN** a source declares no `retry_interval`
- **THEN** it retries 30 seconds after a failed fetch, without any config change

#### Scenario: Explicit retry interval overrides the default
- **WHEN** a source declares `retry_interval = "10s"`
- **THEN** it retries 10 seconds after a failed fetch

#### Scenario: Retry interval with cron rejected
- **WHEN** a source declares both `cron = "0 */5 * * * *"` and `retry_interval = "10s"`
- **THEN** startup fails naming the source and stating that `retry_interval` has no effect on a cron-scheduled source

#### Scenario: Stream reopen waits retry_interval
- **WHEN** a `stream` source with `retry_interval = "10s"` has its command exit
- **THEN** the collector reopens the command roughly 10 seconds later, not immediately

### Requirement: Cron schedule
A `query` source MAY declare a `cron` field containing a cron expression (evaluated with the `croner` crate; standard cron syntax with an optional leading seconds field, e.g. `"0 0 3 * * *"` for daily at 03:00) instead of `interval`. A source MUST NOT declare both `cron` and `interval`, nor both `cron` and `retry_interval` (spec: source-configuration — Per-source fetch retry interval). A `stream` source MUST NOT declare `cron` (streams are continuous, not scheduled). The system SHALL validate the cron expression at startup and reject an invalid expression, naming the source and the invalid value.

#### Scenario: Cron schedule accepted
- **WHEN** a `query` source declares `cron = "0 0 3 * * *"` and no `interval`
- **THEN** the source is scheduled to fetch at that cron expression's occurrences instead of a fixed interval

#### Scenario: Both interval and cron rejected
- **WHEN** a source declares both `interval = "5m"` and `cron = "0 */5 * * * *"`
- **THEN** startup fails naming the source and stating that only one of `interval`/`cron` may be set

#### Scenario: Invalid cron expression rejected
- **WHEN** a source declares `cron = "not a cron"`
- **THEN** startup fails naming the source and the invalid cron expression

#### Scenario: Cron on a stream rejected
- **WHEN** a `stream` source declares `cron = "0 */5 * * * *"`
- **THEN** startup fails naming the source and stating that streams take `expected_interval`, not a schedule

### Requirement: Configurable stored value type
A source MAY declare a `value_type` field controlling how its fetched value is stored: `string` (default, unchanged behavior), `bigint`, `double`, or `json`. Any other value MUST be rejected at startup, naming the source and the invalid value. When a source's `value_type` is not `string`, every fetched value MUST convert cleanly to that type (a valid integer for `bigint`, a valid number for `double`, valid JSON text for `json`); a value that fails to convert MUST be treated the same as a transport-level fetch failure (spec: source-configuration — Generic query source type, Stream source type): no reading is recorded, and the attempt is recorded in the fetch log as failed, naming the conversion error.

#### Scenario: Default preserves current behavior
- **WHEN** a source declares no `value_type`
- **THEN** its readings are stored exactly as before this change, with no typed column populated

#### Scenario: Bigint value stored
- **WHEN** a source declares `value_type = "bigint"` and its fetch returns `"42"`
- **THEN** the reading is recorded with its typed bigint representation stored alongside the existing string value

#### Scenario: Double value stored
- **WHEN** a source declares `value_type = "double"` and its fetch returns `"98.6"`
- **THEN** the reading is recorded with its typed double representation stored alongside the existing string value

#### Scenario: JSON value stored
- **WHEN** a source declares `value_type = "json"` and its fetch returns `{"ok":true}`
- **THEN** the reading is recorded with its typed JSON representation stored alongside the existing string value

#### Scenario: Non-convertible value fails the fetch
- **WHEN** a source declares `value_type = "bigint"` and its fetch returns `"not-a-number"`
- **THEN** no reading is recorded, and the fetch log entry records the failure and the conversion error

#### Scenario: Invalid value_type rejected
- **WHEN** a source declares `value_type = "decimal"`
- **THEN** startup fails naming the source and the invalid value

## ADDED Requirements

### Requirement: Generic query source type
The system SHALL support sources of type `query` that run a local shell command oneshot per schedule tick and take its stdout as the value. Plain (non-JSON) stdout is stored as the value exactly as before. When the stdout parses as a `jsonl` row (spec: source-configuration — JSONL row schema), it is applied structurally: the row's `value`, `ts`, and `threshold` take effect instead of the raw text.

#### Scenario: Command output captured
- **WHEN** a `query` source runs `df / --output=pcent | tail -1` and it prints `42%`
- **THEN** a reading of `42%` is recorded for that source

#### Scenario: Failing command logged
- **WHEN** the command exits non-zero
- **THEN** no reading is recorded and the fetch log entry contains the exit status and stderr

#### Scenario: JSONL output applied structurally
- **WHEN** a `query` source prints `{"value":"42%","threshold":[{"bound":50.0,"level":"green"},{"bound":100.0,"level":"red"}]}`
- **THEN** a reading of `42%` is recorded and the source's thresholds are replaced for that and later readings

### Requirement: Stream source type
The system SHALL support sources of type `stream`: a long-running shell command whose stdout is a sequence of `jsonl` rows (one JSON object per line), each line producing one reading. A `stream` source MUST declare `command` and `expected_interval` (a humantime duration); it MUST NOT declare `interval`, `cron`, or `retry_interval`-incompatible combinations — `interval`/`cron` alongside a stream MUST be rejected at startup naming the source. `retry_interval` MAY be declared and controls the reopen delay. `timeout` applies to the `setup` command only, never to a running stream: silence is governed by staleness, not by killing the process.

#### Scenario: Stream lines become readings
- **WHEN** a `stream` source's command prints `{"value":"21.5"}` then `{"value":"22.0"}`
- **THEN** two readings `21.5` and `22.0` are recorded for that source

#### Scenario: Stream schedule fields rejected
- **WHEN** a `stream` source declares `interval = "5m"`
- **THEN** startup fails naming the source and stating that streams take `expected_interval`, not `interval`

#### Scenario: Missing expected_interval rejected
- **WHEN** a `stream` source declares no `expected_interval`
- **THEN** startup fails naming the source and the missing field

#### Scenario: Silent stream is stale but not killed
- **WHEN** a `stream` source with `expected_interval = "1m"` emits nothing for over a minute
- **THEN** the source reports stale while its process keeps running

### Requirement: JSONL row schema
A `jsonl` row is a single-line JSON object with `value` (required string — the new reading), `ts` (optional — the reading's timestamp, either an RFC 3339 timestamp or epoch seconds as a number; invalid or absent falls back to arrival time), and `threshold` (optional — a `Vec<Threshold>` of `{bound, level}` pairs validated like declared bands, persistently replacing the source's bands). Any other field MUST be rejected: a row carrying an unknown field records no reading and its attempt is logged as failed naming the field. A row missing `value`, or with a non-string `value`, likewise records no reading and is logged as failed.

#### Scenario: Full row applied
- **WHEN** a row `{"value":"ok","ts":"2026-09-09T12:00:00Z","threshold":[{"bound":1.0,"level":"green"},{"bound":2.0,"level":"red"}]}` arrives
- **THEN** a reading `ok` stamped at that timestamp is recorded and the source's bands are replaced

#### Scenario: Minimal row uses arrival time
- **WHEN** a row `{"value":"ok"}` arrives
- **THEN** a reading `ok` stamped at arrival time is recorded and bands are unchanged

#### Scenario: Unknown field rejected
- **WHEN** a row `{"value":"ok","color":"blue"}` arrives
- **THEN** no reading is recorded and the attempt is logged as failed naming `color`

#### Scenario: Missing value rejected
- **WHEN** a row `{"ts":"2026-09-09T12:00:00Z"}` arrives
- **THEN** no reading is recorded and the attempt is logged as failed

### Requirement: Per-type source fields
Source declarations SHALL be deserialized through an explicit per-type enum so each type carries only its own fields: `query` accepts `command` plus `interval`/`cron`; `stream` accepts `command` plus `expected_interval`; fields belonging to a removed type (`url`, `selector`) or to the other type are rejected at startup naming the source and the field. An unknown `type` value (including the removed `http` and `script`) MUST be rejected at startup naming the source and the unknown value.

#### Scenario: Removed http type rejected
- **WHEN** a source declares `type = "http"` with `url` and `selector`
- **THEN** startup fails naming the source and the unknown type `http`

#### Scenario: Renamed script type rejected
- **WHEN** a source declares `type = "script"` with `command`
- **THEN** startup fails naming the source and the unknown type `script`

#### Scenario: Cross-type field rejected
- **WHEN** a `query` source declares `expected_interval = "1m"`
- **THEN** startup fails naming the source and the field

## REMOVED Requirements

### Requirement: Generic http source type
**Reason**: Barduck runs shell commands only; the HTTP transport and selector logic are removed.
**Migration**: Replace `type = "http"` with `type = "query"` running the fetch through the shell (e.g. `command = "curl -s <url>"`), or with `type = "stream"` for continuous output.

### Requirement: Generic script source type
**Reason**: Renamed to `query` as part of the shell-only source model.
**Migration**: Change `type = "script"` to `type = "query"`; all other fields are unchanged.
