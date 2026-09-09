## MODIFIED Requirements

### Requirement: Readings persisted with provenance
Each stored reading MUST include source name, value, unit if declared, and collection timestamp. The timestamp is the moment the value was collected, unless the value arrived as a `jsonl` row carrying a valid `ts` (spec: source-configuration — JSONL row schema), in which case the row's `ts` is stored instead.

#### Scenario: Reading queryable by source and time range
- **WHEN** readings exist for multiple sources over several days
- **THEN** they can be queried filtered by source name and time range

#### Scenario: Row timestamp preserved
- **WHEN** a `jsonl` row carries `ts = "2026-09-09T12:00:00Z"`
- **THEN** the stored reading's timestamp equals that instant, not the arrival time
