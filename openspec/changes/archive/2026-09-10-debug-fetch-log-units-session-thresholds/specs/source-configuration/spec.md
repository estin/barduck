## MODIFIED Requirements

### Requirement: Threshold bands
A source MAY declare threshold bands as `{bound, level}` pairs where `level` is `green`, `yellow`, or `red`. The reading's numeric value selects the first band whose bound is greater or equal (bands sorted by bound); levels may be ordered green→red or red→green. Non-numeric readings get no band color. Invalid levels or a single-band list MUST be rejected at startup. A `jsonl` row carrying a `threshold` field replaces the source's bands for the current daemon session only: the override colors that and all later readings until the daemon stops, and the config's bands apply again after a restart. The replacement bands are validated with the same rules, and an invalid replacement MUST be rejected the same way as an invalid declaration (the reading is still recorded, but the source's bands are left unchanged).

#### Scenario: High value turns red
- **WHEN** a source has bands 60→green, 85→yellow, 100→red and reports `92`
- **THEN** its panel renders with the red band style

#### Scenario: Row threshold override persists
- **WHEN** a source declaring no bands emits a `jsonl` row with `threshold = [{bound=100.0, level="red"}]` (plus a second valid band) and a later plain reading of `92`
- **THEN** both the row's reading and the later reading are colored with the override bands until the daemon restarts

#### Scenario: Invalid row threshold rejected
- **WHEN** a `jsonl` row carries `threshold = [{bound=50.0, level="blue"}]`
- **THEN** the row's reading is still recorded, but the source keeps its previous bands

### Requirement: JSONL row schema
A `jsonl` row is a single-line JSON object with `value` (required string — the new reading), `ts` (optional — the reading's timestamp, either an RFC 3339 timestamp or epoch seconds as a number; invalid or absent falls back to arrival time), and `threshold` (optional — a `Vec<Threshold>` of `{bound, level}` pairs validated like declared bands, replacing the source's bands for the current daemon session only). Any other field MUST be rejected: a row carrying an unknown field records no reading and its attempt is logged as failed naming the field. A row missing `value`, or with a non-string `value`, likewise records no reading and is logged as failed.

#### Scenario: Full row applied
- **WHEN** a row `{"value":"ok","ts":"2026-09-09T12:00:00Z","threshold":[{"bound":1.0,"level":"green"},{"bound":2.0,"level":"red"}]}` arrives
- **THEN** a reading `ok` stamped at that timestamp is recorded and the source's bands are replaced for the session

#### Scenario: Minimal row uses arrival time
- **WHEN** a row `{"value":"ok"}` arrives
- **THEN** a reading `ok` stamped at arrival time is recorded and bands are unchanged

#### Scenario: Unknown field rejected
- **WHEN** a row `{"value":"ok","color":"blue"}` arrives
- **THEN** no reading is recorded and the attempt is logged as failed naming `color`

#### Scenario: Missing value rejected
- **WHEN** a row `{"ts":"2026-09-09T12:00:00Z"}` arrives
- **THEN** no reading is recorded and the attempt is logged as failed
