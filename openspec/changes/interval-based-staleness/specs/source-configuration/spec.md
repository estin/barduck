## MODIFIED Requirements

### Requirement: Environment variables override top-level settings
Each top-level `Config` scalar setting — `database_path`, `listen`, `interval`, `failure_threshold`, `history_points`, `tui_width` — MAY be overridden at startup by an environment variable named `BARDUCK_<FIELD>` (the field name upper-cased, e.g. `BARDUCK_LISTEN`, `BARDUCK_HISTORY_POINTS`). When such a variable is set (including to an empty string), its value SHALL be parsed using the same rules as the field's TOML representation (e.g. humantime for `interval`, integer for `history_points`/`failure_threshold`, `"auto"` or an integer for `tui_width`) and SHALL take precedence over both the config file's value for that field and the field's built-in default. A value that fails to parse MUST cause startup to fail with an error naming the environment variable and the parse failure. This override applies only to these top-level scalar fields, not to `sources` or `layouts`.

#### Scenario: Environment variable overrides config file value
- **WHEN** the config file sets `listen = "127.0.0.1:8420"` and `BARDUCK_LISTEN=0.0.0.0:9000` is set in the environment
- **THEN** the daemon binds to `0.0.0.0:9000`

#### Scenario: Environment variable overrides the built-in default
- **WHEN** the config file does not set `history_points` and `BARDUCK_HISTORY_POINTS=100` is set in the environment
- **THEN** the effective `history_points` is `100`

#### Scenario: No environment variable leaves the config file value in effect
- **WHEN** the config file sets `failure_threshold = 5` and no `BARDUCK_FAILURE_THRESHOLD` variable is set
- **THEN** the effective `failure_threshold` is `5`

#### Scenario: Unparseable override rejected
- **WHEN** `BARDUCK_FAILURE_THRESHOLD=not-a-number` is set in the environment
- **THEN** startup fails with an error naming `BARDUCK_FAILURE_THRESHOLD` and the parse failure

### Requirement: Human-readable duration configuration
Every duration-valued config field — a source's `interval`, `retry_interval`, and `timeout`, and the top-level default `interval` — SHALL be a humantime-formatted string (e.g. `"30s"`, `"5m"`, `"1h30m"`, `"2d"`), not a raw integer of seconds. A field that is not a valid humantime duration string (malformed text, or a nonzero bare number with no unit) MUST be rejected at startup with an error naming the offending source (or the top-level field) and the invalid value. An unrecognized field name on a source or at the top level (for example a pre-rename `interval_secs`, or a leftover `stale_after`) MUST also be rejected at startup rather than silently ignored, so a config left over from before this change fails loudly instead of quietly reverting to a default.

#### Scenario: Humantime string accepted
- **WHEN** a source declares `interval = "5m"`
- **THEN** the source is fetched every 5 minutes

#### Scenario: Invalid duration string rejected
- **WHEN** a source declares `timeout = "banana"`
- **THEN** startup fails naming the source, the field, and the invalid value

#### Scenario: Bare number without a unit rejected
- **WHEN** a source declares `interval = "300"`
- **THEN** startup fails naming the source, the field, and the invalid value

#### Scenario: Leftover pre-rename field rejected
- **WHEN** a config written before this change still declares `interval_secs = 300` on a source
- **THEN** startup fails naming the source and the unrecognized field, instead of silently falling back to the default interval

#### Scenario: Leftover top-level stale_after rejected
- **WHEN** a config written before this change still declares a top-level `stale_after = "30m"`
- **THEN** startup fails naming the unrecognized field, instead of silently ignoring it
