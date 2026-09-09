## Context

See proposal.md for motivation. Current state: `Schedule::new` in `src/collector.rs` builds each source's startup schedule; interval sources call `first_interval_tick`, which queries `Db::last_success` (`fetch_logs` where `error IS NULL`, newest first) and defers the first tick by the remaining freshness window or fires immediately. `Db` already has `logs()` (newest-first with `id` tie-break) and `last_success`; there is no "latest attempt regardless of outcome" query. Steady-state `Schedule::advance` (success → `interval`, failure → `retry_interval`) is correct and unchanged. Cron sources compute `next_cron_delay` fresh from wall clock each tick and never consult history.

## Goals / Non-Goals

**Goals:**
- Interval sources resume on daemon startup from the most recent `fetch_logs` timestamp for that source, with success → remaining `interval` and failure → remaining `retry_interval` (immediate when overdue).
- Preserve existing no-history → immediate, cron-unchanged, and one-shot-ignores-history behavior.

**Non-Goals:**
- No change to per-tick scheduling, retry policy, health derivation, retention, config schema, CLI, or HTTP API.
- No catch-up of missed cron occurrences; no backfill of missed interval ticks beyond a single next-tick computation.
- No new tables, columns, or indexes (existing `fetch_logs(source, ts_epoch, id, error)` suffices).

## Decisions

- **Add `Db::last_attempt` alongside `last_success` (keep `last_success` for health).** New query mirrors `last_success` exactly — `WHERE source = ? ORDER BY ts_epoch DESC, id DESC LIMIT 1` without the `error IS NULL` filter — so outcome is derived from `error` presence per the fetch-logged contract. Alternative (reuse `logs(source, limit=1)`) rejected: it builds a `Vec` and parses extra columns for a single-row lookup; a dedicated method keeps the hot startup path narrow and testable. Alternative (repurpose `last_success`) rejected: health computation still needs last-success semantics.
- **Rework `first_interval_tick` to take both intervals and branch on outcome.** Signature becomes `(db, source, interval, retry_interval)`: fetch `last_attempt`; `None` → now; `error IS NULL` → `now + (interval - age)` clamped at now; `error IS SOME` → `now + (retry_interval - age)` clamped at now. Alternative (compute in `Schedule::new` directly) rejected: keeps the pure time-math unit-testable in one place with the existing `SystemTime`-vs-`ts_epoch` age computation.
- **Clamp negative/overdue remainders to immediate, reuse existing clock pattern.** `age >= wait` → `Instant::now()`; else `Instant::now() + (wait - age)`. Keeps the current `SystemTime::now → UNIX_EPOCH → as_secs_f64` age derivation so startup skew handling matches today's behavior; per-tick NTP robustness (`next_cron_delay` recompute, `advance` from `Instant::now`) is untouched.
- **Propagate DB errors, never fall back to immediate on query failure.** A failed `last_attempt` query returns `Err` from `Schedule::new` → logged per-source (`collector for ... stopped`) as today, rather than silently treating the source as due. Alternative (fallback immediate) rejected: hides storage outages and risks a retry storm across all sources.

## Risks / Trade-offs

- [Clock skew between writer and reader] → Mitigation: same `ts_epoch` source (`now()`) and same `Instant::now + remaining` clamping as today; negative ages (future timestamps) naturally yield the full wait, never immediate.
- [Retention purges old logs → source looks history-less → immediate fetch] → Mitigation: acceptable and documented; a source with no retained history is indistinguishable from new and fetching once is the safe direction.
- [Burst of immediate fetches after long downtime (all sources overdue)] → Mitigation: unchanged from today; per-source tasks (`spawn_graceful`) already isolate sources so one slow fetch never blocks others.
- [Same-millisecond tie between success and failure rows] → Mitigation: reuse the established `ts_epoch DESC, id DESC` ordering (monotonic sequence tie-break, consistent with `last_success`/`logs`/health) so "latest attempt" is deterministic.

## Migration Plan

- No migration: read-only change to startup scheduling; `fetch_logs` schema and config are untouched. Rollback is a plain revert to last-success scheduling with no data cleanup.

## Open Questions

- None.
