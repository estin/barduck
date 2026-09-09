## Why

Daemon restarts currently schedule interval sources from their last *successful* fetch only, so a source whose recent attempts all failed (or whose success is old but a failure is fresh) re-fires immediately on every restart instead of resuming from its actual last run. Starting each tick from the last logged attempt per source makes restarts resume the true schedule and avoids duplicate fetches and retry storms.

## What Changes

- On daemon startup, for each interval-scheduled source, look up the most recent `fetch_logs` entry (any outcome) for that source instead of only the most recent success.
- If no log entry exists for the source, the first tick is due immediately.
- If the last attempt succeeded, defer the first tick until the remaining freshness window elapses (`interval` minus age of last attempt); fire immediately if already overdue.
- If the last attempt failed (error detail present), schedule the first tick at `retry_interval` after the last attempt (immediate if that time already passed), instead of the full `interval`.
- Cron-scheduled sources are unchanged: still wait for the next absolute cron occurrence on startup.
- One-shot collection (`collect_once`) is unchanged: still fetches every source immediately.

## Capabilities

### New Capabilities

- None.

### Modified Capabilities

- `data-collection`: daemon-startup first-tick scheduling for interval sources changes from last-success freshness to last-attempt (success → remaining `interval`, failure → `retry_interval`) semantics.

## Impact

- Affected code: `src/collector.rs` (`Schedule::new`, `first_interval_tick`, `loop_source` startup path), `src/db.rs` (new `last_attempt` query alongside existing `last_success`).
- No config, CLI, HTTP API, or schema changes; `fetch_logs` table already records timestamp + error detail needed to derive outcome.
- Behavior change is restart-observable only: steady-state per-tick scheduling (`advance` on success/failure) is unchanged.
