## Why

Today a source's schedule tick is fixed: whether a fetch succeeds or fails, the collector always waits the full `interval` (or the next cron occurrence) before trying again. For an interval-scheduled source with a long interval (e.g. `30m`), a transient failure right after startup means the source stays failing — with no new data and no fresh attempt — for the rest of that interval. Cron-scheduled sources are deliberately schedule-driven and should keep that "call once per occurrence" behavior; the problem is specific to interval-scheduled sources.

## What Changes

- Add a new optional per-source field, `retry_interval` (humantime duration, same style as `interval`/`timeout`), controlling how soon an interval-scheduled source retries after a failed fetch, instead of waiting the full `interval`.
- While an interval-scheduled source's fetches keep failing, it retries every `retry_interval` (not the full `interval`); as soon as a fetch succeeds, it resumes waiting the normal `interval` for its next attempt.
- `retry_interval` defaults to `30s` when not declared, so every interval-scheduled source gets this faster-retry-on-failure behavior without any config change — not opt-in.
- `retry_interval` has no effect on setup-command failures (those keep retrying on the normal schedule tick, unchanged) and no effect on cron-scheduled sources: a cron source is always fetched once per cron occurrence, never retried early, regardless of outcome. A source MUST NOT declare both `cron` and `retry_interval`.

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `source-configuration`: adds the `retry_interval` field (validation: humantime duration, `> 0`, mutually exclusive with `cron`); adds it to the list of humantime-formatted duration fields.
- `data-collection`: modifies "Per-source schedules" to describe the retry-on-failure cadence for interval-scheduled sources; cron-scheduled sources and setup-command retries are explicitly unaffected.

## Impact

- `src/config.rs`: new `SourceCfg::retry_interval: Option<Duration>` field, `default_retry_interval()` (`30s`), `SourceCfg::effective_retry_interval()`; validation for `> 0` and mutual exclusivity with `cron`.
- `src/collector.rs`: the interval-scheduled `Schedule` variant needs to choose its next wait based on the previous fetch's outcome (`effective_interval()` after success, `effective_retry_interval()` while failing) instead of ticking on a fixed period; `fetch_once` needs to report success/failure back to the scheduling loop. `Schedule::Cron` and the setup-retry path are unchanged.
- No changes to the web UI, TUI, CLI, or HTTP API — this is collection-scheduling and config-only.
