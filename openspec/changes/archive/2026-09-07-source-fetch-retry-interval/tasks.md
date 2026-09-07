## 1. Config schema: retry_interval field

- [x] 1.1 Add `SourceCfg::retry_interval: Option<Duration>` to `src/config.rs` (humantime, same style as `interval`), and `default_retry_interval()` returning `30s`
- [x] 1.2 Add `SourceCfg::effective_retry_interval()` (returns `retry_interval.unwrap_or_else(default_retry_interval)`), mirroring `effective_interval()` — with a unit test covering both the declared and default case
- [x] 1.3 Update `validate_source` to reject `retry_interval = 0` and to reject declaring both `cron` and `retry_interval` — with unit tests for both, plus a test proving a valid `{ interval, retry_interval }` combination is accepted

## 2. Collector: retry-on-failure scheduling

- [x] 2.1 Change `fetch_once` (or its caller in `loop_source`) to report whether the fetch succeeded, without altering its existing side effects (reading insert, log insert, health-event bookkeeping)
- [x] 2.2 Rework `Schedule::Interval` to a `next: tokio::time::Instant`-driven wait (replacing the fixed-period `tokio::time::Interval`): first wait fires immediately (`Instant::now()`), and after each fetch `next` is set to `effective_interval()` out on success or `effective_retry_interval()` out on failure. `Schedule::Cron` and `try_setup`'s retry path are unchanged
- [x] 2.3 Add a collector-level test (mirroring `cron_schedule_fetches_repeatedly`) proving: a source that fails every fetch is retried at `retry_interval` cadence, not `interval`; a source that fails then succeeds resumes the `interval` cadence for its next wait; a cron-scheduled source's next attempt timing is unaffected by fetch outcome

## 3. Verification

- [x] 3.1 Run `cargo build`, `cargo clippy --all-targets -- -D warnings`, and `cargo nextest run`; confirm no new failures beyond the pre-existing sandbox-only TCP test failures
