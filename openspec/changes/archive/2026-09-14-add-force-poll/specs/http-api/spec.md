## ADDED Requirements

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
