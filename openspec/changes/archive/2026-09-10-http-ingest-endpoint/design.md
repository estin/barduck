## Context

The daemon already owns every piece ingest needs except the trigger. `store_parsed_value` (src/collector.rs) converts, stores, logs, applies session bands, and refreshes health for both `fetch_once` and `ingest_stream_line`. `Db::set_session_bands` gives exactly the requested thresholds semantics: daemon-memory-only overrides, forgotten on restart. The missing parts are the HTTP entry point, a per-attempt origin, and a way for the handler to move an interval source's next tick.

Constraints shaping the approach:
- Interval `Schedule` state is owned by each source's collector task; `tick()` sleeps on its `next` deadline. Any reset from outside must preempt that sleep, not wait for it.
- `last_status` (health-transition memory) is `&mut` state local to each collector task; the HTTP handler cannot borrow it.
- `create_schema` is `IF NOT EXISTS` only — there is no migration precedent; the origin column must land on existing databases without one.
- Proposal decision (locked): unknown sources get 4xx; no auth beyond the listen socket.

## Goals / Non-Goals

**Goals:**
- `POST /api/ingest` stores a reading + successful `push`-origin log row through the same validation/conversion pipeline as fetches.
- Ingest resets interval waits and clears retry backoff; cron occurrences never move.
- Origin is queryable and rendered in CLI `logs` and the web per-source log view.

**Non-Goals:**
- Auth/tokens/rate limiting; auto-creating undeclared sources; TUI origin column (CLI + web only, per request); replaying or persisting session bands.

## Decisions

1. **Handler reuses the store pipeline, not a copy.** The ingest handler parses the body into the same `ParsedOutput` shape (`ts` defaulting to arrival time, `threshold` bands validated by `validate_thresholds`), then calls a small refactor of `store_parsed_value` whose health-refresh step re-derives status from the DB (as collector startup already does via `last_health`) instead of taking the task-local `last_status`. Collector call sites keep passing their state through a thin wrapper; behavior unchanged.
2. **Origin is a `fetch_logs` column defaulting to `poll`.** Migration without a migrator: `ALTER TABLE fetch_logs ADD COLUMN IF NOT EXISTS origin VARCHAR`, then `UPDATE ... SET origin='poll' WHERE origin IS NULL`, and every log `SELECT` uses `COALESCE(origin,'poll')` so half-migrated rows still read sanely. `LogRow` gains `origin: String` (serde-defaulted for old daemon JSON). Insert paths take an explicit origin; all existing callers pass `poll`.
3. **Schedule reset is a message, not shared mutation.** `AppState` gains one reset sender per interval-scheduled query source; the collector loop's `select!` gains a third arm that, on reset, sets the interval deadline to `now + interval` (the success path of `advance`) and re-enters `tick`. Cron and stream tasks get no sender — ingest to those sources stores + logs only. Holding no lock across the sleep avoids the deadlock where a handler-side reset waits out the very deadline it wants to move.
4. **Origin names: `push` / `poll`.** Symmetric, short for table columns, and already the domain verbs (producers push; the daemon polls). Alternatives rejected: `ingest`/`fetch` (asymmetric, `fetch` collides with the debug-fetch command name), `api`/`schedule` (mechanism-flavored, leaks where the tick came from instead of how data arrived).
5. **Ingest-while-fetching is last-writer-wins.** If a scheduled fetch lands around an ingest, whichever completes last sets the reading; the log keeps both rows. No locking across the round trip — duplicate rows are already the system's normal tie-break story (`id` ordering).

## Risks / Trade-offs

- **Reset-arm restructuring touches the collector hot loop.** The `select!` currently races `tick()` against shutdown; adding a channel arm must preserve shutdown precedence and one-shot (`None` shutdown) behavior. Mitigation: keep the arm additive, cover with the existing schedule unit tests plus a new reset test.
- **DuckDB `ADD COLUMN IF NOT EXISTS` availability.** If the vendored DuckDB version rejects the guard, fall back to probing `PRAGMA table_info` before altering. Verify against the pinned version during implementation.
- **Handler runtime.** The design assumes topcoat handlers run on the same tokio runtime as the collector (needed for `Db` async ops + reset send). If handlers are on a foreign executor, the handler must use `try_send` + `block_on`-free paths — verify first, this is task 1.
- **Log-table growth.** Push-heavy producers can write far more log rows than polling did; retention (`purge_older_than`) already bounds the table, no new mechanism needed.
