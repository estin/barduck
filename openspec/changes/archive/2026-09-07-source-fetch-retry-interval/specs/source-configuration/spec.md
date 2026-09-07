## ADDED Requirements

### Requirement: Per-source fetch retry interval
A source MAY declare a `retry_interval` field (humantime duration) controlling how soon an interval-scheduled source retries after a failed fetch, instead of waiting the full `interval`. It defaults to `30s` when not declared. `retry_interval` has no effect on a source's setup-command retries (those always wait the normal schedule tick) and no effect on a cron-scheduled source. A source MUST NOT declare both `cron` and `retry_interval`; declaring one with the other MUST be rejected at startup, naming the source.

#### Scenario: Default retry interval applies without configuration
- **WHEN** a source declares no `retry_interval`
- **THEN** it retries 30 seconds after a failed fetch, without any config change

#### Scenario: Explicit retry interval overrides the default
- **WHEN** a source declares `retry_interval = "10s"`
- **THEN** it retries 10 seconds after a failed fetch

#### Scenario: Retry interval with cron rejected
- **WHEN** a source declares both `cron = "0 */5 * * * *"` and `retry_interval = "10s"`
- **THEN** startup fails naming the source and stating that `retry_interval` has no effect on a cron-scheduled source

## MODIFIED Requirements

### Requirement: Human-readable duration configuration
Every duration-valued config field — a source's `interval`, `retry_interval`, and `timeout`, the top-level default `interval`, and `stale_after` — SHALL be a humantime-formatted string (e.g. `"30s"`, `"5m"`, `"1h30m"`, `"2d"`), not a raw integer of seconds. A field that is not a valid humantime duration string (malformed text, or a nonzero bare number with no unit) MUST be rejected at startup with an error naming the offending source (or the top-level field) and the invalid value. An unrecognized field name on a source or at the top level (for example a pre-rename `interval_secs`) MUST also be rejected at startup rather than silently ignored, so a config left over from before this change fails loudly instead of quietly reverting to a default.

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

### Requirement: Cron schedule
A source MAY declare a `cron` field containing a cron expression (evaluated with the `croner` crate; standard cron syntax with an optional leading seconds field, e.g. `"0 0 3 * * *"` for daily at 03:00) instead of `interval`. A source MUST NOT declare both `cron` and `interval`, nor both `cron` and `retry_interval` (spec: source-configuration — Per-source fetch retry interval). The system SHALL validate the cron expression at startup and reject an invalid expression, naming the source and the invalid value.

#### Scenario: Cron schedule accepted
- **WHEN** a source declares `cron = "0 0 3 * * *"` and no `interval`
- **THEN** the source is scheduled to fetch at that cron expression's occurrences instead of a fixed interval

#### Scenario: Both interval and cron rejected
- **WHEN** a source declares both `interval = "5m"` and `cron = "0 */5 * * * *"`
- **THEN** startup fails naming the source and stating that only one of `interval`/`cron` may be set

#### Scenario: Invalid cron expression rejected
- **WHEN** a source declares `cron = "not a cron"`
- **THEN** startup fails naming the source and the invalid cron expression
