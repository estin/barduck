# Spec Delta

## MODIFIED Requirements

### Requirement: Value formats
A source MAY declare a `format` of `text` (default), `markdown`, `duration`, or `elapsed`; other values — including the removed `json` — MUST be rejected at startup (**BREAKING**: configs using `format = "json"` fail to load and must switch to `text` or `markdown`). UIs SHALL render markdown as HTML. A `duration` value SHALL be parsed as a finite, non-negative number of seconds and shown in compact human-readable units (e.g. `1d 2h`, `45s`). An invalid or negative duration SHALL be shown unchanged. An `elapsed` value SHALL be parsed as an RFC 3339 timestamp and shown as the time since that timestamp (e.g. `2d 6h`, `45m`, `just now`). An invalid timestamp SHALL be shown unchanged. A future timestamp SHALL be shown as `just now`. The `duration` and `elapsed` formats SHALL omit the configured unit. These formats SHALL apply to single-source and grouped web panels and to the CLI `latest` table. The raw stored value and `latest --json` output SHALL remain unchanged.

#### Scenario: Unknown format rejected
- **WHEN** a source declares `format = "rst"`
- **THEN** startup fails naming the unknown format

#### Scenario: Removed json format rejected
- **WHEN** a source declares `format = "json"`
- **THEN** startup fails naming the unknown format

#### Scenario: Markdown rendered as markup
- **WHEN** a markdown-format source yields a reading containing `# Heading`
- **THEN** the web UI renders it as an HTML heading

#### Scenario: Duration format accepted
- **WHEN** a source or composite child declares `format = "duration"`
- **THEN** startup succeeds and the source's numeric seconds value is rendered as a duration

#### Scenario: Numeric seconds rendered as duration
- **WHEN** a `duration`-format source's value is `93784`
- **THEN** the web panel and CLI `latest` table show `1d 2h` without the configured unit

#### Scenario: Invalid duration falls back to raw value
- **WHEN** a `duration`-format source's value is `not-a-number`
- **THEN** the displayed value is the raw string `not-a-number`, with no error

#### Scenario: Elapsed format renders time since timestamp
- **WHEN** an `elapsed`-format source's value is `2026-11-05T00:33:49Z` and the current time is `2026-11-05T06:33:49Z`
- **THEN** single-source and grouped web panels and the CLI `latest` table show `6h`

#### Scenario: Invalid timestamp falls back to raw value
- **WHEN** an `elapsed`-format source's value is `not-a-timestamp`
- **THEN** the displayed value is the raw string `not-a-timestamp`, with no error

#### Scenario: Future timestamp shows just now
- **WHEN** an `elapsed`-format source's value is a timestamp later than the current time
- **THEN** the displayed value is `just now`

#### Scenario: JSON output keeps raw value
- **WHEN** a `duration`- or `elapsed`-format source is printed with `latest --json`
- **THEN** the output carries the original value unchanged, not the rendered duration
