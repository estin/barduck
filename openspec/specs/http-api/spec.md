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
