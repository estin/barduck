## Context

`src/collector.rs`'s `Schedule` enum currently has two variants: `Interval(tokio::time::Interval)`, a fixed-period ticker built once from `src.effective_interval()`, and `Cron(Box<Cron>)`, which computes the next occurrence fresh each tick via `sleep_until`. `loop_source` calls `schedule.tick().await`, then `fetch_once(...)`, in an unconditional loop — the wait before the next attempt is always the same regardless of whether the previous fetch succeeded or failed. See proposal.md for motivation.

## Goals / Non-Goals

**Goals:**
- An interval-scheduled source retries at `retry_interval` while failing, and returns to its normal `interval` cadence the moment a fetch succeeds.
- Cron-scheduled sources and the setup-retry path are untouched — both already have their own schedule, defined either by the cron expression or the existing "retry setup on next schedule tick" behavior.

**Non-Goals:**
- No backoff curve (exponential, jitter, capped growth, etc.) — `retry_interval` is a single fixed duration, matching the existing `interval`/`timeout` style.
- No change to how the failure/health threshold (`failure_threshold`, `stale_after`) is computed — this change only affects *when the next attempt happens*, not what counts as failing or stale.

## Decisions

**`Schedule::Interval` becomes a manually-driven wait, like `Schedule::Cron` already is.** `tokio::time::Interval` has a fixed period set at construction; there's no supported way to change its period between ticks. The fix replaces it with the same `sleep_until`-based approach `Cron` uses: track a `next: tokio::time::Instant`, and after each fetch, set `next = Instant::now() + (if last fetch succeeded { effective_interval() } else { effective_retry_interval() })`. The first wait is `Instant::now()` (fires immediately) to preserve today's "fetch immediately at startup" behavior.

**`fetch_once` reports success/failure back to the caller.** Today `fetch_once` returns `()`; the loop only inspects `last_status` for health-transition bookkeeping (failing/healthy/stale), which is a *derived*, threshold-smoothed status — not the same thing as "did this one fetch attempt succeed." Scheduling needs the raw per-attempt outcome, so `fetch_once` changes to return `bool` (or the loop matches on the `outcome` it already computes) and `loop_source` uses that directly to pick `effective_interval()` vs `effective_retry_interval()` for the next `Schedule::Interval` wait.

**Validation rejects `cron` + `retry_interval` together**, the same pattern as the existing `interval`/`cron` mutual exclusivity in `validate_source` — `retry_interval` genuinely has no effect on a cron schedule, so accepting-and-ignoring it would be a silent footgun; failing fast at startup is consistent with how this codebase already treats mutually-exclusive schedule fields.

**Setup-command retries are untouched.** `try_setup`'s failure path already just returns `false` and lets the existing `schedule.tick()` govern the retry timing; since `retry_interval` only changes what `Schedule::Interval` waits *after a fetch outcome*, and setup failure never reaches `fetch_once`, no code path change is needed there — `retry_interval` simply doesn't apply.

## Risks / Trade-offs

- [A source stuck permanently failing now polls far more often than before (every `retry_interval` instead of every `interval`)] → this is the intended behavior per the proposal; `retry_interval` defaults to `30s`, a deliberately short but not aggressive default matching the existing `timeout` default, so a persistently-broken http/script source doesn't hammer whatever it's calling. Users with a cheap-to-fail-but-expensive-to-retry source can set a longer `retry_interval` explicitly.
- [Replacing `tokio::time::Interval` with manual `sleep_until` loses `MissedTickBehavior::Skip`'s catch-up-avoidance for the success path] → not a real loss: the manual version always computes `next` relative to "now" after the last completed fetch, so it can never build up a backlog of missed ticks the way a fixed-period `Interval` theoretically could under sustained overrun.

## Migration Plan

1. `src/config.rs`: add `SourceCfg::retry_interval: Option<Duration>`, `default_retry_interval()` (`30s`), `SourceCfg::effective_retry_interval()`; extend `validate_source` to reject `retry_interval.is_some() && cron.is_some()`, and to reject a zero `retry_interval` (mirroring the existing zero-`interval` check).
2. `src/collector.rs`: rework `Schedule::Interval` to a `next: tokio::time::Instant`-driven wait; change `fetch_once`'s signature (or `loop_source`'s use of its result) to report the fetch outcome, and use it to pick `effective_interval()` vs `effective_retry_interval()` for the next `Schedule::Interval` wait. `Schedule::Cron` and `try_setup`'s retry path are unchanged.
3. `demo/config.toml` / docs: no required change — `retry_interval` is optional and defaults sensibly; a mention in a config-reference doc is a nice-to-have, not required for this change to be complete.
4. Verification: unit tests for `default_retry_interval`/`effective_retry_interval` and the new validation rules; a collector-level test (mirroring the existing `cron_schedule_fetches_repeatedly` integration test) that a source failing every fetch is retried at `retry_interval` cadence, and that a source that starts failing then succeeds resumes the normal `interval` cadence. `cargo build`/`clippy`/`nextest` clean beyond the pre-existing sandbox TCP failures.

No rollback complexity: purely additive config field plus an internal scheduling change; existing configs with no `retry_interval` get the new `30s` default retry behavior automatically, which only ever makes an already-failing source retry sooner — no existing passing behavior changes.
