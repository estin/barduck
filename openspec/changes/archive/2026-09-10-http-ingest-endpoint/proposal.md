## Why

Barduck is scrape-only: every value arrives via a scheduled command run. Operators with push-based producers (webhooks, event handlers, external scripts) cannot feed readings in, forcing wasteful poll-every-second intervals or out-of-band hacks. An HTTP ingest endpoint turns the daemon into a collector service as well as a scraper.

## What Changes

- New `POST /api/ingest` endpoint on the daemon accepting a JSON reading:
  `source` (required), `value` (required), `ts` (optional, defaults to arrival
  time), `thresholds` (optional, session-only override via the existing
  `set_session_bands` mechanism — never written back to config).
- Unknown (not config-declared) source names are rejected with a 4xx JSON error,
  matching the history endpoint. Values go through the same parse/convert/store
  pipeline as fetched values (value-type conversion, threshold validation,
  health refresh).
- Ingest counts as a successful attempt for interval scheduling: an ingested
  value resets the source's interval wait (`next = now + interval`), clearing
  retry backoff. Cron-scheduled sources are unaffected — ingest stores the
  reading and log row but never touches the cron occurrence computation.
- Every fetch-log row records its origin: `push` (HTTP ingest) or `poll`
  (scheduled fetch), backfilled as `poll` for pre-existing rows. The CLI `logs`
  table and the web UI per-source log view render an ORIGIN column.

## Capabilities

### New Capabilities

None — ingest extends the existing collection and query surfaces.

### Modified Capabilities

- `http-api`: new `POST /api/ingest` endpoint with 4xx semantics for unknown
  sources and bad payloads.
- `data-collection`: ingested values reset interval schedules (not cron) and
  every fetch attempt records its origin.
- `cli`: `logs` output renders the attempt origin.
- `web-ui`: per-source log view renders the attempt origin.

## Impact

- `src/api.rs`: new ingest handler + request body type.
- `src/db.rs`: `fetch_logs.origin` column (migration with `poll` default),
  `LogRow.origin`, ingest-aware insert path, origin surfaced in log queries.
- `src/collector.rs`: reuse of `store_parsed_value` from the HTTP handler;
  interval-schedule reset hook reachable from outside the collector task.
- `src/cli_report.rs`, `src/web/routes.rs`: ORIGIN column in log tables.
- No auth: the endpoint trusts the daemon's listen socket (default
  loopback; binding to a wider interface exposes ingest to that network —
  documented, not enforced).
