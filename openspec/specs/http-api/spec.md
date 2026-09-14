# http-api Specification

## Purpose

Exposes dashboard data over HTTP so the daemon can serve the web UI, daemon-mode CLI/TUI queries, and external consumers from one API.

## Requirements
### Requirement: Daemon mode serves the HTTP API
Running the binary in daemon mode SHALL start an HTTP server exposing the query API. The listen address comes from config.

#### Scenario: Daemon serves queries
- **WHEN** the daemon is running with default config
- **THEN** an HTTP request for latest values returns current data
### Requirement: Query endpoints
The API SHALL expose endpoints returning: latest value per source, reading history filtered by source and time range, per-source health, and fetch logs.

#### Scenario: History endpoint filters correctly
- **WHEN** `/api/sources/<name>/history?from=...&to=...` is requested
- **THEN** only readings for that source within the range are returned

#### Scenario: Health endpoint lists all sources
- **WHEN** the health endpoint is requested
- **THEN** every configured source appears with its current status
### Requirement: Machine-readable responses
All API responses SHALL be JSON. Unknown source names or bad time ranges MUST return client-error statuses with an explanatory message, not 500s.

#### Scenario: Unknown source returns 404-style error
- **WHEN** history is requested for a nonexistent source name
- **THEN** the response is a JSON error with a 4xx status naming the unknown source
### Requirement: Ping/pong health endpoint
The daemon SHALL expose `GET /api/ping`, answering any request with a JSON body containing the server's current time, so a client can confirm the server is reachable and read its clock.

#### Scenario: Ping answered with server time
- **WHEN** a client requests `GET /api/ping`
- **THEN** the response is JSON containing the server's current time and a success status
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
### Requirement: Force poll endpoint
The daemon SHALL expose `POST /api/sources/{name}/poll`, which fetches that source immediately regardless of its schedule and records the result exactly as a scheduled fetch does: a reading on success, a fetch-log entry with origin `poll` in either outcome, a health refresh, and any threshold override the fetched value carries. The request body is ignored; no body is required.

The response SHALL be JSON describing the attempt's outcome — the source name, whether it succeeded, the stored value and timestamp on success, and the error message on a failed fetch. A fetch that ran and failed is a completed request, not a server error: the endpoint SHALL answer with a success status and report the failure in the body, so callers can distinguish "the daemon could not run this" from "the source's command failed".

The fetch SHALL honor the source's configured timeout. A request that arrives while that source is already fetching MUST NOT start a second concurrent fetch of the same source.

A request naming a source not declared in config MUST fail with a 4xx JSON error naming the unknown source. A request naming an `ingest` or `stream` source — neither of which has a fetch to force — MUST fail with a 4xx JSON error naming the source and why, recording nothing.

Like every other endpoint, this one has no authentication of its own and trusts the daemon's listen address.

#### Scenario: Forced poll stores a reading
- **WHEN** `POST /api/sources/cpu/poll` is requested for a `query` source printing `42`
- **THEN** the response is success JSON reporting value `42`, and a reading plus a successful fetch-log entry with origin `poll` exist for `cpu`

#### Scenario: Failed fetch reported in a success response
- **WHEN** a forced poll runs a command that exits non-zero
- **THEN** the response status is success, the body reports the attempt as failed with the error message, and a failed fetch-log entry is recorded

#### Scenario: Unknown source rejected
- **WHEN** `POST /api/sources/nope/poll` names a source not declared in config
- **THEN** the response is a 4xx JSON error naming the unknown source and nothing is recorded

#### Scenario: Unpollable source rejected
- **WHEN** the endpoint names an `ingest` or `stream` source
- **THEN** the response is a 4xx JSON error naming the source and why it cannot be polled, and nothing is recorded

#### Scenario: Timeout reported as a failed attempt
- **WHEN** a forced poll's command hangs past the source's configured timeout
- **THEN** the response reports the attempt as failed naming the timeout, and a failed fetch-log entry is recorded
### Requirement: Force poll endpoint accepts composite roots and children
`POST /api/sources/{source_name}/poll` (spec: http-api — Force poll endpoint) SHALL accept a composite source's own name or one of its children's full name, with the same single-command fan-out described in data-collection (spec: data-collection — Force polling a composite source or its children). The response is the poll outcome for the requested name only — the root's command/parse outcome, or the named child's resulting value.

#### Scenario: POST to a composite root's poll path
- **WHEN** a client sends `POST /api/sources/load/poll` for a composite source named `load`
- **THEN** the parent's command runs once, every declared child is refreshed, and the response describes the root's command/parse outcome

#### Scenario: POST to a child's poll path
- **WHEN** a client sends `POST /api/sources/load::1m/poll`
- **THEN** the parent's command runs once, every declared child is refreshed, and the response describes `load::1m`'s resulting value
