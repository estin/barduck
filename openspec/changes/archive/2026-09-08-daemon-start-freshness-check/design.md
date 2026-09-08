## Context

`Schedule::new` (`src/collector.rs`) currently builds an interval-scheduled source's schedule as a pure, synchronous function of `SourceCfg` alone, always seeding `next = Instant::now()`. `loop_source` already has a `Db` handle in scope when it constructs the schedule, and `Db::last_success(source)` already exists (used by `health::compute`) and returns the most recent successful `LogRow`, including `ts_epoch`. See proposal.md - Why.

## Goals / Non-Goals

**Goals:**
- Avoid re-fetching an interval-scheduled source on daemon startup when its last success is still within its `interval`.
- Keep the fix scoped to the daemon's startup schedule construction — no change to steady-state scheduling, retry behavior, or health/staleness derivation.

**Non-Goals:**
- Changing cron scheduling (already startup-safe — see proposal.md).
- Changing `collect_once`/one-shot CLI behavior (explicit user action should always fetch now).
- Persisting or caching "next due" time anywhere — it's recomputed from `last_success` each time a schedule is built, same pattern as the existing health staleness check.

## Decisions

- **Where the check lives**: inside `Schedule::new`, which becomes `async fn new(db: &Db, src: &SourceCfg) -> Self`. This is the single place a source's initial `next` is decided, so it's the natural place to make that decision freshness-aware. Alternative considered: computing freshness once in `loop_source` and passing a `first_next: Instant` into `Schedule::new` — rejected as extra plumbing for no benefit, since `Schedule::new` already owns the interval-branch fields it would need to set.
- **Freshness threshold is `interval`, not `2 * interval`**: this answers "is it time to fetch again yet", not "should this be reported unhealthy" — the latter is `health::is_stale`'s `2 * interval` question and is intentionally left untouched. Reusing that threshold here would delay a source's re-fetch well past when fresh-looking data actually goes out of date.
- **"Never succeeded" counts as due-now**: matches `is_stale`'s existing treatment of an infinite `last_ok_age`, and preserves today's behavior for a fresh source (no rows yet) or a source whose last log entries are all failures — it still gets fetched immediately, same as now.
- **`db.last_success` is the freshness source of truth**, not `db.last_health`: `last_success` gives the actual last-good timestamp needed for an age computation; `last_health` only gives a status string. This is the same query `health::compute` already uses for the analogous staleness computation, so no new query is introduced.
- **Deferring setup along with the fetch**: when the first fetch is pushed out by freshness, the source's `setup` command (if declared) is not run until that first tick fires either. No separate "run setup now, fetch later" path is introduced — setup exists to prepare for a fetch that is about to happen, and there is nothing to prepare for while the source is still fresh.

## Risks / Trade-offs

- [A source's `interval` is edited downward in config between restarts, and the last recorded success was fresh under the *old* interval but is now stale under the *new* one] → Handled correctly by construction: the freshness check re-reads the *current* config's `effective_interval()` at startup, so this recomputes correctly and fetches immediately if the new interval makes it due.
- [Clock skew between the last recorded success (stored via `SystemTime`) and the monotonic `Instant` used for scheduling] → Same pattern already used by `health::is_stale` (wall-clock age compared against a `Duration` threshold); no new skew exposure beyond what already exists there.
- [A very long-lived daemon instance whose `Instant`-based `next` was computed once at startup from a wall-clock reading now drifts if the system clock is stepped] → Out of scope: this only affects the one-time startup `next` computation, identical in kind to the existing `retry`/`interval` advance logic already using `Instant::now() + wait` on every tick thereafter.

## Migration Plan

No data migration. Purely a scheduling behavior change confined to daemon startup; ships as a normal code change with the accompanying test coverage in `tasks.md`. No rollback concerns beyond reverting the commit.
