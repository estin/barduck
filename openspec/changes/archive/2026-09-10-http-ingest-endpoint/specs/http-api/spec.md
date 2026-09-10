## ADDED Requirements

### Requirement: HTTP ingest endpoint
The daemon SHALL expose `POST /api/ingest` accepting a JSON reading that is stored exactly like a successfully fetched value: one reading plus one successful fetch-log entry with origin `push`. The body fields are `source` (required string), `value` (required string), `ts` (optional timestamp; defaults to arrival time), and `thresholds` (optional band list validated like config-declared bands, applied as a session-only override — never written back to config). The value goes through the same type conversion as fetched values for the source's configured value type. A request naming a source not declared in config MUST fail with a 4xx JSON error naming the unknown source; a malformed body (missing `source`/`value`, unparseable `ts`, invalid bands, value failing type conversion) MUST fail with a 4xx JSON error naming the problem, recording nothing. A stored ingest returns success JSON echoing the stored row. Ingest is served from the same listen socket with no additional auth: the endpoint trusts the daemon's listen address.

#### Scenario: Ingest stores value and log entry
- **WHEN** `POST /api/ingest` carries `{"source": "deploy-count", "value": "42"}`
- **THEN** a reading for `deploy-count` exists with value `42` and a successful fetch-log entry with origin `push` exists for it

#### Scenario: Unknown source rejected
- **WHEN** `POST /api/ingest` names a source not declared in config
- **THEN** the response is a 4xx JSON error naming the unknown source and nothing is recorded

#### Scenario: Bad payload rejected without side effects
- **WHEN** `POST /api/ingest` omits `value` or carries invalid `thresholds`
- **THEN** the response is a 4xx JSON error and no reading or fetch-log entry is recorded

#### Scenario: Session-only threshold override
- **WHEN** an ingest carries valid `thresholds`
- **THEN** the source is colored with those bands until the next reading or restart, and the config file is unchanged
