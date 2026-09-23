## ADDED Requirements

### Requirement: Fetch-log retrieval supports an error-only filter

The fetch-log retrieval interface SHALL accept an error-only flag. When set, it
returns only entries with a non-null `error`, ordered newest-first by
`(ts_epoch DESC, id DESC)`.

#### Scenario: Error-only retrieval hides successful attempts
- **WHEN** error-only retrieval is requested for a source with mixed success
  and failure attempts
- **THEN** only failed attempts (non-null `error`) are returned

### Requirement: Fetch-log retrieval supports pagination

The fetch-log retrieval interface SHALL accept an offset and a page size
(`limit`) and return that window of entries, newest-first, preserving the
deterministic `(ts_epoch DESC, id DESC)` ordering used by health computation.
A requested page size MUST be clamped to `MAX_LOGS_LIMIT` (10,000) so neither an
unbounded nor a negative value can force a full-table scan.

#### Scenario: Offset window returns the correct slice
- **WHEN** offset=50 and limit=50 are requested on a source with 120 entries
- **THEN** entries 51–100 (newest-first) are returned

#### Scenario: Requested limit is clamped to the maximum
- **WHEN** a page size greater than `MAX_LOGS_LIMIT` is requested
- **THEN** it is clamped to `MAX_LOGS_LIMIT`

#### Scenario: Offset past the end returns an empty result
- **WHEN** offset exceeds the number of stored entries
- **THEN** an empty result is returned

### Requirement: Configurable default log page size

The daemon SHALL support a top-level config field `logs_per_page` (a positive
integer, default `50`) with a `BARDUCK_LOGS_PER_PAGE` environment-variable
override, used as the default page size for log retrieval. Validation SHALL
reject a non-positive value (spec: source-configuration — config validation).

#### Scenario: Default page size applies when unset
- **WHEN** `logs_per_page` is omitted from config
- **THEN** the default page size is 50

#### Scenario: Environment variable overrides the config default
- **WHEN** `BARDUCK_LOGS_PER_PAGE` is set to a valid positive integer
- **THEN** that value is used as the default page size

#### Scenario: Non-positive page size is rejected
- **WHEN** `logs_per_page = 0` is declared in config
- **THEN** daemon startup rejects the configuration
