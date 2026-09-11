## Context

The daemon currently supports two source types: `query` (scheduled shell command) and `stream` (continuous process). The `add-http-ingest-endpoint` change already added `POST /api/ingest` to accept push-based readings for any config-declared source. This change adds a new `ingest` source type — a source with no command that receives data exclusively via HTTP push.

The `SourceCfg` enum is tag-dispatched by `type`. `SourceKind` similarly dispatches fetch behavior. The collector (`spawn_graceful`, `loop_source`, `collect_once`) iterates over all sources and spawns a collector task per source. The health module (`is_stale`) determines staleness based on source type and `last_ok_age`.

See proposal.md - Why for motivation.

## Goals / Non-Goals

**Goals:**
- Add `ingest` as a third source type with no command, receiving data via HTTP push.
- `ingest` sources report `stale` when no push arrives within `expected_interval`.
- Collector skips `ingest` sources on schedule ticks (no task spawned).
- Config validation rejects `ingest` sources missing `expected_interval` or declaring `command`/`interval`/`cron`.

**Non-Goals:**
- Implementing the HTTP endpoint itself — already done by `add-http-ingest-endpoint`.
- TUI rendering of ingest sources — web UI only, per the existing pattern.
- Auth/rate limiting for pushes — inherited from the existing endpoint.

## Decisions

1. **`Ingest` has no collector task.** Since `ingest` sources have no schedule and receive data only via push, they don't need a running collector loop. The `spawn_graceful` function skips `ingest` sources entirely. Staleness is computed on-demand by `health::compute` reading the DB's last successful reading timestamp.

2. **Reuse `expected_interval` semantics from `stream`.** `ingest` uses the same staleness logic as `stream`: stale when `last_ok_age > expected_interval`. The `is_stale` function in `health.rs` gets a match arm for `ingest` that delegates to the same condition as `stream`.

3. **`SourceKind::Ingest` is a zero-field variant.** Unlike `Query { command }` and `Stream { command }`, `Ingest` carries no command string. `source::build()` skips the command-presence check for `ingest` sources.

4. **`SourceCfg::Ingest` has `expected_interval` as the only required field** besides `name`. Optional fields match `stream`: `title`, `format`, `thresholds`, `history_points`, `show_history`, `show_in`, `value_type`. No `command`, `interval`, `cron`, or `retry_interval` (there's no process to reopen).

5. **`collect_once` skips `ingest` sources.** `collect_once` already skips `stream` sources; `ingest` is skipped too — it has no fetch to perform. The push endpoint handles all data insertion.

## Risks / Trade-offs

- **No running task means no background staleness tracking.** The health module computes staleness on-demand, which means stale status is only reflected when health is queried. This matches the existing pattern — `query` and `stream` sources also rely on `last_ok_age` from the DB, not real-time staleness flags.
- **`expected_interval` required for `ingest`.** Unlike `stream` where `expected_interval` is also required, a missing `expected_interval` means staleness can't be determined. This is validated at startup.
- **Push-heavy sources write many log rows.** Retention (`purge_older_than`) already bounds the table; no new mechanism needed.
